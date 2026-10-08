# Godot 后端接入收尾（Stage 6 / 7）

状态：通用 Godot 后端主功能已实现，本机四配置场景矩阵、桌面与复杂资产便携包验收已通过，最终 Rurix 帧基线一致。仍有明确未完成项和三个游戏规划测试失败，详见末节；本记录不代表整仓测试全绿或全部计划边界均已验收。

最终核验日期：2026-09-30。GPU 测试串行执行；使用隔离的 `forge-agentd-completion.exe`，未提交工作区改动。

## 范围

- Godot 4.7.2 / gdext 0.5.5，维持现有版本。
- Forward+ / D3D12、Forward+ / Vulkan、Mobile / D3D12、Compatibility / OpenGL 3。
- 补齐通用引擎渲染、运行时打包和桌面集成；不迁移 Code Sentinels，不修改其项目后端或游戏规则。
- 保留 Forge 逻辑、物理、资产导入、JSON-RPC、WebSocket 及原 Rurix 渲染路径。

## 已验证的收尾修正

### 雾背景颜色

`crates/godot-host/src/env.rs` 补偿固定 Godot 版本的雾天空路径：

- Forward+ / Mobile：消除重复 sRGB 解码和重复背景能量乘法。
- Compatibility：单独反解 GLES 天空着色器的三次多项式 sRGB 近似和能量重复乘法；只覆盖 linear / reinhard 与正曝光，其他 tone map 和零曝光如实列为受限。
- 仅作用于 clearColor / color 背景；天空材质保持原生行为；零能量保持黑色。
- `g7_background` 覆盖两类背景、普通/体积雾、背景能量、相机曝光及 tone-map 曝光，四配置通过。修正前已实际复现 `(9,11,15) -> (1,1,1)`。
- `env::tests` 验证反变换的数学关系和零能量边界。

### 金属环境补偿

`crates/godot-host/src/material.rs` 将原先对所有 metallic > 0 的补偿限定为实际全金属表面；显式 Environment 沿用 Godot 语义。纹理分支按 MR 贴图后的 metallic 判定，保留材质自发光。

- 修正前半金属探针最大通道偏差约 10.4–11.5，修正后约 5.4–6.0。
- 全金属探针偏差仍为 1。
- `g5_fixes` 为半金属增加实际断言，不再只打印数据。
- 两套 BRDF 仍非逐像素等价，已在 `coverage.limited` 说明；补偿不是通用 PBR 等价转换。

### 回归工具

- `g3_host` 的能力列表按已实现的三条渲染路径更新；其他像素阈值保持原样。
- `g7_settings` 验证 Forward+ 的 FSR / FSR2 实际改变缩放重建、其他配置按能力回退，并验证恢复 bilinear 后帧一致。
- `f4-scene-matrix.ps1` 新增 `-RequireContent -EvidenceStage stage6`：参考帧有内容而 Godot 为空时必须失败；保留逐场景哈希、稳定性及错误记录。
- 新增 `g7_preview` 主/预览通道尺寸和内容隔离测试，结果以最终验收记录为准。

### 普通精灵的真实像素验收

`g6_sprite_render` 使用临时项目和合成 RGBA 图集，不依赖或修改具体游戏资产。`completion-sprite-state-probes.log` 记录两个 GPU 测试通过，均串行遍历四种配置：

- Canvas 正交 2D、3D quad 正交及透视：四象限图集裁切、三种翻转组合、编码颜色 tint、切帧与 pivot、色键开关和删除后恢复背景；不透明颜色允许每通道最多 3 的偏差。
- Canvas 与 3D quad：opaque / alpha / additive 实际像素不同，透明精灵 sortingOrder 动态交换前后遮挡，选中状态改变像素。
- 显式 `asset.reload` 在不重启宿主的情况下刷新 Canvas 图集；删除后不残留精灵。
- 实测修正了 Canvas 上下倒置、翻转导致矩形偏移、3D quad 未使用裁切/翻转参数，以及空间着色器重复颜色转换。

