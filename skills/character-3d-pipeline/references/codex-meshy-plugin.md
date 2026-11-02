# Codex Meshy 插件适配

适配本机 `meshy-openai-plugin` 0.6.0 的 `meshy-3d-generation`，执行器为官方 `meshy-cli@0.4.0`。插件是技能与 CLI，不是 Meshy MCP server；不要寻找虚构的 `mcp__meshy__*` 工具。插件原文件和凭据不随项目复制，规程也不依赖某个用户目录。

## 运行环境与登录

Node.js 需 >=22.12。先检查 `node --version` 与 `meshy --version`；已有 0.4.0 时直接使用。没有命令或版本不同时，用以下固定 runner，不改变全局安装：

```powershell
# 每个独立 shell 调用都要重新设置缓存路径。
$env:npm_config_cache = 'F:\RurixForge\.forge\cache\meshy-cli-npm'
npm.cmd exec --yes --package=meshy-cli@0.4.0 -- meshy doctor --output-schema v1 --format json --no-update-check
```

在其他检出目录，把缓存路径改为该检出目录的 `.forge/cache/meshy-cli-npm`。下文 `meshy ...` 都可替换成 `npm.cmd exec --yes --package=meshy-cli@0.4.0 -- meshy ...`；每次调用重新设置缓存路径。不要把 Node 的 npm 缓存改到 D/E 盘而跳过项目容量维护要求。

`doctor` 不带 `--check-api` 只做本地诊断，不证明 API 登录成功。需要实际执行用户的生成任务时，再检查：

```powershell
meshy auth status --format json --no-update-check
```

`auth status` 返回裸 JSON：`authenticated:true`、`verified:true` 且 exit 0 才是验证成功。无凭据时执行 `meshy auth login --device --format json --no-update-check`。用持续的工具会话运行，及时把 stderr 的验证网址与用户代码在进度消息中展示，等待同一进程完成，再验证 status 并继续原任务。不要要求用户把 token/密钥贴进聊天，也不要新建自己的凭据文件。已存在但未验证的凭据可能是网络故障，不直接重登录或删除。

单纯编辑/校验本技能不要求登录，不因此启动授权或调用付费接口。CLI 复用官方安全存储的 OAuth 或环境变量 `MESHY_API_KEY`，与 RurixForge `data/keystore.json` 分别管理，不读取并复制其中的密钥。

## 工作区与提交记录

在 RurixForge 需要后续入库时，以目标游戏项目的绝对根目录为 `WORKSPACE`，记录目录选 `.forge/tmp/meshy`，最终待入库 GLB 放 `.forge/tmp/gen/<唯一名称>.glb`。仅需要单独文件时遵从用户指定位置；未指定才使用 `meshy_output`。所有写入命令传 `--workspace`，路径存在时选择新文件名，只有已授权覆盖时才用 `--overwrite`。

下列大写变量是待替换值，每一步读取返回后再进行下一步，不能整段盲跑。

1. 第一次 `create --async` 先不传尚不存在的 `--project`，从 JSON 的 `result.submission.task_id` 取得接受的 ID。可提供稳定 `--operation-id`；本地 journal 防重复不等于服务器幂等键。
2. 建立本地记录并读取实际 `result.project_dir`：

```powershell
meshy project init --root "PROJECT_ROOT" --name "JOB_NAME" --task-id TASK_ID --task-type RESOURCE --workspace "WORKSPACE" --output-schema v1 --format json --no-update-check
```

3. 用创建该任务的实际资源等待：

```powershell
meshy RESOURCE wait TASK_ID --timeout 3600 --project "PROJECT_DIR" --stage STAGE --workspace "WORKSPACE" --output-schema v1 --format json --no-update-check
```

CLI 自行轮询；持续等待 shell 会话，保持进度更新，不再写外层轮询脚本。exit 8 是等待预算耗尽，继续同一资源的同一 ID。exit 10 是提交结果未知，先按 `error.recovery.command` 对账，不能换 operation ID 重发。exit 9 额度不足时停止新提交。

