# 编译防线 · Code Sentinels

使用 RurixForge 原生引擎制作的 2D 塔防。浏览器显示引擎真实帧流并传递输入；战斗、资源、射程、命中、协同、波次和胜负均由原生 Rust 游戏脚本运行。

## 开始游戏

**独立 Windows 版：双击 `dist/CodeSentinels-Windows/Start-Game.cmd`。** 已包含 Node 与原生引擎，不需要编译器、账号、API 密钥或正在运行的 RurixForge 服务。关闭启动窗口可停止游戏。

在 RurixForge 服务运行时打开：

**http://127.0.0.1:3080/?play=code-sentinels**

服务未运行时，执行本目录的 `Start-Game.ps1`。项目已注册为「编译防线 · Code Sentinels」，可以返回编辑器查看场景、逻辑图和精灵。

## 怎么玩

1. 点击「部署防线」，获得 420 初始算力。选择角色卡，再点三条通路上的空位。
2. 按 `1–4` 切换 VS Code、PyCharm、DeepSeek 娘、GPT 娘。准备后点「下一波」或按 `N`。
3. 入侵者从右向左进入；抵达左端会损伤核心。守住八波并击败最终 Boss 获胜。
4. 点击已部署单元可升级，最高三级；回收返还已投入算力的 70%。
5. 选择上、中、下路后按 `Q` 释放热修复：该通路伤害、减速、标记，冷却 25 秒。
6. 空格暂停/继续，右上切换 1×、2×、3×。失败后可重新部署。

## 协同玩法

- VS Code：80 算力，快速单体攻击。
- PyCharm：115 算力，范围攻击；与 VS Code 联动可穿透护甲。
- DeepSeek 娘：145 算力，减速和标记敌人，高阶协同可扩散控制。
- GPT 娘：165 算力，战斗中每 4 秒生产算力，并给同路其他单元增伤。
- 同一路有 VS Code + PyCharm，或两位 AI：一级协同。双工具 + 至少一位 AI：二级。四种单元齐全：三级。
- 协同提高伤害与攻速；满链强大，但要在三路生存与集中投入之间做取舍。

## 美术来源与动画

角色采用具体网络二创版本，来源详见 `references/sources.md` 和游戏内「角色档案」。不宣称存在统一官方 AI 娘化形象。

- DeepSeek：上善无形原型 / ZipZipPipe 女仆二创 / Neko3000 鲸鱼娘参考。
- GPT：YouTube ゆうまEthan〜 / JPEthan Token Monitor 白龙娘版本；参考仅展示半身，游戏也保留半身投影形式。
- 两张战斗定妆图及服务器花园背景由 Codex 内置 image_gen 生成；完整提示词保存在 `Content/Concepts/`。
- 两位角色动作由**本项目 Codex agent**调用真实 MiniMax 图生视频后截帧。真实原视频保存在 `SourceMedia/`；动画在 `Content/Sprites/`，不依赖在线服务即可游玩。
- 软件标识来自各自官方品牌资源。研究参考图不等于再分发许可；本作用于本地非商业同人制作与流程验证，商业授权未核实。

## 工程入口

- 项目：`forge.toml`，模式 `2d`，零重力，正交相机半高 6。
- 场景：`Content/Scenes/Main.rxscene`。
- 原生玩法：`Content/Scripts/sentinels.rs`，由 `.rxgraph` 调用标量 C 导出。
- 场景构建：`game/build_native.py`，采用已导入的真实视频 `.rxsprite`。
- 媒体流程、任务回执、来源哈希与验证：`pipeline/`。
- 原生引擎回归证据：`game/native/`。
- 界面：仓库 `packages/client/src/views/CodeSentinelsPlayer.tsx`。

服务仅监听本机。游玩无需额外图像或视频生成调用。
