//! 网格解析(2026-08-28 资产→视口断链接线):`MeshRenderer.mesh` 引用字符串 →
//! 项目 Content 资产(.meta GUID / 相对路径 / 文件名)→ `.forge/cache/rxmesh/<cache_key>.rxmesh`
//! (assetd 构建链产物,RXGB ClusterDag)→ 展平顶点缓冲(pos3+normal3 交错,stride 24)。
//!
//! 边界(诚实):
//! - 只消费**离线已构建**的 .rxmesh;不触发 gltf 导入/构建(那是 assetd 管线的职责,
//!   未构建 → 诚实 Err,由 viewport 回退 cube 并计入 meshFallbacks 上报,不伪造几何)。
//! - 顶点 = 三角形炸开 + 面法线(RXGB P0 无法线属性;叉积逐面计算,零面积退化 →
//!   [0,0,0] 渲染为不受光暗色,不伪造)。
//! - 上限:单网格 [`MAX_MESH_TRIANGLES`] 三角、每会话 [`MAX_MESH_CLASSES`] 个不同网格
//!   (VB 显存有界);超限诚实 Err/回退,不静默简化。
//! - 解析结果(Ok 与 Err)进程内缓存:Err 缓存避免逐帧全 Content 扫描;新增资产需
//!   触发会话重建(改视口尺寸/重载场景)后生效。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// 单网格三角上限(262 144 tri → 786 432 顶点 × 24B ≈ 18.9 MiB VB)。
pub const MAX_MESH_TRIANGLES: usize = 1 << 18;
/// 单会话不同网格类上限(res 资源数有界;超出回退 cube)。
pub const MAX_MESH_CLASSES: usize = 8;

/// 顶点缓冲(交错 pos3+normal3 f32 LE,stride 24;三角形炸开 + 面法线)。
#[derive(Debug, Clone, PartialEq)]
pub struct MeshGpu {
    pub bytes: Vec<u8>,
    /// 顶点数 = bytes.len()/24 = 三角数×3(炸开语义)。
    pub vertex_count: u32,
    pub triangle_count: u32,
}

type V3 = [f32; 3];

fn v3_cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn v3_norm(a: V3) -> V3 {
    let len = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    if len < 1e-12 {
        [0.0, 0.0, 0.0]
    } else {
        [a[0] / len, a[1] / len, a[2] / len]
    }
}

/// TriMesh(位置 + 三角索引)→ 展平顶点缓冲。确定性:按索引序逐三角形、逐顶点。
pub fn mesh_vertex_bytes(positions: &[[f32; 3]], indices: &[u32]) -> MeshGpu {
    let tri_count = indices.len() / 3;
    let mut bytes = Vec::with_capacity(tri_count * 3 * 24);
    for tri in indices.chunks_exact(3) {
        let a = positions[tri[0] as usize];
        let b = positions[tri[1] as usize];
        let c = positions[tri[2] as usize];
        let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = v3_norm(v3_cross(e1, e2));
        for p in [a, b, c] {
            for f in [p[0], p[1], p[2], n[0], n[1], n[2]] {
                bytes.extend_from_slice(&f.to_le_bytes());
            }
        }
    }
    MeshGpu {
        vertex_count: (tri_count * 3) as u32,
        triangle_count: tri_count as u32,
        bytes,
    }
}

