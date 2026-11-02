//! 出包工具(scripts/package-windows.ps1 调用):
//!
//! ```text
//! forge-setup-pack --stub forge-setup.exe --payload <NSIS 安装包> --meta meta.json --out RurixForge-Setup-<版本>.exe
//! forge-setup-pack --inspect RurixForge-Setup-<版本>.exe     # 打印载荷元数据(不含许可证全文)
//! ```

use forge_setup::payload::{self, Meta};
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("forge-setup-pack: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let (mut stub, mut inner, mut meta, mut out) = (None, None, None, None);
    while let Some(flag) = args.next() {
        let mut value = || args.next().map(PathBuf::from).ok_or(format!("{flag} 缺少参数"));
        match flag.as_str() {
            "--stub" => stub = Some(value()?),
            "--payload" => inner = Some(value()?),
            "--meta" => meta = Some(value()?),
            "--out" => out = Some(value()?),
            "--inspect" => return inspect(value()?),
            other => return Err(format!("未知参数 {other}")),
        }
    }
    let (Some(stub), Some(inner), Some(meta_path), Some(out)) = (stub, inner, meta, out) else {
        return Err("用法:--stub <exe> --payload <exe> --meta <json> --out <exe>,或 --inspect <exe>".into());
    };
    let text = std::fs::read_to_string(&meta_path).map_err(|e| format!("读取 {}: {e}", meta_path.display()))?;
    let meta: Meta = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|e| format!("{} 不是有效的元数据:{e}", meta_path.display()))?;
    if meta.product_name.is_empty() || meta.version.is_empty() || meta.exe_name.is_empty() {
        return Err("元数据缺少 productName / version / exeName".into());
    }
    let total = payload::pack(&stub, &inner, &meta, &out).map_err(|e| format!("写出 {}: {e}", out.display()))?;
    // 立刻读回校验,确保尾标可用。
    let check = payload::read(&out).map_err(|e| format!("校验 {}: {e}", out.display()))?;
    if check.meta != meta {
        return Err("写出的元数据与输入不一致".into());
    }
    println!("{} ({} 字节,内层安装包 {} 字节)", out.display(), total, check.len);
    Ok(())
}

fn inspect(path: PathBuf) -> Result<(), String> {
    let found = payload::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut meta = found.meta;
    meta.license = format!("<{} 字符>", meta.license.chars().count());
    let summary = serde_json::json!({ "meta": meta, "payloadOffset": found.offset, "payloadBytes": found.len });
    println!("{}", serde_json::to_string_pretty(&summary).unwrap());
    Ok(())
}
