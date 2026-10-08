# Design 模式

Design 是第八个 composer 模式(D-045):按用户的设计意图调用生图模型出**场景或界面 UI 设计稿**,用户审阅(多候选挑选、文字修改、重新生成)后采用其中一张,agent 再在引擎里对图中画面做**原子级复刻**——设计稿上每个可分辨的视觉元素都成为场景里一个独立实体,位置、尺寸、叠放与定稿像素对齐,文字是可编辑的 Text 组件;服务端截图与定稿逐元素对比验收。

本地与 Codex 引擎的 coding 代理可用。模型必须能看图(Codex,或在设置·模型里勾选了视觉能力的渠道),会话权限不能是只读。

## 前置配置

Codex 引擎的设计稿生成、参考图修改、干净底图与单体重绘使用 Codex 原生 `imagegen`，复用当前 Codex 登录态，无须额外填写生成后端密钥。服务端会检查上游是否支持原生生图；渠道不支持、额度耗尽或生图失败时会显示实际错误。普通 Codex 聊天中的生图结果也会保存到当前项目，并显示可放大、下载的图片预览。

本地引擎的生图与改图走 gend 的 `remote-openai-compatible` 后端:在设置·模型的生成后端卡里启用并填写端点、模型与密钥。端点需要同时支持 `POST /v1/images/generations`(文生图,含 `1536x1024` / `1024x1536` 尺寸)与 `POST /v1/images/edits`(multipart:`image` / `image[]`、可选 `mask`、`prompt`、`n`、`size`、`quality`、`background`)。改图不可用时 Design 的修改、干净底图与单体重绘会如实失败(`GEN_UNSUPPORTED`),不会拿文生图冒充。

只想跑通链路时,可在 `data/gen-backends.json` 启用 `local-mock`(确定性占位图,非 AI 模型)。

文字渲染需要项目里有字体资产:复刻轮 agent 会用 `font_list` 挑字体、`font_import` 把系统字体复制进 `Content/Fonts/`。系统字体仅供本地制作,发行前须确认授权(provenance 里有提醒)。

## 流程

| 阶段 | 发生什么 | 用户能做的 |
|---|---|---|
| `concept` | agent 定设计类型(ui / scene)与画幅,写提示词出 2–4 张候选,视觉自检后提交 | 失败时在 Design 模式补充说明重试 |
| `design_review` | 聊天里出现审阅卡:候选网格、点击放大、选中环 | 采用并开始复刻;提出修改(在选中稿上改图);重新生成;直接在输入框写意见 = 对选中稿修改 |
| `replication` | 看定稿 → 选字体 → 元素清单(叠框图卡)→ 生产素材 → 编译场景 → 截图验收 → 按差异修正 | 失败或中断时点状态条「继续复刻」 |
| `done` | 完成卡:结论、场景路径、未通过原因 | 「提出修复」再修一轮;在 Design 模式发新消息开新流程 |

状态条在输入框上方显示进度与状态,并提供「重新开始」(结束流程指针,产物保留)。

## 复刻工序

1. **元素清单** `design_layout`:画布等于定稿尺寸;恰有一个铺满画布的 `background`(`source: cleanplate`);其余元素逐个列出像素 bbox、叠放 z 与素材来源——`crop`(从定稿切下)、`regen`(被遮挡或切不干净,单体重绘)、`text`(Text 组件,内容逐字照抄)。上限 150 个。
2. **素材** `design_assets`:先做干净底图(前景 bbox 并集外扩 6 像素作蒙版改图,蒙版外逐像素恢复定稿);切图用「定稿 − 底图」差分抠图,底图缺失时退回四角采样色键;重绘以定稿局部为参考出透明底单体,修边后等比放回 bbox。全部入库 `Content/Designs/<slug>/`,provenance `gen-image`,`detail.op` 区分 mockup / cleanplate / crop / regen。
3. **场景** `design_build`:新建 `Content/Scenes/Design/<slug>.rxscene`。正交相机 `orthoSize = 截帧高 / 2 / 100`,元素中心像素换算到世界坐标,全部在 z=0 平面、叠放靠 `sortingOrder`;Sprite 关色键、alpha 混合;Text 的 `boxSize` = 元素 bbox。切换前当前活动场景另存进流程目录;引擎处于 play 态时拒绝切换。
4. **验收** `design_verify`:`viewport_frame{camera:"scene", exact:true}` 按定稿尺寸截帧(超过 1920×1080 时整体等比缩小,素材入库前已同比缩放)。指标:全局亮度 SSIM ≥ 0.85、平均色差 ≤ 18;切图元素区域 SSIM ≥ 0.92、重绘 ≥ 0.6、文字 ≥ 0.5,区域色差 ≤ 40;场景文件里每个元素实体都在、文字内容逐字一致、渲染未截断。每轮最多 6 次。
5. **收尾** `design_complete`:最近一次验收通过才能直接收尾;否则必须带 `acceptFailures` 与原因如实收尾。

