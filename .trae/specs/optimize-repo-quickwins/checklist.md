# Checklist

- [x] `crates/forge-util` 存在且含 `utc_now_iso8601`/`unix_millis` 与合并单测；根 `Cargo.toml` members 含 `crates/forge-util`（Test-Path True×2 + Cargo.toml:5 命中）
- [x] `crates/engine-host/src/timeutil.rs`、`crates/gend/src/timeutil.rs`、`crates/mcp/engine-scene-mcp/src/timeutil.rs` 三文件已删除，调用点改用 `forge_util::timeutil`（Test-Path False×3；gend 以 `pub use` 重导出兼容 gen-image-mcp/gen-model-mcp）
- [x] `cargo test --workspace` 全绿（269 passed / 0 failed，含 forge-util 3 单测 known_timestamps/now_is_well_formed/unix_millis 冒烟）
- [x] `forge-agentd/src/events.rs` 中 `actor()`/`correlation()` 与其 `#[allow(dead_code)]` 已删除（Select-String 零命中）；`cargo test -p forge-agentd` 112/0 无新 dead_code 警告
- [x] `BottomPanel.tsx` LogsView/OutputView/MetricsView 派生数据已 useMemo 化，依赖数组为对应 store 切片引用（5 处 useMemo 落盘核验）
- [x] `pnpm --filter @forge/client test`（30 文件 246/246）与 `pnpm -r typecheck`（4 包 exit 0）全绿
- [x] 8 个一次性脚本位于 `scripts/archive/`（Get-ChildItem 实测 8 文件），原 `scripts/` 顶层不再有 `_f1w3_*`；`_f1w4_upstream_patch8.ps1` 保留原位（Test-Path True）
- [x] `.gitignore` 不再含 `apps/ide/` 条目（Select-String 零命中）
- [x] 聚合门五路全绿：typecheck exit 0 / test client 246+host 27 exit 0 / build exit 0 / cargo 269-0 / go ok exit 0，数字均来自命令输出
