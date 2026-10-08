# UltraPlan 验证记录

日期：2026-10-03 UTC。本记录只列本次实际执行的浏览器、分发、原生引擎及已安装 Codex 兼容性检查；Rust/前端全集及协调器契约测试结果由主验证记录另行补充。


## 最终代码回归与构建

- `cargo test -p forge-agentd --bin forge-agentd -- --include-ignored --test-threads=1`：**546 通过，0 失败，0 忽略**。包含 Forge/Codex × rurix/Godot 四种阶段契约、真实 Edge Demo 输入闭环、版本/重复动作、取消/恢复、定向返修、文件修改后证据失效、Codex 线程隔离、Forge 任务持久化和真实终态。四种组合中的模型输出与正式制作证据由脚本提供；Edge Demo 的输入探测是真实浏览器。日志：[agentd-tests.log](../evidence/ultraplan-validation/2026-10-03-final/agentd-tests.log)。
- 前端 10 个相关测试文件：**206 通过**。覆盖 UltraPlan 工作流/状态、问卷、Demo、计划确认及 Composer/API；`pnpm --filter @forge/client typecheck` 通过。命令和结果另存 [summary.json](../evidence/ultraplan-validation/2026-10-03-final/summary.json)。
- `cargo build -p forge-agentd`、`pnpm --filter @forge/client build`、`pnpm --filter @forge/desktop build:ultraplan-runtime` 均通过。对应产物为 `target/debug/forge-agentd.exe`、`packages/client/dist/`、`apps/desktop/dist/ultraplan-runtime/`。Rust 存在未使用项等警告，Vite 存在既有 chunk 大小与混合 import 警告；未生成整套安装器。后端构建日志：[agentd-build.log](../evidence/ultraplan-validation/2026-10-03-final/agentd-build.log)。

本次修复并覆盖：无效数值断言不能 PASS、播放控制错误不能忽略、未验证/空任务/终审拒绝不能完成、修复后原任务须重新 QA、Windows 路径别名不能绕过 Demo 或流程目录限制、取消授权等待后不能继续创建项目、Demo 与计划的外部修改会使确认失效。

## Web Demo 与可搬迁运行时

执行：

```powershell
node --test tools/e2e/web-demo-probe.test.mjs apps/desktop/scripts/ultraplan-runtime.test.mjs
```

结果：4 项通过，0 失败、0 跳过。日志：[node-tests.log](../evidence/ultraplan-validation/2026-10-03-browser-runtime/node-tests.log)。

- 真实系统浏览器接收键盘事件，Demo 状态发生变化，断言与截图成立；错误期望值、缺字段、仅运行 hooks 而没有真实输入均保持失败。
- 运行器拒绝非授权本地路由、未知 hooks、越界输入时长及危险属性路径。
- 将 Node、runner 和完整 `playwright-core` 复制到新的临时目录；从项目外 cwd、空 `PATH`/`NODE_PATH`/`NODE_OPTIONS` 启动复制的 Node，仍能驱动系统浏览器并得到真实输入、状态与截图证据。测试副本已清理。

核对版本：Node v24.19.0、Playwright core 1.62.1。core 包无外部 npm 依赖，复制时解引用 pnpm 链接，依赖不指回开发目录。系统 Edge/Chrome 是明确的外部前提；未测试无浏览器机器上的完整安装体验，也未生成安装器。

## Rurix 与 Godot 实际小游戏

执行：

```powershell
node tools/e2e/ultraplan-native-smoke.mjs
```

结果：两个后端均通过。证据：[summary.json](../evidence/ultraplan-native/2026-10-03T02-34-01-032Z/summary.json)、[Rurix 报告](../evidence/ultraplan-native/2026-10-03T02-34-01-032Z/rurix/report.json)、[Godot 报告](../evidence/ultraplan-native/2026-10-03T02-34-01-032Z/godot/report.json)。

每个后端使用新的微型项目、独立 MCP 进程和随机引擎端口。游戏包含角色、目标触发器、地面及真实 `.rxgraph` 逻辑。Playwright 的两次 ArrowRight 经输入桥接进入该私有 MCP，角色位置为 `[2,0.5,0]` 且仍是 `player`；真实鼠标点击进入指针输入工具后，角色到达 `[2,1.5,0]`，目标触发器把标签改成 `winner`。每个后端记录两项位置/组件断言、输入序列、前后 GPU 截图及 21 条事件，没有 unsupported、call_error、host.crashed 或 anim.warn。

Rurix 使用 Intel(R) Graphics；Godot 报告实际配置 `forward_plus/d3d12`，启动时按 GPU 提示对齐一次。两边的前后画面均已查看，是实际场景帧。测试结束均回到 `edit`；所创建的宿主 PID 33980、18356 已确认退出。既有游戏和桌面会话未参与测试。

这次 native smoke 复用了工作区已有的 2026-09-30 引擎/MCP/Godot runtime 二进制，没有重新编译它们。验证经过真实 MCP、玩法解释器和 GPU，浏览器页面是测试输入/画面桥接器；它没有经过完整 IDE 的输入组件、新 `ultraplan_verify` HTTP/agent 调用或真实模型制作流程。脚本后来增加二进制哈希清单与可复用 `matrix.json` 的输出；本次已保存的报告不追溯声称具有新增产物。

## 已安装 Codex app-server 的真实兼容性

可复用脚本：`tools/e2e/ultraplan-codex-smoke.mjs`。指定 `--codex <可执行文件>`，默认只检查协议与隔离配置；显式添加 `--live` 才会发一个只调用回显动态工具的短模型轮次。

