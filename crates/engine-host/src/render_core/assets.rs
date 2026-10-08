//! 资产解码缓存(02 §3.4),自 viewport.rs 逐字搬来:.rxsprite 文档、内置 cube 网格、贴图 RGBA8 解码
//! (`TexGpu` 本来就是 CPU 数据,名字里的 Gpu 是历史叫法)、资产代次 `ASSET_GENERATION`、
//! 实体配色 / 材质 albedo 平均色、guid → 源文件映射。`invalidate_assets` 仍在 viewport(它还要清 rurix 会话)。

use std::sync::OnceLock;

use super::math::V3;

/// .rxsprite 文档缓存(逐 GUID 2s TTL:精灵编辑器/agent 随时改写 bbox/clip,
/// 不能像贴图那样泄漏进程级;解析失败缓存 None 同 TTL,避免坏文档逐帧刷盘)。
pub fn sprite_doc_cached(sprite_guid: &str) -> Option<std::sync::Arc<assetd::sprite::SpriteDoc>> {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};
    type Entry = (std::time::Instant, Option<Arc<assetd::sprite::SpriteDoc>>);
    static CACHE: OnceLock<Mutex<HashMap<String, Entry>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some((at, doc)) = cache.lock().unwrap().get(sprite_guid) {
        if at.elapsed() < std::time::Duration::from_secs(2) {
            return doc.clone();
        }
    }
    let loaded = (|| {
        let guid_map = content_guid_map_cached();
        let path = guid_map.get(sprite_guid)?;
        assetd::sprite::load_rxsprite(path).ok().map(Arc::new)
    })();
    cache
        .lock()
        .unwrap()
        .insert(sprite_guid.to_string(), (std::time::Instant::now(), loaded.clone()));
    loaded
}

/// 单位立方体 36 顶点(每面 2 三角形,面法线;交错 pos3+normal3,stride 24)。
pub(crate) fn cube_mesh_bytes() -> &'static [u8] {
    static MESH: OnceLock<&'static [u8]> = OnceLock::new();
    MESH.get_or_init(|| {
        // (法线, 该面四角(逆时针));每角 = 基准 ± 两轴半长。
        let faces: [(V3, [V3; 4]); 6] = [
            ([1.0, 0.0, 0.0], [[0.5, -0.5, 0.5], [0.5, 0.5, 0.5], [0.5, 0.5, -0.5], [0.5, -0.5, -0.5]]),
            ([-1.0, 0.0, 0.0], [[-0.5, -0.5, -0.5], [-0.5, 0.5, -0.5], [-0.5, 0.5, 0.5], [-0.5, -0.5, 0.5]]),
            ([0.0, 1.0, 0.0], [[-0.5, 0.5, 0.5], [0.5, 0.5, 0.5], [0.5, 0.5, -0.5], [-0.5, 0.5, -0.5]]),
            ([0.0, -1.0, 0.0], [[-0.5, -0.5, -0.5], [0.5, -0.5, -0.5], [0.5, -0.5, 0.5], [-0.5, -0.5, 0.5]]),
            ([0.0, 0.0, 1.0], [[-0.5, -0.5, 0.5], [0.5, -0.5, 0.5], [0.5, 0.5, 0.5], [-0.5, 0.5, 0.5]]),
            ([0.0, 0.0, -1.0], [[0.5, -0.5, -0.5], [-0.5, -0.5, -0.5], [-0.5, 0.5, -0.5], [0.5, 0.5, -0.5]]),
        ];
        let mut bytes = Vec::with_capacity(36 * 24);
        let mut push_vert = |p: V3, n: V3| {
            for f in [p[0], p[1], p[2], n[0], n[1], n[2]] {
                bytes.extend_from_slice(&f.to_le_bytes());
            }
        };
        for (n, c) in faces {
            // 两面三角形 (0,1,2) / (0,2,3),法线侧朝外(逆时针,正面剔除约定沿 render_exec 默认)。
            for i in [0usize, 1, 2, 0, 2, 3] {
                push_vert(c[i], n);
            }
        }
        Box::leak(bytes.into_boxed_slice())
    })
}

/// 已解码贴图(进程级缓存,'static 泄漏与会话同生命周期纪律)。
pub struct TexGpu {
    pub w: u32,
    pub h: u32,
    pub rgba: &'static [u8],
}

/// 贴图 GUID → 解码缓存。
pub fn load_tex_static_cached(project: &std::path::Path, tex_guid: &str) -> Option<&'static TexGpu> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static TEX_CACHE: OnceLock<Mutex<HashMap<String, Option<&'static TexGpu>>>> = OnceLock::new();
    let cache = TEX_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = format!("{}:{}:{}",project.display(),tex_guid,ASSET_GENERATION.load(std::sync::atomic::Ordering::Relaxed));
    if let Some(t) = cache.lock().unwrap().get(&key) {
        return *t;
    }
    let loaded = (|| {
        let guid_map = content_guid_map_cached();
        let tex_path = guid_map.get(tex_guid)?;
        let (w, h, rgba) = assetd::texture::decode_rgba(tex_path).ok()?;
        let rgba: &'static [u8] = Box::leak(rgba.into_boxed_slice());
        Some(TexGpu { w, h, rgba })
    })();
    let leaked: Option<&'static TexGpu> = loaded.map(|t| &*Box::leak(Box::new(t)));
    cache
        .lock()
        .unwrap()
        .insert(key, leaked);
    leaked
}

