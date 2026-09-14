//! 多源聚合与源配置持久化。
//!
//! 配置落 `<data>/store-sources.json`(引擎级,非项目级;原子写 tmp+rename)。
//! token 不在其中——`SourceConfig::token` 是 `#[serde(skip)]` 字段,聚合时由调用方
//! 经 `token_of` 回调从 keystore 注入(R-5:密钥只有一个存放处)。
//!
//! **诚实纪律(I-5)**:`search_all` 遇到不可达的源**不静默跳过**,把
//! `(sourceId, StoreError)` 收进 `errors` 与命中结果一并返回,由上层如实展示
//! 「这 N 个源没查到,原因是……」,而不是让用户误以为源上确实没有这个包。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::source::{open_source, path_to_file_url, PackageSummary, SearchQuery, SourceConfig};
use crate::{Result, StoreError};

/// 源配置文件名。
pub const SOURCES_FILE: &str = "store-sources.json";
/// 内置官方源 id。
pub const OFFICIAL_SOURCE_ID: &str = "official";

/// 源清单(落盘形态)。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcesConfig {
    #[serde(default)]
    pub sources: Vec<SourceConfig>,
}

impl SourcesConfig {
    pub fn find(&self, id: &str) -> Option<&SourceConfig> {
        self.sources.iter().find(|s| s.id == id)
    }

    /// 读-改-写单条(保留其他条目;照 `gend::config::upsert_entry` 体例)。
    pub fn upsert(&mut self, entry: SourceConfig) {
        match self.sources.iter_mut().find(|s| s.id == entry.id) {
            Some(old) => *old = entry,
            None => self.sources.push(entry),
        }
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.sources.len();
        self.sources.retain(|s| s.id != id);
        self.sources.len() != before
    }
}

/// `<data>/store-sources.json`。
pub fn sources_config_path(data_dir: &Path) -> PathBuf {
    data_dir.join(SOURCES_FILE)
}

/// 内置官方源:`<workspace>/registry` 的 `file://` 形态。
/// 本波官方源就是仓内目录(离线可用、可审计);上线后改 baseUrl 即切远程,源实现不动。
pub fn default_official_source(workspace_root: &Path) -> SourceConfig {
    SourceConfig {
        id: OFFICIAL_SOURCE_ID.to_string(),
        name: "官方源".to_string(),
        base_url: path_to_file_url(&workspace_root.join("registry")),
        enabled: true,
        token: None,
    }
}

/// 读源清单。缺文件 = 内置官方源一条(首次启动即可用);解析失败同样回落到内置默认,
/// 并经 stderr 如实提示(照 `gend::config::load_from` 纪律:不静默把用户配置当空)。
///
/// 注:`data_dir` 惯例是 `<workspace>/data`,故内置默认源的 workspace 根取其父目录。
pub fn load_sources(data_dir: &Path) -> SourcesConfig {
    let workspace_root = data_dir.parent().unwrap_or(data_dir);
    let builtin = || SourcesConfig { sources: vec![default_official_source(workspace_root)] };
    let path = sources_config_path(data_dir);
    match std::fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str::<SourcesConfig>(&text) {
            Ok(cfg) if cfg.sources.is_empty() => builtin(),
            Ok(cfg) => cfg,
            Err(e) => {
                eprintln!(
                    "[forge-store] {} 解析失败({e}),按内置默认源处理",
                    path.display()
                );
                builtin()
            }
        },
        Err(_) => builtin(),
    }
}

/// 写源清单(原子写;token 字段不落盘)。
pub fn save_sources(data_dir: &Path, cfg: &SourcesConfig) -> Result<()> {
    crate::write_json_atomic(&sources_config_path(data_dir), SOURCES_FILE, cfg)
}

/// 聚合搜索结果:命中项带来源 id;失败源如实列出,**不与「无结果」混为一谈**。
#[derive(Debug, Default)]
pub struct AggregatedSearch {
    pub items: Vec<(String, PackageSummary)>,
    pub errors: Vec<(String, StoreError)>,
}

impl AggregatedSearch {
    /// 是否存在失败源(上层据此决定是否给出「结果可能不完整」提示)。
    pub fn is_partial(&self) -> bool {
        !self.errors.is_empty()
    }
}

