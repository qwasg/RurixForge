# 全方向全流程自动化测试编排 Spec

## Why

F0–F8 已有单元、栈级冒烟与 F8 八任务浏览器矩阵；F9 全项目 journey 只有 2026-08-19 手工 evidence，不可复跑。需要一条可重复编排，把静态门、仍活着的 API 冒烟、F8、F9 串起来，失败如实登记。

## What Changes

- 新增 `tools/e2e/lib/harness.mjs`（进程/端口/HTTP/浏览器/断言）。
- 新增 `tools/e2e/f9-full-journey.mjs`：先 3080 API 闭环，再 UI 薄层。
- 新增 `tools/e2e/run-all.mjs` + `scripts/full-auto-test.ps1`；根脚本 `test:full` / `test:full:core`。
- F8 仅加 `FORGE_E2E_SKIP_BUILD=1` 跳过重复构建。
- 不修改产品行为；不修 D2/D3/D4/D7/D8。

## ADDED Requirements

### Requirement: profile 分层

系统 SHALL 提供 `core` 与 `stack`（默认）两档。`stack` = 静态门 + f0 + 七条仍活 API 冒烟 + F8 + F9。退役/Electron/本机 fps/真 LLM 列入汇总 `skipped`，不进默认硬门。

### Requirement: 失败继续 + 诚实 verdict

任一层失败 SHALL 继续后续层，总 exit 在任一硬门失败或孤儿残留时为 1。`pass-degraded` / `annotated-mock` 不算 F8/F9 硬红。

### Requirement: F9 先 API 再 UI

F9 SHALL 先经 host:3080 完成建实体/材质/逻辑图/保存/PIE/playtest/pack，再开浏览器补 F8 未覆盖巡检。不伪造空项目叙事。pack 无 UI，只打 `POST /api/forge/project/pack`。
