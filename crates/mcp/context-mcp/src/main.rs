//! context-mcp — F10 语义索引/RAG 检索 MCP 服务器(stdio NDJSON JSON-RPC)。
//!
//! 内嵌 forge-index 库(无 supervisor,库非进程);`--project <dir>` 指定项目根,
//! 缺省 = <workspace>/projects/demo。`--docs <dir>` 指定文档腿根(递归 md/txt),
//! 缺省 = workspace 根(gend::config)——作用域波起由 agentd 按当前项目传入,
//! 免得 A 项目的 agent 检索到的全是仓库根的公共设计文档。

mod mcp;

use std::path::PathBuf;

fn main() {
    let mut project_root: Option<PathBuf> = None;
    let mut docs_root: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--project" => project_root = args.next().map(PathBuf::from),
            "--docs" => docs_root = args.next().map(PathBuf::from),
            _ => {}
        }
    }
    let root = project_root.unwrap_or_else(|| {
        // CARGO_MANIFEST_DIR = crates/mcp/context-mcp;上三级 = workspace 根。
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("CARGO_MANIFEST_DIR 上三级须存在")
            .join("projects")
            .join("demo")
    });
    mcp::serve_stdio(root, docs_root);
}
