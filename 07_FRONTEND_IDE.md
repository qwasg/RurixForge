# 07 · 前端 IDE

> 技术栈:Electron(main + renderer)+ React 18 + TypeScript + Tailwind + shadcn 风格组件(决策 D-001)。
> 面板设计参考:UE5(Viewport/Outliner/Details/Content Browser 四联动)与
> Unity(Hierarchy/Inspector/Project/Scene-Game 双态)提炼至七区;
> IDE 类模块(chat/workbench/底部面板)照搬 agent-cowork agent-ide 模式;
> 设置页照搬 cindy 模式。UI 极简原则见 `01 §P-3`,常驻面板集合冻结,新增须决策批准(I-3)。

## 1. 主窗口布局(七区,冻结)

```
┌──────────────────────────────────────────────────────────────────┐
│ TitleBar: 项目名 | 场景名(脏标记) | Play/Pause/Step | 布局预设 | 设置 │
├────────────┬────────────────────────────────────────┬────────────┤
│ A          │                                        │ D          │
│ Hierarchy  │  C  Viewport(渲染帧 + gizmo + 工具条)  │ Inspector  │
│ (实体树)   │                                        │ (组件属性) │
├────────────┴───────────────┬────────────────────────┴────────────┤
│ B  Assets(素材网格/列表)  │ E  Workbench 底部页签                │
│                            │  Console/Problems/Output/Terminal/  │
│                            │  Logs/Metrics                       │
├────────────────────────────┴─────────────────────────────────────┤
│ F  Chat(agent 对话 + composer 模式 + Proposal 卡片)  可折叠右 dock │
└──────────────────────────────────────────────────────────────────┘
  G = 节点图页签(与 Viewport 同位切换,编辑逻辑时展开,10 §5)
```

| 区 | 面板 | 参考 | 职责(只做这些) |
|---|---|---|---|
| A | Hierarchy | UE Outliner / Unity Hierarchy | 实体树显示与点选、文件夹组织、可见性/锁定切换、重命名、拖挂父子;搜索过滤(含 `-`/`+` 操作符,UE 语法) |
| B | Assets | UE Content Browser / Unity Project | 素材网格预览(缩略图)、文件夹导航、类型过滤、拖入视口实例化、右键(导入/重导入/生成/删除提案);**不提供批量操作 UI**(批量→Chat) |
| C | Viewport | UE/Unity Scene view | 渲染帧显示、相机操控(右键+WASD、F 聚焦)、W/E/R gizmo、网格吸附、实体点选、PIE 状态条;帧通道接入(`03 §5.3`) |
| D | Inspector | UE Details / Unity Inspector | 选中 Entity/资产的组件分节属性编辑(§3);搜索框过滤属性;改动高亮与重置 |
| E | Workbench 底部 | agent-cowork 底部面板 | Console(事件流)/ Problems(诊断汇总)/ Output(构建输出)/ Terminal / Logs / Metrics(帧统计)六页签,照搬 agent-ide `BottomPanelTab` |
| F | Chat | agent-cowork chat/composer | 会话消息流、composer 五模式切换、Plan 卡片(批准/编辑)、Todo 进度、Proposal 确认卡(`12 §3`)、subagent 轨迹折叠显示 |
| G | NodeGraph | UE Blueprint(精简) | 逻辑节点图查看与微调,`10 §5`;与 Viewport 同位页签,不常驻 |

Play/Pause/Step 三键 = Unity 工具栏语义,状态机 = engine-host `play.*`(`05 §2.3`)。

## 2. Viewport(区 C)

- 渲染:Electron 侧原生共享纹理组件呈现 host 帧(D3D12 shared texture 首选);回退 canvas 解码流。分辨率跟随面板尺寸,经帧协商上报。
- 输入:键鼠事件捕获 → `input.inject`;编辑态 = 编辑器相机 + gizmo,Play 态 = 游戏输入。
- gizmo:平移/旋转/缩放(W/E/R),世界/本地坐标切换,吸附(移动 0.5m/旋转 15°/缩放 0.1,设置页可调,`Ctrl` 临时关吸附——UE 对齐)。
- 点选:单击 → `viewport_pick` → 选中同步 Hierarchy/Inspector;`F` 聚焦;`Alt+左键` 环绕。
- 叠加层:选中高亮、实体图标(灯/相机/音源)、网格线;**不做**编辑态特效调试叠加(交给 agent debug 工具)。

