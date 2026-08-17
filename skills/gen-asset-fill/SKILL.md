---
name: gen-asset-fill
description: AI 生成素材补缺。当任务涉及「生成贴图 / 生成模型 / 补素材 / AI 素材」时使用。
---

# gen-asset-fill · 生成素材补缺

> SEAM 标注:依赖 gen-image / gen-model MCP 工具面(F5 生成接入承接,未落地),当前不可执行;无后端配置时必须显式报 GEN_BACKEND_NOT_CONFIGURED(I-5),不得伪造生成产物。

## 目标
按缺口清单生成候选素材,用户挑拣后入管线,provenance 完整。

## 必须遵守
- provenance 强制(I-7):接受入库的素材必须带生成来源记录。
- 用户挑拣是必经环节:候选不自动入库。

## 分步骤执行流程(F5 工具面落地后生效)
1. 列缺口清单(场景/材质引用了但 Content 缺失的资产)。
2. gen-image / gen-model 生成候选(每缺口 ≥2 候选)。
3. 候选呈现用户挑拣;`gen_accept` 入库(自动 .meta + provenance)。
4. `mcp__asset-pipeline__asset_list` 复核入库;材质/网格引用挂接后 `mcp__asset-pipeline__asset_refs` 验证。
5. 报告:缺口/候选/接受清单 + provenance 摘要。

## 输出约束
- 未配置后端 → 显式 GEN_BACKEND_NOT_CONFIGURED,流程终止,不降级伪造。

## 失败回退策略
- 生成质量不达标:重新生成或报告缺口留存;已入库错误素材走 asset-cleanup 流程(Proposal 门)。
