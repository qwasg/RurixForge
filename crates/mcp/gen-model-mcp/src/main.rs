//! gen-model-mcp — Forge 生成网格 MCP 服务器(stdio NDJSON JSON-RPC,05 §8)。
//!
//! 内嵌 gend 库(无 supervisor,库非进程);`--project <dir>` 指定项目根,
//! 缺省 = <workspace>/projects/demo(照 gen-image-mcp 先例)。

mod mcp;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use assetd::project::ForgeProject;

fn main() {
    let mut project_root: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--project" {
            project_root = args.next().map(PathBuf::from);
        }
    }
    let root = project_root.unwrap_or_else(|| {
        // CARGO_MANIFEST_DIR = crates/mcp/gen-model-mcp;上三级 = workspace 根。
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("CARGO_MANIFEST_DIR 上三级须存在")
            .join("projects")
            .join("demo")
    });
    let project = ForgeProject::load(&root).unwrap_or_else(|e| {
        eprintln!("[gen-model-mcp] forge.toml 加载失败: {e};用缺省配置");
        ForgeProject::with_defaults(root.clone())
    });
    let proj = Arc::new(Mutex::new(project));
    mcp::serve_stdio(proj);
}