能力边界：Forward+ 的 3D 精灵与 Mobile 的 Canvas/3D 精灵使用 HDR 线性混合，不能声称与编码空间混合逐像素等价。例如同一半透明纹理的 Forward+ Canvas 样本为 `[92,52,31]`，3D 样本为 `[118,60,31]`；两者均正确产生混合效果，但视觉数值不同。此差异已加入 `Sprite.blendMode` 能力限制。

`completion-sprite-mixed-final.log` 另验证四配置混合 MeshRenderer/ModelRenderer 的前后深度遮挡、同场景正交→透视→正交分流、加入/删除模型恢复 Canvas，以及 90 度旋转。模型路径原先将普通精灵 `[192,64,32]` 压暗为 `[159,62,32]`；现仅在无显式 Environment 的模型路径逆补偿缺省 Reinhard，前三配置恢复精确目标，Compatibility 为 `[192,64,30]`，仍在原定容差 3 内。后续扩展用例还检查显式 Environment 生效与删除恢复。

`g6_sprite_render` 的不透明排序探针确认：Canvas sortingOrder 动态交换有效；不透明 3D quad 依赖深度缓冲，同深度时 sortingOrder 不改变遮挡。前移/后移深度均有效。该边界列入 `Sprite.sortingOrder`，需要同平面手动排序时使用 alpha 模式。精灵缓存长期有界性仍未通过压力测试证明。V6 的独立地面图集验证见下一节，不能以普通精灵通过替代。

### 质量、缩放与 Canvas Glow

- `completion-settings-strengthened.log`：2 个四配置测试通过。FSR / FSR2 在 Forward+ 实际改变缩放像素，其他配置按能力回退；恢复 bilinear 后输出一致。
- SSAO 从 veryLow 到 ultra 的变化像素数：Forward+ 两配置各 12,460，Mobile 为 0，Compatibility 为 176。三类行为均有断言，恢复低质量后帧完全一致。
- `completion-sprite-glow-final.log`：黑色背景排除了背景自身产生 Glow；四配置 Canvas 开关 Glow 都为 0 个变化像素，3D quad 则分别变化 57,578 / 57,578 / 49,826 / 14,853 个像素，关闭后恢复原帧。
- 当前 Canvas 在 Environment 后合成。Mobile 的 HDR 2D 缓冲用于精度/颜色输出，不意味着已支持 2D Glow；`Sprite.canvasEnvironment` 如实上报限制，不新增 Forge 组件。
- 初次 Glow 夹具用了不存在的 `backgroundMode` / `tonemapMode` 属性，因默认背景自身发光而失败；修正为真实字段 `background` / `tonemap` 后重测，原始日志保留。首次显式 tone map 夹具使用 `white=1e20` 导致 GLES 黑帧，改为有限正常值 16；不通过放宽颜色阈值掩盖结果。

### V6 地面图集像素验收

`g6_v6_sprite` 使用临时项目、合成清单和红色选中帧旁的绿色/蓝色干扰像素。GPU 用例默认 ignored，验收时显式使用 `--include-ignored --test-threads=1` 执行；本轮确实执行而非跳过。

首次执行发现真实空帧：普通 Canvas 判定误接管 V6 等距批次，`scene.rs` 的 Canvas 路径随后清除 V6 实例。`list.rs` 现将 Canvas 分流限定为普通 sprite_mesh 路径，并增加 CPU 回归断言。另外补齐 V6 混合模式变量，并将像素缓存查询锁的释放移到 miss 分支之前，避免 Rust 2021 的 if-let 临时锁跨 else 分支存活导致重复加锁。

接入复核还修正了地面与普通精灵共用几何缓存键、无效图集材质回退成无纹理几何的问题，并让 CPU 像素缓存键包含图集文件大小及修改时间。时间戳不是内容哈希，不能据此声称覆盖所有外部文件变更方式。

`completion-v6-atlas-reload-probes.log`：两个测试通过，包含 CPU 夹具检查和四配置 GPU 用例：