## 3. Inspector(区 D,组件分节)

对标 UE Details Panel + Unity Inspector 的交集:

- 顶部:实体名(可改)、Prefab 标记与 Open/Apply/Revert 按钮(Unity 对齐)。
- Transform 节:TRS 九字段,直接输入;右侧重置箭头(UE 对齐)。
- 组件节:每组件一节,标题栏 = 组件名 + 启用勾选(Unity 对齐)+ 移除菜单;字段由组件注册表驱动渲染(`09 §3.4`,property drawer 模式:数值滑杆/颜色/枚举下拉/资产引用拾取器/实体引用拾取器)。
- 「+ Add Component」按钮:弹出注册表类型清单(搜索)。
- 搜索框:按属性名过滤(UE 对齐)。
- 「修改自 Prefab」字段标蓝 + 右键 Revert(Unity 对齐);多选时仅显示公共组件与公共值,多值显示 `—`(不做 UE Property Matrix 批量编辑窗——批量 → Chat/agent,红线 R-2)。
- 资产选中时:显示 `.meta` 导入设置节(Unity 导入设置对齐)+ 预览 + 引用统计。

## 4. Assets(区 B)

- 视图:网格(缩略图)/列表切换;文件夹树左栏;类型过滤 chips(Mesh/Texture/Material/Prefab/Scene/Script/Audio);搜索。
- 缩略图:assetd 生成(网格 = 离屏渲染三视角;贴图 = 原图缩略),缓存 `.forge/cache/thumbs`。
- 拖拽:→ Viewport = 实例化(prefab/网格);→ Inspector 资产引用字段 = 赋值;→ 文件夹 = 移动(自动 redirector)。
- 右键菜单(固定六项):导入到此处 / 重导入 / 在文件夹中显示 / 引用查询 / 删除(进 Proposal)/ 生成(图像/模型,跳 Chat 预填)。
- 状态角标:构建中/失败/stale(对齐 `asset_build_status`)。

## 5. Chat(区 F,照搬 agent-cowork)

- composer 五模式(build/plan/debug/ask/multitask)切换器,快捷键 `Ctrl+Enter` 发送,`Shift+Enter` 换行。
- Plan 卡片:stage/task 树 + 批准/拒绝/编辑;执行中 todo 进度条(照搬 plan/todo UI)。
- Proposal 卡片:操作描述 + 影响面(数量/文件清单)+ 批准/拒绝(`12 §3`)。
- 上下文注入:当前选中实体/资产作为引用 chip 随消息发送(「把这个」语义落地)。
- 工具调用轨迹:折叠显示 MCP 调用名 + 参数摘要 + 结果状态;可展开看全量 JSON。

## 6. 工作流与面板的硬绑定(简洁性验收)

| 用户意图 | 面板路径 | agent 路径(冗余操作必须走这里) |
|---|---|---|
| 摆一个物体 | Assets 拖入 Viewport | `entity_create` + `transform_set` |
| 摆一百个物体 | **不做 UI**,Chat 描述 | `scene-dressing` skill → 批量工具 |
| 改一个灯强度 | Inspector 滑杆 | `component_set` |
| 改全部灯强度 | **不做 UI**,Chat 描述 | `entity_batch_apply` |
| 调逻辑 | NodeGraph 微调 / Chat 生成 | `logic-blueprint-gen` skill |
| 整理素材 | 右键单项操作 | `asset-cleanup` skill |

## 7. 设置页(照搬 cindy 模式)

### 7.1 结构

- 左侧菜单(260px,cindy `DEFAULT_SETTINGS_MENU_WIDTH` 对齐)+ 右侧内容区;tab 切换经 URL 参数深链(`?tab=xxx`),支持 legacy 别名重定向。
- **tab 单一事实源**:`src/renderer/lib/forgeSettingsTabs.ts` 定义 `SettingsTab` 类型、`TAB_IDS`、`TAB_LABEL_KEY`(i18n key 映射)、`isSettingsTab` 守卫——cindy `tabLabels.ts` 模式逐字对齐。

### 7.2 tab 清单(首发,冻结)

