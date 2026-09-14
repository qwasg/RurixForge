//! code-forge-mcp — Forge code-forge MCP 服务器(stdio NDJSON JSON-RPC)。
//!
//! F4 wave.1:五工具(rx_check/rx_build/rx_run/rx_fmt/rx_test)子进程包上游
//! rx CLI / rurixc(H:\rurix\target\debug);无 supervisor(rx CLI 非驻留进程)。
//! F4 wave.2:graph 三工具(graph_validate/graph_create/graph_get),校验器在
//! forge-logic(纯函数);图落 <project>/Content/Graphs/*.rxgraph。
//! F4 wave.4:code_* 三工具(code_symbol_search/code_references/code_structured_edit);
//! references 走 rurixc --tooling-server 常驻 LSP 会话(lspclient),符号表为文本扫描
//! (实测 --emit=reflection 仅 shader entry 且无 span,见 codetool 头注)。

pub mod codetool;
pub mod graphtool;
pub mod lspclient;
pub mod mcp;
pub mod rxtool;