`exact` 截帧在 IDE 推流在线时可能触发一次 1–5 秒的渲染会话重建,视口会短暂卡顿。

## HTTP 与事件

模型轮次走 `POST /api/forge/sessions/{id}/ask:execute`,`mode: "design"`。自由文本按阶段路由(无流程 / done → 新流程;concept → 补充重试;design_review → 修改选中稿)。卡片动作携带 `design: {id, rev, action, candidate?}`:

| action | 阶段 | rev | 正文 |
|---|---|---|---|
| `approve_design` | design_review | designRev | 可空;`candidate` 缺省为已选中 |
| `revise_design` | design_review | designRev | 必填 |
| `regenerate_design` | design_review | designRev | 可空 |
| `resume_replication` | replication | replicationRound | 可空 |
| `fix_replication` | replication / done | replicationRound | 必填 |

错误:阶段 / 版本 / 流程不对 → 409 `DESIGN_STAGE_MISMATCH`(details 带 `stage` 与 `allowed`);模型不能看图 → 400 `DESIGN_VISION_REQUIRED`;只读权限 → 400 `DESIGN_NEEDS_WRITE`;有运行中的 run → 409 `SESSION_BUSY`。

不产生轮次的接口:

- `GET /api/forge/sessions/{id}/design` → `{design, review, layout, assets, verify, result}`(后几项为流程目录里对应 JSON)。
- `GET /api/forge/sessions/{id}/design/file?path=` → 流程目录内的 PNG / JSON 原字节(拒绝 `..`、盘符与符号链接)。
- `POST /api/forge/sessions/{id}/design/select` `{id, rev, candidate}` → 持久化选中候选。
- `POST /api/forge/sessions/{id}/design/restart` → 清流程指针(会话空闲时)。

SSE `design` 频道:`design.started`、`design.stage`、`design.candidates.generated`、`design.review.ready`、`design.decision`、`design.layout.ready`、`design.assets.ready`、`design.scene.built`、`design.verify.result`、`design.done`、`design.notice`。

## 文件

流程目录 `.forge/design/<slug>/`:`brief.md`(意图与历次补充)、`rounds/r<rev>/c<i>.png` + `prompt.json` + `submission.json`、`approved.png` + `approved.json`(sha256)、`layout.json`、`overlay.png`、`elements/<id>.png`、`assets.json`、`build.json`、`previous-scene-r<n>.rxscene`、`verify/<n>/{frame,mockup,diff}.png` + `report.json`、`feedback.md`、`result.json`。候选、元素与验收截图不进版本库(`.gitignore`)。

## 已知限制

- 固定参考分辨率,没有自适应锚点布局;Sprite 不跟随 Parent 层级,元素一律绝对定位。
- 复刻产物是独立 2D 场景;3D 项目里的场景类设计稿也只复刻成 2D 画面。
- 运行时改字(逻辑图节点)不在本期;Text 内容变化会重新光栅化,并让 rurix 渲染会话重建一次。
- Text 只在精灵渲染腿生效(2D 场景);模型渲染腿(3D 场景里的 ModelRenderer 路径)暂不绘制 Text。
- 指标阈值以 rurix 后端标定;Godot 后端混合色彩空间不同,像素对比可能偏低。
