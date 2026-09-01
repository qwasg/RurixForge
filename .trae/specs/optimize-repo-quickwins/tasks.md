# Tasks

- [x] Task 1: Rust timeutil 共享化——新建 `crates/forge-util`（Cargo.toml + src/lib.rs + src/timeutil.rs，含 `utc_now_iso8601`/`unix_millis` 与合并单测）；根 `Cargo.toml` workspace members +1；engine-host / gend / engine-scene-mcp 三 crate 删除本地 `timeutil.rs`、改 `mod` 引用为 crate 依赖、Cargo.toml 加 `forge-util = { path = ... }`；验证 `cargo test --workspace` 全绿（269 passed / 0 failed；gend 侧以 `pub use forge_util::timeutil;` 重导出保住 gen-image-mcp/gen-model-mcp 的 `gend::timeutil` 路径）
- [x] Task 2: 删除 events.rs 死 builder——`crates/forge-agentd/src/events.rs` 删 `actor()`/`correlation()` 及两处 `#[allow(dead_code)]`；`cargo test -p forge-agentd` 112 passed / 0 failed，无新警告
- [x] Task 3: BottomPanel useMemo 化——LogsView/OutputView 的 slice+reverse 与 MetricsView 的 reduce/filter 派生统计改 `useMemo`（依赖对应 store 切片引用）；`pnpm --filter @forge/client test` 246/246 + `pnpm -r typecheck` 全绿（中途命中 IDE 不落盘坑，Write 重写后 Select-String 核验落盘）
- [x] Task 4: 脚本归档与 gitignore——8 个零引用脚本（`_f1w3_upstream_patch.ps1`~`patch7`、`_clean_mock.ps1`，后者实测在仓库根）移至 `scripts/archive/`；`.gitignore` 删 `apps/ide/` 行；grep 复核零活引用（F1 契约 L206 为历史签署行，按约束仅报告未改）、`_f1w4_upstream_patch8.ps1` 原位未动；衍生发现 `apps/ide/out/...app.asar` 残留被进程占用删不掉（RD-F0-001 复发，Trae 重启后补删）
- [x] Task 5: 聚合门——`pnpm -r typecheck`（4 包 exit 0）、`pnpm -r test`（client 246/246 + host 27/27 + protocol，exit 0）、`pnpm -r build`（exit 0）、`cargo test --workspace`（269/0）、`go -C gateway-go test ./...`（ok，exit 0）五路全绿；checklist 逐项核验

# Task Dependencies

- [Task 1] [Task 2] [Task 3] [Task 4] 相互独立，可并行
- [Task 5] depends on [Task 1] [Task 2] [Task 3] [Task 4]
