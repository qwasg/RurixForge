---
name: playtest-regression
description: 玩法回归测试。当任务涉及「回归 / 跑测试矩阵 / playtest」时使用。
---

# playtest-regression · 玩法回归测试

> SEAM 标注:依赖 playtest 工具面(test_run / 断言库 / SSIM 截图断言,F6 试玩回归承接,未落地),当前不可执行。

## 目标
场景 × 断言矩阵全量回归,失败汇总报告,聚合不遮蔽任一子断言。

## 必须遵守
- 矩阵先行:先列场景 × 断言组合矩阵,再执行。
- test-matrix 分片只读:同场景并发须独立 host 实例(04 §5.2)。

## 分步骤执行流程(F6 工具面落地后生效)
1. 建断言矩阵(场景 × 断言:实体状态/事件/截图 SSIM)。
2. test_run 执行(swarm test-matrix 分片并发,headless host)。
3. 汇总:每断言 通过/失败 + 证据(截图/事件/组件快照)。
4. 失败断言逐条给定位信息(场景/实体/字段/期望/实际)。
5. 报告:矩阵覆盖率 + 通过数 + 失败清单。

## 输出约束
- 聚合 PASS 不得遮蔽任一子断言 FAIL/SKIP;SKIP 必须注明原因。

## 失败回退策略
- 环境性失败(host 拉不起)与断言失败分开报告;环境失败 → DEV_ENV_DEGRADE 如实标注,不充绿。
