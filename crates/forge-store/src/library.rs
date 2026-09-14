//! 个人资产库(内容寻址)。
//!
//! 磁盘布局(引擎级,跨项目共享):
//! ```text
//! <data>/store/library/index.json               条目表
//! <data>/store/library/blobs/<前2位>/<sha256>    去重后的字节
//! ```
//!
//! 设计要点:
//! - **内容寻址去重**:同字节只存一份 blob;同一份字节可被多条目(不同名字/标签/来源)
//!   引用,删除时按引用计数决定是否回收 blob——不做「删条目即删文件」的粗暴回收。
//! - **幂等**:条目 id = hash(name|ext|sha256),同名同内容重复 add 不会长出第二条。
//! - 索引原子写(tmp+rename),避免中途崩溃留半截 JSON。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{parse_err, Result, StoreError, STORE_MANIFEST_INVALID, STORE_NOT_INSTALLED};

/// 库条目。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LibraryItem {
    pub id: String,
    pub name: String,
    pub sha256: String,
    pub size: u64,
    /// 扩展名(不带点,小写;可为空表示无扩展名)。
    pub ext: String,
    /// assetd 资产类型字符串(mesh/texture/...);无法识别 → "misc"。
    pub kind: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// 来源标注(I-7):"project" | "store:<sourceId>/<pkgId>" | "import"。
    pub source: String,
    pub added_at: String,
}

/// 索引文件形态。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LibraryIndex {
    #[serde(default)]
    items: Vec<LibraryItem>,
}

/// 个人资产库句柄(全内存索引 + 磁盘 blob)。
pub struct Library {
    root: PathBuf,
    items: Vec<LibraryItem>,
}

impl Library {
    /// 打开(缺目录自动建;索引缺文件 = 空库)。
    pub fn open(data_dir: &Path) -> Result<Self> {
        let root = data_dir.join("store").join("library");
        std::fs::create_dir_all(root.join("blobs"))?;
        let index_path = root.join("index.json");
        let items = match std::fs::read_to_string(&index_path) {
            Ok(text) => {
                serde_json::from_str::<LibraryIndex>(&text)
                    .map_err(|e| parse_err("资产库 index.json", e))?
                    .items
            }
            Err(_) => Vec::new(),
        };
        Ok(Library { root, items })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn index_path(&self) -> PathBuf {
        self.root.join("index.json")
    }

    /// blob 落盘路径(sha256 已由 add_* 保证形态,外部传入前请自验)。
    pub fn blob_path(&self, sha256: &str) -> PathBuf {
        if sha256.len() < 2 {
            return self.root.join("blobs").join("__invalid__");
        }
        self.root.join("blobs").join(&sha256[..2]).join(sha256)
    }

    pub fn list(&self) -> Vec<LibraryItem> {
        self.items.clone()
    }

    pub fn get(&self, id: &str) -> Option<LibraryItem> {
        self.items.iter().find(|i| i.id == id).cloned()
    }

    /// 从字节入库。同 sha256 已存在则复用 blob(不重复写);同 (name, ext, sha256)
    /// 重复入库只更新既有条目的标签/来源,不长出第二条。
    pub fn add_bytes(
        &mut self,
        name: &str,
        ext: &str,
        bytes: &[u8],
        source: &str,
        tags: &[String],
    ) -> Result<LibraryItem> {
        check_name(name)?;
        let ext = normalize_ext(ext)?;
        let sha = forge_util::hashutil::sha256_hex(bytes);
        let blob = self.blob_path(&sha);
        if !blob.is_file() {
            if let Some(parent) = blob.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&blob, bytes)?;
        }
        self.upsert(name, &ext, &sha, bytes.len() as u64, source, tags)
    }

    /// 从磁盘文件入库(分块算 hash + 复制,不把大文件整读进内存);ext 取自文件名。
    pub fn add_file(
        &mut self,
        name: &str,
        path: &Path,
        source: &str,
        tags: &[String],
    ) -> Result<LibraryItem> {
        check_name(name)?;
        if !path.is_file() {
            return Err(StoreError::new(
                STORE_MANIFEST_INVALID,
                format!("入库源文件不存在: {}", path.display()),
            ));
        }
        let ext = normalize_ext(path.extension().and_then(|e| e.to_str()).unwrap_or(""))?;
        let sha = forge_util::hashutil::sha256_file(path)?;
        let size = std::fs::metadata(path)?.len();
        let blob = self.blob_path(&sha);
        if !blob.is_file() {
            if let Some(parent) = blob.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(path, &blob)?;
        }
        self.upsert(name, &ext, &sha, size, source, tags)
    }

