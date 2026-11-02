# RurixForge 原生 Meshy 角色管线

当前环境有 RurixForge MCP 工具时先查工具清单；普通 Codex 会话也可在已运行的项目服务上通过下述 REST 桥接。
没有 MCP 且项目服务未运行时，按 [Codex Meshy 接入](codex-meshy-plugin.md) 处理文件，发布阶段如实报告尚待项目服务，不编造调用。
本模式衔接项目现有角色发布与五阶段验证；各步骤按用户需要执行。
已有 GLB 从缺失的精修、绑骨、动作或发布步骤继续，不为补流程重新生成。
外部 GLB 需保留原件，处理副本放在项目 `.forge/tmp/gen/`，同时保留已有来源记录。
动物、非人形、静态角色和裸模不强行套用人形绑骨、Idle/Walk 或角色发布。

## 1. 查询现状与后端

先用 `mcp__asset-pipeline__asset_list` 检查可复用资产，并检查输入文件及目标预算。
调用 `mcp__gen-image__gen_backends_list` 读取后端配置事实，不读取或输出密钥。
当前该 MCP 清单只聚合图片后端；若没有 `meshy`，继续读取 `GET /api/forge/gen/backends`。
REST 聚合清单中的 `id: "meshy"` 必须为 `configured: true`，再提交 Meshy 生模。
未配置则报告 `GEN_BACKEND_NOT_CONFIGURED`，指出设置页生成后端或 `data/gen-backends.json`。
原生后端使用 `MESHY_API_KEY` 或本地 keystore；不要把密钥写进 skill、日志或报告。
复用用户现有配置；本次任务仅要求改 skill 时，不启动付费生成作为验证。

## 2. 准备输入与 Gate 1

调用 `mcp__gen-model__gen_mesh` 时，三种输入模式择一：

- 文生：`prompt`，最多 600 个字符，具体描述主体、形体、材质及风格。
- 单图：`imageRef`，接受项目相对图片路径、HTTP(S) URL 或 data URI。
- 多图：`imageRefs`，1–4 张同一主体的独立图片；1 张走单图，2–4 张走多图。

`imageRef` 与 `imageRefs` 择一，不依赖实现中的优先级或超过四张时的截断行为。
三视图拼图先拆成独立的正、侧、背视图，保持姿态、尺度与光照一致。
不要将“原画 + 含多个人物的三视图拼图”直接作为多图重建的两张输入。
用户已有合格参考图时直接复用；需要概念图时才调用 `mcp__gen-image__gen_image`。
可动人形宜全身完整、四肢分离的 A-pose 或 T-pose；不强制额外生成无用视图。
对参考图执行 Gate 1：至少 256×256，非透明覆盖率至少 1%；检查主体清楚、无裁切。
Rust 验证器对不透明平色画布还要求亮度方差至少 10；覆盖率不是实际主体分割率。

## 3. Meshy 生模与 Gate 2

原生调用显式传 `backend: "meshy"` 和 `track: "cloud"`。
生模的 `track: "auto"` 可能在云端失败后生成本地人形，不能将其当作 Meshy 成功。
以下为单图人形示例，按用户预算调整；文生时换为 `prompt`，多图时换为 `imageRefs`：

```json
{
  "backend": "meshy", "track": "cloud",
  "imageRef": "Content/ConceptArt/hero_front.png",
  "modelType": "standard", "aiModel": "latest", "topology": "triangle",
  "targetPolyBudget": 10000, "poseMode": "a-pose",
  "texture": true, "pbr": true, "textureResolution": "2k",
  "timeoutSec": 900
}
```

当前代码支持 `latest|meshy-7|meshy-6|meshy-5|meshy-t2`，不要直接套用官网新型号。
`standard` 面数目标为 100–300000；`smart-topology` 用 `meshy-t2`、三角面、100–15000。
图生的 `prompt` 只作贴图引导，不控制几何；显式贴图描述可用 `texturePrompt`。
`styleRefAssetPath` 为图片时充当贴图参考，为其他文字时覆盖贴图提示词；不要混填冲突引导。
只需几何时传 `texture: false`；裸模不据此宣称通过需要 UV 的角色 Gate 2。
文生默认经历 preview 几何→refine 贴图；这两个上游步骤分别计费。
成功读取 `candidates[0].meshFileRef`，不是根级 `meshFileRef`。
保留候选 `meta`、`previews` 及同名 `<模型文件>.json` sidecar。
记录 `meta.taskId`、文生的 `meta.previewTaskId`；Gate 2 检查 glTF 2.0、有效网格、三角面与 UV。
预览仅在返回且文件存在时展示；多图模式不承诺四向预览，缺图不重新付费生成。