/// 跨全部**已启用**源搜索。
///
/// 去重:同一包 id 只保留首个命中的源(源清单顺序 = 优先级,官方源在前)。
/// 排序:按包 id 升序,保证同一查询多次调用结果稳定。
/// `token_of`:按源 id 取私有源令牌(通常包一层 keystore;返回 None = 匿名访问)。
pub fn search_all(
    cfg: &SourcesConfig,
    q: &SearchQuery,
    token_of: &dyn Fn(&str) -> Option<String>,
) -> AggregatedSearch {
    let mut out = AggregatedSearch::default();
    let mut seen: Vec<String> = Vec::new();
    for s in cfg.sources.iter().filter(|s| s.enabled) {
        let with_token = SourceConfig { token: token_of(&s.id), ..s.clone() };
        let src = match open_source(&with_token) {
            Ok(src) => src,
            Err(e) => {
                out.errors.push((s.id.clone(), e));
                continue;
            }
        };
        match src.search(q) {
            Ok(page) => {
                for item in page.items {
                    if seen.iter().any(|id| id == &item.id) {
                        continue;
                    }
                    seen.push(item.id.clone());
                    out.items.push((s.id.clone(), item));
                }
            }
            Err(e) => out.errors.push((s.id.clone(), e)),
        }
    }
    out.items.sort_by(|a, b| a.1.id.cmp(&b.1.id));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{FileEntry, PackageKind, PackageManifest};
    use crate::source::{FileSource, RegistrySource};
    use crate::test_temp_dir;
    use crate::STORE_SOURCE_UNREACHABLE;

    /// 造一个含若干包的临时 FileSource。
    fn seed(dir: &Path, source_name: &str, pkgs: &[(&str, &str)]) {
        let src = FileSource::new(source_name, dir.to_path_buf());
        for (id, name) in pkgs {
            let bytes = format!("blob of {id}").into_bytes();
            let sha = forge_util::hashutil::sha256_hex(&bytes);
            let mut m = PackageManifest::minimal(id, name, "1.0.0", PackageKind::AssetPack);
            m.description = format!("{name} 的说明");
            m.files = vec![FileEntry { path: "a.rx".into(), sha256: sha.clone(), size: bytes.len() as u64 }];
            src.publish(&m, &[(sha, bytes)]).unwrap();
        }
    }

    fn cfg_of(entries: &[(&str, &Path)]) -> SourcesConfig {
        SourcesConfig {
            sources: entries
                .iter()
                .map(|(id, p)| SourceConfig {
                    id: (*id).to_string(),
                    name: (*id).to_string(),
                    base_url: path_to_file_url(p),
                    enabled: true,
                    token: None,
                })
                .collect(),
        }
    }

    #[test]
    fn config_roundtrip_and_defaults() {
        let data = test_temp_dir("reg-cfg");
        // 缺文件 = 内置官方源一条。
        let cfg = load_sources(&data);
        assert_eq!(cfg.sources.len(), 1);
        assert_eq!(cfg.sources[0].id, OFFICIAL_SOURCE_ID);
        assert!(cfg.sources[0].base_url.starts_with("file://"));
        assert!(cfg.sources[0].base_url.ends_with("/registry"));

        // 写-读回环 + token 不落盘。
        let mut cfg = SourcesConfig::default();
        cfg.upsert(SourceConfig {
            id: "priv".into(),
            name: "私有源".into(),
            base_url: "https://priv.example.com".into(),
            enabled: false,
            token: Some("tok-SECRET".into()),
        });
        save_sources(&data, &cfg).unwrap();
        let text = std::fs::read_to_string(sources_config_path(&data)).unwrap();
        assert!(!text.contains("tok-SECRET"), "token 落盘(R-5): {text}");
        let back = load_sources(&data);
        assert_eq!(back.sources.len(), 1);
        assert_eq!(back.find("priv").unwrap().name, "私有源");
        assert!(!back.find("priv").unwrap().enabled);
        assert!(back.find("priv").unwrap().token.is_none());

        // upsert 覆盖同 id;remove 生效。
        let mut back = back;
        back.upsert(SourceConfig {
            id: "priv".into(),
            name: "私有源 v2".into(),
            base_url: "https://priv2.example.com".into(),
            enabled: true,
            token: None,
        });
        assert_eq!(back.sources.len(), 1);
        assert_eq!(back.find("priv").unwrap().name, "私有源 v2");
        assert!(back.remove("priv"));
        assert!(!back.remove("priv"));

        // 坏 JSON → 回落内置默认(如实 stderr,不当成空源清单)。
        std::fs::write(sources_config_path(&data), "{ not json").unwrap();
        assert_eq!(load_sources(&data).sources[0].id, OFFICIAL_SOURCE_ID);
        std::fs::remove_dir_all(&data).ok();
    }

    #[test]
    fn search_all_dedups_sorts_and_reports_failures() {
        let a = test_temp_dir("reg-a");
        let b = test_temp_dir("reg-b");
        seed(&a, "srcA", &[("acme.props", "中世纪道具"), ("zz.last", "末位包")]);
        // srcB 也有 acme.props(重名,应被 srcA 的条目盖住)+ 一个独有包。
        seed(&b, "srcB", &[("acme.props", "道具镜像"), ("bb.tools", "工具集")]);

        let cfg = cfg_of(&[("srcA", &a), ("srcB", &b)]);
        let agg = search_all(&cfg, &SearchQuery::new(""), &|_| None);
        assert!(!agg.is_partial(), "两源均可达时不应有错误: {:?}", agg.errors);
        let ids: Vec<&str> = agg.items.iter().map(|(_, s)| s.id.as_str()).collect();
        assert_eq!(ids, vec!["acme.props", "bb.tools", "zz.last"], "须按 id 升序且去重");
        let (owner, hit) = agg.items.iter().find(|(_, s)| s.id == "acme.props").unwrap();
        assert_eq!(owner, "srcA", "重名包由靠前的源胜出");
        assert_eq!(hit.name, "中世纪道具");

        // 关键词过滤仍跨源生效。
        let agg = search_all(&cfg, &SearchQuery::new("工具"), &|_| None);
        assert_eq!(agg.items.len(), 1);
        assert_eq!(agg.items[0].0, "srcB");

        // 一个源指向不存在目录:errors 里有它,另一源结果照常返回(不静默跳过)。
        let mut broken = cfg_of(&[("dead", &a.join("no-such-dir")), ("srcB", &b)]);
        broken.sources[0].base_url = path_to_file_url(&a.join("no-such-dir"));
        let agg = search_all(&broken, &SearchQuery::new(""), &|_| None);
        assert!(agg.is_partial());
        assert_eq!(agg.errors.len(), 1);
        assert_eq!(agg.errors[0].0, "dead");
        assert_eq!(agg.errors[0].1.code, STORE_SOURCE_UNREACHABLE);
        let ids: Vec<&str> = agg.items.iter().map(|(_, s)| s.id.as_str()).collect();
        assert_eq!(ids, vec!["acme.props", "bb.tools"], "健康源的结果须照常返回");

        // 禁用的源直接不参与(既不出结果也不报错)。
        let mut disabled = cfg_of(&[("srcA", &a), ("srcB", &b)]);
        disabled.sources[1].enabled = false;
        let agg = search_all(&disabled, &SearchQuery::new(""), &|_| None);
        assert!(agg.errors.is_empty());
        assert_eq!(agg.items.len(), 2);

        // 无法识别的 baseUrl 也进 errors(而不是被吞)。
        let bad = SourcesConfig {
            sources: vec![SourceConfig {
                id: "weird".into(),
                name: "weird".into(),
                base_url: "ftp://nope".into(),
                enabled: true,
                token: None,
            }],
        };
        let agg = search_all(&bad, &SearchQuery::new(""), &|_| None);
        assert_eq!(agg.errors.len(), 1);
        assert_eq!(agg.errors[0].1.code, crate::STORE_SOURCE_NOT_FOUND);

        std::fs::remove_dir_all(&a).ok();
        std::fs::remove_dir_all(&b).ok();
    }

    #[test]
    fn token_callback_is_consulted_per_source() {
        let a = test_temp_dir("reg-tok");
        seed(&a, "srcA", &[("acme.props", "道具")]);
        let cfg = cfg_of(&[("srcA", &a)]);
        let seen = std::sync::Mutex::new(Vec::<String>::new());
        let agg = search_all(&cfg, &SearchQuery::new(""), &|id| {
            seen.lock().unwrap().push(id.to_string());
            None
        });
        assert_eq!(agg.items.len(), 1);
        assert_eq!(*seen.lock().unwrap(), vec!["srcA".to_string()], "每个启用源须问一次 token");
        std::fs::remove_dir_all(&a).ok();
    }
}
