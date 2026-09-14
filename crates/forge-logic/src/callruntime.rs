//! `call.call_function` 运行时 dll 腿(RD-F4-004 wave.2,D-RD4-B/C/E)。
//!
//! 链路:module(.rx 项目相对)→ 源 SHA-256 + rurixc.exe SHA-256 缓存键 →
//! `.forge/cache/rxdll/<hash>.dll`(`rurixc <file> --emit=dll` 直包,rx CLI RX7003 不透传
//! dll,D-F4-A 先例;构建目录 = 缓存目录,产物 stem.dll 改名)→ libloading 加载 →
//! 标量编组调用(Windows x64 调用约定:float→XMM/int→GPR 按位分配,**首发只支持
//! 同构参数类型**,混合类型无通用蹦床如实拒,D-RD4-C)。
//!
//! 错误语义(D-RD4-E):构建失败(rurixc stderr 截断透传,含 RX6031/6032/6033 结构化
//! 诊断)/加载失败/导出符号缺失/运行时签名不符 → CallError,interp 转 logic.call_error
//! 续链,不静默不伪造返回值。同步调用无超时——subset v1 无 panic 面(RXS-0255 编译期
//! 结构性保证),死循环风险如实标注(与 play 线程同生命周期)。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// Versioned bulk frame ABI. `kind`/`data` belong to the project script; the
/// host owns entity IDs and validates every returned transform before applying.
#[repr(C)]
#[derive(Clone, Copy, Debug, serde::Deserialize)]
pub struct NativeBinding {
    #[serde(rename="entityId")]
    pub entity_id: u64,
    pub kind: u32,
    pub data: [i32; 6],
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeUpdate {
    pub entity_id: u64,
    pub translation: [f32; 3],
    pub scale: [f32; 3],
    /// -1 preserves the frame; >=0 requests a manual sprite frame.
    pub frame: i32,
    /// -1 preserves the animator parameter; 0/1 sets the named bool.
    pub animator_bool: i32,
}

use crate::rxexport::{scan_export_c_fns, scan_rust_c_fns, ExportedFn};

/// 调用失败(结构化,interp 转 logic.call_error 的 detail)。
#[derive(Debug)]
pub enum CallError {
    /// module 路径非法/不存在。
    ModuleNotFound(String),
    /// fn 不在文本级导出表。
    FnNotExported(String),
    /// 签名超首发编组面(运行时 args 个数/类型/同构性不符)。
    Sig(String),
    /// rurixc --emit=dll 构建失败(stderr 截断)。
    Build(String),
    /// dll 加载/符号解析失败。
    Load(String),
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallError::ModuleNotFound(m) => write!(f, "module 不可解析: {m}"),
            CallError::FnNotExported(m) => write!(f, "fn 未导出: {m}"),
            CallError::Sig(m) => write!(f, "签名不符: {m}"),
            CallError::Build(m) => write!(f, "dll 构建失败: {m}"),
            CallError::Load(m) => write!(f, "dll 加载失败: {m}"),
        }
    }
}

/// 单模块已加载态:dll 库 + 文本级导出表。
struct LoadedModule {
    lib: libloading::Library,
    exports: Vec<ExportedFn>,
}

/// call_function 运行时:项目根 + rurixc 路径 + 缓存目录 + 已加载模块表。
pub struct CallRuntime {
    root: PathBuf,
    rurixc: PathBuf,
    rustc: PathBuf,
    cache_dir: PathBuf,
    modules: HashMap<String, LoadedModule>,
}

