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

- **E-07-001(2026-08-25,F11 / D-025)**:§9 末条「不做插件市场 UI」的适用范围经原文核读限定为**括号所指的 hooks/plugins**(执行第三方代码的扩展面),维持 RD-F7-002 defer 不变。F11「资产商店」不落在该条射程内——它是**纯数据分发**(资产文件 + SKILL.md 文本),安装链路禁止执行包内任何脚本或二进制,分发物一律经既有 `asset_import` 构建链落地。相应地 §1 七区布局**不变**:商店与 Skill 管理不新增常驻面板,入口 = Sidebar 两个导航按钮(与既有 New Agent 行同款),承载 = Workbench tab(`plan` / `todo` / `proposals` 为 F7 既有同类先例)。§7.2 的 `skills` tab 存在性不变,内容收窄为「目录配置(extraDirs)+ 跳转 Skill 管理 tab」,列表与编辑归 Skill 管理 tab 独有以避免两处事实源(裁决见 D-F11-E)。
