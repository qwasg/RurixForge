# 编译防线 II · V2 制作与验收说明

本文件描述开放战场版本。玩家启动与来源导航见[项目 README](../README.md)。[versions/README-v1.md](versions/README-v1.md) 是更新前原文的逐字节备份；原有 `game/README.md`、旧 `native/` 日志及旧独立包属于 V1 历史，不用于证明 V2 帧率或独立包通过。

## 最终交付字段

约定 Windows 交付入口：`../dist/CodeSentinels-V2-Windows/Start-Game.cmd`。下列字段均由最终原生运行、独立包或主任务浏览器实测填写。

- `FINAL_PACKAGE_STATUS`：**通过**。独立on/off各11项集成通过，主任务浏览器实际完成冷启动、5060安装、4角色部署、两次GPT选点技能、第一波无损防守、GPU升级及重开。见 [root-browser-qa.json](v2/root-browser-qa.json) 与 portable-on/off-regression.json。
- `FINAL_PACKAGE_PATH_AND_HASH`：[CodeSentinels-V2-Windows.zip](../dist/CodeSentinels-V2-Windows.zip)，139,814,820字节，119成员。SHA256 `c9b4a4f572c3e8942bfe494323cd98ef233fc79e7a9c46d3644baff595415894`。成员校验见 [archive-integrity.json](v2/archive-integrity.json)。
- `FINAL_GPU_EXPERIMENT_DEFAULT`：常规 Start-Game.cmd 明确off；Start-Game-GPU-Particles.cmd明确on。即使父进程环境相反，两者依然选择正确模式。二者都有144帧真实视频技能。
- `FINAL_PERFORMANCE`：**通过**。RTX5060 Laptop GPU，真实Vulkan1280×720战斗WS，各10秒385帧：off38.499 FPS、on38.490 FPS，未截断；浏览器实见39–40 FPS。renderer125draw压力p95 8.76ms、预热后0重建是独立GPU侧补充证据。
- `FINAL_CAMPAIGN`：**已通过原生实测**。[正常战役](v2/native-campaign.json) 的 `pass=true`、`completeLevels=3`，独立PID64256完成三关十二波，各关胜利HP均20，各Boss地形阶段均0→1→2，`errors=[]`。[无防守失败](v2/native-defeat.json) 的 `pass=true`，独立PID15596在首关第2波真实失败（HP0、phase3），`errors=[]`。
- `FINAL_BROWSER_AND_ERRORS`：**通过**。1280×720，零损坏图片、零浏览器error/warn；原生运行无logic/anim错误。实际重开回到560经费、0算力、0GPU、0单位、20HP、0波。主任务原有正在游玩的实例保留。
- `FINAL_SOURCE_BINARY_EVIDENCE`：**通过**。源脚本、Main场景、144帧技能图集与包内文件逐字一致，原生DLL源/二进制双哈希校验通过，故意禁用rustc仍可冷启动。见 [portable-integrity.json](v2/portable-integrity.json)，engine SHA256 `712de07b4678c2454cc9aff8b26e24053c21d731b9f7fbac87d11ffc5f4f6ea3`。

## 已实证的制作与功能结果

主任务已独立核对七张 GPU 照片与官方原图逐字节一致，三个新技能真实 Codex run 均为 completed，源图、MP4及最终图集 SHA 匹配，合计144帧；[asset-integrity.json](v2/asset-integrity.json) 为通过记录。

前端此前全套 **59个测试文件、655项测试通过**，日志在 [v2-client-tests.log](v2-client-tests.log)，该轮含V2 UI13项、协议7项及旧V1 UI5项。随后新增「胜利发布与暂停确认交叠」回归，当前V2 UI **14/14**单独复测通过，客户端TypeScript检查通过。新增测试将暂停RPC保持pending，先发布Boss死亡后的phase2，再交付暂停确认；断言自动 `play_resume` 只调用一次、清除暂停门后可发送 `5_200_000` 下一关命令。其余测试覆盖零算力开局、精确原生命令、真实小数产能、当前单元技能费用、弹窗隔离、失败反馈、首个完整快照门和旧请求迟到隔离。

媒体截帧核心8项测试通过，三套技能的真实请求、不同帧和透明合成已有最终证据。真实Vulkan Alpha/Additive像素对拍也已通过。最终FPS、战役与独立包另外完成了顶部所列实测。

## 原生事实源与经济

核心为 [sentinels_v2.rs](../Content/Scripts/sentinels_v2.rs)，平衡元数据为 [economy-v2.json](../Content/Data/economy-v2.json)。UI只传递原生命令、显示真实GPU帧并解码只读发布实体。

