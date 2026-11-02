---
name: blender-production
description: 3D DCC 资产制作与同步规程。当任务涉及「Blender 建模 / 3D 资产导出 / DCC 生产 / Blender 模板发布 / 场景与道具模型入库」时使用。支持仓内无头自动化处理与 Blender bridge 后台导出双轨，彻底解除对外部桌面 GUI 与 computer-use 的强依赖。
---

# blender-production · 3D 资产制作与同步规程

## 目标

为 RurixForge 引擎规范化制作并同步复杂的 3D 场景、道具与角色资产：
产出通过 glTF/GLB 校验且包含规范 PBR 材质（或标准骨骼动作）的引擎预制体模板（`.rxprefab`）与模型文档（`.rxmodel`）。
支持仓内确定性无头脚本、Meshy 双轨生成与 Blender bridge 后台无头导出，无需人工打开桌面 GUI 即可全自动流转。

## 必须遵守

1. **消除桌面 GUI 阻塞**：
   - 严禁假定必须有外部 Codex 桌面端 computer-use 或人工点击桌面 Blender GUI；生产流水线全面支持无头（headless）自动化。
   - 角色类 3D 资产优先使用 `character-3d-pipeline` 技能，通过 MCP 工具面（`gen_mesh`、`gen_mesh_refine`、`gen_mesh_rig`、`gen_mesh_animate`、`gen_character_publish`）或仓内 Python 脚本（`tools/model/`）全自动完成，无需启动 Blender。
2. **材质与几何规范**：
   - PBR 材质必须使用 Principled BSDF，贴图为 PNG/JPEG；Base Color 为 sRGB，Normal / Roughness / Metallic / AO 必须标记为 Non-Color；Normal 使用切线空间（Tangent Space）。
   - ORM 贴图通道打包标准：R = Ambient Occlusion, G = Roughness, B = Metallic。
   - 地图与道具保持米制比例与真实变换层级，需要碰撞的对象设置自定义属性 `rurixCollision = true`。
3. **角色资产硬门禁**：
   - 角色必须包含有效骨架与蒙皮权重（每顶点严格限制最多 4 骨骼影响，权重归一化和为 1.0）；
   - 必须包含独立且可播放的 `Idle` 与 `Walk` 动作（当前 Action 或保存至 NLA 轨道）；
   - 杜绝静态无骨骼网格伪造角色（红线 I-5）。

## 执行流程

### 轨道 A：原生无头自动化管线（推荐首选，无 GUI 依赖）
当任务为制作/生成 3D 角色或通用模型时，直接使用仓内双轨工具链：
1. **角色生产**：调用 `character-3d-pipeline` 技能，按照 Step 1~6 规程全自动跑通：原画准备 $\rightarrow$ 3D 生模 $\rightarrow$ 目标面数精修（`gen_mesh_refine` / `decimate_mesh.py`）$\rightarrow$ 人形自动绑骨（`gen_mesh_rig` / `auto_rig.py`）$\rightarrow$ 动作包合并（`gen_mesh_animate` / `merge_motion.py`）$\rightarrow$ 原子发布为 `.rxmodel` 与 `.rxprefab`（`gen_character_publish`）。
2. **静态道具生产**：使用 `gen-asset-fill` 技能，调用 `gen_mesh` 生成候选，通过 `gen_mesh_refine` 控制面数，调用 `gen_accept` 入库为 `.rxmesh`。

### 轨道 B：Blender 源工程无头/后台导出管线（DCC 源工程同步）
当任务涉及人工定制的复杂工程或已有 `.blend` 源资产时：
1. **源工程存放**：源工程保存在项目 `Sources/Blender/<name>/<name>.blend`，依赖纹理置于相邻目录。
2. **启用或调用无头导出脚本**：
   - 方式 1（无头 CLI 脚本）：直接调用无头导出器 `python tools/blender/export_bundle.py --source Sources/Blender/<name>.blend --output Content/Models/<name>/`。
   - 方式 2（Blender Bridge 任务调度）：若系统中启用了 `rurix-blender` MCP，通过 `blender_job_get` 获取任务，调用 `blender_source_bind` 绑定工程路径，并调用 `blender_publish` 发起后台无头导出。引擎在后台静默执行校验并生成模板，无需桌面弹窗交互。
3. **PBR 与网格标准化**：
   - 自动烘焙程序纹理，确保导出模型包含合法 UV（`TEXCOORD_0`）与切线法线。
   - 导出为 glTF 2.0 Binary（`.glb`），由 `assetd::model::import_model_bundle` 校验几何面数、骨骼拓扑与动作有效性。
4. **引擎模板验证**：
   - 调用 `mcp__engine-scene__template_preview` 获取真实渲染预览，核对朝向、缩放比例、材质质感与骨骼动作播放是否正常。

## 输出约束

- 成功发布后输出资产的客观事实报告：
  - 发布的引擎预制体路径（`Content/Models/<guid>/template.rxprefab` 或指定 Content 相对路径）；
  - 模型资产路径（`model.rxmodel`）与版本号（revision）；
  - 拓扑指标：顶点数、三角面数、材质数量、骨骼数与动作剪辑列表（如 `Idle`、`Walk`）；
  - 真实渲染预览结果（preview fileRef）。
- 制作过程中的草稿、中间临时文件或失败产物不作为正式资产入库。
- 绝不以无骨骼的占位网格宣告可动角色制作完成。

## 失败回退策略

- 缺少外部 Blender 桌面客户端：直接切换至轨道 A（`character-3d-pipeline` 与 `tools/model/` 无头脚本），零阻塞完成模型制作。
- 导出或门禁校验失败（如面数超预算、缺 UV 或骨骼权重未归一化）：
  - 读取结构化报错日志；
  - 针对面数问题调用 `tools/model/decimate_mesh.py` 进行精修减面；
  - 针对骨骼问题调用 `tools/model/auto_rig.py` 重新生成 4 骨骼归一化蒙皮权重；
  - 针对动作缺失调用 `tools/model/merge_motion.py` 重新合入标准动作包；
  - 修正后重新发起发布校验。
