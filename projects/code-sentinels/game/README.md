# 编译防线 / Code Sentinels

真实 RurixForge 原生 2D 塔防，8 波入侵、3 条数据路径、12 个部署节点。浏览器只显示原生 GPU 帧、发送操作并读取引擎状态；战斗在原生 Rust DLL 中运行。

## 游玩

双击 `../dist/CodeSentinels-Windows/Start-Game.cmd`，或解压 `../dist/CodeSentinels-Windows.zip` 后双击启动。独立包内含 Node 与原生引擎，不需要 RurixForge、Rust、Python、账户或 API 密钥。

选角色后点击空位部署。点击已有单元后，用右侧按钮升级或回收。`1–4` 选角色，`N` 开启下一波，`Q` 释放所选通路的热修复，空格暂停。初始 420 算力；每波可自行准备后开始。

- VS Code：80 算力，快速单体编译攻击。
- PyCharm：115 算力，范围调试攻击；工具链可穿透护甲。
- DeepSeek：145 算力，减速并标记敌人。
- GPT：165 算力，同路伤害支援并每 4 秒提供算力。
- 同路两款开发工具或两位 AI 激活 1 级协同；双工具加 AI 激活 2 级；四者完整链激活 3 级。
- 单元可升至 3 级；回收返还已投入算力的 70%。热修复对所选通路造成伤害、减速和标记，冷却 25 秒。

角色采用已溯源的鲸鱼娘女仆版与白龙娘投影版；具体创作者与原视频链接在游戏「角色档案」。两位动作均来自项目 Codex 模式 Agent 调用真实图生视频后的截帧，保留了原视频与生成溯源，独立包只携带游戏所需资产。

## 原生结构

`Content/Scripts/sentinels.rs` 是单文件原生模拟；`.rxgraph` 适配输入、逐帧运行、图集动画和状态发布。`Main.rxscene` 使用零重力、XY 平面、正交半高 6，相机位于 `[0,0,10]`。

状态只读实体：`CS_State.translation=[算力,核心血量,波次]`，`scale=[阶段,击杀,最高协同]`。阶段 0=布置，1=战斗，2=胜利，3=失败。`CS_StateAux` 和 `CS_Cell0..11` 提供技能、出怪、速度与升级信息。

输入为 `logic_inject_input {action:"cs",value:...}`：部署 `1000+格号*10+类型`，升级 `2000+格号`，出售 `3000+格号`，技能 `4000+通路`，下一波 `5000`，原生暂停切换 `6000`，速度循环 `7000`，重置 `8000`。UI 暂停使用原生 `play_pause/play_resume`，连动画一起暂停。

## 可重放验证

- `build_native.py`：重建正式场景、逻辑适配图与原生几何地图。
- `import_video_atlases.py`：从真实视频图集导入 `.rxsprite`，验证不同帧、统一锚点与尺寸，运行图集均小于 4096。
- `playtest_native.py`：独立引擎检查图校验、扣费、占格事务、运动、击杀和真实 GPU 帧。
- `playtest_campaign.py`：仅正常经济与普通输入完成全部 8 波；`--defeat` 验证无防守失败。
- `playtest_animation.py`：验证两位角色实际帧变化、GPU 像素变化以及 idle/cast 切换。
- `build_portable.py`：调用真实 project/pack API，追加最终编译 UI、便携 Node 与受限本地桥接。
- `test_portable_bridge.mjs`：独立进程测试无编译器冷启动、原生 WS 帧与购买输入、重开、作用域与错误检查。

Python 图像脚本需要 Pillow；本项目使用 Codex 随附的 Python。测试程序启动自己的随机端口引擎，与正在游玩的工作区实例分离。

## 验收证据

`native/campaign-victory.json`：最终场景与最终引擎，独立 PID 55644，正常资源策略完成 8 波、164 击杀、20/20 HP、最高协同 3。

`native/campaign-defeat.json`：独立 PID 2156，无防守在第 2 波真实失败，HP=0，phase=3。

`native/native-animation-test.json`：最终 GPT v2，独立 PID 53156；DeepSeek 与 GPT 均 frame 0→5，真实 GPU 图像不同，均观测 idle/cast，未观测运行错误。运行时图集分别为 2588×3086 和 3086×2294；每角色有 32 个不同视频帧。

`native/portable-regression.json`：便携包独立 PID 38448；强制禁用 rustc，7 项 HTTP/WS 集成测试通过，收到真实 960×540 GPU 二进制帧，购买扣费与重开均正确。

项目主任务另通过真实浏览器人工验收：便携包冷启动、3 路 VS Code 420→180 算力、购买 GPT 剩 15、波次/2×速度/自动击杀；第 2 波清空后 HP20、击杀23；顶路完整四单元链协同3，暂停可用，实际39–40 FPS，浏览器无 error/warn。后续最终浏览器证据由主任务保存。

原生库 46 项测试全部通过（包括显式配置本机 Rurix 编译器的既有标量兼容测试）；pack 7 项通过；游戏规则 7 项通过。

## 本轮修复的项目能力

新增通用 `math.vec3` / `transform.compose` 纯节点，原生脚本输出可直接构成实体位置与缩放。增加 `.rs` 单文件原生 C ABI 脚本后端，保留既有 `.rx` 接口；构建以源码与编译器版本缓存。便携包同时保存源码与 DLL SHA-256 清单，没有编译器时只接受匹配源码及二进制校验的预构建模块。

修复 project/pack 未递归 `.rxsprite` 导致漏打图集的缺陷，加入原生 Rust DLL 清单打包。便携 UI 使用受限 loopback 桥接；冷启动由 UI 执行 `asset_reload→scene_load→play_enter`，重开先 `play_exit`，解决 `--game` 模式禁编辑接口与 UI 重开的冲突。

发现的 Rurix 上游坏码保存在 `native/probe.rx` / `probe.ll`：静态状态 unsafe 块编译成功，但 LLVM 从未初始化的 `%l0` 返回（预期 HEALTH 累加，实测为无意义值）；数组导出还会按原 C 子集限制被拒绝。本游戏通过已实测的原生 Rust 脚本后端实现完整状态、数组与函数调用，上游复现完整保留，未把这些探针当作可用游戏逻辑。