初始560 credits、0 energy、0 GPU；显卡槽8个，防御单元槽24个。建设经费用于显卡、单元和升级，击杀发放建设经费；只有安装的显卡持续产出算力。普攻逐发扣算力，技能一次扣费，缺能停火。**GPT不再具有V1产能行为**，其主动技能用于回滚修复和区域支援。

七张官方显卡图对应 RTX 5060 / 5070 / 5080 / 5090、RTX PRO 6000 Blackwell Workstation Edition、A100 80GB PCIe、H200 NVL 141GB。真实图像、型号及显存见 [GPU来源资料](../references/sources-gpu.md)；`energyPerSecond`、容量、经费和升级收益是游戏平衡值，不是硬件benchmark或价格。

发布实体包括 `CS_State`、`CS_Economy`、`CS_StateAux`、`CS_Campaign`、`CS_Meta`、24组 `CS_Unit/CS_UnitAux/CS_UnitCost`、8个 `CS_GPU` 和14个 `CS_MapRow`。地图24×14；每行四个18-bit打包数，每组包含六个3-bit地形。地图、战区与revision必须一致才接受为完整状态。

[sentinelsV2.ts](../../../packages/client/src/lib/sentinelsV2.ts)负责协议解码与命令；[CodeSentinelsV2.tsx](../../../packages/client/src/views/CodeSentinelsV2.tsx)负责交互。首个完整原生发布之前不开放购买/部署；无效快照关闭指令门。重载使用epoch拒绝旧请求迟到结果。WebSocket发送成功不会使浏览器先扣钱、生产算力、创建单元或切换关卡。

## 指令与交互契约

全部输入使用 `action="cs"` 和可被f32精确表示的整数。`cell=row*24+column`，范围0–335；unit slot为0–23，GPU slot为0–7。

- 部署：`1_000_000+kind*1000+cell`，kind为1–4；VS Code部署到159格是 `1_001_159`。
- 单元升级/回收：`2_000_000+slot` / `2_100_000+slot`。
- 主动技能：`3_000_000+slot*1000+cell`；slot6对183格施法是 `3_006_183`。先点已部署单元，再Q/技能按钮，再点落点；Q不是V1按通路释放的通用热修复。
- 普攻目标策略：`3_100_000+slot*10+mode`，mode为逼近基地、最低血量、Boss三档。
- 安装GPU：`4_000_000+slot*10+model`，model为1–7；首槽RTX5060是 `4_000_001`。GPU升级/回收：`4_100_000+slot` / `4_200_000+slot`。
- 下一波：`5_000_000`；切换已解锁战区：`5_100_000+level`；胜利后下一战区：`5_200_000`。
- 速度循环：`7_000_000`。UI暂停使用 `play_pause/play_resume`，原生动画与战斗一起暂停。完整重开通过 `play_exit→asset_reload→scene_load→play_enter` 重建场景。

技能卡采用当前原生单元费用、冷却与射程。手册/图鉴打开时快捷键不穿透；单位失效或换关取消瞄准，待施法落点不会穿入部署分支。最新原生拒绝反馈优先于一般缺能提示。

单元最高三级，每级增加45最大生命并改变普攻消耗/射程；回收返还投入经费70%。直接切换已解锁战区会建立该关初始状态；正常胜利后「进入下一战区」携带显卡和剩余算力，原生结算旧单元经费返还与过关补贴。

## 地形、错误类型与Boss

断点森林、泄漏湿地、递归高地各4波。平地、道路、高地、基地外的桥可以部署；岩壁、水域、危险区、基地保留区及敌人占据的位置不能部署。放置后做可通行性检查，封死入口的事务被拒绝。高地提高射程并影响通视；显示地图和寻路读取同一份原生地形。

