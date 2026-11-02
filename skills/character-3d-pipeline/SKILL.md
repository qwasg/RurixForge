---
name: character-3d-pipeline
description: 使用 Meshy AI 生成人物、动物 3D 模型及处理已有模型。当任务涉及文/图/多视图生 3D、Meshy 重贴图、低模或 LOD 时使用；兼容 Codex Meshy 插件，并衔接 RurixForge 角色绑骨、动画及 .rxmodel/.rxprefab 发布。
---

# Meshy AI 3D 生成与角色发布

从文字、单张参考图或同一对象的多张视图生成本地 GLB；复用已有模型完成重贴图、减面与 LOD。需要可动人形角色时，继续项目既有绑骨、动画、五阶段验证和角色发布流程。默认用于人物与动物；用户明确要求 Meshy 或本技能制作其他题材时也适用。

保留技能名 `character-3d-pipeline`，兼容项目现有调用。运行入口为 RurixForge 的 `skills/` 与 Codex 的 `.agents/skills/`；后者只路由到本文件，正文在这里维护。

## 执行方式

- **Codex / Meshy 插件**：读取 [Codex Meshy 插件适配](references/codex-meshy-plugin.md)。使用插件的官方 `meshy-cli@0.4.0`，包括登录、任务记录、生成、重贴图、减面、下载和恢复。这个版本的插件是 CLI 型技能包，没有独立 Meshy MCP 连接器；不要编造插件工具名。
- **RurixForge 内建 agent**：读取 [项目原生管线](references/rurixforge-pipeline.md)。使用项目真实的 `mcp__gen-model__*` 工具和已配置的 `meshy` 后端。
- 用户指定插件时优先插件；同一阶段只用一种执行方式。衔接另一种方式时，携带本地文件、实际任务类型与来源记录，不能把 CLI 的 OAuth 登录视为项目 keystore 已配置。
- 本技能不依赖附件原先使用的 Kimi `agent-gw`、Tripo 脚本或插件目录结构。不要调用未随本项目提供的 `model3d_generation_tool.py` / watcher。

## 分步骤执行流程

### 1. 选取输入与最短操作链

先确定主体、形状与细节、材质、是否要贴图、三角面数预算、输出位置及是否需要骨骼动画。已有可用 GLB 或成功任务时，从缺少的处理步骤继续，不重新生模。

- **文字生成**：Meshy `text-to-3d` 的 preview 只生成几何；需要贴图才对成功 preview 创建 refine。项目 `gen_mesh` 可在一次调用中完成这两个阶段。
- **单图生成**：完整可见的单个主体，使用 `image-to-3d`。本地图像可交给 CLI 或项目工具编码，无需先上传第三方图床。
- **多视图生成**：1–4 张同一对象的独立视图；通常用正面、侧面、背面，正面放首位。保持比例、姿势和光照一致。先把三视图拼图拆成独立图片，不能把拼图当作一个相机视图。
- **仅换外观**：对成功模型调用 `retexture`。`text-to-3d refine` 只接受同资源成功的 preview，不能用于上传的 GLB、图生模型或 Remesh 结果。
- **低模 / LOD**：用 standard 生成并 remesh，或在当前执行器确实支持时选 smart-topology。每个 LOD 从同一成功源独立减面，分别保存，不串联反复损失细节。
- **可动人形角色**：有贴图且四肢分离的 A/T-pose 模型 → 减面 → 绑骨 → 合并动画 → 验证 → 发布。动物不直接套用项目的人形骨架。

只需 GLB 的任务交付 GLB；只有明确需要引擎内可动角色时才执行完整发布链。按需要使用现有原画或视图，不为每次请求强制生成额外付费参考图。

### 2. 核对执行器实际支持的参数

- 模型、拓扑、分辨率按 CLI `create --help` 或项目聚合接口 `GET /api/forge/gen/backends` 返回能力选择；图片工具的 `gen_backends_list` 未列出 Meshy 不能直接判为未配置。官网的新参数不一定已经暴露在本地执行器中。
- 项目当前 `aiModel` 接受 `latest`、`meshy-7`、`meshy-6`、`meshy-5`、`meshy-t2`；通常用 `standard` + `latest`。项目提示词限制为 600 字符。不要直接传入执行器尚未接受的版本名。
- 项目 smart-topology 仅支持 `meshy-t2`、triangle、100–15,000 目标面；standard 目标范围为 100–300,000。图生的 `prompt` 是贴图引导，不会修改参考图中的几何。
- 裸模跳过 texture/refine，省略 PBR 和贴图分辨率控制。贴图任务明确设置 texture 与 PBR；不要沿用 Tripo 的 `texture-quality` 或 `face-limit` 参数名。
- Remesh 的面数是目标值。遍历实际使用的全部 mesh / primitive 检查三角数后，才能声明满足硬预算；quad 的面数不能直接当作三角预算。已有蒙皮或动画的模型先确认处理工具能够保留它们。
- 文生 preview、文生 refine、图生、多图生、重贴图分别有自己的任务类型与参数；记录类型，不能只保留一个无类型的任务 ID。