后续 create 带 `--project "PROJECT_DIR" --stage STAGE --workspace "WORKSPACE"`。成功 wait 会保存 `task_TASK_ID.json`。已有成功任务可直接初始化项目，从处理步骤继续。

## 按需操作

以下 create 示例都使用 `--async --output-schema v1 --format json --no-update-check`。示例中的模型和参数以 0.4.0 实际帮助为准；该版本文本和贴图提示词限制为 600 字符。高级原始字段先核对官方 API 与 `create --help`，再通过 `--data '@payload.json'` 提供，不能自行发明 CLI 选项。PowerShell 中 `@文件路径` 必须作为带引号的字符串传入。

**文字 → 几何 → 可选贴图**：

```powershell
meshy text-to-3d create --mode preview --prompt "MODEL_DESCRIPTION" --pose-mode a-pose --target-formats glb --async --output-schema v1 --format json --no-update-check
# 初始化并等待 preview 成功；只有需要贴图才执行下一条。
meshy text-to-3d create --mode refine --preview-task-id PREVIEW_ID --enable-pbr true --texture-resolution 4k --target-formats glb --async --project "PROJECT_DIR" --stage refine --workspace "WORKSPACE" --output-schema v1 --format json --no-update-check
```

`pose-mode` 只在人形姿势有要求时使用。refine 不能引用 image-to-3d 或 remesh 的 ID。

**单图**：

```powershell
meshy image-to-3d create --image-url "REFERENCE_PATH" --model-type standard --should-texture true --enable-pbr true --texture-resolution 4k --target-formats glb --async --output-schema v1 --format json --no-update-check
```

裸模改为 `--should-texture false` 并省略 PBR、贴图分辨率选项。smart-topology 使用 `--model-type smart-topology --target-polycount N`，当前 CLI 范围为 100–15,000 三角面，不与 `--ultra-mode` 配合；standard 要目标面数时使用后续 remesh。

**多视图**：

```powershell
meshy multi-image-to-3d create --image-urls "FRONT_PATH,SIDE_PATH,BACK_PATH" --should-texture true --enable-pbr true --texture-resolution 4k --target-formats glb --async --output-schema v1 --format json --no-update-check
```

每项为 URL 或本地文件，最多 4 张。路径本身含逗号时用 JSON 文件的 `image_urls` 数组并传 `--data '@payload.json'`。不把多视图排版图直接作为一个视角；不假定此资源支持单图的 smart-topology 或 ultra 选项。

**重贴图 / 减面**：以下为已有项目任务的独立处理步骤，不自动连做：

```powershell
meshy retexture create --input-task-id SOURCE_ID --text-style-prompt "TEXTURE_DESCRIPTION" --enable-original-uv true --enable-pbr true --texture-resolution 4k --target-formats glb --async --project "PROJECT_DIR" --stage retexture --workspace "WORKSPACE" --output-schema v1 --format json --no-update-check
meshy remesh create --input-task-id SOURCE_ID --topology triangle --target-polycount 5000 --target-formats glb --async --project "PROJECT_DIR" --stage lod5000 --workspace "WORKSPACE" --output-schema v1 --format json --no-update-check
```

外部本地模型将 `--input-task-id SOURCE_ID` 换成 `--model-url "MODEL_PATH"`，二选一。重贴图的文字风格、`--image-style-url`、`--multiview-image-urls` 三者择一；多视图重贴图要求该 API 的 `ai_model:"meshy-7"`，0.4.0 无独立模型选项时经已核对的 `--data` 传入。多个 LOD 共用同一 SOURCE_ID，分别建任务。remesh 的 `--decimation-mode` 会覆盖固定目标，不要同时依赖两者。

**绑骨与动画**：只对适合的人形、带贴图 GLB，核实实际面数 <=300,000、四肢清晰及姿势后进行。先检查 task snapshot 的面数：

```powershell
meshy inspect faces --task-json "PROJECT_DIR/task_SOURCE_ID.json" --max-faces 300000 --output-schema v1 --format json --no-update-check
meshy rigging create --input-task-id SOURCE_ID --height-meters 1.7 --async --project "PROJECT_DIR" --stage rigging --workspace "WORKSPACE" --output-schema v1 --format json --no-update-check
```