- Forward+ / D3D12、Forward+ / Vulkan、Mobile / D3D12 各产生 18,308 个目标红色像素；Compatibility / OpenGL 3 产生 18,352 个。
- 改变相邻图集像素而保留选中帧后，四配置目标区域的污染像素均为 0。
- 将选中帧改为蓝色并执行 `asset.reload`，不重启宿主，前三配置各有 18,291 个原红色像素变为目标蓝色，Compatibility 为 18,352 个。这排除了用固定颜色几何代替图集的假通过。
- 本夹具使用地面 additive 图集；验证绘制、裁切隔离及热重载，不证明 additive 与 alpha 的完整数值差异、非地面 pivot/旋转、单位建筑特效排序或与 Rurix 逐像素一致。

原始失败保留在 `completion-v6-sprite-probes.log` 和 `completion-v6-sprite-diagnostic.log`；路由修正后的初次通过在 `completion-v6-routing-probes.log`。均未修改具体游戏项目或游戏规划逻辑。

最终重建后定向回归：`completion-v6-routing-unit.log` 的 5 项清单 CPU 测试通过；`completion-v6-sprite-regression.log` 的 `g4_v6` 2 项、`g6_sprite` 3 项、`g6_sprite_render` 2 项、`g6_v6_sprite` 2 项全部通过，共 9 项且 0 忽略。GPU 顺序执行，包含普通精灵、原有 V6 几何对照、粒子以及图集裁切/热重载，未替代全场景矩阵或最终桌面回归。


### V6 非地面建筑图集

`completion-v6-non-ground-retry.log`：真实 `game.session.open` 通过临时 `models.command-core` 清单加载非地面图集，四配置全部通过。初始品红像素均为 21；修改 pivot 后均为 42、包围盒位移 3 像素；alpha→additive 均改变 42 个像素；关闭并重开后仍产生 25 个目标像素。没有修改游戏项目资产或规则。

首次测试失败是夹具错误地沿用了地面数千精灵的 `draws > 100` 断言：该夹具只提供建筑图集，真实 draws 为 3。修正为相对基础几何新增实际精灵的 `draws > 2`，保留颜色、位移、混合和重开断言，原日志 `completion-v6-non-ground-acceptance.log` 保留。地面用例在同一轮仍通过。

边界：本轮非地面覆盖建筑，不代表单位/特效的旋转和异色重叠排序已验收，也不是游戏级发布测试。`SentinelsV6Batch.sprites` 从 skipped 移到 limited，并列明已测内容与未测项。

### 运行时清单、打包与监督器

- `assetd::godot_runtime` 校验必需文件、清单路径、SHA-256 及渲染缺省配置；开发目录额外比较当前构建 DLL，外部便携目录只依赖自身清单。父级补齐中间路径的符号链接检查和真实路径穿越测试；Windows junction 等其他重解析点尚未专项验收。
- `pack.rs` 根据项目配置选择 Godot 或 Rurix，保留原 `build_pack` 接口。父级修复 Godot 分支漏带通用原生脚本缓存及其 `FORGE_RURIXC` 启动环境的问题；模型包内嵌 UUID 不再被误判为外部悬空 GUID，当前模型包也执行结构校验，旧 `.mat` 文件的贴图引用加入闭包。
- `completion-runtime-unit-final.log`：4 项运行时校验测试通过；`completion-pack-unit-final.log`：7 项打包测试通过，包含网格缓存读取、历史模型加载、贴图解码、Godot/Rurix 启动脚本、缺失/损坏/非法配置和闭包检查。原生脚本单测使用合成缓存，不能视为便携包内真实脚本执行验收。
- `completion-runtime-supervisor.log`：真实 Godot 宿主强杀后重启、事件写入、新 PID 与 RPC 可用性通过。恢复行为是创建名为 `restored` 的空场景，不是还原崩溃前所有编辑状态。
- 同一日志中旧 watchdog 测试因工具数断言仍为 50 失败；核实本次新增两个渲染元数据工具后改为 52 并显式断言名称，`completion-runtime-watchdog-retry.log` 复跑通过。两个监督器测试现在使用独立事件日志，避免其他运行实例的事件造成假通过。

#### 便携包四配置实机测试

`scripts/godot-pack-smoke.ps1` 通过独立 daemon 与临时项目打包，复制到新目录并删除临时源项目，启动前清除全部继承的 `FORGE_*` 环境变量，从无关工作目录执行包内启动脚本。最终夹具包含真实 PNG 图集、rxsprite、rxmodel、GUID 元数据和历史模型缓存，不再只检查简单立方体或背景非黑。