### 3. 提交一次，持续跟踪同一个任务

先完成参数与输入检查，按用户已授权的步骤提交一次，拿到 ID 后立即保留任务类型、来源、参数、日志或项目记录位置。CLI 使用 `create --async` → `project init` → 对应资源 `wait`；详细命令见插件引用。

本地等待超时、MCP 连接中断、进度停在 99% 都不等于远端任务失败。继续查询同一任务；若提交结果不确定，先查操作记录或资源列表核对，不重复付费创建。遵循返回的并发/限流信息；额度不足时停止新提交，保留已完成结果。

用户已要求的贴图、减面、绑骨或发布，在前置成功后继续完成。额外变体、重生或新增付费处理遵循原有预算授权，不因缺少缩略图而重新生成。

### 4. 下载、检查与项目衔接

成功后及时保存 GLB 与可用预览图，下载链接只是临时地址。保留不同阶段文件和任务记录，方便继续修改。检查 glTF 2.0 Binary 头、非空网格、实际面数及所需材质；预览存在时打开检查，再展示给用户。没有预览仍可交付，视觉检查状态如实说明。

仅修改材质的请求应保留几何；减面后重新检查 UV、纹理与轮廓。原 Tripo 的语义 `segment` → `complete` 没有可直接替换的 Meshy 游戏资产流程。Meshy Auto Split 是打印切块，不能当作语义分件或 AI 补全；此类请求使用已验证的本地建模工具，或明确说明缺口。

需要 RurixForge 角色交付时，按 [项目原生管线](references/rurixforge-pipeline.md) 继续；插件产物先落到目标项目 `.forge/tmp/gen/` 并保留来源侧车。发布前核对 Gate 1–5：参考图、GLB、几何预算、蒙皮、真实动画形变；已有 GLB 的参考图阶段可标记不适用，后续门禁仍需实测。严格区分已验证、失败、未验证与不适用。保留 `.rxmodel`、`.rxprefab`、GUID、provenance 及预览检查，不能用静态 `.rxmesh` 声称已完成可动角色。

## 输出约束

- 明确说明哪些模型或处理步骤使用 **Meshy API**，哪些来自本地工具；读取侧车 `backendId` / task ID 核实，不能把本地回退产物归为云端生成。
- 交付已存在的本地文件、预览（如有）、任务类型与 ID、后续恢复记录位置。面数按实测值报告，未测则写未验证。
- 消耗额度读取任务的 `consumed_credits`；字段缺失写未知，不推断为零。
- 引擎角色交付另外报告实际 Gate 指标、骨骼数、Idle/Walk 时长、形变检查和资产路径；只交付静态 GLB 时不声称完成角色发布。
- API Key、OAuth token 和完整 data URI 不写入对话、技能、日志或受 Git 跟踪的配置。项目密钥与插件登录分别由各自的安全存储维护。

## 失败回退策略

- **未配置**：项目报 `GEN_BACKEND_NOT_CONFIGURED` 时指向生成后端设置；插件按登录流程处理。缺少凭据不表示可切换到别的账户或伪造成功。
- **参数拒绝**：检查实际执行器帮助和返回错误，再修正参数。保留用户目标，不通过改题材、算法或输出类型隐瞒降级。
- **超时 / 未知提交**：恢复同一任务或操作记录；不根据通用错误消息自动重发 create。
- **生成成功但下载失败**：保留 ID，修复下载和保存路径；不重生模型。保留已成功下载的其他文件。
- **质量或硬预算不达标**：报告具体差异；用已授权的减面/编辑步骤修复。新一轮生成按实际预算授权处理。
- **角色门禁失败**：在失败阶段修复并复测；不得绕过 Gate 发布。云端与本地切换要记录实际执行轨道。

官方契约核对日期：2026-10-28。按需核对 [Text to 3D](https://docs.meshy.ai/en/api/text-to-3d)、[Image to 3D](https://docs.meshy.ai/en/api/image-to-3d)、[Multi-image](https://docs.meshy.ai/en/api/multi-image-to-3d)、[Retexture](https://docs.meshy.ai/en/api/retexture)、[Remesh](https://docs.meshy.ai/en/api/remesh) 与 [任务生命周期](https://docs.meshy.ai/en/api/task-lifecycle)。执行时仍以安装版本的帮助与实际返回为准。
