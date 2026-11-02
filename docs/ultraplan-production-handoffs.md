# UltraPlan 制作与恢复契约（2026-10-18）

模型轮次结束只表示 agent job 结束。制作任务必须调用 `ultraplan_task_result`，并由宿主验证批准产物与局部检查后，才能从 `running → verifying → completed` 放行下游。缺结果、取消、提供方失败及能力不足均不得被当成已确认产品缺陷自动返工。

批准任务须包含 `requiredTools`、具体文件 `outputs` 和覆盖全部产物的 `handoffChecks`。`.rx`、`.rxgraph`、`.rxscene` 分别至少经过 `rx`、`graph`、`scene` 检查；运行矩阵是额外检查。宿主核对角色最终工具集、MCP 可用性、会话权限与实际工具链路径。编译器显式设置失效直接阻塞，不回退到固定磁盘。

`pending-submissions` 保存模型申请，`task-results` 保存宿主结果与交接凭据。下游收到完整上游报告、产物摘要和回执路径。`execution.json` 保存流程/计划身份、attempt、历史、能力指纹、修复预算与候选 QA 缓存。程序恢复先核对原 job 的精确终态；不能确认时停止，不能重放。

产品修复只更新批准任务原 ID 的委派并授权一次新 attempt，不新增第二条修复链。五轮预算跨恢复和计划版本继承；环境阻塞及补取证不消耗产品修复预算。相同能力指纹不会重复派发阻塞任务。全局 QA 按候选产物指纹集中执行批准检查；只有宿主确认的产品失败进入修复，缺失/过期证据只补取证。全部真实检查通过后才进入 reviewer。

## 本次验证

证据位于 `evidence/ultraplan-quality/2026-10-18/`。定向 Rust 回归去重 413 项通过；前端状态相关 47 项与 TypeScript 检查通过。Rust 范围包括排程、持久恢复、宿主动态工具契约、普通 agent、Codex、权限、原生工具与工具链。没有执行整个仓库所有测试。

`ultraplan::lifecycle::native_smoke::real_2d_godot_production_handoff_qa_and_review_gate` 已使用新建置的引擎、code-forge MCP 与 Godot runtime 实测通过。隔离 2D 产物经过真实交接检查；真实输入移动角色并触发胜利；QA 保存实际截图和检查结果；没有 reviewer 批准时，完成门禁正确阻止收尾。该切片采用确定性制作夹具，**不代表 live 模型独立完成完整游戏，也不代表人工试玩通过**。

## 部署与原会话

核心版本已部署到本机后端（PID 47112，运行程序 SHA-256 `0633E204B5B56C4BB57E7067E3F8E3B6E649A97ED378766A9DF655E89147BA48`）。新 Godot runtime 位于 `target/godot-runtime-ultraplan-20261018`，本次后端通过 `FORGE_GODOT_RUNTIME_DIR` 使用它；保留旧 runtime 以免中断其他视口。原 55 个会话保留，旧活动 job 已核对终态。

恢复《愤怒的小鸟》原会话时，新增制作派遣为零，服务端返回 `UP_CONTRACT_UPGRADE_REQUIRED`。旧批准 hash 保持 `298a94debd97105b6d64937d2b7e286b906f9784f7511da59a0f76c6afbdce9e`。实机工具链探测同时确认 `rx`、`rurixc` 均不可用。原计划、产物与历史证据保留；同范围未批准修订草案在原 flow 的 `recovery-20261018/`，保留 12 个原任务与 24 个历史修复关联。草案未标为可执行，没有偷偷批准或重建游戏。

实机检查另发现：开工前契约错误没有保存在流程面板。此显示补丁已通过新增回归并建置，候选 SHA-256 为 `E0B6597972690D020D72E27E0F7EB1AE9CCFB3F2C220EACBCA177C18F969F13B`；最后替换/重启被自动批准审核以 `blocked by policy` 拒绝，**该补丁尚未部署**。人工可审阅 `evidence/ultraplan-quality/2026-10-18/apply-final-ui-patch.ps1`；默认只读，显式 `-Apply` 才检查空闲状态、备份、替换及健康验证，失败回滚。当前核心版本的聊天错误仍完整显示阻塞原因。

构建复用 C 盘隔离缓存，D 盘只写源码、紧凑记录和部署文件，E 盘没有安排写入。start/finish 容量检查已记录；结束时 D 约 75.75 GiB / 13.74%，E 约 128.41 GiB / 13.46%。未删除项目、历史证据或有效交付物；没有把运行中引擎当作可清理缓存。
