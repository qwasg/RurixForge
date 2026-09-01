//! asset-cleanup 检测面(06 §3 asset-cleanup / 08 §5):扫描 Content/ 产出 dryRun 整理提案。
//!
//! 三类检测(确定性、可复现):
//! - misplaced:资产类型与所在目录不符(按 08 §3.1 七目录映射)。
//! - naming:文件名含空格/括号(移动时同步改名;GUID 不变,引用不断链)。
//! - orphan:非场景资产且 referencedBy 为空(仅报告,不自动删除——删除走 asset_delete 双门)。
//!
//! 本模块只读 + 产出提案(dryRun);执行经 asset_move/asset_delete,Proposal 门在 agentd。

use crate::meta::MetaDoc;
use crate::project::ForgeProject;
use crate::refs::RefGraph;
use crate::{meta_path_for, AssetType, Result};

/// 一条整理建议。
#[derive(Debug, Clone)]
pub struct CleanupProposal {
    pub asset_path: String,
    pub guid: String,
    /// misplaced / naming / orphan。
    pub issue: String,
    /// 目标文件夹(misplaced = 类型应有目录;naming = 原目录;orphan = None)。
    pub dest_folder: Option<String>,
    /// 改名(naming 时 = 清洗后文件名;否则 None)。
    pub new_name: Option<String>,
    /// 人类可读原因。
    pub reason: String,
}

/// asset_cleanup_scan 返回。
#[derive(Debug, Clone)]
pub struct CleanupReport {
    pub scanned: usize,
    pub proposals: Vec<CleanupProposal>,
    /// 影响面统计(提案数按 issue 分类)。
    pub impact: Vec<(String, usize)>,
}

/// 类型应有目录(08 §3.1 七目录;AssetType → 目录名)。
fn expected_folder(atype: &str) -> Option<&'static str> {
    Some(match atype {
        "mesh" => "Meshes",
        "texture" => "Textures",
        "material" => "Materials",
        "prefab" => "Prefabs",
        "scene" => "Scenes",
        "script" => "Scripts",
        "audio" => "Audio",
        _ => return None,
    })
}

/// 文件名是否需要清洗(空格 / 中英文括号)。
fn needs_rename(file_name: &str) -> bool {
    file_name.contains(' ')
        || file_name.contains('(')
        || file_name.contains(')')
        || file_name.contains('(')
        || file_name.contains(')')
}

/// 清洗文件名:空格→下划线,去中英文括号;其他字符保留。
fn sanitize_name(file_name: &str) -> String {
    file_name
        .replace(' ', "_")
        .replace(['(', ')', '(', ')'], "")
}

/// 扫描项目产出整理提案(dryRun;不写任何文件)。
pub fn scan_cleanup(project: &ForgeProject) -> Result<CleanupReport> {
    let graph = RefGraph::rebuild(project)?;
    let mut proposals = Vec::new();
    let mut scanned = 0;

    for rel in project.scan_content()? {
        let meta_path = meta_path_for(&project.content_root(), &rel);
        if !meta_path.is_file() {
            continue;
        }
        let meta = MetaDoc::load(&meta_path)?;
        scanned += 1;

        let file_name = rel.rsplit('/').next().unwrap_or(&rel);
        let cur_folder = rel.rsplit_once('/').map(|(f, _)| f).unwrap_or("");

        // 1. misplaced:类型应有目录 ≠ 当前一级目录。
        if let Some(want) = expected_folder(&meta.atype) {
            if cur_folder != want {
                proposals.push(CleanupProposal {
                    asset_path: rel.clone(),
                    guid: meta.guid.clone(),
                    issue: "misplaced".into(),
                    dest_folder: Some(want.into()),
                    new_name: None,
                    reason: format!("{} 类型应在 {want}/,当前在 {cur_folder}/", meta.atype),
                });
            }
        }

        // 2. naming:文件名需清洗(目录不动,只改名)。
        if needs_rename(file_name) {
            proposals.push(CleanupProposal {
                asset_path: rel.clone(),
                guid: meta.guid.clone(),
                issue: "naming".into(),
                dest_folder: Some(cur_folder.to_string()),
                new_name: Some(sanitize_name(file_name)),
                reason: format!("文件名含空格/括号: {file_name}"),
            });
        }

        // 3. orphan:非场景资产且无入边(场景是入口根,不算孤儿)。
        if meta.atype != AssetType::Scene.as_str() && graph.referenced_by(&meta.guid).is_empty() {
            proposals.push(CleanupProposal {
                asset_path: rel.clone(),
                guid: meta.guid.clone(),
                issue: "orphan".into(),
                dest_folder: None,
                new_name: None,
                reason: "无任何资产/场景引用(仅报告;删除须 asset_delete + Proposal 双门)".into(),
            });
        }
    }

    let mut impact: Vec<(String, usize)> = Vec::new();
    for p in &proposals {
        if let Some(e) = impact.iter_mut().find(|(k, _)| *k == p.issue) {
            e.1 += 1;
        } else {
            impact.push((p.issue.clone(), 1));
        }
    }
    impact.sort_by(|a, b| a.0.cmp(&b.0));

    Ok(CleanupReport {
        scanned,
        proposals,
        impact,
    })
}