最新四种配置均通过，目录内保存 `report.json`、daemon 与宿主日志：

- Forward+ / D3D12：`pack-smoke-20260930-030433/`，日志 `completion-complex-pack-d3d12-retry.log`。
- Forward+ / Vulkan：`pack-smoke-20260930-030541/`，日志 `completion-complex-pack-forward_plus-vulkan.log`。
- Mobile / D3D12：`pack-smoke-20260930-030608/`，日志 `completion-complex-pack-mobile-d3d12.log`。
- Compatibility / OpenGL 3：`pack-smoke-20260930-030628/`，日志 `completion-complex-pack-gl_compatibility-opengl3.log`。

每轮均验证资产引用闭包、必需文件与启动环境设置。删除源项目后，场景加载 2 个实体并提交 2 次绘制；精灵图集的红、绿区域各为 256 像素，固定采样点保持原定每通道容差 3。场景引用 revision 1 的蓝色历史模型，而当前 revision 2 为白色；移相机观察模型后，四配置中心颜色均为 `[55,77,178]`，证明包内历史缓存实际参与绘制。再将相机移出模型区域，中心恢复背景。

四份报告均为 `passed=true`、`sourceRemoved=true`、`forgeOverridesCleared=true`，实际渲染后 `backend.ready=true`，Godot / gdext 版本为 4.7.2 / 0.5.5。便携测试使用 NVIDIA；D3D12 因与缺省 Intel 适配器不一致而如实走回读路径，共享传输另由宿主专项与桌面测试验证。

保留首次失败 `completion-complex-pack-d3d12.log`：测试夹具试图在游戏模式调用 `transform.set`，被现有权限正确拒绝；改用相机观察模型后通过，没有放开游戏模式编辑权限。早期简单场景四配置证据 `pack-smoke-20260930-020532/`、`020626/`、`020702/`、`020731/` 仍保留。

边界：最新包已纳入模型 UUID 闭包修正和真实历史模型缓存验收；旧 `.mat` 外部贴图闭包仍以单测为证。全部资产格式、glTF 外部 URI、真实原生脚本便携执行及另一台未安装开发工具的机器尚未端到端验证。

`main.rs` 曾被本轮交付意外整文件格式化。已检查可用历史和候选副本，未找到可信的修改前工作树基线，因此尚未安全恢复无关格式差异；功能改动与编译保留。不能以 Git HEAD 覆盖该文件原有用户改动，此项明确保留为未完成的差异清理。

### 桌面集成

- 视口头尾展示当前工作区的真实后端、驱动、设备、帧出口和能力限制；项目配置与运行实例不一致时只提示重启，不自动切换。
- 共享呈现依据 `frameExits.sharedD3d12`，2D 网格显示时使用网页回读帧并在本地叠加，编辑辅助不进入 RPC 或共享帧；统计和提示移出原生子窗口边界。
- bounds 携带推流尺寸、DPI 和工作区 ID；旧工作区能力不能用于开启新工作区的原生视口。
- 呈现进程崩溃释放共享资源与重开操作串行执行，避免旧清理请求关闭新缓冲；打开共享失败时关闭呈现进程并保留网页回读。
- `apps/desktop/test/presenter-lifecycle.test.cjs` 三项测试通过，覆盖崩溃清理顺序、跨工作区释放与共享打开失败。
- `completion-desktop-unit-tests.log`：5 个前端定向测试文件共 54 项通过；类型检查、客户端/协议/网页服务构建通过。能力详情改为普通文档流，展开时增加头部高度，避免被原生子窗口遮挡；2D 网格测试覆盖共享能力为 true 时仍关闭原生呈现。
- 已补齐 `forge-agentd` 的两个渲染元数据工具白名单。初始化与工具响应分别计时：引擎连接/排队/重连使用有界 90s 启动窗口，正常工具响应仍为 10s，已有长响应工具仍为 40s；响应超时废弃连接且不自动重放。网页代理和桌面 HTTP 客户端预留 135s 外层预算，避免先于服务端截断冷启动。
- `completion-desktop-mcp-tests.log` 6 项通过，其中真实 PowerShell stdio 子进程覆盖慢初始化、正常响应及响应超时废弃连接；`completion-desktop-proxy-tests.log` 19 项通过。
- 桌面冒烟支持独立端口、独立 Electron 用户目录和证据目录，不抢占现有开发服务或覆盖交互用户的数据；后台服务 stdout/stderr 单独留痕。独立测试账户走已有“使用自带密钥”入口，不伪造登录状态。
- 冷启动复测已实际经过约 29s 的校验与 GPU 重试，成功查询真实 Godot Forward+/D3D12 元数据。失败记录保留：`desktop-presenter-godot-20260930-005326/` 被独立账户登录页遮挡；`desktop-presenter-godot-20260930-005905/` 因默认 2D 场景按设计使用网页网格而未产出原生呈现证据。脚本现显式创建 3D 透视测试场景，不修改项目默认维度。