| tab id | 内容 Section(每域一个组件) |
|---|---|
| `general` | 语言、外观(主题/字号)、窗口行为、自动保存间隔 |
| `providers` | LLM 渠道列表(增删改、enabled 模型勾选、fetch-models);密钥输入不回显 |
| `subagent-models` | 各 subagent profile 的模型覆盖(cindy SubagentModelSection 对齐) |
| `mcp` | MCP server 清单(enabled/autoStart/命令/参数)、状态与重连、工具数 |
| `skills` | skill 列表(启用/禁用)、目录配置、新建入口(跳 skill-creator) |
| `permissions` | 权限模式默认、规则表(工具/路径白名单)、Proposal 阈值 |
| `viewport` | 帧率上限、分辨率缩放、gizmo 吸附步长、相机速度 |
| `generation` | gen-image/gen-model 后端配置(本地/远程、端点、密钥走 keystore) |
| `shortcuts` | 快捷键表(只读展示 + 自定义覆盖) |
| `storage` | 缓存占用、清理按钮、项目列表 |
| `about` | 版本(rurix-forge / rurix / 引擎 ABI)、许可、诊断导出 |

### 7.3 持久化(cindy settings-store 模式)

- 每域一个 `*-settings-store.ts`(Electron main),JSON 落 `<userData>/settings/<domain>.json`;同步 R/W(文件小);损坏 → 默认值 + 删坏文件。
- renderer 经 `window.forgeAPI.settings.get/patch(domain)` IPC 访问;默认值与 `forge-protocol` 的 settings schema 对齐。
- agent 侧只读白名单(经 `settings_get`,`05 §5`):`viewport`/`general`/`permissions`;写一律禁止(密钥域更禁止,R-5)。

## 8. 快捷键(首发冻结子集)

| 键 | 语义 |
|---|---|
| `W` / `E` / `R` | gizmo 平移/旋转/缩放 |
| `F` | 聚焦选中(视口 ↔ Hierarchy 双向) |
| `Ctrl+S` | 保存场景 |
| `Ctrl+Z` / `Ctrl+Y` | 撤销/重做(场景操作栈,engine-host 侧) |
| `Ctrl+P` | Play;`Ctrl+.` Step;`Ctrl+Shift+P` Stop |
| `Ctrl+K` | 聚焦 Chat 输入框 |
| `Ctrl+Shift+F` | 全局搜索(实体/资产/设置项) |
| `Space`(NodeGraph) | 节点搜索 |

## 9. 非目标(UI 不做清单,冻结)

- 不做材质节点编辑器(材质参数化由组件字段 + agent 调整;图编辑待 F5 后评估)。
- 不做动画状态机编辑器/时间轴(本期无动画系统)。
- 不做地形编辑器、植被笔刷(摆放 = `scene-dressing` skill)。
- 不做批量编辑窗、不做命令面板之外的第二操作面(红线 R-2)。
- 不做插件市场 UI(hooks/plugins 照搬后端但 UI 后置)。

## Errata(只追加区)

