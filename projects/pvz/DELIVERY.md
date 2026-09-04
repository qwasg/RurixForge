# PvZ 冒险模式交付说明

## 在 IDE 里直接玩(推荐)

1. 启动栈:`forge-agentd`(8103)+ `pnpm dev:host`(3080)+ `pnpm dev:client`(5173),浏览器打开 http://localhost:5173 。
2. 左下角工作区选择器切到 **pvz** 工作区(视口、层级、资产面板都会跟着切到本项目的 engine-host)。
3. 打开「编辑器」页签 → Assets 面板 → 类型过滤选 **Scene** → 双击 `Scenes/Levels/Level_1_1.rxscene`(任意 `Level_*` 都可以)。
4. 点工具条 **Play**。建议把左侧会话栏和右侧层级/属性栏收起,让视口接近 16:9(HUD 按 16:9 布局)。
5. 玩法:
   - 点顶部 **种子卡** 选中植物(黄框高亮),再点草坪 **格子** 种下;阳光不够不会种,已占格会弹回并退款。
   - 天上掉的 **阳光** 落在某格上,点那个格子收集(+25);向日葵产的阳光同样点格收。
   - 僵尸从右侧进场,豌豆射手自动射击同行僵尸;僵尸啃到植物会把植物吃掉;走到最左由小推车清场一次,再来一只就判负。
   - 打完全部波次且场上无僵尸 → `LEVEL COMPLETE!`;僵尸越线 → `THE ZOMBIES ATE YOUR BRAINS!`。
6. Stop 退出 PIE,再 Play 即重开本关;换关就再双击别的 `Level_*.rxscene`。

自动化实机试玩(真实浏览器,含截图):`node tools/web_playtest.mjs 1-1`;不开浏览器的引擎级试玩:`python tools/playtest_pointer.py 1-1`。
旧的命令行启动器 `python tools/run_level.py 1-1`(`plant/collect` 文本协议)仍兼容。

## 交互契约(引擎 ↔ 图)

- IDE 视口 play 态单击 → engine-host `pointer` 输入:按游戏相机把归一化坐标反投影到 2D 平面,同帧依序派发
  `click_x` / `click_y` / `click_z`(世界坐标)与 `click`(=1);MCP 面对应 `logic_inject_pointer {x,y}`。
- 关卡控制图用 `var.set(name=<action>)` 把坐标接进同名变量;`pvz_rules.rx` 的 `click_col / click_row5|6 / card_slot / in_card_bar`
  把世界坐标换成格子/卡位;格心位置经 `find_by_tag(cell_rXcY) → get_transform → set_transform(cell_probe)` 查表得到,
  植物落位直接复制探针 transform;探针的 Trigger 盒覆盖整格,进入盒内的阳光即被收集。
- 旧文本协议 `plant_at`(2*1e6 + 植物*1e4 + 行*100 + 列)、`collect_sun`(3*1e6+1)保留。

## 交付内容

- 49 种植物、26 种僵尸的事实数据、AI 精灵图集与 `.rxsprite`(锚点已统一到格底沿);5 个场景背景按 2x3 源图集正确重切。
- HUD:阳光四位数字(`PZ_Digits`)、种子卡图集(卡底 + 植物缩略图 + 费用,`PZ_Cards`)、选卡光标、胜/负横幅、小推车。
- 冒险模式 1-1 至 5-10 共 50 份关卡清单、50 个关卡场景(含 `.meta`,IDE 可直接双击)、50 个数据驱动控制图;
  每植物独立行为图 `Graphs/Plants/plant_<id>.rxgraph`(射击 / 产阳光 / 挡路 / 接触爆炸四类共享主体 + 被啃掉血 + 已占格弹回退款);
  26 个僵尸图、豌豆 / 阳光 / 探针 / 判负线 / 小推车 / HUD 共享图。
- 对象池:每关每种植物 6 株、僵尸每种 5 只(总 15)、豌豆 24、阳光 8;停在屏外的池子经 engine-host 视锥裁剪不占 128 draw 槽。
- 教程关规则:1-1 只开中间一行,1-2~1-4 只开中间三行(出怪与种植同时收窄);黑夜/浓雾无天降阳光。

## 事实源和生成器(全部可重放)

- `docs/data/*.json`:49/26/50/5 内容事实源(`stages.json` 天降阳光间隔 9s);`tools/gen_pvz_data.py` → `Content/Scripts/pvz_data.rx`。
- `tools/build_all_assets.py`:AI 素材构建;`tools/polish_assets.py`:切帧修正 / 锚点 / HUD 与卡牌素材 / 背景重切(纯 PIL,确定性)。
- `tools/gen_graphs.py` + `tools/build_all_levels.py`:50 关 / 场景 / 图 / `.meta` 构建并经 code-forge `graph_validate` 全量校验。
- `tools/regression_all.py`:静态门 + 50 关 PIE 回归(结果落 `evidence/pvz/`)。

## 验收证据

`evidence/pvz/`:`regression.json` / `REGRESSION.md`(50/50 通过)、`stage_*.png` 五场景实渲;
`evidence/pvz/web-playtest/`:`W01`~`W09` 真实浏览器 IDE 里从打开关卡、选卡、种植、收阳光到 `LEVEL COMPLETE!` 的截图与 `web_playtest.log`,
`P0*.png` 为引擎级指针协议试玩帧。

## 已知能力边界

- 无卡牌冷却与铲子;八类特殊关(保龄球 / 砸罐 / 传送带 / 僵王等)按普通关规则可玩,适配图只记录事件入口。
- 渲染管线无 alpha 混合(只有品红色键丢弃):半透明 UI 以空心框表达;泳池 / 屋顶的水路、花盆规则未强制。
- 植物被啃按 40/s 恒定掉血(原版按僵尸 dps),爆炸类植物为接触引爆(近似地雷),未做 3x3 范围。
- 引擎事件环(1024)会被逐帧 `logic.call` 事件冲刷,`LEVEL_WIN/LOSE` 日志不可靠,胜负以横幅实体进屏为准(自动化脚本即如此判定)。
- 音频、游戏内存档、图内切场景仍受当前引擎能力限制。
