# 全方向全流程自动化测试 — Tasks

- [x] Task 1: 抽出 `tools/e2e/lib/harness.mjs`；F8 加 `FORGE_E2E_SKIP_BUILD`
- [x] Task 2: `f9-full-journey.mjs` + journey-matrix 模板（API 闭环 + UI 薄层，绕行 D2/D3/D4）
- [x] Task 3: `run-all.mjs` / `scripts/full-auto-test.ps1` / 根 `test:full`；profile=core|stack
- [x] Task 4: 本 spec / checklist
- [x] Task 5: 本机跑一次默认 `stack`，汇总 JSON 落 `evidence/full-auto-2026-08-22T10-02-14Z.json`；F9 脚本修参后复跑 `gateGreen=true`（`f9-e2e-summary-2026-08-22T10-07-46Z.json`）