#### 真实桌面四配置复测

以下四轮各 8 项判据通过，使用独立端口 18103/13080，不占用现有 8103/3080 服务：

- `desktop-presenter-godot-20260930-010117/`：Forward+ / D3D12，`zero_copy`，Intel Graphics。
- `desktop-presenter-godot-20260930-010228/`：Forward+ / Vulkan，`readback_upload`，NVIDIA RTX 5060 Laptop GPU。
- `desktop-presenter-godot-20260930-010303/`：Mobile / D3D12，`zero_copy`，Intel Graphics。
- `desktop-presenter-godot-20260930-010348/`：Compatibility / OpenGL 3，`readback_upload`，NVIDIA RTX 5060 Laptop GPU。

每轮均验证至少 3 次呈现、共享内容非底色、Windows PrintWindow 合成中心像素与引擎回读一致、原生子窗口拉伸铺满、WS_DISABLED 输入透传标记、RPC 点选命中 cube-a、编辑→运行→编辑及 Electron 正常退出。测试进程已清理。

最新运行时最终复测再次四配置通过，各 8 项判据：

- `desktop-presenter-godot-20260930-025333/`：Forward+ / D3D12，Intel，zero_copy。
- `desktop-presenter-godot-20260930-025430/`：Forward+ / Vulkan，NVIDIA，readback_upload。
- `desktop-presenter-godot-20260930-025514/`：Mobile / D3D12，Intel，zero_copy。
- `desktop-presenter-godot-20260930-025614/`：Compatibility / OpenGL 3，NVIDIA，readback_upload。

对应 `completion-delivery-desktop-*.log`。这轮使用 `completion-delivery-build` 后重新生成的 `target/godot-runtime`，包含最终精灵与能力声明修改，已替代仅旧快照的验收限制。

边界：未启用强制置顶的 `-OnScreen`，因此没有真实屏幕鼠标命中测试；点选是 RPC 校验，不冒充真实鼠标操作。跨工作区/崩溃恢复目前由生命周期单测覆盖；跨显示器 DPI 切换尚未实测。

## 当前验证证据

证据位于 `evidence/godot-backend/`，属于本机实际执行记录：

