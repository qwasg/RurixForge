//! Lossless runtime pages derived from the existing native raster_crop result.
use crate::sentinels_v6_assets::Frame;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::SystemTime,
};
const INDEX: &str = "Content/Animations/v6/runtime-frames/index.json";
const DECODED_CAPACITY_BYTES: usize = 1024 * 1024 * 1024;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Index {
    schema_version: u32,
    format: String,
    pub atlases: BTreeMap<String, Atlas>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Atlas {
    pub metadata: String,
    source_atlas_sha256: String,
    source_metadata_sha256: String,
    source_atlas_bytes: u64,
    source_metadata_bytes: u64,
    pub width: u32,
    pub height: u32,
    frame_count: usize,
    pub frames: Vec<Page>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Page {
    pub index: usize,
    pub bbox: [u32; 4],
    pub tile_size: u32,
    pub sample_size: [u32; 2],
    path: String,
    sha256: String,
    rgba_sha256: String,
}
#[derive(Clone, PartialEq, Eq)]
struct Stamp {
    modified: SystemTime,
    bytes: u64,
}
fn stamp(path: &Path) -> Result<Stamp, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Stamp {
        modified: meta.modified().map_err(|e| e.to_string())?,
        bytes: meta.len(),
    })
}
#[derive(Default)]
struct Cache {
    indexes: BTreeMap<PathBuf, (Stamp, Arc<Index>)>,
    hashes: BTreeMap<PathBuf, (Stamp, String)>,
    pages: DecodedPages,
}
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PageKey {
    path: PathBuf,
    png_sha: String,
    rgba_sha: String,
    atlas_sha: String,
    metadata_sha: String,
}
struct DecodedPage {
    stamp: Stamp,
    pixels: Arc<Vec<u8>>,
    used: u64,
    atlas: String,
}
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct AtlasCounts {
    requests: u64,
    hits: u64,
    decodes: u64,
    evictions: u64,
}
struct DecodedPages {
    capacity: usize,
    bytes: usize,
    peak: usize,
    clock: u64,
    requests: u64,
    hits: u64,
    decodes: u64,
    evictions: u64,
    invalidations: u64,
    entries: BTreeMap<PageKey, DecodedPage>,
    by_atlas: BTreeMap<String, AtlasCounts>,
}
impl Default for DecodedPages {
    fn default() -> Self {
        Self::new(DECODED_CAPACITY_BYTES)
    }
}
impl DecodedPages {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            bytes: 0,
            peak: 0,
            clock: 0,
            requests: 0,
            hits: 0,
            decodes: 0,
            evictions: 0,
            invalidations: 0,
            entries: BTreeMap::new(),
            by_atlas: BTreeMap::new(),
        }
    }
    fn remove(&mut self, key: &PageKey, evicted: bool) {
        if let Some(old) = self.entries.remove(key) {
            self.bytes -= old.pixels.len();
            if evicted {
                self.evictions += 1;
                self.by_atlas.entry(old.atlas).or_default().evictions += 1;
            }
        }
    }
    fn get(&mut self, key: &PageKey, stamp: &Stamp, atlas: &str) -> Option<Arc<Vec<u8>>> {
        self.requests += 1;
        self.clock = self.clock.wrapping_add(1);
        self.by_atlas.entry(atlas.into()).or_default().requests += 1;
        if self.entries.get(key).is_some_and(|old| old.stamp != *stamp) {
            self.remove(key, false);
            self.invalidations += 1;
        }
        let entry = self.entries.get_mut(key)?;
        entry.used = self.clock;
        self.hits += 1;
        self.by_atlas.entry(atlas.into()).or_default().hits += 1;
        Some(Arc::clone(&entry.pixels))
    }
    fn insert(&mut self, key: PageKey, stamp: Stamp, atlas: &str, pixels: Vec<u8>) -> Arc<Vec<u8>> {
        self.decodes += 1;
        self.by_atlas.entry(atlas.into()).or_default().decodes += 1;
        let pixels = Arc::new(pixels);
        if pixels.len() > self.capacity {
            return pixels;
        }
        self.remove(&key, false);
        while self.bytes + pixels.len() > self.capacity {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            self.remove(&oldest, true);
        }
        self.clock = self.clock.wrapping_add(1);
        self.bytes += pixels.len();
        self.peak = self.peak.max(self.bytes);
        self.entries.insert(
            key,
            DecodedPage {
                stamp,
                pixels: Arc::clone(&pixels),
                used: self.clock,
                atlas: atlas.into(),
            },
        );
        pixels
    }
}
static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
pub fn metrics() -> serde_json::Value {
    let cache = CACHE
        .get_or_init(|| Mutex::new(Cache::default()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let p = &cache.pages;
    serde_json::json!({"scope":"Source-validated registered-page requests; decoded pages are lazy resident exact RGBA. Counter resets with asset cache clear.","capacityBytes":p.capacity,"residentBytes":p.bytes,"peakResidentBytes":p.peak,"cachedPages":p.entries.len(),"requests":p.requests,"hits":p.hits,"decodes":p.decodes,"evictions":p.evictions,"invalidations":p.invalidations,"byAtlas":p.by_atlas})
}
pub fn clear() {
    if let Some(cache) = CACHE.get() {
        *cache.lock().unwrap_or_else(|e| e.into_inner()) = Cache::default();
    }
}
fn file_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("derived page path must stay inside the project".into());
    }
    Ok(root.join(path))
}
pub(crate) fn load_index(root: &Path) -> Result<Option<Arc<Index>>, String> {
    let path = root.join(INDEX);
    let meta = match std::fs::metadata(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let stat = Stamp {
        modified: meta.modified().map_err(|e| e.to_string())?,
        bytes: meta.len(),
    };
    let mut cache = CACHE
        .get_or_init(|| Mutex::new(Cache::default()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some((old, index)) = cache.indexes.get(&path) {
        if old == &stat {
            return Ok(Some(Arc::clone(index)));
        }
    }
    let index: Index = serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?)
        .map_err(|e| format!("native runtime page index: {e}"))?;
    if index.schema_version != 1 || index.format != "native-raster-pages-png-v1" {
        return Err("unsupported native runtime page index".into());
    }
    for (atlas, entry) in &index.atlases {
        file_path(root, atlas)?;
        file_path(root, &entry.metadata)?;
        if entry.frame_count != entry.frames.len() || entry.width == 0 || entry.height == 0 {
            return Err("invalid derived atlas dimensions/count".into());
        }
        for (i, p) in entry.frames.iter().enumerate() {
            file_path(root, &p.path)?;
            if p.index != i
                || !matches!(p.tile_size, 256 | 512)
                || p.sample_size.iter().any(|n| *n == 0 || *n > p.tile_size)
                || p.bbox[2] == 0
                || p.bbox[3] == 0
                || p.bbox[0]
                    .checked_add(p.bbox[2])
                    .is_none_or(|x| x > entry.width)
                || p.bbox[1]
                    .checked_add(p.bbox[3])
                    .is_none_or(|y| y > entry.height)
            {
                return Err("invalid native derived frame geometry".into());
            }
        }
    }
    let index = Arc::new(index);
    cache.indexes.insert(path, (stat, Arc::clone(&index)));
    Ok(Some(index))
}
fn verify_source(
    root: &Path,
    relative: &str,
    expected_bytes: u64,
    expected_sha: &str,
) -> Result<(), String> {
    let path = file_path(root, relative)?;
    let stat = stamp(&path)?;
    if stat.bytes != expected_bytes {
        return Err(format!(
            "derived source changed: {relative}; regenerate native pages"
        ));
    }
    let mut cache = CACHE
        .get_or_init(|| Mutex::new(Cache::default()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let hash = if let Some((_, hash)) = cache.hashes.get(&path).filter(|(old, _)| old == &stat) {
        hash.clone()
    } else {
        let mut file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
        let mut digest = Sha256::new();
        let mut chunk = [0u8; 65536];
        loop {
            let n = file.read(&mut chunk).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            digest.update(&chunk[..n]);
        }
        let hash = format!("{:x}", digest.finalize());
        cache.hashes.insert(path, (stat, hash.clone()));
        hash
    };
    if hash != expected_sha {
        return Err(format!(
            "derived source hash differs: {relative}; regenerate native pages"
        ));
    }
    Ok(())
}
fn page_matches(page: &Page, frame: &Frame) -> bool {
    page.index == frame.index
        && page.bbox == frame.bbox
        && page.tile_size == frame.tile_size
        && page.sample_size == frame.sample_size
}
pub(crate) fn pixels(root: &Path, frame: &Frame) -> Result<Option<Vec<u8>>, String> {
    let Some(index) = load_index(root)? else {
        return Ok(None);
    };
    let Some(atlas) = index.atlases.get(&frame.atlas) else {
        return Ok(None);
    };
    if atlas.metadata != frame.metadata {
        return Err("derived metadata path differs from the real asset".into());
    }
    let page = atlas
        .frames
        .get(frame.index)
        .filter(|p| page_matches(p, frame))
        .ok_or("derived page differs from the native raster geometry")?;
    verify_source(
        root,
        &frame.atlas,
        atlas.source_atlas_bytes,
        &atlas.source_atlas_sha256,
    )?;
    verify_source(
        root,
        &atlas.metadata,
        atlas.source_metadata_bytes,
        &atlas.source_metadata_sha256,
    )?;
    let path = file_path(root, &page.path)?;
    let page_stamp = stamp(&path)?;
    let key = PageKey {
        path: path.clone(),
        png_sha: page.sha256.clone(),
        rgba_sha: page.rgba_sha256.clone(),
        atlas_sha: atlas.source_atlas_sha256.clone(),
        metadata_sha: atlas.source_metadata_sha256.clone(),
    };
    {
        let mut cache = CACHE
            .get_or_init(|| Mutex::new(Cache::default()))
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(pixels) = cache.pages.get(&key, &page_stamp, &frame.atlas) {
            return Ok(Some(pixels.as_ref().clone()));
        }
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("derived PNG {}: {e}", page.path))?;
    let pixels = decode_page(&bytes, page)?;
    let cached = CACHE
        .get_or_init(|| Mutex::new(Cache::default()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .pages
        .insert(key, page_stamp, &frame.atlas, pixels);
    Ok(Some(cached.as_ref().clone()))
}
fn decode_page(bytes: &[u8], page: &Page) -> Result<Vec<u8>, String> {
    if format!("{:x}", Sha256::digest(bytes)) != page.sha256 {
        return Err("derived PNG checksum mismatch".into());
    }
    let (w, h, rgba) = assetd::texture::decode_rgba_bytes(&bytes).map_err(|e| e.to_string())?;
    if w != page.tile_size
        || h != page.tile_size
        || rgba.len() != page.tile_size as usize * page.tile_size as usize * 4
    {
        return Err("derived PNG is not its declared native page size".into());
    }
    if format!("{:x}", Sha256::digest(&rgba)) != page.rgba_sha256 {
        return Err("derived RGBA checksum mismatch".into());
    }
    Ok(rgba)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decoded_lru_reuses_exact_bytes_evicts_least_recent_and_invalidates_changed_pages() {
        let mut pages = DecodedPages::new(8);
        let stamp = Stamp {
            modified: SystemTime::UNIX_EPOCH,
            bytes: 4,
        };
        let key = |name: &str| PageKey {
            path: PathBuf::from(name),
            png_sha: "page".into(),
            rgba_sha: "rgba".into(),
            atlas_sha: "source".into(),
            metadata_sha: "meta".into(),
        };
        let (a, b, c) = (key("a"), key("b"), key("c"));
        assert!(pages.get(&a, &stamp, "atlas").is_none());
        pages.insert(a.clone(), stamp.clone(), "atlas", vec![1, 2, 0, 255]);
        pages.insert(b.clone(), stamp.clone(), "atlas", vec![3; 4]);
        assert_eq!(
            pages.get(&a, &stamp, "atlas").unwrap().as_ref(),
            &vec![1, 2, 0, 255]
        );
        pages.insert(c.clone(), stamp.clone(), "atlas", vec![4; 4]);
        assert!(pages.entries.contains_key(&a) && pages.entries.contains_key(&c));
        assert!(!pages.entries.contains_key(&b));
        assert_eq!(pages.bytes, 8);
        assert_eq!(pages.evictions, 1);
        let changed = Stamp {
            modified: SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1),
            bytes: 4,
        };
        assert!(pages.get(&a, &changed, "atlas").is_none());
        assert_eq!(pages.invalidations, 1);
        assert_eq!(pages.bytes, 4);
        let mut generation = c.clone();
        generation.atlas_sha = "new-source".into();
        assert!(pages.get(&generation, &stamp, "atlas").is_none());
        assert_eq!(pages.hits, 1);
        assert_eq!(pages.decodes, 3);
        assert_eq!(pages.peak, 8);
    }
    #[test]
    fn corrupted_or_reinterpreted_pages_fail_without_using_an_old_atlas() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../projects/code-sentinels");
        let index = load_index(&root).unwrap().unwrap();
        let page = &index.atlases["Content/Animations/v6/characters/claude.png"].frames[0];
        let original = std::fs::read(root.join(&page.path)).unwrap();
        let mut corrupt = original.clone();
        corrupt[40] ^= 1;
        assert!(decode_page(&corrupt, page)
            .unwrap_err()
            .contains("PNG checksum"));
        let mut wrong = page.clone();
        wrong.tile_size = if page.tile_size == 256 { 512 } else { 256 };
        assert!(decode_page(&original, &wrong)
            .unwrap_err()
            .contains("page size"));
        wrong = page.clone();
        wrong.rgba_sha256 = "0".repeat(64);
        assert!(decode_page(&original, &wrong)
            .unwrap_err()
            .contains("RGBA checksum"));
    }
    #[test]
    fn native_page_paths_cannot_leave_the_project() {
        let root = Path::new("project");
        for invalid in [
            "",
            "../outside.png",
            "Content/../outside.png",
            "/absolute.png",
            "C:/outside.png",
        ] {
            assert!(file_path(root, invalid).is_err(), "{invalid}");
        }
        assert_eq!(
            file_path(root, "Content/Animations/v6/runtime-frames/page.png").unwrap(),
            root.join("Content/Animations/v6/runtime-frames/page.png")
        );
    }
}
