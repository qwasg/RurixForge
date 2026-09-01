---
name: asset-cleanup
description: 素材整理/引用修复。当任务涉及「整理素材 / 清理资产目录 / 修复引用 / 资产目录混乱 / cleanup」时使用。
---

# asset-cleanup

## 目标

对混乱素材目录产出整理提案,经 Proposal 门批准后执行,全程 GUID 引用不断链。

## 必须遵守

- **先查询后修改**(06 §4):第一步必须 `mcp__asset-pipeline__asset_list` 读现状,禁止盲改。
- **dryRun 优先**:整理提案一律来自 `mcp__asset-pipeline__asset_cleanup_scan`(只读,不写盘)。
- **Proposal 门不可绕**:整理执行前必须 POST `/api/forge/proposals`(kind=`asset.cleanup`,impact.assets = 待移动清单)并等批准;`asset_delete` force=true 为 destructive,agentd 强制门会自动建 Proposal,未批准一律 GOV_PROPOSAL_REQUIRED(I-6)。
- **孤儿只报告**:orphan 提案不自动删除;删除须单独 asset_delete Proposal 双门。
- **可验证收尾**(06 §4):执行后必须 `mcp__asset-pipeline__asset_refs` 抽验被移动资产的引用边仍在(GUID 不变)。

## 执行流程

1. `mcp__asset-pipeline__asset_list` 拉全量资产;`mcp__asset-pipeline__asset_cleanup_scan` 拿 dryRun 提案(misplaced/naming/orphan 三类 + impact 统计)。
2. 若 proposals 为空 → 报告「目录健康」并结束。
3. POST `/api/forge/proposals`:kind=`asset.cleanup`,summary = 提案摘要(各类数量),impact.assets = 全部待移动 assetPath 列表。
4. 等用户批准(UI Proposal 卡片或 PATCH `/api/forge/proposals/{id}` action=approve);rejected → 报告并停止。
5. 对每条 misplaced/naming 提案调 `mcp__asset-pipeline__asset_move`(destFolder = 提案值,naming 项带 newName)。
6. `mcp__asset-pipeline__asset_fix_redirectors` 收敛 redirector。
7. 验证:对移动过的资产抽验 `mcp__asset-pipeline__asset_refs`(direction=referencedBy)——引用边必须仍在(GUID 引用不断链);`asset_cleanup_scan` 复扫应无 misplaced/naming 残留。
8. 输出报告:已移动清单 + orphan 清单(仅报告)+ 验证结论。

## 输出约束

- 报告必含:提案数(按 issue 分类)、执行数、跳过数、验证结果。
- 禁止伪造:任一步失败如实报告,不得把未执行的移动写成已执行。

## 失败回退策略

- asset_move 单项失败:记 failed 清单继续其余,报告标注;已移动项 redirector 在盘,可重跑本 skill 续作。
- asset_fix_redirectors 失败:redirector 仍在(GUID 引用未断),报告如实说明,下次执行可续。
- Proposal 被拒/超时:零副作用退出,只留 dryRun 报告。
