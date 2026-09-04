# 实现任务分解

验收中的“可机验”指必须记录实际 MCP 返回；未通过不得标 completed。

## A. 框架

- [ ] **A1 建立 2D 场景与相机**  
  产出：`Content/Scenes/pvz_main.rxscene`、相机实体。验收：`scene_summary` mode=2d；`scene_graph_dump` 检查 Camera projection=orthographic、相机 +Z、玩法 z=0；`viewport_frame` 无透视形变。
- [ ] **A2 网格与分类基础**  
  产出：棋盘背景、Category/Tag 约定文档与实体模板。验收：`scene_index` 分类计数；`entity_get` 检查 TRS、Sprite sortingOrder。
- [ ] **A3 生成运行时数据脚本**  
  产出：`Content/Scripts/pvz_data.rx`。验收：`rx_check`；逐函数测试/调用结果覆盖 49/26 编码边界及关卡/场景字段；函数不跨调用。
- [ ] **A4 LevelController/HUD/Projectile 模板**  
  产出：`Content/Scenes/pvz_main.rxscene`、`Content/Scripts/pvz_level.rx`、`Content/Scripts/pvz_hud.rx`。验收：`entity_list`/`component_get` 检查 Script props、graphRef；`rx_check`。
- [ ] **A5 图节点基础图**  
  产出：`Content/Graphs/pvz_economy.rxgraph`、`pvz_grid.rxgraph`、`pvz_wave.rxgraph`、`pvz_combat.rxgraph`、`pvz_flow.rxgraph`。验收：每张 `graph_validate` 通过后 `graph_create`，`graph_get` 复核；不得出现未实现节点。
- [ ] **A6 进度宿主桥接**  
  产出：`Content/save.json`（由编辑器/宿主写入）及加载 props 约定。验收：`read_file` 校验格式；运行时明确不写文件，缺文件回退 1-1。

## B. 素材

- [ ] **B1 角色动作表生成**  
  产出：`Content/Textures/PZ_Plant_<Name>.png`、`PZ_Zombie_<Name>.png`。验收：`asset_list`/`asset_get_meta`；`asset_thumbnail` 人工核对纯品红隔离与一致性。
- [ ] **B2 动作图集切帧**  
  产出：`Content/Sprites/PZ_<Name>.rxsprite`（49 植物及 26 僵尸）。验收：`sprite_autoslice` bbox 数量；`sprite_get` 帧和 clip 引用完整。
- [ ] **B3 种子卡图集**  
  产出：`Content/Textures/PZ_SeedCards_7x7.png`及对应 `.rxsprite`。验收：`asset_thumbnail`、`sprite_autoslice` 49 个图标、`sprite_get`。
- [ ] **B4 引擎资产构建与引用**  
  产出：可构建的 `.rxmesh` 不适用于此 2D 项目；贴图/精灵元数据和引用。验收：`asset_build_status` 为 current；Sprite GUID 可由 `component_get` 解析。

## C. 关卡与机制

- [ ] **C1 50 关内容索引**  
  产出：`Content/Scripts/pvz_levels.rx`、50 个关卡 props/图。验收：`rx_check`；逐关 `graph_validate`/`graph_get`，对照 `levels.json` 的 stage、waves、flags、zombie_ids、reward。
- [ ] **C2 五种场景机制**  
  产出：`Content/Scenes/Stage_Day.rxscene`、`Stage_Night.rxscene`、`Stage_Pool.rxscene`、`Stage_Fog.rxscene`、`Stage_Roof.rxscene`。验收：`scene_graph_dump`、`component_get`；夜晚无 sky sun，泳池行/雾 4 列/屋顶前 5 列可见且 z=0。
- [ ] **C3 经济与卡栏**  
  产出：`pvz_economy.rxgraph`挂载实体、`Content/Scripts/pvz_economy.rx`。验收：PIE 输入/单帧与 `host_events_drain` 检查收集、HUD 计数、价格、冷却。
- [ ] **C4 网格种植/铲除**  
  产出：`pvz_grid.rxgraph`、植物模板/预置池。验收：viewport 点选或输入后 `viewport_pick`、`transform_get`、`entity_list`；泳池睡莲/屋顶花盆/占用校验。
- [ ] **C5 波次导演**  
  产出：`pvz_wave.rxgraph`及 wave props。验收：PIE `play_step` 逐波；`host_events_drain` 顺序和实体 Tag 对照 JSON。
- [ ] **C6 战斗、特殊状态与判负**  
  产出：`pvz_combat.rxgraph`、植物/僵尸 Script。验收：Trigger 命中、HP/armor、减速/冻结计时、推车 spent、判负事件；`component_get`/`transform_get`核对结果。
- [ ] **C7 八类特殊关**  
  产出：`Content/Graphs/pvz_special_<rule>.rxgraph`（bowling、whack、conveyor、big_trouble、vasebreaker、storm、bobsled、boss）。验收：逐图 `graph_validate`，PIE 专项输入和 `host_events_drain`；boss 血条/阶段状态可读。
- [ ] **C8 关卡流**  
  产出：`pvz_flow.rxgraph`、选关/选卡/奖励 HUD 精灵。验收：PIE 状态转移事件顺序；`read_file` 验证宿主生成 save，运行时不宣称写盘。

## D. 回归

- [ ] **D1 内容数据回归**  
  产出：`docs/spec/qa-data-report.json`。验收：读取四份 JSON，计数 49/26/50/5；脚本查询与事实源抽样一致，`rx_check`。
- [ ] **D2 50 关矩阵**  
  产出：`docs/spec/qa-level-report.json`。验收：每关可进入、出怪 ID/波数正确、胜利状态可达；使用 `play_enter`、`play_step`、`host_events_drain`、`play_exit`。
- [ ] **D3 49 植物矩阵**  
  产出：`docs/spec/qa-plant-report.json`。验收：每种可种植、成本/冷却来自查询、行为/动画无 `anim.warn`；`component_get`、PIE、事件环。
- [ ] **D4 26 僵尸矩阵**  
  产出：`docs/spec/qa-zombie-report.json`。验收：每种行为状态、HP/护甲/速度与事实源；`component_get`、Trigger、事件环。
- [ ] **D5 跨系统回归**  
  产出：`docs/spec/qa-system-report.json`。验收：经济、卡冷却、铲子、小推车、判负；`logic_inject_input`、`play_step`、`host_events_drain`。
- [ ] **D6 特殊关回归**  
  产出：`docs/spec/qa-special-report.json`。验收：八类每类至少一条成功和失败路径，图校验+PIE事件序列。
- [ ] **D7 发布终审**  
  产出：`docs/spec/release-report.md`。验收：`scene_diff`、`asset_build_status`、全部 `rx_check`/图校验，无未处置 `logic.unsupported`/`anim.warn`。