    /// 删条目。blob 仅在**无其他条目引用**时回收(引用计数)。
    pub fn remove(&mut self, id: &str) -> Result<()> {
        let idx = self
            .items
            .iter()
            .position(|i| i.id == id)
            .ok_or_else(|| {
                StoreError::new(STORE_NOT_INSTALLED, format!("资产库无此条目: {id}"))
            })?;
        let gone = self.items.remove(idx);
        let still_referenced = self.items.iter().any(|i| i.sha256 == gone.sha256);
        if !still_referenced {
            std::fs::remove_file(self.blob_path(&gone.sha256)).ok();
        }
        self.save()
    }

    /// 索引原子落盘。
    pub fn save(&self) -> Result<()> {
        let mut doc = LibraryIndex { items: self.items.clone() };
        doc.items.sort_by(|a, b| a.id.cmp(&b.id));
        crate::write_json_atomic(&self.index_path(), "资产库 index.json", &doc)
    }

    fn upsert(
        &mut self,
        name: &str,
        ext: &str,
        sha: &str,
        size: u64,
        source: &str,
        tags: &[String],
    ) -> Result<LibraryItem> {
        let id = item_id(name, ext, sha);
        let item = LibraryItem {
            id: id.clone(),
            name: name.to_string(),
            sha256: sha.to_string(),
            size,
            ext: ext.to_string(),
            kind: kind_of_ext(ext).to_string(),
            tags: tags.to_vec(),
            source: source.to_string(),
            added_at: forge_util::timeutil::utc_now_iso8601(),
        };
        match self.items.iter_mut().find(|i| i.id == id) {
            Some(old) => *old = item.clone(),
            None => self.items.push(item.clone()),
        }
        self.save()?;
        Ok(item)
    }
}

/// 条目 id:hash(name|ext|sha256) 取前 16 位——同名同内容重复入库幂等,
/// 同内容不同名则是两条(用户视角确实是两份素材)。
fn item_id(name: &str, ext: &str, sha: &str) -> String {
    let seed = format!("{name}\u{0}{ext}\u{0}{sha}");
    forge_util::hashutil::sha256_hex(seed.as_bytes())[..16].to_string()
}

/// 展示名校验:非空且不含路径分隔符/盘符(名字只是标签,但仍可能被上层拼进文件名)。
fn check_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        return Err(StoreError::new(STORE_MANIFEST_INVALID, "资产名不可空"));
    }
    if name.contains('/') || name.contains('\\') || name.contains(':') {
        return Err(StoreError::new(
            STORE_MANIFEST_INVALID,
            format!("资产名不可含路径分隔符或 `:`: {name}"),
        ));
    }
    Ok(())
}

/// 扩展名归一:去前导点、转小写;只允许 ASCII 字母数字(空 = 无扩展名)。
fn normalize_ext(ext: &str) -> Result<String> {
    let e = ext.trim_start_matches('.').to_ascii_lowercase();
    if !e.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(StoreError::new(
            STORE_MANIFEST_INVALID,
            format!("扩展名只允许 ASCII 字母数字: {ext}"),
        ));
    }
    Ok(e)
}

