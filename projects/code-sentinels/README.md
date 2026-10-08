# 编译防线 · 动态战线（V5）

V5 候选版已完成 44 个真实源视频，导入 12 类建筑/炮台与 8 类特效的 20 个图集，共 1856 帧。建筑包含落地、随实际经营状态播放的工作循环、摧毁和完整残骸尾段；大厅与资源界面展示原生战役进度和资源快照。

运行候选目录 `dist/CodeSentinels-V5-Windows` 中的 `Start-Game.cmd`，在大厅点击“开始单人行动”；`Start-Game-GPU-Particles.cmd` 开启附加 GPU 粒子。游玩不需要编译器、开发环境、账号或生成服务。详见 [V5 说明](game/V5-README.md) 和 [V5 QA](game/v5/QA.md)。

已验证前端全量 67 文件/735 项测试，后续 UI 修复另通过最终 3 文件/32 项定向补测，两轮不累计为不同用例总数。原生 CPU 6 组场景保留 42 条检查记录，完成三关十二波。独立包在 NVIDIA GeForce RTX 5060 Laptop GPU、1280×720 下实测普通模式 54.75 FPS、粒子模式 54.72 FPS，两者均通过 30 FPS 门槛。

真实浏览器操作验收已通过，覆盖大厅、资源定位、快建后单次正确布线、科技提示、实际技能命中扣费和房间准备。最终 Web 入口为 `index-u-AInC4p.js`，144 个生产文件通过内容一致与 HTTP 检查；包内 VC 运行库及原生 DLL 冷启动加载也已验证。结果范围详见 QA，不等同于所有电脑的兼容性验收。

局域网目前仅提供准备室，双玩家 PVP 战斗同步尚未接入。V5 在开发客户端使用 `version=5`，场景为 `Content/Scenes/CommandV5.rxscene`，原生脚本为 `Content/Scripts/sentinels_v5.rs`。

## 保留的 V4 · 算力边疆

以地图交互为中心的 2D 实时经营塔防：建设电站和数据中心，安装真实型号显卡，通过电力线、算力线和移动基站经营前线；移动 AI 使用随身算力，VS Code 与 PyCharm 为固定炮台。cudad 护城河的真实闭合区域可投入算力形成护盾，研究院逐级解锁科技与兵种。

**[下载 V4 独立包](dist/CodeSentinels-V4-Windows.zip)**，解压后双击 `Start-Game.cmd`。实验 GPU 粒子入口为 `Start-Game-GPU-Particles.cmd`。详细经营规则、操作及开发边界见 [V4 说明](game/V4-README.md)。

开局只有指挥核心、2000 金币和科技 0。先在矿脉建采集器获得持续经费，再建设风电、数据中心与电力线，点击数据中心安装显卡，最后铺算力线支持炮台；研究科技后展开移动 AI 与基站作战。

`B/U/I` 打开建筑/单位/显卡牌组，`L/C` 铺电力线/算力线，`W` 拖建城墙，`F` 切换充盾；拖框选择，右键或 `A` 后点击移动，`Q` 技能，`E/R` 升级/回收，`Enter` 安装显卡，`N` 下一波，空格暂停。地图支持滚轮缩放、方向键或中键平移、小地图定位，`H` 打开完整手册。

运行 `Start-PVP-Lobby.cmd` 可建立实际局域网双人准备室，支持蓝红席位、准备与短时重连。**多人战斗同步尚未接入**；本版预留玩家对战与基地攻防的权威协议、身份归属和指令日志，详见 [多人基础设施](game/v4/MULTIPLAYER.md)。

战斗、经济、寻路、电力、算力、科技和闭合防御由原生 Rust 核心运行；浏览器展示 Rurix 原生渲染帧并发送指令。新建筑美术由内置 imagegen 制作，人物与技能继续使用既有真实图生视频帧。真实 GPU 图片及人物来源保留在 [显卡来源](references/sources-gpu.md) 与 [角色来源](references/sources.md)。游戏内数值为平衡设计，不代表实际硬件功耗、售价或性能。

V4 制作时按当时要求未追加测试；上方 V5 的独立验收结果不倒推为 V4 或其他旧包的验收结论。

## 开发入口

保留的 V4 入口使用 `?play=code-sentinels`，场景为 `Content/Scenes/Command.rxscene`，原生脚本为 `Content/Scripts/sentinels_v4.rs`；V5 使用 `version=5`。需要已构建的 RurixForge 服务时可使用 [Start-Game.ps1](Start-Game.ps1)。独立包不依赖开发服务、账号或编译器。

## 历史版本

- [V3 战术牌组独立包](dist/CodeSentinels-V3-Windows.zip) 与 [V3 说明](game/V3-README.md)：用户已认可的卡牌界面版本。开发客户端可用 `version=3` 进入旧界面。
- [V2 开放战线独立包](dist/CodeSentinels-V2-Windows.zip) 与 [V2 说明](game/V2-README.md)：显卡经济、三关地形与真实技能帧。
- V1 包及旧原生场景继续保留。新包不覆盖旧运行实例或旧存档。