/// mesh 引用 → .rxmesh 缓存文件路径。
///
/// 匹配序:逐 Content 资产(.meta 可读者)按 ①GUID 精确 ②Content 相对路径精确
/// ③文件名尾段(`/hero.fbx` 结尾)比对;命中即按 `MetaDoc::cache_key(源字节)` 取
/// `.forge/cache/rxmesh/<key>.rxmesh`。已命中但缓存未构建 → 记候选继续找,
/// 找不到已构建者时以该候选报错(提示先走资产管线 import)。
fn resolve_rxmesh(project_root: &Path, mesh_ref: &str) -> Result<PathBuf, String> {
    let project =
        assetd::project::ForgeProject::load(project_root).map_err(|e| format!("项目解析失败: {e}"))?;
    let content = project.content_root();
    let mut unbuilt: Option<PathBuf> = None;
    let rel_matches = |rel: &str| {
        rel == mesh_ref
            || rel.replace('\\', "/").ends_with(&format!("/{mesh_ref}"))
    };
    for rel in project
        .scan_content()
        .map_err(|e| format!("Content 扫描失败: {e}"))?
    {
        let mp = assetd::meta_path_for(&content, &rel);
        let meta = match assetd::meta::MetaDoc::load(&mp) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.guid != mesh_ref && !rel_matches(&rel) {
            continue;
        }
        let source_bytes = std::fs::read(content.join(&rel))
            .map_err(|e| format!("源文件读取失败 {rel}: {e}"))?;
        let key = meta.cache_key(&source_bytes);
        let cache_file = project
            .cache_root()
            .join("rxmesh")
            .join(format!("{key}.rxmesh"));
        if cache_file.is_file() {
            return Ok(cache_file);
        }
        if unbuilt.is_none() {
            unbuilt = Some(cache_file);
        }
    }
    match unbuilt {
        Some(p) => Err(format!(
            "资产已匹配但 .rxmesh 未构建(先经资产管线 import/build):{}",
            p.display()
        )),
        None => Err(format!(
            "Content 内无资产匹配 mesh 引用 {mesh_ref:?}(支持 .meta GUID / Content 相对路径 / 文件名尾段)"
        )),
    }
}

/// 全分辨率网格重建:叶层簇局部顶点/索引拼接(叶层划分源三角形,恰一次;
/// `ClusterRecord::vertex_offset/triangle_offset` 为簇内局部基址)。
fn dag_full_mesh(dag: &rurix_geom_build::ClusterDag) -> (Vec<[f32; 3]>, Vec<u32>) {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    for id in dag.leaf_ids() {
        let rec = dag.record(id);
        let verts = dag.cluster_vertices(id);
        let base = positions.len() as u32;
        positions.extend_from_slice(verts);
        for t in 0..rec.triangle_count {
            for li in dag.cluster_triangle(id, t) {
                indices.push(base + li as u32);
            }
        }
    }
    (positions, indices)
}

fn load_mesh_fresh(project_root: &Path, mesh_ref: &str) -> Result<MeshGpu, String> {
    let path = resolve_rxmesh(project_root, mesh_ref)?;
    let bytes =
        std::fs::read(&path).map_err(|e| format!(".rxmesh 读取失败 {}: {e}", path.display()))?;
    let dag = rurix_geom_build::read_dag(&bytes)
        .map_err(|e| format!(".rxmesh 反序列化失败: {e}"))?;
    let (positions, indices) = dag_full_mesh(&dag);
    if positions.is_empty() || indices.is_empty() {
        return Err(format!("空网格({})", path.display()));
    }
    if indices.len() / 3 > MAX_MESH_TRIANGLES {
        return Err(format!(
            "三角数 {} 超单网格上限 {MAX_MESH_TRIANGLES}({})",
            indices.len() / 3,
            path.display()
        ));
    }
    Ok(mesh_vertex_bytes(&positions, &indices))
}

/// 进程内缓存加载(键 = 项目根 + 引用;Ok/Err 均缓存,Err 首见时 eprintln 一次)。
pub fn load_mesh_cached(project_root: &Path, mesh_ref: &str) -> Result<MeshGpu, String> {
    static CACHE: OnceLock<Mutex<HashMap<String, Result<MeshGpu, String>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = format!("{}\u{0}{mesh_ref}", project_root.display());
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(v) = guard.get(&key) {
        return v.clone();
    }
    let v = load_mesh_fresh(project_root, mesh_ref);
    if let Err(e) = &v {
        eprintln!("[meshres] mesh 引用 {mesh_ref:?} 解析失败(进程内缓存该结果): {e}");
    }
    guard.insert(key, v.clone());
    v
}

/// `'static` 提升缓存(会话描述块 `ResourceDesc::data` 须 `'static`;每个不同引用
/// 进程内至多一次 `Box::leak`,上界 = 进程内实际解析成功的不同 mesh 引用数)。
pub fn load_mesh_static_cached(
    project_root: &Path,
    mesh_ref: &str,
) -> Result<&'static MeshGpu, String> {
    static CACHE: OnceLock<Mutex<HashMap<String, Result<&'static MeshGpu, String>>>> =
        OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = format!("{}\u{0}{mesh_ref}", project_root.display());
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(v) = guard.get(&key) {
        return v.clone();
    }
    let v = load_mesh_fresh(project_root, mesh_ref).map(|m| &*Box::leak(Box::new(m)));
    if let Err(e) = &v {
        eprintln!("[meshres] mesh 引用 {mesh_ref:?} 解析失败(进程内缓存该结果): {e}");
    }
    guard.insert(key, v.clone());
    v
}