面数检查 exit 12 为不合格、13 为未知，未知不能当合格。云端 rigging 通常提供 walking/running；先检查已返回动画能否满足请求，别猜动作 ID。需要自定义动画才查询并使用实际目录：

```powershell
meshy animation-catalog list --search walk --output-schema v1 --format json --no-update-check
meshy animate create --rig-task-id RIG_ID --action-id ACTION_ID --async --project "PROJECT_DIR" --stage animation --workspace "WORKSPACE" --output-schema v1 --format json --no-update-check
```

`text-to-motion` 产生独立骨骼片段，不能当作已经给目标角色合入动画。需要引擎 Idle/Walk 合并和验证时继续项目引用中的本地动作包步骤。

## 下载、预览与引擎衔接

先列真实资源，再下载所需资产：

```powershell
meshy download --task-json "PROJECT_DIR/task_TASK_ID.json" --list --output-schema v1 --format json --no-update-check
meshy download --task-json "PROJECT_DIR/task_TASK_ID.json" --model-format glb --output "OUTPUT_GLB" --project "PROJECT_DIR" --stage delivered --workspace "WORKSPACE" --output-schema v1 --format json --no-update-check
```

rigging 的选择键是 `result.rigged_character_glb_url`，animate 是 `result.animation_glb_url`，替换 `--model-format glb` 为相应 `--asset`。预览以 listing 中真实 thumbnail key 下载并打开；某阶段缺图时回看 lineage 中的生成预览，不因此重生。

快照里的签名 URL 不是长期存档。下载失败时先以同一 `--resource RESOURCE --task-id TASK_ID` 重新查询现有任务再下载；不能保证过期产物仍可下载，不把刷新查询说成延长资源保留期限。

用于 RurixForge 时，同时保存 `<模型文件名>.glb.json` 侧车（完整文件名后加 `.json`）。必须先核实任务快照的 ID、所属资源、成功状态、所选 asset 与本地 GLB；不把 2D task、rigging task、animation task 混当同一类型。侧车示例：

```json
{
  "backendId": "meshy",
  "kind": "mesh",
  "sourceRefs": [".forge/tmp/meshy/<job>/task_<id>.json"],
  "generatedAt": "<实际 UTC ISO8601>",
  "meta": {
    "provider": "meshy",
    "taskId": "<实际产出任务 ID>",
    "operation": "<实际 CLI resource>",
    "assetKey": "<实际选择的 asset>",
    "aiModel": null,
    "consumedCredits": null
  }
}
```

已知字段按快照填写；不知道的值保留 null，不编造。CLI 后续云处理继续使用 CLI 自己的任务记录；原生工具当前仅按字符串读取侧车 taskId，不能自动识别其资源。特别不能将 animate 的 producing ID 当作 rigging ID 再传原生 cloud animate。

插件得到的最终文件可交给项目的本地精修、绑骨或动作包工具，以及 `mcp__gen-model__gen_character_publish` / `mcp__gen-model__gen_accept`；选择本地处理时显式 `track:"local"`。完成五阶段适用门禁后再发布，侧车及 sourceRefs 随阶段保留。具体参数见 [项目原生管线](rurixforge-pipeline.md)。

## 成本与恢复

授权范围内的阶段继续完成。只在生成任务真正需要时用 `meshy balance` 检查余额；估价可用 `meshy make "..." --dry-run`（仅适用于它支持的链），其他操作核对 [官方价格](https://docs.meshy.ai/en/api/pricing)。只以 finished task 的实际 `consumed_credits` 报告消耗，未知就是未知。

exit 6/7 的安全查询按限流/网络状态恢复；不把通用重试包在 create 外层。任何本地保存失败都保留远端 ID；已有模型追加 LOD、纹理或格式时只执行缺少步骤。官方插件升级时重新核对版本和 help，保留上述任务复用与项目发布约束。
