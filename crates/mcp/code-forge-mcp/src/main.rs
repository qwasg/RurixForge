//! code-forge-mcp 入口:stdio NDJSON JSON-RPC 服务。
//! F4 wave.2:`--project <dir>` 指定项目根(照 asset-pipeline-mcp 先例),
//! 缺省 = <workspace>/projects/demo。
//! F4 wave.4:env FORGE_CODE_FORGE_PROJECT 兜底项目根(优先级低于 --project;照
//! FORGE_RX_CLI/FORGE_RURIXC env 先例)——agentd 无参 spawn 时供栈级冒烟指向临时项目。

use std::path::PathBuf;

fn main() {
    // 解析 --project 参数。
    let mut project_root: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--project" {
            project_root = args.next().map(PathBuf::from);
        }
    }
    let root = project_root
        .or_else(|| {
            std::env::var("FORGE_CODE_FORGE_PROJECT")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| {
            // CARGO_MANIFEST_DIR = crates/mcp/code-forge-mcp;上三级 = workspace 根。
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(3)
                .expect("CARGO_MANIFEST_DIR 上三级须存在")
                .join("projects")
                .join("demo")
        });
    code_forge_mcp::mcp::serve_stdio(&root);
}
