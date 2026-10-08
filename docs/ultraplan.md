# UltraPlan 工作流

UltraPlan 在 Coding 会话中把完整游戏介绍依次推进到可验收 MVP。可选择 Forge 或 Codex agent 引擎；2D 默认使用 Godot，3D 可选择 Godot 或 rurix（普通新建项目的 3D 默认仍为 rurix）。玩法、场景与资产通过 Forge 工具制作。

## 使用

1. 选择 **UltraPlan**，输入游戏介绍。已有内容由编排器并行派出三个探索任务，分别检查资产美术、场景玩法、项目文档。空项目不探索无关的默认 Demo 项目。
2. 填写分章节问卷。服务端必定加入正式实现技术栈确认题：新项目选 **2D · Godot（默认）**、**3D · Godot** 或 **3D · Rurix**；已有项目明确确认沿用现有维度与后端。该题必答，不能委托模型或自填，批量委托其他问题后也须亲自确认。其余问题支持推荐选项、自填、委托和本地草稿；修改需求会产生新版本。
3. 提交答案后，单个 `web-demo-builder` 在自己的目录制作 HTML/CSS/JavaScript Demo。完整需求包分段保存，必须全部读取。系统浏览器执行真实键鼠操作、状态断言及截图检查后才发布试玩入口。
4. 试玩后选择修改、重试、回退或批准。批准当前版本才生成正式计划。规格与计划提示词明确要求规划技术和实施细节：资源与脚本路径、数据结构和玩法状态、模块接口、输入与存档方案、后端能力限制、任务依赖、实现方法及验收步骤。计划与任务图、检查表、交付入口及目标后端一起绑定版本和哈希。
5. 在计划卡或 Plan 页签选择 **确认并开始制作**。系统自动进入 Team：独立任务最多四个并行，共享场景、引擎和试玩任务串行。QA 或终审失败会定向修复，默认最多五轮。
6. 自动检查与终审全部通过后，集中完成人工检查。必要项必须通过；可选项跳过需写原因。失败项自动派返修，修复后重新验收。完成页提供入口、操作说明和记录。

UltraPlan 沿用会话权限。计划只读权限不能开始制作；确认按钮不会提升权限。工具、浏览器、后端或模型能力缺失都会显示可诊断的失败，不把“未验证”当作通过。

## HTTP 与事件契约

阶段：`discovery → questionnaire → demo_review → plan_review → production → acceptance → done`。每个阶段另有 `waiting / running / failed` 相位。

模型轮次使用 `POST /api/forge/sessions/{id}/ask:execute`。首次请求传 `mode:"ultraplan"` 与 `userInput`。后续请求携带 `ultraplan:{id,action,rev,...}`：

- `answer`：问卷版本与完整 `answers`；同版本失败重试可以复用已保存答案。
- `revise_demo` / `approve_demo`：当前 Demo 版本；修改需要反馈文本。
- `revise_plan`：当前计划版本与反馈文本。
- `start_production` / `resume_production` / `fix_production`：`mode:"team"`，使用当前流程版本和适用的权限确认。

`GET /api/forge/sessions/{id}/ultraplan` 返回流程、问卷、答案、Demo 宿主地址、检查、制作记录、验收、目标和交付信息。

不产生模型轮次的操作使用 `POST /api/forge/sessions/{sessionId}/ultraplan/{action}`。这里路径中的 `sessionId` 是会话 ID，正文 `id` 是当前 UltraPlan 流程 ID：

- `acceptance`：正文为 `{id,rev,round,results:[{id,status,note?}]}`。`rev` 必须等于当前 `planRev`，`round` 必须等于 `acceptanceRound`，每个人工检查必须恰好提交一次。`status` 为 `pass / fail / skip`；`fail` 必填原因，只有可选检查可以 `skip` 且也须填原因。提交时再次核对计划、项目指纹、自动检查与终审证据；返回 `next:"fix"` 或 `next:"done"`。
- `rollback_demo`：正文为 `{id,rev}`，`rev` 是当前 `demoIteration`。仅在空闲的 `demo_review` 阶段恢复之前的可用快照，生成新的 Demo 版本并清除验证标记，必须重新探测、试玩和批准。
- `restart`：不要求正文，清除会话流程指针和试玩路由，保留原始产物；没有流程时幂等返回 `{ok:true}`。

有活动 run 时返回 `409 SESSION_BUSY`。过期流程、版本、轮次、重复验收或证据失效不能推进状态，客户端应刷新流程后再提交。

SSE 沿用 `ultraplan.stage`、`ultraplan.questionnaire`、`ultraplan.spec.ready`、`ultraplan.demo.ready`、`ultraplan.plan.ready`、`ultraplan.production.started`、`ultraplan.acceptance.ready`、`ultraplan.acceptance.recorded` 和 `ultraplan.done`，另复用任务、子 agent、工具及审批事件。

## 文件与恢复

流程资料保存在工作区 `.forge/ultraplan/<slug>/`；正式计划在 `.forge/plans/<slug>.plan.md`。需求包包含原始介绍、补充、探索全文、问卷原始问答、规格、资产引用和模型代定事项。`requirements.json` 记录流程 ID、问卷/Demo/计划版本、需求内容版本哈希及各文件哈希；Demo 的 `_requirements/chunks/` 每段最多 3000 字符，避免工具反馈截断。

