---
name: blender-production
description: 当 RurixForge 的地图、角色或道具制作任务需要在 Blender 建模、UV 和贴图时，使用 Codex 原生 computer-use 制作，并通过 RurixForge Blender bridge 发布和同步引擎模板。
---

## 执行流程

在独立 Codex 桌面任务中工作。使用当前 Codex 的原生 computer-use 插件操作 Blender 完成建模、UV、材质、贴图和角色动作；不切换为引擎内 open-computer-use，也不将任意 Python 程序交给引擎执行。任务 job ID 由引擎交接提示给出。

1. 通过 `rurix-blender` MCP 的 `blender_job_get` 查询任务，确认原生 computer-use 在当前任务中确实可调用，再用 `blender_job_claim` 领取并上报 `capabilities.computerUse=true`。保留返回的 leaseToken，每分钟调用 `blender_job_progress` 汇报进展以续租。没有桌面工具时如实报告，勿领取或声称已开始制作。
2. 从任务读取 `sourceAbsolutePath`，在 Blender 保存源工程到此位置。在 Blender Preferences → Add-ons → Install from Disk 选择任务 `setup.addonPath` 返回的 `rurix_forge.py` 并启用；插件保存时给对象写入稳定 `rurixId`，因此对象重命名后依然能同步。不要删除这些自定义属性。源工程保存在项目 `Sources/Blender/`，图片可放相邻目录。
3. 用 Principled BSDF 制作 PBR 材质。为模型提供 UV，使用 PNG/JPEG 图片贴图；程序纹理先烘焙。Base Color 为颜色数据，normal/roughness/metallic/AO 为 Non-Color；normal 使用切线空间。ORM 通道为 R=AO、G=roughness、B=metallic。地图保持层级、原点和米制比例，需要碰撞的对象设置自定义属性 `rurixCollision=true`。角色必须有骨架、蒙皮权重（每顶点最多四个骨骼）以及独立的 `Idle`、`Walk` Actions，动作须为当前Action或保存到NLA轨道。若任务提供 `idleClip`、`walkClip`，使用其指定的两个独立动作名；创建任务时也可用这两个可选字段映射已有动画。
4. 调用 `blender_source_bind`，传保存后的 sourcePath 与 leaseToken；调用 `blender_publish`。发布是异步工作，通过 `blender_job_get` 查询 exporting、validating、importing、ready 或 failed。导出是引擎内固定后台脚本，无需桌面逐次操作导出对话框。
5. 通过 `blender_template_preview` 获取本引擎真实模板预览，核对贴图、形状、朝向、比例、角色动作。首次成功后自动监听已保存的 .blend 和依赖图片，保存修改即可同步；未保存的 Blender 更改不会导入。

## 输出约束

完成时给出引擎 prefab 路径、revision 和真实预览结果。制作中的文件和失败结果不是已导入资产。保存源工程和依赖图片以便后续修改。引擎从 GLB 校验并生成模板，不接受任意脚本字段；首版不发布相机、灯光、物理模拟或程序材质。

## 失败回退

租约过期后重新领取，切勿复用旧 token。导出、校验或导入失败时读取错误，修正 Blender 源后重试；上一成功版本应继续可用。取消后停止制作和汇报。原生 computer-use 不可用时保留待领取任务并说明所缺插件能力，不能以占位文件宣告完成。