普通敌人五类：空指针CWE-476、内存泄漏CWE-401、竞态条件CWE-362、死锁CWE-833、栈溢出/失控递归CWE-674。三个Boss：堆损坏巨兽CWE-122、死锁巨像CWE-833、递归巨构CWE-674。真实CWE链接在[项目README](../README.md#真实形象与来源)和图鉴中保留；怪物外形与战斗行为属于游戏创作。

Boss半血进入地形阶段1，死亡后进入阶段2，可能改变岩壁、水路和桥梁。敌人依据更新后的通路寻路，防御必须适应新的射程与通视。验收除UI阶段数字外，还要检查原生地图revision/行数据和真实战场画面。

## 角色、原片与144帧技能

角色采用有来源的女仆鲸鱼娘和白紫龙娘半身版本。B站、YouTube、作者仓库链接、软件官方标识及核实边界见 [sources.md](../references/sources.md)，GPU另见 [sources-gpu.md](../references/sources-gpu.md)。所选YouTube白紫龙娘与不同B站版本不混合署名，也不补造没有参考的下半身。

本项目Codex模式agent实际发出MiniMax-H3图生视频请求。最终角色影片为 `SourceMedia/deepseek.mp4` 和 `SourceMedia/gpt.mp4`；GPT首版完整归档，正式使用扩大安全边距、通过全部158原帧检查的v2。两位角色各32帧。

新增技能原片位于 `SourceMedia/effects/`：`deepseek-tide.mp4`、`gpt-nova.mp4`、`pycharm-matrix.mp4`。每段请求6秒，实际6.58秒/24fps；12fps提取前六秒72帧，再等时选择48个实际帧，没有插入合成动作。每项先形成1808×1808图集，单帧256×256，24fps播放2秒。三项组合为 [v2-skills-video.png](../Content/Textures/v2-skills-video.png) 的144帧原生图集，3098×3098；导入记录见 [skill-import.json](v2/skill-import.json)。

供应商task ID、原图/原片SHA-256、项目Codex session/run、工具完成结果和帧索引完整保留在 [角色媒体证据](../pipeline/evidence.json)、[技能媒体证据](../pipeline/v2/evidence.json)及同目录 `*.codex-events.jsonl`。媒体文件永久保存，不只留在可清理的 `.forge/tmp`；正常技能任务没有重复付费提交，既有失败及重试原因也保留。

黑底通过 `gend::video_frames::ChromaKey::Black` 转straight-alpha并反预乘RGB，保留彩色软辉光。潮汐/星核附带的边界细镜头光线仅在外缘25px（9.77%）平滑羽化；中央206×206 RGBA逐字节不变，全图RGB不变，外缘alpha为0。原片保存在 `SourceMedia/effects/`，羽化前48帧保存在 `pipeline/v2/unprocessed/`，原片边界检测没有被改写成无缺陷。深浅地面合成已亲查，详见[媒体说明](../pipeline/v2/README.md)。

## 真正的透明混合与可选GPU粒子

只在shader返回alpha不足以形成正确合成。本轮仓内vendor修复让Rurix `RasterPass`具有真正的Opaque/Alpha/Additive模式，并将模式计入管线缓存key。Alpha使用 `SRC_ALPHA / ONE_MINUS_SRC_ALPHA`，Additive使用 `SRC_ALPHA / ONE`。Sprite采用纹理alpha，并能关闭旧品红色键以保留紫色能量；视频技能明确使用 `chromaKey=none`、`blendMode=additive`。

实现、固定上游revision、补丁SHA和复跑命令见 [FORGE_BLEND_PATCH.md](../../../vendor/rurix/FORGE_BLEND_PATCH.md)。[sprite-blend-evidence.json](../artifacts/rgba-blend/sprite-blend-evidence.json)验证真实Vulkan帧缓冲像素，而非只检查shader字符串。设备与采样像素以JSON为准；这些用例不代表最终游戏FPS。

额外GPU粒子由64个 `ParticleEmitter` 事件驱动GPU compute与实例化绘制，受独立实验开关控制。在启动引擎前设置：

```powershell
$env:FORGE_GPU_PARTICLES = 'on'
```

关闭时设为 `off`，或启动前不设置该变量。改变后重启对应engine-host，不支持进程内热切换。**粒子off时144帧视频技能仍正常显示。** 普攻小粒子与高耗技能overlay分别绘制，粒子不会取代真实I2V。

实现、on/off验证及限制见 [GPU_PARTICLES_EXPERIMENT.md](../GPU_PARTICLES_EXPERIMENT.md)、[活动粒子viewport证据](../artifacts/gpu-particles/viewport-emitter-evidence.json)和 [off证据](../artifacts/gpu-particles/viewport-emitter-off-evidence.json)。这是接入底层render_exec的实验路径，不宣称完整上游G35、流体/物理碰撞或百万粒子性能。

## 复跑入口

下面命令从仓库根目录运行。构建会改写生成资产，应在独立验证流程使用，不打断正在进行的游戏会话。

```powershell
python projects/code-sentinels/game/build_v2.py
python projects/code-sentinels/game/playtest_v2.py
python projects/code-sentinels/game/playtest_v2.py --defeat
pnpm --dir packages/client exec vitest run test/codeSentinelsV2.test.tsx test/sentinelsV2.test.ts test/codeSentinelsPlayer.test.tsx
pnpm --dir packages/client exec tsc --noEmit -p tsconfig.json
```

原生战役动态记录为 `game/v2/native-campaign.json`，失败记录目标为 `game/v2/native-defeat.json`。只有最终结果明确包含通过状态、完整三关和对应源码/二进制身份，才能填写顶部最终战役字段。不能从中间记录、文件存在或旧V1包测试推断V2最终通过。