impl std::fmt::Debug for CallRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallRuntime")
            .field("root", &self.root)
            .field("rurixc", &self.rurixc)
            .field("cache_dir", &self.cache_dir)
            .field("modules", &self.modules.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl CallRuntime {
    /// One checked native call replaces hundreds of per-entity scalar calls.
    /// The ABI passes emitter events, never individual GPU particle positions.
    pub fn invoke_frame(&mut self,module:&str,fn_name:&str,dt:f32,bindings:&[NativeBinding])->Result<Vec<NativeUpdate>,CallError>{
        if bindings.len()>4096 {return Err(CallError::Sig("native frame binding count exceeds 4096".into()));}
        if !self.modules.contains_key(module){let loaded=self.build_and_load(module)?;self.modules.insert(module.into(),loaded);}
        let m=self.modules.get(module).unwrap();
        if !m.exports.iter().any(|f|f.name==fn_name){return Err(CallError::FnNotExported(fn_name.into()));}
        let mut updates=vec![NativeUpdate::default();bindings.len()];
        type FrameFn=unsafe extern "C" fn(u32,f32,*const NativeBinding,u32,*mut NativeUpdate,u32)->u32;
        let count=unsafe{
            let f=m.lib.get::<FrameFn>(fn_name.as_bytes()).map_err(|e|CallError::Load(e.to_string()))?;
            f(1,dt,bindings.as_ptr(),bindings.len()as u32,updates.as_mut_ptr(),updates.len()as u32)
        }as usize;
        if count!=bindings.len(){return Err(CallError::Sig(format!("native frame returned {count} updates for {} bindings",bindings.len())));}
        for (b,u) in bindings.iter().zip(&updates){
            if b.entity_id!=u.entity_id||u.translation.iter().chain(u.scale.iter()).any(|v|!v.is_finite())|| !(-1..=1).contains(&u.animator_bool){
                return Err(CallError::Sig("native frame returned an invalid entity/transform/animator value".into()));
            }
        }
        Ok(updates)
    }
    /// root = 项目根;rurixc 取 FORGE_RURIXC env,缺省 H:\rurix\target\debug\rurixc.exe(F4 先例)。
    pub fn new(root: PathBuf) -> Self {
        let rurixc = std::env::var("FORGE_RURIXC")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(r"H:\rurix\target\debug\rurixc.exe"));
        let cache_dir = root.join(".forge").join("cache").join("rxdll");
        let rustc = std::env::var("FORGE_RUSTC").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("rustc"));
        CallRuntime { root, rurixc, rustc, cache_dir, modules: HashMap::new() }
    }

    /// 调用 module 的 fn(args)。同模块重复调用走已加载缓存(构建零重跑,08 §4.3 同源缓存纪律)。
    pub fn invoke(&mut self, module: &str, fn_name: &str, args: &[Value]) -> Result<Value, CallError> {
        if !self.modules.contains_key(module) {
            let loaded = self.build_and_load(module)?;
            self.modules.insert(module.to_string(), loaded);
        }
        let m = self.modules.get(module).expect("刚插入");
        let efn = m
            .exports
            .iter()
            .find(|e| e.name == fn_name)
            .ok_or_else(|| CallError::FnNotExported(format!("{module} 无 #[export(c)] pub fn {fn_name}")))?;
        marshal_call(&m.lib, efn, args)
    }

    /// 构建(缓存命中跳过)+ 加载 + 导出表扫描。
    fn build_and_load(&self, module: &str) -> Result<LoadedModule, CallError> {
        let mpath = Path::new(module);
        if mpath.is_absolute() || mpath.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return Err(CallError::ModuleNotFound(format!("{module}(须项目根相对路径,不越界)")));
        }
        let abs = self.root.join(module);
        let source = std::fs::read(&abs).map_err(|e| CallError::ModuleNotFound(format!("{module}: {e}")))?;
        let native_rust = mpath.extension().and_then(|s| s.to_str()) == Some("rs");
        let exports = if native_rust { scan_rust_c_fns(&String::from_utf8_lossy(&source)) }
            else { scan_export_c_fns(&String::from_utf8_lossy(&source)) };
        if exports.is_empty() {
            return Err(CallError::FnNotExported(format!("{module} 无任何 #[export(c)] 导出(空导出表)")));
        }
        let dll_path = self.ensure_dll(module, &source)?;
        // Windows:dll 被加载后文件锁定,缓存复跑不重建(缓存键含源 hash,源变才重建——
        // 重建前须 play_exit 卸载;热重载语义 = 实例重建(D-F4-B),Library Drop 卸载)。
        let lib = unsafe { libloading::Library::new(&dll_path) }
            .map_err(|e| CallError::Load(format!("{}: {e}", dll_path.display())))?;
        Ok(LoadedModule { lib, exports })
    }

    /// 缓存键 = sha256(源字节) + sha256(rurixc.exe 字节)前 16 hex;命中(锁文件+ dll 在)零重建。
    fn ensure_dll(&self, module: &str, source: &[u8]) -> Result<PathBuf, CallError> {
        if Path::new(module).extension().and_then(|s| s.to_str()) == Some("rs") {
            return self.ensure_rust_dll(module, source);
        }
        std::fs::create_dir_all(&self.cache_dir)
            .map_err(|e| CallError::Build(format!("建缓存目录 {}: {e}", self.cache_dir.display())))?;
        let mut h = rurix_pkg::sha256::Sha256::new();
        h.update(source);
        h.update(b"|");
        // rurixc 版本面:exe 字节 hash(构建器版本照 assetd cache_key 纪律)。
        let exe = std::fs::read(&self.rurixc)
            .map_err(|e| CallError::Build(format!("rurixc 不可读 {}: {e}", self.rurixc.display())))?;
        h.update(&exe);
        let key_full = rurix_pkg::sha256::hex(&h.finalize());
        let key = &key_full[..16];
        let stem = Path::new(module)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("module")
            .to_string();
        let dll_path = self.cache_dir.join(format!("{stem}-{key}.dll"));
        if dll_path.is_file() {
            return Ok(dll_path);
        }
        // 构建:rurixc -o 直出缓存键名(2026-08-18 实测:-o 对 --emit=dll 生效,全体
        // 副产物 .h/.lib/.exp 同落 -o 目录;无 -o 时产物落源文件旁)。
        // 并发构建同模块:文件锁(D-RD4-F,照 SUBAGENTS_DIR_LOCK 先例)。
        let lock_path = self.cache_dir.join(format!("{stem}-{key}.lock"));
        let _lock = std::fs::File::create(&lock_path)
            .map_err(|e| CallError::Build(format!("建锁 {}: {e}", lock_path.display())))?;
        // 锁内再查(等待方拿到已建产物)。
        if dll_path.is_file() {
            return Ok(dll_path);
        }
        let abs = self.root.join(module);
        let out = Command::new(&self.rurixc)
            .arg(&abs)
            .arg("--emit=dll")
            .arg("-o")
            .arg(&dll_path)
            .output()
            .map_err(|e| CallError::Build(format!("spawn rurixc: {e}")))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let truncated: String = stderr.chars().take(1024).collect();
            return Err(CallError::Build(format!("rurixc exit={:?}: {truncated}", out.status.code())));
        }
        if !dll_path.is_file() {
            return Err(CallError::Build(format!("rurixc exit=0 但缺产物 {}", dll_path.display())));
        }
        Ok(dll_path)
    }

    /// Self-contained Rust modules are an additional native script backend.
    /// They share the checked scalar C ABI, project path rules and DLL lifetime.
    /// No cargo dependencies are resolved or downloaded at play time.
    fn ensure_rust_dll(&self, module: &str, source: &[u8]) -> Result<PathBuf, CallError> {
        std::fs::create_dir_all(&self.cache_dir).map_err(|e| CallError::Build(e.to_string()))?;
        let version = match Command::new(&self.rustc).args(["--version", "--verbose"]).output() {
            Ok(v) if v.status.success() => v,
            result => {
                // Portable packs carry a verified binary/source manifest. They
                // can run on a player's machine without installing a compiler.
                if let Some(dll) = self.prebuilt_rust_dll(module, source) { return Ok(dll); }
                return Err(CallError::Build(format!("rustc unavailable and no verified prebuilt native script: {result:?}")));
            }
        };
        let mut h = rurix_pkg::sha256::Sha256::new();
        h.update(source); h.update(b"|rust-cdylib-v1|edition2021|opt2|panic-abort|");
        h.update(&version.stdout);
        let key = rurix_pkg::sha256::hex(&h.finalize());
        let stem = Path::new(module).file_stem().and_then(|s| s.to_str()).unwrap_or("script");
        let dll = self.cache_dir.join(format!("{stem}-{}.dll", &key[..16]));
        if dll.is_file() { self.write_native_manifest(module, source, &dll)?; return Ok(dll); }
        let result = Command::new(&self.rustc).arg(self.root.join(module))
            .args(["--crate-type", "cdylib", "--edition", "2021", "-C", "opt-level=2", "-C", "panic=abort"])
            .arg("-o").arg(&dll).output()
            .map_err(|e| CallError::Build(format!("spawn rustc: {e}")))?;
        if !result.status.success() {
            return Err(CallError::Build(format!("rustc: {}", String::from_utf8_lossy(&result.stderr).chars().take(4096).collect::<String>())));
        }
        if !dll.is_file() { return Err(CallError::Build("rustc reported success without DLL".into())); }
        self.write_native_manifest(module, source, &dll)?;
        Ok(dll)
    }

    fn write_native_manifest(&self, module: &str, source: &[u8], dll: &Path) -> Result<(), CallError> {
        let bytes = std::fs::read(dll).map_err(|e| CallError::Build(e.to_string()))?;
        let manifest = serde_json::json!({"backend":"rust-cdylib-v1","module":module,
            "sourceSha256": digest(source), "dllSha256": digest(&bytes),
            "dll":dll.file_name().unwrap().to_string_lossy()});
        std::fs::write(dll.with_extension("native.json"), manifest.to_string())
            .map_err(|e| CallError::Build(e.to_string()))
    }

    fn prebuilt_rust_dll(&self, module: &str, source: &[u8]) -> Option<PathBuf> {
        let source_hash = digest(source);
        let mut paths = std::fs::read_dir(&self.cache_dir).ok()?.filter_map(Result::ok)
            .map(|e| e.path()).filter(|p| p.to_string_lossy().ends_with(".native.json")).collect::<Vec<_>>();
        paths.sort();
        for path in paths {
            let Ok(bytes) = std::fs::read(path) else { continue };
            let Ok(m) = serde_json::from_slice::<Value>(&bytes) else { continue };
            if m["backend"] != "rust-cdylib-v1" || m["module"] != module || m["sourceSha256"] != source_hash { continue; }
            let Some(name) = m["dll"].as_str() else { continue };
            if Path::new(name).file_name().and_then(|s| s.to_str()) != Some(name) { continue; }
            let dll = self.cache_dir.join(name);
            if let Ok(bytes) = std::fs::read(&dll) {
                if m["dllSha256"] == digest(&bytes) { return Some(dll); }
            }
        }
        None
    }
}