- **E-07-013(2026-10-07,D-047)——Cursor 式运行态:轮次状态行 + 运行态扫光 + 自上而下入场(as-built)**:①**轮次状态行** `components/chat/TurnStatusLine.tsx`(纯状态机 `lib/turnStatus.ts`,`turnStatus.test.ts` 全分支):流式助手卡末尾取代原 accent 闪烁 caret(`StreamCaret` 与 `.forge-stream-caret` 下线)。优先级:连接(事件流掉线 ≥1.5s「Reconnecting」;≥10s、host 健康探测失败「Connection lost · retrying」;浏览器离线「Connection lost · network offline」)> 等用户(未决审批含嵌套子代理里的「Waiting for approval」,userInput / elicitation「Waiting for your input」,静止不扫)> 有行在执行时让位(运行中的工具 / 子代理、思考中、Plan · Writing、正文仍在流)> 去抖(工具收尾 400ms、正文停流 1.5s,连续调用之间不闪)>「Planning next moves」,空档 ≥15s 升级「Taking longer than expected · {n}s」。空档以「这条消息的 blocks 数组最后一次换身份」为准(chatStore 每次改动都克隆),半秒一拍重算;换态按 kind 重挂。`chatStore` 新增 `streamLink` / `streamDownSince`(SSE onOpen / onError;连续失败不刷新起点)与 `pendingTurnSince`(sendMessage 记,root `agent.started` / 终止事件 / 发送失败 / reset 清)。②**开轮前占位卡** `PendingAssistantMessage`(testid `assistant-pending`):已发出、`agent.started` 未到时 MessageList 末尾挂同款头 + 状态行,计时从发出时刻起算,真卡出现即原位接替;头部抽成 `AssistantHeader` 共用。③**运行态扫光** `.forge-shimmer`(theme.css,取代仅 Thinking 用的 `.forge-thinking`):亮带从左到右循环扫过「正在执行」的那一行——段汇总、运行中的工具行 / 命令块 / 文件变更块、Thinking、子代理行进展末行、Plan · Writing、状态行;对比(同日用户反馈初版「太弱看不清」后加强——初版底色 text_2、亮带 text、单点峰值,Codex 亮色实测只差约 28 L*,再被底色描边吃掉一截):亮色底色 text_4 + 亮带「text 压黑一半」,0.2px 描边钉在亮带色上(静息观感约 text_3、字重不掉,扫到处近黑);暗色无描边,底色 text_3 + 亮带「text 提亮」到近白;亮带带 46%–54% 满强度平台,不借 accent,背景 300% 宽留扫出空档;挂在贴字宽的元素上(`SummaryLine` 文字段改为内层 inline 段,chevron 自带颜色);一律以所在消息仍在流式为前提(收束后残留 running 的块不扫),子代理浮层 WORK 同口径。④**自上而下入场** `.forge-stream-in`:流式中新出现的时间线项、markdown 块(段落 / 列表项 / 代码块)、展开态下新到的明细行、状态行换态——软边纵向遮罩自上而下擦出 + 下落 4px 淡入(0.4s);`StreamEnter` 在挂载时定夺(class 不随流式结束增删,包装恒在,收束不重挂、展开态与表单草稿不丢,null 子组件随 `empty:hidden` 不占 gap),遮罩只写在关键帧、fill-mode backwards 播完即撤;历史回放与浮层打开时已在的项不播,reduce-motion 全部关闭。⑤吸底:滚动区多一层内容列(testid `message-list-content`),由 ResizeObserver 兜底跟到底——状态行按计时出现、代码块高亮完成等不经 messages 变化的增高同样吸底。事件面、后端与过程链英文口径(E-07-008)不变。

- **E-07-012(2026-10-07,用户指令)——可读性、主页首屏与上下文面板(as-built)**:①**可读性**:`applyPalette` 的 text_2/3/4 混色基比由 0.42/0.62/0.78 收到 0.33/0.46/0.64(全部预设与自定义双表生效,forge 亮表对白底 5.3/3.0/2.0 → 6.9/4.6/2.8:1);Windows 中文回落微软雅黑只有 300/400/700,亮色主题下 Shell 与登录门根节点挂 `.forge-legible`(0.2px 文字描边,代码面与游戏页不受影响,暗色不加)。②**主页首屏**:撤 `.home-ambient`(accent 微光 + 网格,蓝色系预设下整片泛蓝;该类现仅登录门品牌面板使用);问候改无衬线粗体 + `ForgeMark` 领起、整组居中,副标题加深一档。E-07-009 ⑦ 的问候口径不变。③**上下文面板**(修订 2026-08-24 明细表):去掉按文件 / 工具逐条的表格,只留占用条 +「系统提示与工具 / 对话历史 / 待发送」三行,字号 12.5–13px;底部「压缩上下文」调 `POST /sessions/{id}/compact`(11 E-11-009),在途时 Composer 暂停发送;`context.compacted` 到达后时间线在当时最后一条消息之后画分隔线,计量只估算分隔线之后的消息 + 摘要体量,实测值作废到下一次 usage。Codex 会话的计量改用最近一次请求的 `last.promptTokens` 与 `modelContextWindow`。