`team-plan.json` 保存依赖图；`checks.json` 包含自动检查及人工步骤/预期；`target.json` 在问卷答案提交时固定已确认的后端与维度，并规范对应的渲染方法和驱动，需求定稿不能改选。已有项目配置或环境后端覆盖与答案冲突时需重新确认，不会静默切换；旧问卷缺少 `implementation_stack` 时需重新生成问卷。`delivery.json` 指定入口与操作；`production.json`、`verification/`、`reviewer.json` 和 `acceptance.json` 保存执行、截图、终审和验收证据。

父流程独占会话运行锁，重复请求不能并行派工。修改计划、任务图、检查表、交付信息或目标后端会使批准哈希失效。重启会将中断相位标记为可恢复失败；已完成任务保留，运行中的任务须恢复核对，不能凭旧文本结果直接判定验收成功。

自动检查和终审还绑定 `projectRoot` 与 `projectFingerprint`。指纹覆盖 `forge.toml`、其声明的内容/脚本目录与入口场景、兼容的源代码目录及根目录源文件；不包含 `.forge` 流程记录和缓存、`.git`、`.godot`、`node_modules`、`target` 等生成目录。检查场景必须属于这些正式发布文件，不能使用缓存场景冒充当前游戏。游戏源码或资产在验证后变化会使证据过期，必须重跑检查和终审；截图本身按 SHA-256 核对，落盘报告须与制作记录一致。

两种引擎均使用统一 AgentJob 规格和结果。`jobs/` 保存流程/角色/需求版本、模型、推理档、读写范围、执行代次、真实终态和工具记录。Forge 的未知运行态会先停为 recovery-required，下一次显式恢复才重新执行；循环耗尽不会标记完成。Codex 使用每条活动流程独立的 app-server 进程，每个角色独立受管线程，另保存 thread/turn 映射，不覆盖普通聊天线程。原生沙箱只读；写入仅经 Forge 动态工具与权限服务。Codex 通过 `skills/list` 枚举后在当前线程的 `skills.config` 禁用继承技能；MCP、插件、shell 和内建委派也在当前线程隔离，不修改用户配置。能力扫描失败或隔离配置警告会停止任务。工具能力不兼容会明确失败；停止必须确认对应 turn 终态，不能只凭 interrupt ACK 假定完成。

## 验证与发行

`ultraplan_verify` 只在正式制作阶段接受 `{id,matrix}`，服务端执行工具并记录结果，不能提交模型自报的 `pass`。`id` 必须来自批准检查，矩阵场景须与该检查一致。玩法要求进入 Play、实际输入以及组件/位置断言；工具调用都经过会话权限服务并作用于目标项目。后端不符、控制调用错误、运行异常、缺报告或缺截图均判失败。最终人工必要项尚未通过时不能进入 `done`。

首次视觉检查没有基准图时使用 `screenshot_nonblank`，保存真实视口 PNG 并把截图回传给 QA/reviewer。它只证明画面有内容，布局、美术、可见操作反馈与批准需求仍由 agent 看图审阅。已有合适基准时使用 `screenshot_ssim`：`golden` 必须是工作区内现存图像，与请求尺寸一致；UltraPlan 限定尺寸为 8×8 至 1920×1080、阈值为 0.5～1，默认 0.98。SSIM 用于回归比较，也不单独证明游戏美术或玩法质量。

`flow_tests.rs` 的四个 stage contract 覆盖 Forge/Codex × rurix/Godot 的协调器状态、产物与恢复契约。它们使用脚本构造的模型输出和验证记录，不会调用真实 LLM，也不代表四种组合已完成模型生成游戏。可选的真实浏览器 contract 仅把 Demo 探测换成真实浏览器。实际执行范围和结果见 [验证记录](ultraplan-validation.md)。

浏览器探测运行器是 `tools/e2e/web-demo-probe.mjs`，使用 `playwright-core` 与系统 Edge/Chrome，不下载浏览器。先在工作区安装构建依赖（包括 `tools/e2e` 的 `playwright-core`）；`pnpm --filter @forge/desktop build` 在上游产物检查后生成 `apps/desktop/dist/ultraplan-runtime/`，也可单独运行 `pnpm --filter @forge/desktop build:ultraplan-runtime`。目录包含执行构建的 Node、runner、解引用 pnpm 链接后的完整 `playwright-core`、版本清单与说明。当前 core 包没有外部 npm 依赖；若后续版本增加依赖，构建会明确失败要求更新分发脚本。

发行时完整保留该目录，放在 `forge-agentd` 旁的 `ultraplan-runtime/`、其 `resources/ultraplan-runtime/` 或上一级的 `resources/ultraplan-runtime/`，也可设置 `FORGE_ULTRAPLAN_RUNTIME_DIR`。显式配置的运行时缺文件会直接失败。开发环境才回退到工作区暂存目录或源码与系统 Node。仓库当前没有安装器配置；此脚本产出可搬迁运行时，不等于已生成安装包。目标电脑仍须安装 Edge 或 Chrome，缺浏览器不会假通过。模型制作质量仍以真实游戏验收结果为准。