fn digest(bytes: &[u8]) -> String {
    let mut hash = rurix_pkg::sha256::Sha256::new(); hash.update(bytes);
    rurix_pkg::sha256::hex(&hash.finalize())
}

/// 标量编组调用(同构参数类型,arity 0..=4;bool ↔ C _Bool,Rust bool ABI 兼容)。
fn marshal_call(lib: &libloading::Library, efn: &ExportedFn, args: &[Value]) -> Result<Value, CallError> {
    if args.len() != efn.params.len() {
        return Err(CallError::Sig(format!("{} 参数个数:签名 {},实参 {}", efn.name, efn.params.len(), args.len())));
    }
    if args.len() > 4 {
        return Err(CallError::Sig(format!("{} arity {} 超首发面(≤4)", efn.name, args.len())));
    }
    let pty = match efn.params.first() {
        Some((_, t)) => t.as_str(),
        None => "void",
    };
    if !matches!(pty, "void" | "f32" | "f64" | "i32" | "bool") {
        return Err(CallError::Sig(format!("{} 参数类型 {pty} 超首发标量子集", efn.name)));
    }
    if efn.params.iter().any(|(_, t)| t != pty) {
        return Err(CallError::Sig(format!("{} 混合参数类型无通用蹦床(Windows x64 float→XMM/int→GPR 按位分配;首发同构)", efn.name)));
    }
    // 运行时 args 类型复核(NodePin 来源校验期放行,此处把关,D-RD4-E)。
    for (i, a) in args.iter().enumerate() {
        let ok = match pty {
            "f32" | "f64" | "i32" => a.is_number(),
            "bool" => a.is_boolean(),
            _ => true,
        };
        if !ok {
            return Err(CallError::Sig(format!("{} 第 {} 实参 {a} 类型须 {pty}", efn.name, i + 1)));
        }
    }
    let ret = efn.ret.as_str();
    if !matches!(ret, "void" | "f32" | "f64" | "i32" | "bool") {
        return Err(CallError::Sig(format!("{} 返回类型 {ret} 超首发标量子集", efn.name)));
    }
    let sym = &efn.export_name;
    unsafe {
        match (pty, args.len()) {
            ("void", 0) => call_arity0(lib, sym, ret),
            ("f32", n) => call_homo(lib, sym, ret, n, &args.iter().map(|a| a.as_f64().unwrap_or(0.0) as f32).collect::<Vec<_>>()),
            ("f64", n) => call_homo(lib, sym, ret, n, &args.iter().map(|a| a.as_f64().unwrap_or(0.0)).collect::<Vec<_>>()),
            ("i32", n) => call_homo(lib, sym, ret, n, &args.iter().map(|a| a.as_f64().unwrap_or(0.0) as i32).collect::<Vec<_>>()),
            ("bool", n) => call_homo(lib, sym, ret, n, &args.iter().map(|a| a.as_bool().unwrap_or(false)).collect::<Vec<_>>()),
            _ => Err(CallError::Sig(format!("{} 不可编组形态(arity {})", efn.name, args.len()))),
        }
    }
}

