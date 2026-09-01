# 全项目确认正优化清理 Spec

## Why

F0–F8 交付后仓内积累了一批可实证的小包袱：三份逐字重复的 `timeutil.rs`、零调用的死 builder、失效的一次性脚本、`.gitignore` 残留条目、前端渲染热路径上未 memo 的派生数组。本 spec 由 agent-team 全项目检索 + 逐条读码核验产生，只收**确认是正优化**（行为保持、风险低、可验证）的项，一次性清掉。

## What Changes

- **Rust 去重**：`engine-host/src/timeutil.rs`、`gend/src/timeutil.rs`、`mcp/engine-scene-mcp/src/timeutil.rs` 三份逐字重复（Howard Hinnant 历法换算 + 各自单测）→ 抽共享 crate `crates/forge-util`（含 `utc_now_iso8601` + `unix_millis`，单测合一），三个消费方改为依赖引用。
- **Rust 死代码**：`forge-agentd/src/events.rs` 删除零调用 builder `actor()`（L155-159）与 `correlation()`（L161-165）及其 `#[allow(dead_code)]`（全仓 grep 零命中）。
- **前端 perf**：`packages/client/src/components/shell/BottomPanel.tsx` 的 `LogsView`/`OutputView`/`MetricsView` 每渲染重建派生数组（`slice(-120).reverse()` / `slice(-200).reverse()` / `reduce/filter`）→ `useMemo` 化（依赖 store 引用，行为不变）。
- **仓库卫生**：`scripts/` 下 8 个零引用一次性脚本（`_f1w3_upstream_patch.ps1` ~ `patch7`、`_clean_mock.ps1`，后者目标 `mock.ts` 已不存在）→ 移至 `scripts/archive/`；`.gitignore` 移除失效条目 `apps/ide/`（D-015 已删目录）。
- **明确不做**（诚实登记，留用户拍板）：`_f1w4_upstream_patch8.ps1`（F1 契约 L223 引用留档，保留原位）；`.tmp-frames/`（已 git tracked，清理涉 `git rm --cached`）；`shots/`（untracked 设计稿）；`llm.rs` `mode` 字段 `allow(dead_code)`（wire 刻意保留面，注释明示）；gateway-go（审计无确认项）。

## Impact

- Affected specs: 无既有门禁变更；纯卫生波
- Affected code:
  - 新增 `crates/forge-util/`（Cargo.toml + src/lib.rs + src/timeutil.rs）
  - `Cargo.toml`（workspace members +1）
  - `crates/engine-host/Cargo.toml`、`crates/gend/Cargo.toml`、`crates/mcp/engine-scene-mcp/Cargo.toml`（依赖切换）及三处 `mod timeutil` 引用点
  - `crates/forge-agentd/src/events.rs`（-12 行）
  - `packages/client/src/components/shell/BottomPanel.tsx`（useMemo）
  - `scripts/` 8 文件移至 `scripts/archive/`、`.gitignore` -1 行
- 验证门禁：根门禁 `pnpm -r typecheck / test / build` 全绿 + `cargo test --workspace` 全绿 + `go -C gateway-go test ./...` 全绿

## ADDED Requirements

### Requirement: timeutil 共享化

系统 SHALL 将三份逐字重复的 `timeutil.rs` 收敛为 workspace 共享 crate `forge-util`，对外提供 `utc_now_iso8601() -> String` 与 `unix_millis() -> u128`，原有三处模块删除；既有单测断言（known_timestamps/now_is_well_formed）在共享 crate 内保留且全绿。

#### Scenario: 消费方切换

- **WHEN** engine-host / gend / engine-scene-mcp 改为依赖 `forge-util`
- **THEN** `cargo test --workspace` 全绿，三 crate 内不再有本地 `timeutil` 模块，调用点行为不变

### Requirement: 死代码删除

系统 SHALL 删除 `forge-agentd/src/events.rs` 中零调用的 `actor()`、`correlation()` builder 方法及其 `#[allow(dead_code)]` 标注。

#### Scenario: 删除后编译与测试

- **WHEN** 两个方法被删除
- **THEN** `cargo test --workspace` 全绿，无 dead_code 警告回归

### Requirement: BottomPanel 派生数组 memo 化

系统 SHALL 对 `BottomPanel.tsx` 的 LogsView（`slice(-120).reverse()`）、OutputView（`slice(-200).reverse()`）、MetricsView（toolCalls/done 派生统计）以 `useMemo` 缓存派生结果，依赖为对应 store 切片引用；渲染输出与交互行为不变。

#### Scenario: 现有测试回归

- **WHEN** useMemo 化完成
- **THEN** `pnpm --filter @forge/client test` 与 `pnpm -r typecheck` 全绿（bottomPanel/consoleMetrics 等既有测试不红）

### Requirement: 一次性脚本归档与 gitignore 清理

系统 SHALL 将 8 个零引用一次性脚本移至 `scripts/archive/`（内容不改动），并从 `.gitignore` 移除失效条目 `apps/ide/`。

#### Scenario: 归档后引用核验

- **WHEN** 移动完成
- **THEN** 全仓 grep 确认无任何活引用指向旧路径（`_f1w4_upstream_patch8.ps1` 保留原位不动），`git status` 无意外变更

## MODIFIED Requirements

无（不改既有门禁语义）。

## REMOVED Requirements

无。