- **E-07-011(2026-10-07,D-046)——登录门与账户界面重设计:Claude 式版式 + 折带品牌标(as-built)**:①新增 `components/ForgeMark.tsx`(logo 折带字形矢量版,flat / folded 两档;`reveal` 沿主斜带方向擦出入场、`busy` 亮带扫过,动效类 `.forge-mark-reveal` / `.forge-mark-shimmer` 在 `index.css`,reduce-motion 降级为静态);`ForgeLogo`(PNG 图块)保留在标题栏 / 关于 / 助手头像。②`AuthScreen` 宽屏(≥1024px)双栏:左表单列 = 品牌组合 + 衬线标题(`auth-title`,随页签切换「欢迎回来 / 创建你的账号」)+ 表单卡(胶囊页签、h-10 圆角输入、反色主按钮、带图标的错误/提示条),卡下「服务器地址 · 使用自带密钥(高级)」合为一行;右品牌面板(`auth-brand-panel`,aria-hidden)= `.home-ambient` + 三条折带斜条 + folded 品牌标 + 衬线标语。窄屏只留表单列,标题上方补 56px 品牌标。桌面窗口:overlay 形态下品牌面板贴右上缘,底色与系统三钮同为 `--bg-sunk`;表单列头部与面板顶部是拖拽区(关闭钮 no-drag);窄屏头部经 `.auth-overlay-safe` 让出三钮宽度;macOS inset 形态头部左侧让出 88px。③账户界面:`AccountPage` 资料卡头(56px 头像 + 衬线昵称 + 品牌标水印)、余额大数字(衬线 30px + 小号币种)、额度条加粗、用量图柱圆角且最近一天加深、未登录改空状态卡(folded 品牌标 +「登录或注册」反色按钮);`CloudAvatar` 与侧栏 `AccountCard` 的首字母头像改 `.forge-fold-avatar` 折面底;侧栏账户卡悬停/展开露出 up-down 箭头,账户菜单新增「账户」项直达设置·账户页;设置左下账户条同步(未登录的「登录」改反色小胶囊)。④字体:`main.tsx` 导入 `@fontsource-variable/noto-serif-sc`,`theme.css --font-serif` 与 `themeStore FONT_SERIF_STACK` 首选 `"Noto Serif SC Variable"`。testid、role/aria、store 与 API 不变;共享设置控件(SetCard / SetRow / SetSectionLabel)与配色 token 不变;§1 七区布局不变。
- **E-07-010(2026-10-06,D-045)——Composer 第八模式 Design + 设计流程卡(as-built)**:①`composerModes` 增 `{id:'design', label:'Design', icon: Palette}`(coding 代理、本地与 Codex 引擎);流程停在审阅关口时 Composer 自动切到 Design(用户亲手改模式后不再干预),输入提示随关口变。②`lib/designFlowStore.ts`(命名避开画板 `designBoardStore`):快照 `activeSession.design` 为阶段事实源,实时 `design.*` / `session.updated` 触发重拉,卡片动作经 `act()` 直接 POST ask:execute 带 `{id, rev, action, candidate}`,选中经 `POST …/design/select` 持久化。③时间线 `ChatBlock` 增 `kind:'design'`(step = review / layout / verify / done),由持久化事件建卡、`design.decision` 回填只读。④组件 `components/chat/design/`:审阅卡(候选网格 + 选中环 + 采用 / 提出修改 / 重新生成)、元素清单卡(叠框图 + 明细)、验收卡(引擎截帧与定稿滑块对比、差异热力图、未对上元素)、完成卡(结论 + 提出修复)、`ImageLightbox`(缩放平移)、`DesignBar`(Composer 上方三步进度 + 继续复刻 / 重新开始)。⑤Inspector:Text 组件给小编辑器(文字失焦提交、字号、颜色、对齐、字体下拉),`editorStore.setComponentProps` 走 component_set 整份替换。