/// 符号解析(错误 → CallError::Load;$fty 为完整 fn 类型,如 unsafe extern "C" fn(f32) -> f32)。
macro_rules! resolve {
    ($lib:expr, $sym:expr, $fty:ty) => {
        $lib.get::<$fty>(($sym).as_bytes())
            .map_err(|e| CallError::Load(format!("符号 {}: {e}", $sym)))?
    };
}

/// 按返回类型分发:($pt,*) = 形参类型表,($an,*) = 实参表达式表。
macro_rules! ret_dispatch {
    ($lib:expr, $sym:expr, $ret:expr, ($($pt:ty),*), ($($an:expr),*)) => {{
        match $ret {
            "void" => {
                let f = resolve!($lib, $sym, unsafe extern "C" fn($($pt),*));
                f($($an),*);
                Ok(Value::Null)
            }
            "f32" => {
                let f = resolve!($lib, $sym, unsafe extern "C" fn($($pt),*) -> f32);
                Ok(serde_json::json!(f($($an),*) as f64))
            }
            "f64" => {
                let f = resolve!($lib, $sym, unsafe extern "C" fn($($pt),*) -> f64);
                Ok(serde_json::json!(f($($an),*)))
            }
            "i32" => {
                let f = resolve!($lib, $sym, unsafe extern "C" fn($($pt),*) -> i32);
                Ok(serde_json::json!(f($($an),*)))
            }
            "bool" => {
                let f = resolve!($lib, $sym, unsafe extern "C" fn($($pt),*) -> bool);
                Ok(serde_json::json!(f($($an),*)))
            }
            _ => Err(CallError::Sig(format!("返回 {} 超子集", $ret))),
        }
    }};
}