/// 资产代次:`viewport::invalidate_assets`(asset.reload)时 +1;贴图 / 网格缓存键都带上它。
pub(crate) static ASSET_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 实体确定性配色(id 哈希 → 调色板;选中高亮橙)。
#[cfg_attr(not(feature = "backend-rurix"), allow(dead_code))]
fn entity_color(id: u64, selected: bool) -> [f32; 4] {
    if selected {
        return [1.0, 0.62, 0.18, 1.0];
    }
    const PALETTE: [[f32; 3]; 8] = [
        [0.45, 0.62, 0.85],
        [0.62, 0.78, 0.52],
        [0.85, 0.62, 0.45],
        [0.72, 0.55, 0.78],
        [0.50, 0.75, 0.72],
        [0.82, 0.70, 0.45],
        [0.60, 0.60, 0.66],
        [0.75, 0.52, 0.58],
    ];
    let c = PALETTE[(id as usize) % PALETTE.len()];
    [c[0], c[1], c[2], 1.0]
}

/// 实体 MeshRenderer.material(GUID)→ albedo 贴图平均色。
/// 视口渲染管线尚无纹理采样(逐实体纯色 push constant),用贴图平均色着色是
/// 「真实生成素材可见」的保真替身(F-GAME-2);解码结果按材质 GUID 进程级缓存。
#[cfg_attr(not(feature = "backend-rurix"), allow(dead_code))]
pub(crate) fn entity_tint(e: &forge_scene::Entity, selected: bool) -> [f32; 4] {
    if selected {
        return [1.0, 0.62, 0.18, 1.0];
    }
    let mat_guid = e
        .components
        .iter()
        .find(|c| c.ctype == "MeshRenderer" && c.enabled)
        .and_then(|c| c.props.get("material"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if mat_guid.is_empty() {
        return entity_color(e.id, false);
    }
    material_avg_color(&mat_guid).unwrap_or_else(|| entity_color(e.id, false))
}

/// 材质 GUID → albedo 平均色(进程级缓存;Content 资产量级 <百,首帧扫盘一次可接受)。
#[cfg_attr(not(feature = "backend-rurix"), allow(dead_code))]
fn material_avg_color(mat_guid: &str) -> Option<[f32; 4]> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<String, Option<[f32; 4]>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(c) = cache.lock().unwrap().get(mat_guid) {
        return *c;
    }
    let color = resolve_material_avg_color(mat_guid);
    cache
        .lock()
        .unwrap()
        .insert(mat_guid.to_string(), color);
    color
}

/// guid → Content 内源文件路径映射(进程级缓存:render_scene_frame 每帧逐实体解析
/// 材质,无缓存时每帧数百次 .meta 文件 IO,实测单帧 4-6s 全卡在这——F-GAME-2)。
/// 未命中且距上次构建 >2s 时重建一次(容纳 agent 运行中新导入的资产)。
pub(crate) fn content_guid_map_cached() -> std::collections::HashMap<String, std::path::PathBuf> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    type Map = HashMap<String, std::path::PathBuf>;
    static CACHE: OnceLock<Mutex<(std::time::Instant, Map)>> = OnceLock::new();
    let cell = CACHE.get_or_init(|| {
        Mutex::new((
            std::time::Instant::now() - std::time::Duration::from_secs(3600),
            HashMap::new(),
        ))
    });
    let mut guard = cell.lock().unwrap();
    let (built_at, map) = &mut *guard;
    if map.is_empty() || built_at.elapsed() > std::time::Duration::from_secs(2) {
        let project = crate::rpc::project_root();
        *map = content_guid_map(&project.join("Content"));
        *built_at = std::time::Instant::now();
    }
    map.clone()
}

/// guid → Content 内源文件路径映射(扫全部 .meta;失配容忍——无 meta 的文件跳过)。
fn content_guid_map(content: &std::path::Path) -> std::collections::HashMap<String, std::path::PathBuf> {
    let mut map = std::collections::HashMap::new();
    let mut stack = vec![content.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            if p.extension().and_then(|x| x.to_str()) == Some("meta") {
                if let Ok(doc) = assetd::meta::MetaDoc::load(&p) {
                    // 源文件 = sidecar 去掉 .meta 后缀
                    let src = p.with_extension("");
                    if src.is_file() {
                        map.insert(doc.guid, src);
                    }
                }
            }
        }
    }
    map
}

/// 解析材质:mat guid → .rxmat 文件 → textures.albedo guid → 贴图文件 → 平均色。
#[cfg_attr(not(feature = "backend-rurix"), allow(dead_code))]
fn resolve_material_avg_color(mat_guid: &str) -> Option<[f32; 4]> {
    let guid_map = content_guid_map_cached();
    let mat_path = guid_map.get(mat_guid)?;
    let mat_json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(mat_path).ok()?).ok()?;
    let albedo_guid = mat_json
        .get("textures")?
        .get("albedo")?
        .as_str()?
        .to_string();
    let tex_path = guid_map.get(&albedo_guid)?;
    assetd::texture::decode_average_rgba(tex_path).ok()
}

/// 材质 GUID → albedo 贴图 GUID(贴图精灵槽分类用)。
pub(crate) fn material_albedo_guid(mat_guid: &str, _project: &std::path::Path) -> Option<String> {
    let guid_map = content_guid_map_cached();
    let mat_path = guid_map.get(mat_guid)?;
    let mat_json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(mat_path).ok()?).ok()?;
    mat_json
        .get("textures")?
        .get("albedo")?
        .as_str()
        .map(str::to_string)
}