/// 扩展名 → assetd 资产类型字符串;不识别 → "misc"(如实标注,不硬塞成某类)。
fn kind_of_ext(ext: &str) -> &'static str {
    match assetd::AssetType::from_extension(ext) {
        Some((t, _)) => t.as_str(),
        None => "misc",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_temp_dir;

    /// blobs/ 下的文件总数(去重效果的直接证据)。
    fn blob_count(lib: &Library) -> usize {
        let mut n = 0;
        let blobs = lib.root().join("blobs");
        if let Ok(rd) = std::fs::read_dir(&blobs) {
            for shard in rd.flatten() {
                if shard.path().is_dir() {
                    n += std::fs::read_dir(shard.path()).map(|r| r.flatten().count()).unwrap_or(0);
                }
            }
        }
        n
    }

    #[test]
    fn dedups_blob_and_refcounts_on_remove() {
        let data = test_temp_dir("lib-dedup");
        let mut lib = Library::open(&data).unwrap();
        assert!(lib.list().is_empty());

        let bytes = b"same content bytes".to_vec();
        let a = lib.add_bytes("木箱", "png", &bytes, "project", &["道具".to_string()]).unwrap();
        let b = lib.add_bytes("酒桶", "png", &bytes, "import", &[]).unwrap();
        assert_eq!(a.sha256, b.sha256);
        assert_ne!(a.id, b.id, "同内容不同名 = 两条目");
        assert_eq!(lib.list().len(), 2);
        assert_eq!(blob_count(&lib), 1, "同 sha256 只落一份 blob");
        assert_eq!(a.kind, "texture");
        assert_eq!(a.tags, vec!["道具".to_string()]);
        assert_eq!(b.source, "import");
        assert!(lib.blob_path(&a.sha256).is_file());

        // 幂等:同名同内容再入库不长第二条。
        let a2 = lib.add_bytes("木箱", "png", &bytes, "project", &[]).unwrap();
        assert_eq!(a2.id, a.id);
        assert_eq!(lib.list().len(), 2);

        // 删一条 → blob 仍在(另一条引用)。
        lib.remove(&a.id).unwrap();
        assert_eq!(lib.list().len(), 1);
        assert!(lib.blob_path(&b.sha256).is_file(), "尚有引用,blob 不得回收");
        assert_eq!(blob_count(&lib), 1);

        // 删光 → blob 回收。
        lib.remove(&b.id).unwrap();
        assert!(lib.list().is_empty());
        assert_eq!(blob_count(&lib), 0, "无引用后 blob 须回收");
        // 删不存在的条目 → 显式错误。
        assert_eq!(lib.remove(&b.id).unwrap_err().code, STORE_NOT_INSTALLED);

        std::fs::remove_dir_all(&data).ok();
    }

    #[test]
    fn persists_across_reopen_and_reads_file_source() {
        let data = test_temp_dir("lib-persist");
        let src_dir = data.join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        let f = src_dir.join("chair.gltf");
        std::fs::write(&f, b"{\"asset\":{\"version\":\"2.0\"}}").unwrap();

        let mut lib = Library::open(&data).unwrap();
        let item = lib.add_file("餐椅", &f, "store:official/acme.props", &["家具".into()]).unwrap();
        assert_eq!(item.ext, "gltf");
        assert_eq!(item.kind, "mesh");
        assert_eq!(item.size, std::fs::metadata(&f).unwrap().len());
        assert_eq!(item.sha256, forge_util::hashutil::sha256_file(&f).unwrap());
        assert_eq!(std::fs::read(lib.blob_path(&item.sha256)).unwrap(), std::fs::read(&f).unwrap());

        // 重开句柄索引仍在。
        let lib2 = Library::open(&data).unwrap();
        assert_eq!(lib2.list().len(), 1);
        assert_eq!(lib2.get(&item.id).unwrap().name, "餐椅");
        assert!(lib2.get("no-such-id").is_none());

        std::fs::remove_dir_all(&data).ok();
    }

    #[test]
    fn rejects_bad_name_ext_and_missing_file() {
        let data = test_temp_dir("lib-bad");
        let mut lib = Library::open(&data).unwrap();
        for bad in ["", "  ", "a/b", "a\\b", "C:x"] {
            let e = lib.add_bytes(bad, "png", b"x", "import", &[]).unwrap_err();
            assert_eq!(e.code, STORE_MANIFEST_INVALID, "名字 {bad:?} 应被拒");
        }
        assert_eq!(
            lib.add_bytes("x", "p n g", b"x", "import", &[]).unwrap_err().code,
            STORE_MANIFEST_INVALID
        );
        assert_eq!(
            lib.add_file("x", &data.join("no-such.png"), "import", &[]).unwrap_err().code,
            STORE_MANIFEST_INVALID
        );
        // 无扩展名合法,kind 如实标 misc(不硬塞成某类)。
        let it = lib.add_bytes("readme", "", b"x", "import", &[]).unwrap();
        assert_eq!(it.ext, "");
        assert_eq!(it.kind, "misc");
        // 带点扩展名归一。
        let it = lib.add_bytes("s", ".RX", b"y", "import", &[]).unwrap();
        assert_eq!(it.ext, "rx");
        assert_eq!(it.kind, "script");
        std::fs::remove_dir_all(&data).ok();
    }
}