/// arity 0 调用。
unsafe fn call_arity0(lib: &libloading::Library, sym: &str, ret: &str) -> Result<Value, CallError> {
    ret_dispatch!(lib, sym, ret, (), ())
}

/// 同构参数类型 arity 1..=4 调用($T 由调用点 Vec 元素类型决定)。
trait HomoCall: Copy {
    unsafe fn call(lib: &libloading::Library, sym: &str, ret: &str, arity: usize, args: &[Self]) -> Result<Value, CallError>;
}

macro_rules! impl_homo {
    ($T:ty) => {
        impl HomoCall for $T {
            unsafe fn call(lib: &libloading::Library, sym: &str, ret: &str, arity: usize, args: &[Self]) -> Result<Value, CallError> {
                match arity {
                    1 => ret_dispatch!(lib, sym, ret, ($T), (args[0])),
                    2 => ret_dispatch!(lib, sym, ret, ($T, $T), (args[0], args[1])),
                    3 => ret_dispatch!(lib, sym, ret, ($T, $T, $T), (args[0], args[1], args[2])),
                    4 => ret_dispatch!(lib, sym, ret, ($T, $T, $T, $T), (args[0], args[1], args[2], args[3])),
                    _ => Err(CallError::Sig(format!("arity {arity} 超首发面(1..=4)"))),
                }
            }
        }
    };
}
impl_homo!(f32);
impl_homo!(f64);
impl_homo!(i32);
impl_homo!(bool);

