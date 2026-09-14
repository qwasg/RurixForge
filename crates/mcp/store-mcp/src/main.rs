//! store-mcp — F11 资产商店 MCP 服务器(stdio NDJSON JSON-RPC)。
//!
//! 内嵌 forge-store 库(无 supervisor,库非进程)。两个根不能合并:
//! - `--project <dir>`:资产落地根(Content/ 与 .forge/store/installed.json),缺省 `<workspace>/projects/demo`;
//! - `--workspace <dir>`:官方源 `registry/`、skill 落地根 `skills/`、源配置 `data/store-sources.json` 的锚,
//!   缺省 = CARGO_MANIFEST_DIR 上三级。

mod mcp;

use std::path::PathBuf;

fn main() {
    let mut project_root: Option<PathBuf> = None;
    let mut workspace_root: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--project" => project_root = args.next().map(PathBuf::from),
            "--workspace" => workspace_root = args.next().map(PathBuf::from),
            _ => {}
        }
    }
    // CARGO_MANIFEST_DIR = crates/mcp/store-mcp;上三级 = workspace 根。
    let ws = workspace_root.unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("CARGO_MANIFEST_DIR 上三级须存在")
            .to_path_buf()
    });
    let project = project_root.unwrap_or_else(|| ws.join("projects").join("demo"));
    mcp::serve_stdio(project, ws);
}
