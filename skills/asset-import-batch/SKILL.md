---
name: asset-import-batch
description: 批量导入素材目录。当任务涉及「批量导入 / 导这个目录 / 素材入库」时使用。
---

# asset-import-batch · 批量导入素材

## 目标
把一个源目录的素材按类型分类导入 Content 对应目录,失败项如实报告。

## 必须遵守
- 先查询后修改(06 §4):`mcp__asset-pipeline__asset_list` 读现状,确认目标目录不覆盖既有资产。
- 小样本先行:先导入 1~2 个验证设置,再全量;>20 资产须升 Proposal(06 §4)。

## 分步骤执行流程
1. 扫描源目录,按扩展名分类:gltf/glb→Meshes,png/jpg→Textures。
2. `mcp__asset-pipeline__asset_import` 小样本导入 1 个,`mcp__asset-pipeline__asset_build_status` 确认 current。
3. 全量导入(按类分批,每批一次 asset_import 多路径)。
4. `mcp__asset-pipeline__asset_list` 复核:数量、目录归位、无 failed。
5. 失败项逐条记录(path + error),报告给用户,不重试死磕。

## 输出约束
- 报告:导入成功清单(path + guid)+ 失败清单(path + 原因)+ 缓存命中情况;不伪造成功。

## 失败回退策略
- 单文件失败不阻塞批次;全部失败 → 停止并报告,检查源文件合法性(image/gltf 样本须真实可解码)。
