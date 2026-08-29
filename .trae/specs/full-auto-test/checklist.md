# 全方向全流程自动化测试 — Checklist

- [x] harness 抽出且 F8 构建可跳过
- [x] F9 脚本存在：API A0–A7 + UI U0–U7
- [x] 编排器 profile=core|stack，退役脚本列入 skipped
- [x] `pnpm test:full`（`node tools/e2e/run-all.mjs --profile stack`）跑完并写出 `evidence/full-auto-2026-08-22T10-02-14Z.json`
- [x] 每层独立 verdict；F8 `gateGreen=true`（`annotated-mock` 不充绿）
- [x] F9 临时场景/pack 目录已清理；复跑 `gateGreen=true` / `orphanFree=true` / exit 0
- [x] 结束后无编排器拉起的 8103/3080 孤儿进程
- [x] 失败项只登记，未改产品代码充绿

首次 stack 未全绿（如实）：`rurix-rt` 外部 crate 编译失败拖红 build/cargo/`f6-w4`/`f7-w2`；`f0` 在 scene_summary 后因 engine-host 进程计数未过；F9 初跑因脚本参数（Script 缺 props、asset_refs 参数、设置页读错）失败，修脚本后复跑过门。
