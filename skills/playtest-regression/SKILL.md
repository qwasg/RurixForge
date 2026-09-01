---
name: playtest-regression
description: 玩法回归测试。当任务涉及「回归 / 跑测试矩阵 / playtest」时使用。
---

# playtest-regression · 玩法回归测试

> 工具面已落地(F6 wave.1/2):断言库四类 + 矩阵执行器 + test-matrix 真并发,本 skill 可直接执行。

## 目标
场景 × 断言矩阵全量回归,失败汇总报告,聚合不遮蔽任一子断言。

## 必须遵守
- 矩阵先行:先列场景 × 断言组合矩阵,再执行。
- test-matrix 分片只读:同场景并发须独立 host 实例(04 §5.2;FreshSession 每分片独立 engine-scene-mcp/engine-host 进程)。

## 工具面(全部真实存在,勿声明缺失)
- 单矩阵执行:REST `POST /api/forge/playtest/run` { matrixRef } → 结构化报告(逐 case pass/fail + actual/expected/detail;红矩阵 ok=false 如实)。
- 并发回归:REST `POST /api/forge/swarm/execute` { shardType:"test-matrix", items:[case 名], shardCount, operation:{ kind:"test_run", matrixRef, maxConcurrent? } }——每分片独立 engine-host(FreshSession 短连接),聚合报告含分片 pid/时间窗证据;maxConcurrent 缺省 2(显存预算)。
- 断言四类(matrix JSON case.assert.kind):`entity_count` / `component_field` / `transform_near` / `screenshot_ssim`(golden PNG,SSIM 自实现,阈值缺省 0.98)。
- 矩阵 schema:{ scene, camera?, enterPlay?, inputs?:[{action,value,settle?}], settleFrames?, cases:[{name,assert}] }——示例:tests/maze/matrix.json、tests/playtest/matrix_green.json。
- 编排经 MCP 引擎面:mcp__engine-scene__{scene_load, viewport_set_camera, play_enter, play_pause, logic_inject_input, play_step, play_exit, entity_list, component_get, transform_get, scene_summary, viewport_frame}。

## 分步骤执行流程
1. 建断言矩阵(场景 × 断言:实体状态/事件/截图 SSIM;输入序列逐条注入各自 settle——引擎输入队列每逻辑帧取空)。
2. test_run 执行(swarm test-matrix 分片并发,headless host)。
3. 汇总:每断言 通过/失败 + 证据(截图/事件/组件快照)。
4. 失败断言逐条给定位信息(场景/实体/字段/期望/实际)。
5. 报告:矩阵覆盖率 + 通过数 + 失败清单。

## 输出约束
- 聚合 PASS 不得遮蔽任一子断言 FAIL/SKIP;SKIP 必须注明原因。

## 失败回退策略
- 环境性失败(host 拉不起)与断言失败分开报告;环境失败 → DEV_ENV_DEGRADE 如实标注,不充绿。