/// 同构调用入口(泛型转具体 impl)。
unsafe fn call_homo<T: HomoCall>(lib: &libloading::Library, sym: &str, ret: &str, arity: usize, args: &[T]) -> Result<Value, CallError> {
    T::call(lib, sym, ret, arity, args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn native_frame_abi_updates_in_bulk_and_rejects_bad_output_atomically(){
        let root=std::env::temp_dir().join(format!("forge_native_frame_{}",std::process::id()));std::fs::create_dir_all(&root).unwrap();
        let source=r#"
#[repr(C)]
pub struct NativeBinding{pub entity_id:u64,pub kind:u32,pub data:[i32;6]}
#[repr(C)]
pub struct NativeUpdate{pub entity_id:u64,pub translation:[f32;3],pub scale:[f32;3],pub frame:i32,pub animator_bool:i32}
#[no_mangle]
pub extern "C" fn frame(abi:u32,dt:f32,input:*const NativeBinding,count:u32,output:*mut NativeUpdate,capacity:u32)->u32{
 if abi!=1||count>capacity{return 0;}unsafe{for i in 0..count as usize{let b=&*input.add(i);output.add(i).write(NativeUpdate{entity_id:if b.kind==99{999}else{b.entity_id},translation:[b.data[0]as f32+dt,2.,0.],scale:[1.;3],frame:2,animator_bool:0});}}count
}
"#;
        std::fs::write(root.join("frame.rs"),source).unwrap();
        {
            let mut rt=CallRuntime::new(root.clone());let bindings=vec![NativeBinding{entity_id:3,kind:0,data:[7,0,0,0,0,0]},NativeBinding{entity_id:8,kind:0,data:[11,0,0,0,0,0]}];
            let u=rt.invoke_frame("frame.rs","frame",0.25,&bindings).unwrap();
            assert_eq!(u[0].translation,[7.25,2.,0.]);assert_eq!(u[1].entity_id,8);assert_eq!(u[1].frame,2);
            let bad=vec![NativeBinding{entity_id:3,kind:99,data:[0;6]}];assert!(rt.invoke_frame("frame.rs","frame",0.,&bad).is_err());
            assert_eq!(std::mem::size_of::<NativeBinding>(),40);assert_eq!(std::mem::size_of::<NativeUpdate>(),40);
        }std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn rust_native_state_survives_calls_and_source_change_rebuilds() {
        let dir = std::env::temp_dir().join(format!("forge_native_state_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let make_source = |initial: i32| format!("use std::sync::atomic::{{AtomicI32, Ordering}};\nstatic STATE: AtomicI32 = AtomicI32::new({initial});\n#[no_mangle]\npub extern \"C\" fn advance(n: i32) -> i32 {{ STATE.fetch_add(n, Ordering::SeqCst) + n }}\n");
        std::fs::write(dir.join("state.rs"), make_source(10)).unwrap();
        {
            let mut rt = CallRuntime::new(dir.clone());
            assert_eq!(rt.invoke("state.rs", "advance", &[serde_json::json!(3)]).unwrap(), serde_json::json!(13));
            assert_eq!(rt.invoke("state.rs", "advance", &[serde_json::json!(4)]).unwrap(), serde_json::json!(17));
            assert!(rt.invoke("state.rs", "advance", &[serde_json::json!(true)]).is_err());
        }
        std::fs::write(dir.join("state.rs"), make_source(100)).unwrap();
        {
            let mut rt = CallRuntime::new(dir.clone());
            assert_eq!(rt.invoke("state.rs", "advance", &[serde_json::json!(3)]).unwrap(), serde_json::json!(103));
        }
        {
            let mut rt = CallRuntime::new(dir.clone());
            rt.rustc = dir.join("not-installed-rustc.exe");
            assert_eq!(rt.invoke("state.rs", "advance", &[serde_json::json!(5)]).unwrap(), serde_json::json!(105), "portable prebuilt works without compiler");
        }
        // A changed source must never run an older prebuilt binary.
        std::fs::write(dir.join("state.rs"), make_source(200)).unwrap();
        {
            let mut rt = CallRuntime::new(dir.clone());
            rt.rustc = dir.join("not-installed-rustc.exe");
            assert!(rt.invoke("state.rs", "advance", &[serde_json::json!(5)]).is_err());
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
    use std::path::PathBuf;

    /// 临时项目根 + fixture .rx(真实 rurixc 构建,D-RD4-F)。
    fn tmp_project(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "forge_callrt_{}_{}_{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(dir.join("Content/Scripts")).unwrap();
        std::fs::write(
            dir.join("Content/Scripts/math.rx"),
            "#[export(c)]\npub fn add(a: f32, b: f32) -> f32 { a + b }\n\
             #[export(c)]\npub fn mul3(a: f64, b: f64, c: f64) -> f64 { a * b * c }\n\
             #[export(c)]\npub fn negate(x: i32) -> i32 { 0 - x }\n\
             #[export(c)]\npub fn not(flag: bool) -> bool { !flag }\n\
             #[export(c)]\npub fn forty_two() -> i32 { 42 }\n\
             #[export(c)]\npub fn poke(x: i32) { }\n",
        )
        .unwrap();
        dir
    }

    fn rurixc_available() -> bool {
        let path = std::env::var("FORGE_RURIXC").map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(r"H:\rurix\target\debug\rurixc.exe"));
        path.is_file()
    }

    #[test]
    fn invoke_scalar_forms() {
        if !rurixc_available() {
            eprintln!("跳过:rurixc 不在库");
            return;
        }
        let root = tmp_project("forms");
        let mut rt = CallRuntime::new(root.clone());
        let m = "Content/Scripts/math.rx";
        assert_eq!(rt.invoke(m, "add", &[serde_json::json!(2.0), serde_json::json!(3.5)]).unwrap(), serde_json::json!(5.5));
        assert_eq!(rt.invoke(m, "mul3", &[serde_json::json!(2.0), serde_json::json!(3.0), serde_json::json!(4.0)]).unwrap(), serde_json::json!(24.0));
        assert_eq!(rt.invoke(m, "negate", &[serde_json::json!(7)]).unwrap(), serde_json::json!(-7));
        assert_eq!(rt.invoke(m, "not", &[serde_json::json!(true)]).unwrap(), serde_json::json!(false));
        assert_eq!(rt.invoke(m, "forty_two", &[]).unwrap(), serde_json::json!(42));
        assert_eq!(rt.invoke(m, "poke", &[serde_json::json!(1)]).unwrap(), Value::Null);
        // 缓存命中:二次调用零重建(模块已在 modules 表)。
        assert_eq!(rt.invoke(m, "add", &[serde_json::json!(1.0), serde_json::json!(1.0)]).unwrap(), serde_json::json!(2.0));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn invoke_error_surfaces() {
        if !rurixc_available() {
            eprintln!("跳过:rurixc 不在库");
            return;
        }
        let root = tmp_project("errs");
        let mut rt = CallRuntime::new(root.clone());
        let m = "Content/Scripts/math.rx";
        // fn 未导出。
        assert!(matches!(rt.invoke(m, "ghost", &[]), Err(CallError::FnNotExported(_))));
        // 个数不符。
        assert!(matches!(rt.invoke(m, "add", &[serde_json::json!(1.0)]), Err(CallError::Sig(_))));
        // 运行时类型不符(bool 进 f32)。
        assert!(matches!(rt.invoke(m, "add", &[serde_json::json!(1.0), serde_json::json!(true)]), Err(CallError::Sig(_))));
        // module 不存在。
        assert!(matches!(rt.invoke("Content/Scripts/none.rx", "add", &[]), Err(CallError::ModuleNotFound(_))));
        // 越界。
        assert!(matches!(rt.invoke("../x.rx", "add", &[]), Err(CallError::ModuleNotFound(_))));
        let _ = std::fs::remove_dir_all(&root);
    }
}