## 4. 几何精修与 Gate 3

调用 `mcp__gen-model__gen_mesh_refine`，例如：

```json
{
  "meshFileRef": ".forge/tmp/gen/source.glb",
  "targetFaces": 5000, "track": "auto",
  "ops": { "retopo": { "targetFaces": 5000 } }
}
```

该工具是 Meshy Remesh / 本地减面，区别于文生任务的 refine 贴图阶段。
`track` 接受 `cloud|local|auto`；本地轨道使用 `tools/model/decimate_mesh.py`。
读取 `refinedMeshFileRef`、`originalTriangles`、`refinedTriangles`、顶点统计及 `stats`。
执行 Gate 3，以实际文件复核三角面数 ≤ 目标、无重复索引退化面、每面面积 > 1e-10。
当前 Rust/Python 几何 Gate 主要读取首个 mesh 的首个 primitive；多网格模型必须额外遍历全部实际使用的 mesh / primitive 计数及检查，不能只凭该 Gate 的数字声明整模达标。
本地 `decimate_mesh.py` 会重建网格，不保留已有 skins / JOINTS / WEIGHTS / animations；已有可动模型不能经此回退后声称动作已保留。需先验证适合的保骨减面工具，或把绑骨与动画作为明确的后续步骤重新验证。
统计缺失或解析失败时不能把估算当实测；可用 `mcp__asset-pipeline__inspect_3d_asset` 辅助核对。
注意精修和下节绑骨即使传 `track: "cloud"`，当前实现仍可能转本地脚本。
读取 sidecar 的 `backendId`、`meta.taskId` 与 `stats.meshyTaskId` 判明来源。
要求云端结果而实际为 `local-headless` 时如实报告，不将参数值当作服务端执行证据。

## 5. 人形绑骨与 Gate 4

仅为需要可动的人形调用 `mcp__gen-model__gen_mesh_rig`。
输入精修结果的 `meshFileRef`、真实 `heightMeters`，并按需求传 `track: "cloud"|"local"|"auto"`。
本地自动绑骨使用 `tools/model/auto_rig.py`，可衔接现有标准人形动作包。
读取 `riggedMeshFileRef`、`jointsCount`、`normalizedWeights`、`rigTaskId`，并保存 sidecar。
执行 Gate 4：非空 skin，joints 与 inverseBindMatrices 数量一致，调色板索引有效。
核验 `JOINTS_0` / `WEIGHTS_0`、每顶点最多四个影响、权重非负且归一化。
本地标准骨架通常为 19 关节；云端关节数按实测记录，不把 19 作为所有模型的固定事实。

## 6. 动作合入与 Gate 5

调用 `mcp__gen-model__gen_mesh_animate`，输入上步 `riggedMeshFileRef` 作为 `meshFileRef`。
优先复用已检查的本地动作包：`motionPack: "assets/motion-packs/humanoid"`、`track: "local"`。
确认目录及骨架兼容，至少包含可用的 `idle.glb` 与 `walk.glb`；底层使用 `tools/model/merge_motion.py`。
需要 Meshy 云端动作时，按 CLI 引用先查当前动作目录，再传真实 `actionIds`；不猜测 0/30 的语义。
读取 `animatedMeshFileRef` 与 `animations`；不能只凭剪辑名称宣布动作有效。
执行 Gate 5 动作部分：独立 Idle/Walk、递增时间戳、正时长、FK LBS 顶点差至少 0.01。
记录动画实际来源；当前动画 sidecar 来源标记不应替代调用过程与文件证据。

## 7. 原子发布与复核