- `stage6/scene-matrix-20260930-024537/`：最新运行时四配置各 66/66 出帧、66 稳定、0 错误、0 空帧，共 264 个配置/场景组合；启用 `-RequireContent`，参考图有内容时 Godot 不能以纯背景通过。日志为 `completion-delivery-matrix.log`。先前 `scene-matrix-20260930-021540/` 同样通过，保留为阶段证据。
- `completion-sprite-acceptance.log`：5 个普通精灵测试全部通过，每项串行遍历四配置，涵盖图集、混合、Glow 边界、显式/缺省 Environment、混合深度、动态分流及不透明排序限制。
- `completion-neutral-final.log`：中立数据 24 通过、1 忽略（忽略项不计通过）；`completion-logic-final.log`：逻辑 47、场景 17 通过；`completion-runtime-final.log`：运行时 4 通过；`completion-pack-final.log`：打包 7 通过。
- `completion-rurix-baseline.json`：396 个帧条目。最终构建后的 `completion-delivery-rurix.log`，标签 `completion-final`：396/396 帧匹配，0 哈希不匹配，0 不稳定；按基线固定 Intel Vulkan 驱动。先前 `completion-rebuild` 同样通过，仅保留为阶段证据。
- `completion-delivery-transport.log`：`g3_host` 6 项通过、0 忽略，包括四配置稳定帧、点选、编辑/运行切换、版本/能力、WebSocket 及 D3D12 共享与回读一致性。D3D12 调试层 corruption=0、errors=0，保留 1 条仅屏障命令的优化提示 warning，不描述为零警告。
- `completion-delivery-opaque.log`：加强不透明空间精灵交换 sortingOrder 后重叠采样点颜色相等断言，四配置再次通过；Canvas 排序和空间前后深度断言保留。
- `completion-delivery-core-contract.log`：编辑/撤销/保存回读、流水线、后端元数据、核心启动、JSON-RPC 与 WebSocket 共 14 项再次通过、0 忽略。
- `completion-render-tests.log`：宿主传输、环境、粒子、光照参数、体积效果、雾背景及缩放模式的串行四配置测试，17 个测试通过（后续新增的质量采样测试尚未包含在该轮结果中）。
- `completion-core-unit-tests.log`：122 通过、3 失败、7 忽略；忽略项不计为验证通过。
- `completion-geometry-tests.log`：模型、动画、材质、V6 通用几何及主/预览通道隔离，18 个测试通过。
- `completion-scene-logic-tests.log`：`forge-scene` 17 个和 `forge-logic` 47 个测试全部通过。
- `completion-core-contract-tests-retry.log`：编辑、流水线后端、核心启动、JSON-RPC 和 WebSocket 共 14 个测试通过。第一次构建曾遇到并行修改中的 V6 精灵字段类型不一致，完成对应中立数据修改后复跑通过；原始失败日志保留。

### 必须保留的失败记录

本轮扩大回归发现 `crates/engine-host/src/sentinels_v6.rs` 中三个游戏规划测试失败：

- `construction_preview_releases_lock_and_projects_only_its_private_copy`：建造费用 credits 未大于零。
- `stale_advice_does_not_replace_current_authority_or_skip_its_order_checks`：null 不能反序列化为 Command。
- `suggestion_releases_host_lock_before_computation_and_keeps_authority_unchanged`：规划 commands 为空。

该文件的游戏规划逻辑不属于本次接入修改范围。保留失败及原断言，不能把整仓测试描述为全部通过。

## 交付结论与剩余边界

通用 Godot 后端的普通精灵、通用 V6 地面/建筑精灵、效果差异修正、运行时校验、双后端打包、桌面能力展示与宿主恢复已实现。本机最新产物完成四配置 264 个场景组合、396 条 Rurix 基线、四配置桌面及复杂便携资产验收；已测主功能不再列为“尚待集成”。Code Sentinels 的后端选择和游戏级发布验收保持原状。

### 仍未完成

- `crates/forge-agentd/src/main.rs` 无关格式改写尚未安全恢复。缺少可信的修改前工作树基线，不能用 HEAD 覆盖用户已有改动。
- 三个游戏规划单测仍失败，具体名称与错误见上节；未修游戏逻辑，也没有证明这些失败在本轮修改前已存在。
- V6 单位/特效旋转、异色重叠排序和与 Rurix 的像素一致性；精灵缓存长期压力与有界性。
- 真实原生脚本在便携包内执行、glTF 外部依赖、干净机器运行，以及 Windows junction 等重解析点专项。
- 桌面真实屏幕鼠标命中和跨显示器 DPI 切换。跨工作区/呈现进程崩溃由单测覆盖，Godot 看门狗恢复仅验证空场景重建。

### 已验证并上报的限制

- Canvas 不参与 Environment Glow；不透明空间精灵同深度时 sortingOrder 不生效。
- HDR 线性混合与编码空间混合、Godot 与 Rurix 的材质 BRDF 不保证逐像素一致。
- Mobile / Compatibility 原生不支持或本接入实测无效的效果继续按 `unsupported` / `limited` / `skipped` 上报，不以四配置通过冒充全部效果支持。

以上未完成项与限制不因主功能验收通过而关闭。全部改动保留在工作区，未执行提交，也未将整份工作区 Git 差异归为本轮修改。