- **E-07-009(2026-09-26,D-040)——壳布局与占位清零;§1 七区常驻面板集合不变**:新能力只落进既有位置(标题栏、侧栏、状态栏、右栏文件树、命令面板、底部 Metrics、关于页),不新增常驻面板。①**桌面外壳**:系统标题栏隐藏,Windows 用 `titleBarOverlay` 画三钮(主题色由 renderer 下发),面板开关移到标题栏右侧;Ctrl+B / Ctrl+Alt+B 开关会话栏与右栏。②**状态栏**重排为可点击的连接 / 工作区+2D·3D / git 分支 / 待办 / 引擎·模型 / Codex 额度,数据来自 `systemStore`(5s,页面隐藏暂停)与 `gitStore`(15s);模型不可用显示「模型未配置」并链到设置,不再用「Mock provider」充数。③**侧栏**:搜索与折叠同一行,`/` 聚焦搜索;未归文件夹的会话在「全部会话」下按工作区分组;运行点与未读点来自快照回填;底部账户卡用系统用户名(host 健康接口 `user`)与 Codex 套餐,菜单含设置/主题/快捷键/关于。④**命令面板**分命令 / 会话 / 文件三组,文件走 `GET /api/forge/workspace/search`。⑤**文件树**按扩展名给图标,git 状态字母与目录脏点由外观页「差异标记」(Color 着色 / +/- 增删行)驱动,并有「仅看改动」。⑥**对话 Markdown**(修订 E-07-004 ⑤「MarkdownFlat 不支持嵌套列表/行内强调/任务勾选框」):标记符号不再原样上屏;补标题分级、有序与嵌套列表、任务勾选、分隔线、整表、行内粗斜删与链接(http 交系统浏览器,工作区路径开文件 tab)、代码块语言标签/复制/非流式高亮。正文全加粗与胶囊输入框两条约定不变;行内代码与代码块回正常字重。⑦首页问候用时段 + 用户名(取不到则回落「今天想搭点什么？」),起手式按项目 2D/3D 分话术,状态点读真实连接与模型可用性。⑧其余占位:关于页与关于弹窗合一并展示实测版本与运行时长;Metrics 待办卡改名 Todos 并另计真实会话数;Mock provider 状态取快照;Goal 预算默认留空;Todo 按 `source` 标来源;生成对话框指向设置·模型;音频与缩略图去掉过期文案。
- **E-07-008(2026-09-03,用户指令波)——§5「工具调用轨迹」as-built 改写为「英文过程链 + 中文加粗正文」三级层级**:用户给定目标截图(Cursor 式过程链)后拍板三件,落点全在 `packages/client/src/lib/timeline.ts` + `components/chat/*`,事件面与 store 一字未改。①**过程链文案整体转英文,行形态改两段式「{动词} {目标}」**——动词 `text_2` 深一档、目标 `text_4` 浅一档:`Read timeline.ts L90-625` / `Grepped danger|--fg in theme.css` / `Searched files <glob 串>` / `Created entity e1`;`TOOL_META` 按 KNOWN_TOOLS 全量重写为 `done`(过去式)+ `running`(现在分词)双档,段汇总随之改英文语法「`Edited 2 files, explored 2 files, 2 searches, ran 2 commands`」——**首个非零类目定首动词并大写,其余类目退小写从句,段内有 running 工具时首动词换现在分词**(`Exploring 12 files, 9 searches, ran 2 commands`),原「· 正在{动词}…」后缀随之下线;并组短语作「`Read 5 files`」。汇总行与思考行右侧加 chevron(hover 浮现/展开旋转)。②**正文(中文)加黑加粗**:`MarkdownFlat` 新增 `strong` 档(`text-fg` + `font-bold`,围栏代码块显式回正常字重),助手**中间叙述与最终回答同档**(原中间叙述压成 13px `text_2` 的弱文形态下线)、**流式期间也不退灰**——正文黑粗 / 动词深灰 / 目标浅灰三级分明。语言分工由后端 `SYSTEM_PROMPT` 承担(思考推理英文、面向用户正文中文),前端不做翻译。③**报错不特别标明**:活动段与工具行的 `text-danger` 红字、「· n 失败」后缀、`toolLine` 里的「失败:{首行}」一律下线,失败态助手卡状态点退 `dot-idle`、错误行退 `text_3`,子代理行/浮层徽标/粒子(`.forge-particles[data-state='error']`)一并去红只留形变;`SegmentStats.errors` 仍如实统计,错误原文仍可展开逐字查看——去的是颜色喊话,不是事实(I-5 守住)。④**思考行并入活动段**:`isMilestoneBlock` 不再拿 `reasoning` 断段,思考行与工具行同列(截图里「Thought 47s」就夹在 Read/Grepped 之间),文案改「`Thought 47s`」/ 无计时「`Thought briefly`」(过分钟仍报秒,不进位 `1m 46s`);进行中维持渐变「Thinking」不剧透摘录(2026-09-03 早前指令),**但按截图补上可展开**——想看实时思考流点一下即可;段内只有思考块(无工具)时不套汇总壳,裸行直出。⑤**行区间有真实来源**:后端 `read_file` 增可选 `offset`(1 基起行)+ `limit`(行数),故「`L90-625`」是参数事实而非渲染层猜测——参数缺省时不显示区间(见 04 E-04-005)。§1 七区布局、§9 非目标不变;`ContextMeter` 工具行标签跟随同一动词表(`tool:Listed entities`)。
- **E-07-007(2026-09-03,D-038)——§5 对话面增「回执唤醒」系统卡(修订 E-07-005 末条)**:后台子代理跑完、会话空闲时,服务端自起一轮主 agent(`composer.user.message{source:"receipt"}`),前端把这张「用户卡」画成**系统唤醒卡**:虚线边 + `bg-shell-sunk` + 「回执唤醒」chip(BellRing),正文是服务端生成的「【系统唤醒】后台子代理回执送达(N 条)…」;**不提供编辑重发**(它不是用户说的话,而且 revert 会把已送达的子代理卡片一并截掉)。唤醒轮是完整 turn,发 `agent.started`,故其间输入框按常规锁定(E-07-005 末条「后台子代理运行期间输入框保持可用」仍然成立——锁的是主 agent 处理回执的那一小段,不是子代理跑的全程);用户若在 `agent.started` 抵达前的几毫秒抢发,后端 409 `SESSION_BUSY`,前端撤掉乐观回显并 warning toast「主 agent 正在处理后台回执」。主 agent 正在工作时的中途插入没有独立 UI(回执作为 user 消息进的是模型上下文,不是对话流),可在底部面板 Agent Logs 的 `agent.receipts.injected{midTurn:true}` 事件里查到。
- **E-07-006(2026-09-03,PvZ 可玩化波 / D-037)——§2「输入:键鼠事件捕获 → input.inject」as-built 收口三件**:①**视口 play 态单击带坐标**:此前只发 `click=1`(无位置),PvZ 这类「点格子/点卡牌」玩法在网页里无从成立;现单击发 `pointer{action:"click", x, y}`(归一化到画布,0..1),引擎按游戏相机反投影后同帧派发 `click_x/click_y/click_z + click`(契约见 05 E-05-002);WS 断流回退 `logic_inject_pointer`。②**视口/层级/资产面板按当前工作区作用域**:`callTool` 全部附带 `workspaceId`(11 E-11-003),切工作区 = 视口流重连 + 场景/实体/相机/资产重拉;此前 IDE 恒看 `projects/demo`,与会话工作区是两个真相源。③**Assets 面板双击 `.rxscene` 装进视口**(`openScenePath`,play 态先退出,路径补 `Content/` 前缀;这是 IDE 内切关卡/切场景的唯一 UI 入口——`Load Scene` 钮只重载上次保存路径),空场景兜底装 demo 迷宫的行为在非 demo 项目静默跳过而非报错;Play/Stop/存取场景失败改为 toast 如实上屏(此前只写 `lastError` 无处显示,Play 失败界面毫无反馈)。§1 七区与 §9 非目标不变。
- **E-07-005(2026-09-03,D-036)——§5 composer 模式表中 `multitask` 的对话面改写为「后台子代理卡片 + 回执」**:multitask 轮本身只在助手卡里留一段活动行「派发子代理 · N 次」+ 一句派单说明(`dispatch` 工具刻意不作里程碑块,不渲染成待办行);**每个后台子代理另起一张助手卡**——它的 `subagent.started` 携带自己的后台 `runId`(父轮此时已收束),chatStore 据此新建卡片并补齐时间/模型元数据(后台腿不发 `agent.started`,否则 `activeRunId` 会锁死输入框),卡内是既有 `SubagentRow`(粒子 + 双行摘要,点开 `SubagentOverlay` 看过程),完成后同卡追加回执正文。**Stop 语义分叉**:后台子代理块带 `detachedRunId`,Stop 只中止它自己(`cancelSubagent` → `/api/forge/runs/{runId}/cancel`);同步 `task` 子代理无此字段,维持中止父轮。**输入框在后台子代理运行期间保持可用**(`activeRunId` 恒空)——这正是「多任务时效率高」的落点。§5 其余条款与 §1 七区布局不变。
- **E-07-004(2026-09-03,D-035)——§5「Plan 卡片:stage/task 树 + 批准/拒绝/编辑」条款改址为 Plan 工作台页签**:计划不再是对话流里的一张卡,而是工作区文件 `.forge/plans/<名>.plan.md` 的编辑页——理由见 D-035(卡片形态的计划换会话/刷新即丢、不可编辑,且「开始 Build」的正文前缀注入正是 D-034 判死的形态)。**§1 七区常驻面板集合不变(I-3 守住)**:承载 = Workbench tab(与 F7 `plan`/`todo`/`proposals`、F11 商店/Skill 管理、F-GAME-4 精灵编辑器同例),只是 `plan` 从「无 payload 单例」改为**按 path 多开**(同 `file` tab 形态,id = `plan:<path>`)。页面构成:①头栏左 = 计划名 + 概述 + 路径;右 = 预览/编辑切换、模型切换器、**Build** 主按钮(有 run 在跑时禁用;有未保存改动时先落盘再发——后端读的是磁盘上的计划)。②正文预览态 = `MarkdownFlat` 渲染设计正文 + front matter 待办清单(按 `planTodoId` 映射会话待办显示实时状态与 `done/total` 进度);编辑态 = CodeMirror(与工作区文件编辑器同一套加载/保存/dirty/草稿/EOL/409 冲突纪律,实现下沉为 `lib/useFileEditor`,两页共用一份)。③状态条:plan 模式 run 进行中显「正在调研并撰写计划…」;front matter 解析失败如实提示并回落按原文预览(Build 仍可用);文件读不出如实报错,不伪造空计划。④`plan.created`/`plan.updated` 事件到达自动开/刷新页签(快照回放里的历史 plan.* 只回填指针不弹页签,否则每次切会话都会把旧计划顶到前面)。⑤**已知限制(如实留痕)**:`MarkdownFlat` 不支持嵌套列表/行内强调/任务勾选框——故待办清单由 front matter 结构化数据单独渲染,不依赖 Markdown task-list;富渲染需求驱动再评。§7.2 设置页 tab 清单与 §8 快捷键不变;命令面板 `tab.plan` 改为「打开当前会话的计划」,无计划时 toast 如实告知而非开空壳页。
- **E-07-003(2026-09-03,D-034)——§5「上下文注入:当前选中实体/资产作为引用 chip 随消息发送」条款下线**:该条唯一落地形态是 Composer 上方的上下文 chip 行(场景/选中实体/选中资产三源)+ 发送时拼在正文前的 `【上下文】…` 文本前缀。用户拍板去掉,前端实现(`lib/contextChips.ts`、Composer chip 行与前缀拼装、上下文计量表的 `context` 行)整体删除;后端本就零解析该前缀(纯文本 seam),无对应实现可删。「把这个」语义改由用户在正文中自述,或由 F10 预检索上下文注入(agentd `preamble`,与本条无关)承担——后者不受影响。
- **E-07-002(2026-08-31,F-GAME-4 / D-031)——§9「不做动画状态机编辑器/时间轴(本期无动画系统)」条款的前提失效与收窄**:F-GAME-4 落地引擎帧动画系统(09 E-09-002)后,「本期无动画系统」前提不再成立。处置:①新增 **Sprite 精灵编辑器**,承载 = Workbench tab(F11 先例,§1 七区常驻面板集合**不变**,I-3 守住);入口 = Assets 面板右键(贴图「编辑精灵(新建)」/.rxsprite「编辑精灵」)与双击 .rxsprite。功能 = 图集帧 bbox 拖拽编辑(空白拖拽新建/四角柄 resize/Shift 等比)、pivot 十字准星拖拽(帧级覆盖带 `*` 标记)、前端 Auto-Detect(连通域即时预览,判定与服务端/视口色键同规则)、clip 编辑(帧序列/fps/duration/loop/onFinish)、**动画预览播放器**(rAF 按 clip 时长驱动 + 帧缩略图 rail + 点击 seek)。②「动画状态机**图形化**编辑器」维持不做:animator 段以 JSON 表单编辑(数据即事实源,校验在 assetd;图形化状态机编辑待需求驱动再立项)。③「时间轴」条款收窄:预览 rail 是**只读播放控制**,非关键帧编辑时间轴——后者维持不做。Assets 面板 sprite 类型过滤 chip 与图标、Inspector 精灵摘要随行。
- **E-07-001(2026-08-25,F11 / D-025)**:§9 末条「不做插件市场 UI」的适用范围经原文核读限定为**括号所指的 hooks/plugins**(执行第三方代码的扩展面),维持 RD-F7-002 defer 不变。F11「资产商店」不落在该条射程内——它是**纯数据分发**(资产文件 + SKILL.md 文本),安装链路禁止执行包内任何脚本或二进制,分发物一律经既有 `asset_import` 构建链落地。相应地 §1 七区布局**不变**:商店与 Skill 管理不新增常驻面板,入口 = Sidebar 两个导航按钮(与既有 New Agent 行同款),承载 = Workbench tab(`plan` / `todo` / `proposals` 为 F7 既有同类先例)。§7.2 的 `skills` tab 存在性不变,内容收窄为「目录配置(extraDirs)+ 跳转 Skill 管理 tab」,列表与编辑归 Skill 管理 tab 独有以避免两处事实源(裁决见 D-F11-E)。