已执行一次 live 探测：[报告](../evidence/ultraplan-codex/2026-10-03T02-53-42-101Z/report.json)。本机 `codex-cli 0.159.0-alpha.12.1` 成功完成 `initialize`（`experimentalApi:true`）、`model/list`、带动态工具的只读 `thread/start` 与 `turn/start`。默认模型 `gpt-6.1-sol` 实际发出 `item/tool/call`，只调用 `forge__compat_echo`；回显响应后出现 `dynamicToolCall` 的 `completed / success:true`，随后中止轮次并收到真实 `interrupted` 终态。未执行原生或其他反向工具请求。线程为 ephemeral，服务端报告沙箱 `readOnly`、`networkAccess:false`，私有 cwd 未新增文件，进程退出码为 0。

兼容性探测暴露了旧隔离字段问题：`features.skills` 被忽略，`features.codex_hooks` 已弃用。[警告原始记录](../evidence/ultraplan-codex/2026-10-03T02-54-54-110Z/report.json)只保留非敏感诊断。修正配置采用 `skills/list` 发现路径，再以 `skills.config:[{path,enabled:false}]` 覆盖，保留 `features.hooks:false` 并删除旧字段；与[官方配置参考](https://learn.chatgpt.com/docs/config-file/config-reference)一致。

修后仅做只读复测，没有再调用模型：[修后报告](../evidence/ultraplan-codex/2026-10-03T03-04-29-412Z/report.json)。实际发现 43 个技能，路径均指向 `SKILL.md`；线程接受全部禁用覆盖且零配置警告。独立进程以 `-c` 传入相同覆盖，`skills/list` 回读 43 项全部 `enabled:false`，0 错误。当前协议没有线程级技能配置回读接口，因此这分别证明线程参数接受性与同配置在独立进程中的禁用效果，不宣称读取了不存在的线程配置接口。全程不读取认证文件、不调用配置写入接口，stderr 仅计数后丢弃，证据不保存原始有效配置。

## 尚未由本记录证明的范围

Forge/Codex × rurix/Godot 四个 stage contract 使用脚本输出测试协调器契约，不能计作 live LLM 生成游戏通过。即使启用其中的真实浏览器 probe，模型输出仍是脚本构造。

本机 Codex 的一轮真实模型动态工具调用已经验证；Forge provider、完整 UltraPlan 模型游戏制作、用户对 Demo 和计划的批准，以及最终人工试玩仍未由本记录覆盖。测试夹具、协调器契约和一次回显调用不替代完整游戏验收。

## 2026-10-03 后端默认值与询问确认补充

2D 未配置后端时默认 Godot；3D 新建可选择 Godot 或 rurix，未指定时保留 rurix。显式后端配置保存后继续保留。UltraPlan 服务端加入必答 `implementation_stack` 单选题，禁止委托或自填；答案提交即固定目标和对应 method/driver。需求定稿拒绝改选，制作初始化前再次核对当前环境覆盖和已有项目配置。规格与计划提示词明确要求规划技术与实施细节。

本轮实际执行结果：

- `cargo test -p forge-agentd --bin forge-agentd ultraplan:: -- --test-threads=1`：45 通过、0 失败、1 忽略（真实浏览器变体）。包含 Forge/Codex 各自的 2D/Godot、3D/Godot、3D/rurix 六种阶段契约，以及已有项目变化、环境覆盖冲突、问卷强制确认和定稿锁定。阶段契约仍使用模型与验证夹具，不代表 live LLM 游戏制作。
- `cargo test -p forge-agentd --bin forge-agentd project::tests -- --test-threads=1`：3 通过；`cargo test -p assetd --lib project::tests -- --test-threads=1`：6 通过。
- `cargo test -p engine-scene-mcp --bin engine-scene-mcp render_resolution_follows_02_7_3_priority -- --test-threads=1`：1 通过。首次未限定 `--bin` 的命令因正在运行的 `target/debug/engine-scene-mcp.exe` 占用而构建失败；限定单元测试目标后通过，没有结束活动宿主。
- `pnpm --filter @forge/client exec vitest run test/workspaces.test.tsx test/forgeApi.test.ts`：16 通过；`pnpm --filter @forge/client test test/questionnaireCard.test.tsx`：27 通过；`pnpm --filter @forge/client typecheck` 和变更文件空白检查通过。

额外全量执行并非全绿：后端全套为 610 通过、1 失败、1 忽略，失败项 `tests::swarm_execute_40_blocks_add_rigidbody` 在根工作区创建实体，却由 swarm 固定向 `projects/demo` 宿主查询，报告实体不存在；两处作用域路由在 HEAD 已相同，本轮未修改该路径。一次前端命令额外执行全套，1175 通过、1 失败，失败项 `designBoard.test.tsx:905` 的请求桩假设每次 fetch 均有 body。相关定向测试通过；未将上述全套执行称为通过。

存储检查采用 `D:/Rurix/ci/storage_health.py` 的 start、D=3 GiB preflight 与 finish。08:21 UTC 快照 D 盘剩余 106673692672 字节（约 99.34 GiB，18.02%，healthy）；08:23 UTC 收工复测为 103116738560 字节（约 96.04 GiB，17.41%，healthy）。E 盘剩余 102398074880 字节（约 95.37 GiB，10.00%，warning），本任务未安排 E 盘写入。无临时工具链副本；未清理仍有活动 Cargo 进程的构建缓存，回收量为 0。源码、现有交付物和历史证据保留。

存储：本批次只在 D 盘写入代码和紧凑证据，临时 runtime 测试副本位于 C 并已清理；D 开工剩余 114.90 GB（19.40%）为 healthy，E 10.01% warning 且未写入。有效截图、报告与夹具作为测试证据保留。