用户要求引擎角色交付时，调用 `mcp__gen-model__gen_character_publish`。
传最终 `meshFileRef`、`name`、`category: "role"`、`idleClip: "Idle"`、`walkClip: "Walk"`、`revision`。
`name` 用非空文件名片段；禁止 `/`、`\`、`:`、`.` 与开头空格，宜用英数下划线。
读取真实返回 `modelGuid`、`prefabGuid`、`assetPath`、`prefabPath`、`rxmodelUri`、`rxprefabUri`。
正式文件为 `Content/Models/<guid>/model.rxmodel` 与同目录 `template.rxprefab`；以返回路径为准。
`assetPath` / `prefabPath` 相对 Content，`rxmodelUri` / `rxprefabUri` 已带 Content 前缀，不重复拼接。
检查模型 `.meta` 的 `provenance.origin=gen-model` 与 sidecar 源链，验证发布文档及角色组件。
Gate 5 再传真实发布目录验证两个文件及 Animator / CharacterController；动作检查本身不代表发布成功。
调用 `mcp__engine-scene__template_preview`，用返回预制体引用检查比例、材质、Idle/Walk 播放。
用 `mcp__asset-pipeline__asset_refs` 的 `assetPath`、`direction: "refs"` 复核引用。
静态模型仅在用户已授权入库时调用 `mcp__gen-model__gen_accept`，不冒充角色包。
其参数为 `meshFileRef`、`destFolder`、`name`；可选 `importSettings: {"generateLods": [0.5, 0.25]}`。
入库后可用 `mcp__asset-pipeline__mesh_inspect` 复核构建出的真实三角面与 LOD。

## 五门验证依据与失败恢复

Rust 依据为 [crates/gend/src/gates.rs](../../../crates/gend/src/gates.rs)：
`validate_gate_1_concept_art`、`validate_gate_2_raw_glb`、`validate_gate_3_polycount`、
`validate_gate_4_rigging`、`validate_gate_5_motion_and_publish`，并有对应 `_bytes` 版本。
只读 Python 验证可导入 [tools/e2e/character_3d_pipeline.py](../../../tools/e2e/character_3d_pipeline.py)，
调用同名 Gate 1–4 函数及 `validate_gate_5_motion_publish(..., published_dir=实际发布目录)`，检查 `passed`。
导入需 NumPy/Pillow；不要为验收实际产物运行会生成合成 fixture 的整套 runner 冒充生产结果。
Python Gate 1 未含 Rust 对比度检查；其 Gate 5 用 Walk 0.4 秒，Rust 用 0.4×Walk 时长，报告所用实现。
并非每个工具自动跑齐五门；原生精修/绑骨/动作仅内置相应部分，仍须验证输入、最终包及遗漏项。
验证失败保留文件和原始错误；报告失败门、实测指标、来源与下一步，不把静态模型报作可动角色。
超时后先保存原日志及任务 ID，按实际操作恢复查询/下载，不重新调用原生 create 工具提交同一任务。
原生封装没有通用恢复命令；用 [Codex Meshy 接入](codex-meshy-plugin.md) 对原 ID 查询，或明确报告待恢复。
原生暂未暴露 retexture / segment / complete；不要虚构同名 MCP，所需能力转上述 CLI 引用核对支持范围。
最终交付列出文件、任务 ID、实际 Meshy API / 本地步骤、已验证门与未验证项；保留原始资料和有效交付物。

## Codex 到已运行项目服务的桥接

从项目当前启动配置取得真实服务地址，不猜端口。先 `GET /api/forge/workspaces`，按返回的 `root` 精确匹配目标游戏项目，取得 `workspaceId`，并用 `GET /api/forge/mcp/tools` 核对工具。
处理和发布请求必须显式传该 `workspaceId`；省略或填错可能落到默认示例项目，不能据此交付。
以下直通接口复用上述工具参数，另加 `workspaceId`：

- `POST /api/forge/gen/mesh/refine`：`meshFileRef`、`targetFaces`、`ops`、`track`。
- `POST /api/forge/gen/mesh/rig`：`meshFileRef`、`heightMeters`、`track`。
- `POST /api/forge/gen/mesh/animate`：`meshFileRef`、`motionPack`、`track`。该 REST 结构未透传 `actionIds`，云端自定义动作继续走 CLI。
- `POST /api/forge/gen/mesh/publish`：`meshFileRef`、`name`、`category`、`idleClip`、`walkClip`、`revision`。

预览、静态入库等可用 `POST /api/forge/mcp/call`，请求形如 `{"workspaceId":"<真实 ID>","tool":"mcp__gen-model__gen_accept","arguments":{"meshFileRef":".forge/tmp/gen/prop.glb","destFolder":"Meshes","name":"prop"}}`。
此通用接口返回 MCP 包装，检查 `isError` 并解析 `content[].text` 中实际结果；不能只按 HTTP 200 判成功。
所有调用以实际服务响应为准。项目服务不可达时不虚报发布成功，不为测试技能而启动付费生模。
