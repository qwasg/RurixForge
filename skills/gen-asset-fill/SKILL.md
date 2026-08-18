---
name: gen-asset-fill
description: AI 生成素材补缺。当任务涉及「生成贴图 / 生成模型 / 补素材 / AI 素材」时使用。
---

# gen-asset-fill · 生成素材补缺

## 目标
按缺口清单生成候选素材,用户挑拣后入管线,provenance 完整。

## 必须遵守
- provenance 强制(I-7):接受入库的素材必须带生成来源记录。
- 用户挑拣是必经环节:候选不自动入库。
- 未配置后端 → 显式 GEN_BACKEND_NOT_CONFIGURED,如实报配置路径(设置页 Generation tab / data/gen-backends.json),流程终止,不降级伪造。

## 工具面(F5 已落地,真实可执行)
- `mcp__gen-image__gen_backends_list`:后端 configured 真实判定(条目缺失=false)。
- `mcp__gen-image__gen_image`:文生图,产 n(1..4)候选落 .forge/tmp/gen/(带 dataUrl 缩略图)。
- `mcp__gen-image__gen_texture_set`:材质纹理组逐 map 生成并自动入管线(Content/Textures/)。
- `mcp__gen-image__gen_accept`:候选正式入管线(Content/<destFolder>/ + .meta provenance origin=gen-image)。
- `mcp__gen-model__gen_mesh` / `gen_mesh_refine` / `gen_accept`:模型侧(text2mesh 无后端时显式 GEN_BACKEND_NOT_CONFIGURED)。
- 前端链路:Assets 面板右键「Generate...」对话框 + 候选挑拣 modal;设置页 Generation tab 配置后端。

## 分步骤执行流程
1. 列缺口清单:`mcp__asset-pipeline__asset_list` 分析场景/材质引用了但 Content 缺失的资产。
2. `gen_image` / `gen_texture_set` 生成候选(每缺口 ≥2 候选);先 `gen_backends_list` 确认有 configured 后端。
3. 候选呈现用户挑拣;`gen_accept` 入库(自动 .meta + provenance)。
4. `mcp__asset-pipeline__asset_list` 复核入库;材质/网格引用挂接后 `mcp__asset-pipeline__asset_refs` 验证(asset_refs 验收入管线)。
5. 报告:缺口/候选/接受清单 + provenance 摘要。

## 输出约束
- 未配置后端 → 显式 GEN_BACKEND_NOT_CONFIGURED,并指明配置路径(设置页 Generation tab 或 data/gen-backends.json),不伪造生成产物。

## 失败回退策略
- 生成质量不达标:重新生成或报告缺口留存;已入库错误素材走 asset-cleanup 流程(Proposal 门)。