/// FNV-1a 64(网格集签名;确定性,不引入随机)。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 四面体:4 顶点 4 三角形(外法线由绕序保证)。
    fn tetra() -> (Vec<[f32; 3]>, Vec<u32>) {
        (
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            vec![0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3],
        )
    }

    #[test]
    fn mesh_vertex_bytes_shape_and_normals() {
        let (pos, idx) = tetra();
        let m = mesh_vertex_bytes(&pos, &idx);
        assert_eq!(m.triangle_count, 4);
        assert_eq!(m.vertex_count, 12);
        assert_eq!(m.bytes.len(), 12 * 24);
        // 面法线:底面 (0,2,1) 绕序外法线 -Z;逐顶点一致(面法线语义)。
        let f = |vi: usize| -> V3 {
            let o = vi * 24;
            let g = |i: usize| f32::from_le_bytes(m.bytes[o + i * 4..o + i * 4 + 4].try_into().unwrap());
            [g(3), g(4), g(5)]
        };
        for vi in 0..3 {
            let n = f(vi);
            assert!((n[2] + 1.0).abs() < 1e-6, "底面法线应 -Z:{n:?}");
        }
        let n3 = f(9);
        assert!((n3[0] - n3[1]).abs() < 1e-6 && n3[0] > 0.0, "斜面法线对称:{n3:?}");
    }

    #[test]
    fn mesh_vertex_bytes_deterministic() {
        let (pos, idx) = tetra();
        assert_eq!(mesh_vertex_bytes(&pos, &idx), mesh_vertex_bytes(&pos, &idx));
    }

    #[test]
    fn resolve_by_guid_and_filename_and_miss() {
        let tmp = std::env::temp_dir().join(format!("meshres_test_{}", std::process::id()));
        let content = tmp.join("Content").join("Meshes");
        std::fs::create_dir_all(&content).unwrap();
        std::fs::create_dir_all(tmp.join(".forge").join("cache").join("rxmesh")).unwrap();
        // 源文件 + .meta(snake_case 键,与仓内真实 .meta 同形态)。
        let src = content.join("tetra.gltf");
        std::fs::write(&src, b"fake-gltf-bytes").unwrap();
        std::fs::write(
            content.join("tetra.gltf.meta"),
            "guid: 11111111-2222-4333-8444-555555555555\ntype: mesh\nimporter: gltf\n",
        )
        .unwrap();
        // 用 assetd 真实 cache_key 落缓存(与构建链同键,不复制哈希逻辑)。
        let meta = assetd::meta::MetaDoc::load(&content.join("tetra.gltf.meta")).unwrap();
        let key = meta.cache_key(b"fake-gltf-bytes");
        let dag = rurix_geom_build::build_dag(&rurix_geom_build::TriMesh::new(
            tetra().0,
            tetra().1,
        ));
        std::fs::write(
            tmp.join(".forge").join("cache").join("rxmesh").join(format!("{key}.rxmesh")),
            rurix_geom_build::write_dag(&dag),
        )
        .unwrap();

        let by_guid = resolve_rxmesh(&tmp, "11111111-2222-4333-8444-555555555555").unwrap();
        let by_file = resolve_rxmesh(&tmp, "tetra.gltf").unwrap();
        assert_eq!(by_guid, by_file, "GUID 与文件名命中同一缓存件");
        // 真实读取:回到 MeshGpu(4 三角)。
        let m = load_mesh_cached(&tmp, "tetra.gltf").unwrap();
        assert_eq!(m.triangle_count, 4);
        assert_eq!(m.vertex_count, 12);
        // 缓存命中:二次同值。
        assert_eq!(load_mesh_cached(&tmp, "tetra.gltf").unwrap(), m);
        // 未匹配引用 → 诚实 Err。
        let miss = load_mesh_cached(&tmp, "no-such-mesh").unwrap_err();
        assert!(miss.contains("无资产匹配"), "{miss}");
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn fnv1a64_known_vector() {
        // 经典 FNV-1a 64 空串/“a” 自检向量。
        assert_eq!(fnv1a64(b""), 0xcbf29ce484222325);
        assert_eq!(fnv1a64(b"a"), 0xaf63dc4c8601ec8c);
    }
}
