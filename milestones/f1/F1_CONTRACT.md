---
contract: F1
title: F1 场景编辑闭环
status: active
implementation_status: unlocked
active_scope: wave.4
version: 0.1
date: 2026-08-16
timebox: 会话制推进,做不完转 deferred
rfc_required: []
upstream_docs:
  - 03_ENGINE_LAYER.md (§5)
  - 07_FRONTEND_IDE.md (§1–§5)
  - 09_ENTITY_SCENE_MODEL.md
  - 13_ROADMAP.md (F1)
  - milestones/f0/F0_CONTRACT.md
implementation_unlock:
  required_all:
    - F0 验收门全绿(wave.1+wave.2 §8 已录)
    - 用户开工指令(2026-08-16「进入 F1 场景编辑闭环」)
in_scope:
  - forge-scene:组件注册表(Transform/MeshRenderer/RigidBody/Light/Camera 首发集)+ Entity 组件化模型 + 确定性 .rxscene 序列化
  - engine-host:entity.*/component.*/transform.* 全量 CRUD、Undo/Redo 命令栈(host 侧)、scene.save/load/diff、scene.checkpoint/rollback、play.enter/pause/resume/step/exit/state 双态
  - engine-scene-mcp:上述方法对应工具面全量(snake_case)
  - packages/host:forgeProxy 插件(/api/forge/* → agentd:8103)
  - packages/client:IDE 七区骨架(07 §1),Hierarchy/Inspector 真实接线,PIE 双态 UI,风格保持 cursor+Claude
  - viewport_pick 最小链路(屏幕坐标 → cast_ray → 选中同步)
  - wave.2:engine-host 接入 rurix-rt(vulkan)render_exec 场景实渲染(相机 UBO + 深度 + 逐实体色)+ Readback 回读;viewport.frame/set_camera/pick RPC
  - wave.2:D3D12 共享纹理生产者(engine-host 内 editor glue,NT handle)+ viewport-presenter 进程(Rust windows-rs,子窗口呈现)+ 帧协商(分辨率跟随面板)
  - wave.2:apps/desktop 视口 bounds 同步与 presenter 生命周期;packages/client Viewport 真实帧显示(canvas 回退腿)+ 点选同步 + Alt 环绕/滚轮缩放/F 聚焦 + gizmo 拖拽
  - wave.3:上游 rurix-rt vulkan 档补 VK_KHR_external_memory_win32 import 面(纯 pNext 结构体注入,零新 FFI 函数;设备扩展启用)+ render_exec TextureDesc 外部纹理变体(经 RFC-0001 同配方:D3D12 committed resource NT handle + GetResourceAllocationInfo 尺寸)
  - wave.3:engine-host share.rs 共享纹理创建后导回 VK(方向 B:D3D12 建、VK import 直渲),帧源仍为 viewport::render_scene_frame;viewport.frame 响应带 frame_path=zero_copy|readback_upload 与 CPU upload 计数(机器可证零拷贝)
  - wave.3:同步 v1 = CPU 块(session 帧 fence 有界等待后 D3D12 queue.Signal),不引入 VK external semaphore(转 RD-F1-004)
  - wave.4:H.264 流腿(RD-F1-003 残件):视口帧经 H.264 编码为 Annex B 流,供纯 web 等无法走共享内存的客户端消费
  - RD-F1-004 方向裁决:no-go(F1 wave.4):VK_KHR_external_semaphore_win32 共享语义单向(D3D12 fence handle→VK import 可行,VK semaphore handle→D3D12 不可消费);当前帧流向 VK 渲染→D3D12 呈现与可行方向相反,external semaphore 无法消除 CPU 块;v1 CPU 块已达标,RD-F1-004 保持 open 待帧流向反转场景(如 D3D12 先算→VK 后渲)再评
out_of_scope:
  - Assets 面板全功能(F2);NodeGraph 编辑(F4)
  - 真实 LLM 自然语言理解(mock provider seam;F3 移植后回填)
deferred_refs: [RD-F0-001, RD-F0-002, RD-F0-003]
deliverables:
  - id: D-F1-1
    name: 引擎侧场景编辑全量方法 + Undo/Redo + checkpoint + PIE
    evidence: cargo test 输出
  - id: D-F1-2
    name: IDE 七区骨架与真实接线
    evidence: client vitest + desktop 冒烟截图
  - id: D-F1-3
    name: 确定性重载与批量创建实测
    evidence: scripts/f1-scene-smoke.ps1 输出
acceptance_gates:
  - id: G-F1-1
    name: 确定性重载门
    check: 建 3 实体 → 摆位 → scene.save → 重启 host → load → 与磁盘逐字节同态(cargo test + 冒烟脚本双证)
  - id: G-F1-2
    name: Undo/Redo + checkpoint 门
    check: cargo test:命令栈 undo/redo 语义;scene.checkpoint 后改场景,rollback 恢复逐字段一致
  - id: G-F1-3
    name: 七区骨架门
    check: client vitest 全绿;desktop 冒烟截图可见七区(cursor+Claude 风格)
  - id: G-F1-4
    name: 批量创建门
    check: 经 gateway→agentd→MCP 调用 entity_batch_apply 创建 10 立方体排一列,scene.summary entityCount=10 实测;自然语言→工具映射段 DEV_ENV_DEGRADE(mock provider,无 LLM key),不充绿
  - id: G-F1-5
    name: PIE 双态门
    check: cargo test:play.enter/pause/step/exit 状态机合法迁移与非法迁移拒绝;运行态修改不污染编辑态(退出恢复原值)
  - id: G-F1-6
    name: 场景实渲染门
    check: viewport.frame 经 rurix-rt vulkan render_exec 渲真实场景(实体立方体+透视相机+深度);cargo:锚点像素非恒定 + 同场景两帧逐字节一致 + 移动实体后帧哈希变;无 vulkan 设备如实 DEV_ENV_DEGRADE(本机 spike READY,evidence/f1-w2-device-spike.json)
  - id: G-F1-7
    name: 点选门
    check: viewport.pick 屏幕坐标→射线→实体 OBB→entityId;cargo 射线数学单测;栈级脚本经 gateway 点中指定立方体并选中同步
  - id: G-F1-8
    name: 相机/gizmo 门
    check: 相机 orbit/zoom 后帧内容变化;gizmo 拖拽→transform.batchSet 生效且 edit.undo 回滚逐字段一致
  - id: G-F1-9
    name: 共享纹理门
    check: viewport-presenter 进程经 NT handle 打开 D3D12 共享纹理并呈现至 Electron 视口区;截图非占位且锚点像素与 readback 帧一致;handle 生命周期关闭无泄漏;三态(PASS/SKIP/DEV_ENV_DEGRADE)诚实
  - id: G-F1-10
    name: 零拷贝生产门
    check: share 开启时 viewport.frame 帧内容经 VK→D3D12 import 纹理产出(D3D12 建、VK 直渲):响应 frame_path=zero_copy 且 cpu_uploads 计数零增量;锚点像素与 readback 腿一致;同场景两帧逐字节一致 + 移动实体帧变仍成立(同一帧源);无 vulkan 设备如实 DEV_ENV_DEGRADE
  - id: G-F1-11
    name: 零拷贝呈现门
    check: 零拷贝档下 presenter 呈现 presented>=3 且 OS 级截屏锚点像素与引擎帧一致(复用 f1-w2-desktop-presenter-smoke 判定面);面板 resize→共享纹理/import 重建无错;share_close 幂等;无句柄泄漏(open/close ×10 循环无错)
  - id: G-F1-12
    name: 回退腿回归门
    check: share 未开时 readback→upload 路径行为 0-byte 回归(f1-w2-viewport-smoke 原样 PASS);cargo test --workspace / pnpm -r test / go test 全绿
  - id: G-F1-13
    name: H.264 流腿门
    check: viewport.frame 请求 format=h264 时返回 Annex B 码流(起始码 00 00 00 01 + SPS/PPS/IDR),纯 web 客户端可经 WebCodecs VideoDecoder 解码;cargo:编码器初始化+单帧编码非空+码流起始码断言;栈级:经 gateway 取 h264 帧码流非空且解码后尺寸一致
guardrails:
  - 诚实优先:任何门不过如实报 FAIL/DEV_ENV_DEGRADE,不回写 PASS
  - 数字必须来自命令输出
  - UI 风格回归基准:cursor+Claude 系(同 wave.1 截图基准)
---

# F1 契约:场景编辑闭环

## 1. 目标与双门状态
实现 13_ROADMAP F1:Entity/组件模型 + 注册表、.rxscene 读写、Hierarchy/Inspector、Undo/Redo、PIE 双态、七区骨架。status=active;implementation_status=unlocked。

## 2. 范围与波次
- wave.1(已验收,§8):front matter in_scope 原 wave.1 段全量。
- wave.2(本波):Viewport 真实帧通道——rurix-rt vulkan render_exec 场景实渲染 + Readback(回退腿 canvas 流)+ D3D12 共享纹理 + viewport-presenter 子窗口呈现(主攻腿,用户拍板);viewport_pick 真机链路;相机操控与 gizmo 实操。
- wave.3+:VK→D3D12 零拷贝(上游 VK external memory 面补齐后)、H.264 流、Assets 面板(F1 尾/F2 衔接)。

## 3. 波次门禁
见 acceptance_gates G-F1-1~5。

## 4. Deferred 处置
本波新增 deferred 追加于下方。

deferred:
  - id: RD-F1-001
    content: Viewport 真实帧通道(D3D12 共享纹理优先/H.264 回退)与 viewport_pick 真机链路(屏幕坐标→cast_ray→选中)
    reason: 依赖 rurix-render vulkan 档设备面与 IDE 原生组件,F1 wave.1 以占位+帧统计呈现
    refill: F1 wave.2 立项,先 rurix-render vulkan 档设备实测,再帧协商协议
    owner: F1 wave.2
    status: CLOSED(2026-08-17 wave.2 回填完毕,§8 验收记录)
  - id: RD-F1-002
    content: Chat 自然语言→工具映射(真 LLM 工具循环)
    reason: forge-agentd 为最小骨架 + mock provider seam,无 LLM key;DEV_ENV_DEGRADE 不充绿
    refill: RD-F0-003 七 crate 移植带回真 providers 后回填
    owner: F3 前置波
  - id: RD-F1-003
    content: VK→D3D12 零拷贝帧通道(VK_KHR_external_memory_win32)+ H.264 流备选腿
    reason: 上游 rurix-rt vulkan 档无 external_memory_win32 面;共享纹理腿已达标,零拷贝为性能优化而非功能缺口
    refill: 上游面补齐后立项
    owner: F1 wave.3+
    status: CLOSED(2026-08-17 wave.3 回填完毕,§8 验收记录;H.264 备选腿残件 2026-08-17 wave.4 回填完毕,G-F1-13 PASS)
  - id: RD-F1-004
    content: VK external semaphore(VK_KHR_external_semaphore_win32)真 GPU 侧帧同步
    reason: wave.3 同步 v1 采 CPU 块(session 帧 fence 等待后 queue.Signal),已达标但有 CPU 往返延迟;GPU 侧信号量可消除
    refill: 帧流向反转为 D3D12→VK(如 D3D12 光栅→VK 后处理)或出现 CPU 块实测瓶颈时重评;当前 VK→D3D12 流向与扩展单向语义冲突,external semaphore 无收益
    owner: 帧流向反转场景出现时
    status: OPEN(2026-08-17 wave.4 方向裁决 no-go:单向语义与帧流向冲突)

## 5. 修订
- 2026-08-16 立项:F0 全绿后用户指令开工。
- 2026-08-17 wave.2 立项:用户拍板「直接攻共享纹理」。设备 spike  verdict=READY(RTX 4070 Ti 12GiB / Vulkan 1.4.351 / CUDA 13.3 / MSVC 17.14.37531.7;上游 d3d12_interop_smoke device 段真过 interop_ok=true;render_exec UBO+深度+Readback 面核验在位)——evidence/f1-w2-device-spike.json。RD-F1-001 启动回填(共享纹理主攻 + readback 回退双腿);VK 零拷贝上游无面转 wave.3 RD。
- 2026-08-17 wave.3 立项:用户指令「继续执行 F1 wave.3,推进 VK→D3D12 零拷贝」。方向裁决:**B(D3D12 建共享纹理 → VK import 直渲)**,否决 A(VK 导出 → D3D12 置放资源):B 与上游 RFC-0001 D3D12→CUDA import 同配方(committed resource NT handle + GetResourceAllocationInfo),VK import 仅需 pNext 结构体注入零新 FFI 函数,且规避 VK OPTIMAL tiling 私有布局被 D3D12 误读的风险;A 保留为备胎。同步 v1 采 CPU 块(session 帧 fence 等待后 queue.Signal),VK external semaphore 转 RD-F1-004。RD-F1-003 启动回填。
- 2026-08-17 wave.3 验收:G-F1-10/11/12 全 PASS(§8 验收记录)。实测捕获 committed resource 尺寸天花板:vk req > d3d12 committed alloc 时(960x540 等实尺)bind 静默失败→设备丢失;定案双腿架构(committed/共享堆+placed resource,probe_image_mem_req 先探后建)。RD-F1-003 CLOSED(H.264 残件仍 open)。
- 2026-08-17 wave.4 立项:用户指令「继续执行 F1 wave.4,处理 H.264 流腿和 external semaphore」。H.264 流腿落地(openh264 0.6 纯 Rust CPU 编码,Annex B,viewport_frame format 参,编码器懒建+尺寸变化重建,每 60 帧一关键帧);external semaphore 方向裁决 **no-go**:VK_KHR_external_semaphore_win32 共享语义单向(D3D12 fence handle→VK import 可行,VK semaphore handle→D3D12 不可消费),与当前 VK 渲染→D3D12 呈现帧流向相反,无法消除 CPU 块——RD-F1-004 维持 OPEN 待帧流向反转场景再评,上游 patch8 基础面留档未集成。
- 2026-08-17 wave.4 验收:G-F1-13 PASS(§8 验收记录)。RD-F1-003 H.264 残件回填完毕。

## 6. Close-out(只追加区)
<!-- 禁止预填 PASS -->

### wave.1 验收记录(2026-08-17,host=Windows NT/cargo 1.93.1/Node v22.14.0/pnpm 11.5.0)

**1. 独立断言清单(逐门)**

| 门 | 判定 | 证据 |
|---|---|---|
| G-F1-1 确定性重载 | PASS | cargo:f1_editing 集成「3 实体+组件+摆位→save→新进程 load→再 save 逐字节相等」;栈级:scripts/f1-scene-smoke.ps1 save/new/load 实体表逐字段一致,evidence/f1-scene-smoke-*.log |
| G-F1-2 Undo/Redo + checkpoint | PASS | cargo:undo/redo(create/transform_set 逆操作、redo 栈清空拒绝)、checkpoint→改→rollback 逐字段一致;栈级:checkpoint→+1→rollback→10 PASS |
| G-F1-3 七区骨架 | PASS | `pnpm --filter @forge/client test` 25/25(EditorView 七区冒烟/forgeApi 二次解析/editorStore 迁移);desktop 冒烟截图 evidence 双场景(home 74,667B / editor 135,084B,七区齐整、cursor+Claude 风格、Console 为真实 host-events 流) |
| G-F1-4 批量创建 | PASS(分段)| 栈级全链路(gateway→agentd→MCP→engine-host):entity_batch_apply 10 立方体 x=0..9 排一列,applied=10、entityCount=10、坐标序列实测;**自然语言→工具映射段 DEV_ENV_DEGRADE**(mock provider,无 LLM key,不充绿;F3 全量移植后回填) |
| G-F1-5 PIE 双态 | PASS | cargo:play FSM 全合法迁移 + 非法迁移 -32000 拒绝 + 运行态改 transform 后 exit 恢复编辑态原值;UI Play/Pause/Step/Stop 接 play_* 工具 |

**2. 波聚合**:`cargo test --workspace` 26/26(新增 f1_editing 5 组 + 持久化回归);`pnpm -r test` 48/48(protocol 5 + client 25 + host 18,含 forgeProxy 5 例);`go test` 6/6 不受影响。

**3. 本波修复的实测缺陷(留痕)**:① **agentd 每次调用独立 spawn MCP 致场景状态不跨调用持久**(前端智能体实测抓出:Hierarchy create 后为空)——已改长连接单例(懒加载+断线重连),并加 `mcp_call_entity_persists_across_calls` 回归门;② f1 冒烟脚本踩 PowerShell 自动变量 `$args` 坑(参数被吞成空数组)——改 `$arguments`。

**4. not-triggered / no-go / deferred**:Viewport 真实帧通道(共享纹理/流)与 viewport_pick 真机链路 → 转 F1 wave.2 RD(下波承接);自然语言理解 DEV_ENV_DEGRADE 同上。

**5. 签署**:Assisted-by: TRAE:Kimi-K3 | 影响范围:forge-scene/engine-host/engine-scene-mcp 全量扩展、forge-agentd 长连接改造、packages/client EditorView 七区 + packages/host forgeProxy、scripts/f1-scene-smoke.ps1 | 验证方式:上述命令真实输出 + 截图 + 冒烟日志。

### wave.2 验收记录(2026-08-17,host=Windows NT/cargo 1.93.1/Node v22.14.0/pnpm 11.5.0,GPU=RTX 4070 Ti 12GiB / Vulkan 1.4.351)

**1. 独立断言清单(逐门)**

| 门 | 判定 | 证据 |
|---|---|---|
| G-F1-6 场景实渲染 | PASS | cargo `f1_viewport` device leg 真跑「NVIDIA GeForce RTX 4070 Ti」:中心锚点非底色 [23,24,29] + 四角底色 + 同场景两帧逐字节一致 + 实体移出后帧变且中心回底色;栈级 scripts/f1-w2-viewport-smoke.ps1:draws=3 nonzero=1524、两帧一致、移动帧变 PASS |
| G-F1-7 点选 | PASS | cargo:viewport.pick 中心命中 cube id + 角落未命中;栈级经 gateway→agentd→MCP:中心命中 cube-a(entityId 一致)/角落 miss |
| G-F1-8 相机/gizmo | PASS | cargo+栈级:yaw+90° 后帧变、getCamera 回读一致;transform_batch_set → edit_undo 回滚逐字段一致;client vitest 相机 orbit/zoom/focus/pick/gizmo action 组全绿 |
| G-F1-9 共享纹理 | PASS | ①selftest 腿:presenter 跨进程经 NT handle 打开共享纹理,presented=3/expect=3,share_close 幂等×2(f1-w2-viewport-smoke.ps1);②**桌面腿**(scripts/f1-w2-desktop-presenter-smoke.ps1):presenter 以 WS_CHILD+WS_EX_TRANSPARENT 嵌入可见 Electron 视口区(GetWindowRect 实测 rect=1038,382 674x540),**OS 级截屏(DWM 合成,含原生子窗口)锚点像素 R87G109B73 与引擎 readback 帧中心 (87,109,73,255) 逐字节一致**,非底色占位;句柄生命周期:electron exit 0、presenter 随 stdin 关闭自收。证据:apps/desktop/evidence/viewport-presenter-rect.json + smoke.log + evidence/f1-w2-desktop-presenter-*.log |

**2. 波聚合**:`cargo test --workspace` 31/31(新增 f1_viewport 设备腿 + viewport 数学内联单测);`pnpm -r test` 52/52(protocol 5 + client 29 + host 18);`go test` forge-gateway ok 无回归;`pnpm -r typecheck / build` 全绿。

**3. 本波修复的实测缺陷(留痕)**:① presenter stdin 首行 BOM(PowerShell 5.1 重定向默认 UTF-8 带 BOM)→ parse_cmd `trim_start_matches('\u{feff}')`;② engine-host ViewportRenderer 跨线程 Send(unsafe impl + Mutex 互斥纪律注释);③ jsdom 缺 ResizeObserver/canvas stub(test/setup.ts);④ **Trae IDE 打开中的 main.cjs/preload.cjs 经文件工具编辑未落盘**(写进 IDE 缓冲),首轮桌面腿冒烟跑旧码暴露——改经终端 WriteAllText 落盘 + Select-String 逐标记核验(环境坑,同 RD-F0-001 类,已计入项目记忆)。

**4. 帧通道架构定案**:帧源唯一 = engine-host `viewport::render_scene_frame`(rurix-rt vulkan render_exec,相机 UBO + 深度 + 逐实体色)→ Readback;同帧双写:canvas 回退腿(pixelsB64,纯 web 可用)+ D3D12 共享纹理(desktop presenter 原生呈现)。client 轮询分辨率 = 物理像素(CSS × devicePixelRatio),与共享纹理尺寸同源 1:1;presenter 点击穿透(WS_EX_TRANSPARENT),web 侧点选/环绕/滚轮/gizmo 交互不受影响。H.264 腿未启用(共享纹理腿已达标,非缺口)。

**5. not-triggered / deferred**:VK→D3D12 零拷贝(上游无 external_memory_win32 面)→ RD-F1-003(wave.3+);RD-F1-002(真 LLM 工具循环)仍 open;**RD-F1-001 本波回填完毕,CLOSED**。

**6. 签署**:Assisted-by: TRAE:Kimi-K3 | 影响范围:crates/engine-host(viewport.rs/share.rs/rpc.rs 新增)、crates/viewport-presenter(新 crate)、apps/desktop(presenter 生命周期 + 可见冒烟)、packages/client(ViewportCanvas 物理像素帧 + bounds 上报、editorStore、bridge)、scripts/f1-w2-viewport-smoke.ps1 + f1-w2-desktop-presenter-smoke.ps1 | 验证方式:上述命令真实输出 + 双冒烟日志 + OS 截屏锚点逐字节比对。

### wave.3 验收记录(2026-08-17,host=Windows NT/cargo 1.93.1/Node v22.14.0/pnpm 11.5.0,GPU=RTX 4070 Ti 12GiB / Vulkan 1.4.351)

**1. 独立断言清单(逐门)**

| 门 | 判定 | 证据 |
|---|---|---|
| G-F1-10 零拷贝生产 | PASS | cargo `f1_zerocopy` 3/3:①小尺寸腿(128x96)framePath=zero_copy、cpuUploads=0、同场景两帧逐字节一致、移动实体帧变、中心立方体像素非底色;②**实尺腿(960x540,编辑器视口实尺)zero_copy PASS**(堆腿);③尺寸变化 share 重建(64x64)后仍 zero_copy + share_close 幂等 ×2 + open/close ×10 循环无错 |
| G-F1-11 零拷贝呈现 | PASS | scripts/f1-w2-desktop-presenter-smoke.ps1 **连跑 3/3 PASS**:presented=3、framePath=zero_copy、handleKind=heap(674x540 与 960x540 均走堆腿)、**OS 截屏锚点 R87G109B73 与引擎 readback (87,109,73,255) 零偏差一致**;resize 重建(scripts/_f1w3_repro.ps1:960→674 全帧 zero_copy,错配窗口期诚实报「尺寸不符」不渲错帧)。如实留痕:首轮曾 1 次 flake(DWM 对新嵌入子窗口首帧合成滞后,截到 web 底色 247,247,247),冒烟截屏断言加 4 次×800ms 重试硬化(判据 ±3 不放宽,逐次留痕)后 3/3 |
| G-F1-12 回退腿回归 | PASS | scripts/f1-w2-viewport-smoke.ps1 PASS(canvas 回退腿 0-byte 回归,draws/nonzero/两帧一致/移动帧变);f1-scene-smoke PASS;cargo test --workspace 全绿;pnpm 52/52;go ok |

**2. 波聚合**:`cargo test --workspace` 34/34(f1_zerocopy 新增 3 组:小尺寸全链/实尺腿/探针对账);`pnpm -r test` 52/52(protocol 5 + client 29 + host 18)0 回归;`pnpm -r typecheck / build` 全绿;`go test` forge-gateway ok。

**3. 本波修复的实测缺陷(留痕)**:① **960x540 首帧 import 渲染 VK_ERROR_DEVICE_LOST,设备永久丢失**——根因双叠:vkBindImageMemory 返回值被丢弃 + vk req.size=2,457,600 > d3d12 committed alloc=2,228,224(同 pitch 4096,行补齐 600 vs 544),未绑定图像参与渲染致 GPU fault;128x96 因双双 64KiB 对齐幸存,故小尺寸测试全绿而实尺必崩。修复链:上游 R4(bind 检查 + req>alloc 诚实报错)→ R6(probe_image_mem_req 探针)→ R7(D3D12_HEAP 句柄类型 + heap 档免 dedicated);② 诊断中误读换行断裂数字(2,457,600 误读为 24,576,000)——以探针地图对账为准纠正;③ main.cjs IDE 脏缓冲坑再现(Edit 报成功磁盘未变)——终端 WriteAllText 落盘 + Select-String 核验;④ 桌面冒烟 flake(DWM 合成滞后)→ 重试硬化。

**4. 零拷贝架构定案(方向 B 双腿)**:`viewport.shareOpen` 先经上游 `probe_image_mem_req`(同设备同扩展,VK_KHR_external_memory_win32)实测图像内存需求:**≤ committed alloc → committed resource 腿**(D3D12_RESOURCE import);**> → 共享堆腿**(CreateHeap 64KiB 对齐 + CreatePlacedResource 偏移 0,CreateSharedHandle 作用于堆,D3D12_HEAP import)。VK 直渲共享内存,signal_frame 推共享 fence;presenter 堆/纹理双腿消费(堆腿 OpenSharedHandle→Heap→CreatePlacedResource,desc 与生产者逐字一致)。**探针对账数字(RTX 4070 Ti 实测)**:64x64=16,384 / 128x96=65,536 / 512x288=786,432 / 960x540=2,457,600 / 1024x540=2,621,440 / 1920x1080=8,847,360;**external 旗标零膨胀(plain==external 全尺寸)**;d3d12 committed alloc(960x540)=2,228,224。

**5. RD 处置**:**RD-F1-003 CLOSED**(VK→D3D12 零拷贝帧通道全链落地;H.264 备选腿残件仍 open,非缺口,性能波再评);RD-F1-004(external semaphore GPU 侧同步)open;RD-F1-002(真 LLM 工具循环)open。

**6. 签署**:Assisted-by: TRAE:Kimi-K3 | 影响范围:上游 H:\rurix render_exec.rs(R4 守卫/R5 回滚/R6 探针/R7 堆句柄腿,补丁 scripts/_f1w3_upstream_patch4~7.ps1)、crates/engine-host(share.rs 堆腿、viewport.rs probe+import 键、rpc.rs handleKind)、crates/viewport-presenter(堆腿 bind + 6 段协议)、apps/desktop main.cjs(handleKind 透传)、tests/f1_zerocopy.rs(实尺腿+探针)、scripts/f1-w2-desktop-presenter-smoke.ps1(zero_copy 断言+重试硬化) | 验证方式:上述命令真实输出 + 探针实测数字 + 双冒烟日志 + OS 截屏锚点比对。

### wave.4 验收记录(2026-08-17,host=Windows NT/cargo 1.93.1/Node v22.14.0/pnpm 11.5.0,GPU=RTX 4070 Ti 12GiB / Vulkan 1.4.351,Electron 41.10.3 Chromium WebCodecs)

**1. 独立断言清单(逐门)**

| 门 | 判定 | 证据 |
|---|---|---|
| G-F1-13 H.264 流腿 | PASS | cargo `f1_h264` 1/1:编码器初始化 + 单帧编码非空 + Annex B 起始码 00 00 00 01 + SPS(7)/PPS(8)/IDR(5) + 首帧关键帧、次帧非关键 + rgba8 回退;栈级 scripts/f1-w4-h264-smoke.ps1:经 gateway `viewport_frame format=h264` → nalBytes=753、nalTypes=7/8/5、keyframe=true(device=RTX 4070 Ti),次帧 keyframe=false,缺省 format rgba8 回退 pixelsB64 非空;**解码腿:Annex B 码流经 Electron Chromium WebCodecs VideoDecoder(`avc:{format:"annexb"}`,codec=avc1.42c015 直读 SPS profile/level 字节)解码 decoded=1,visibleRect 320x240 与编码请求尺寸一致**;SPS 独立解析(去 emulation prevention + exp-golomb):320x240、无裁剪、frame_mbs_only=1。证据:evidence/f1-w4-h264-smoke-*.log + f1-w4-frame-*.annexb + f1-w4-decode-*.json |

**2. 波聚合**:`cargo test --workspace` 36/36 全绿(12 套件 0 失败,新增 f1_h264);`pnpm -r test` 52/52(protocol 5 + client 29 + host 18)0 回归;`pnpm -r typecheck / build` 全绿;`go test` forge-gateway ok。

**3. 本波修复的实测缺陷/坑(留痕)**:① **WebCodecs 仅在安全上下文暴露**——data: URL 页 `VideoDecoder` undefined,解码 harness 改环回 http(127.0.0.1:0)供页后可用;② **Chromium VideoFrame codedHeight=258 ≠ 240**(coded 为内部分配对齐值,SPS 独立解析佐证码流声明 320x240 无裁剪,差值非码流缺陷)——尺寸判据改用 visibleRect;③ openh264 0.6 API 适配:YUVBuffer 在 formats 模块、`Encoder::with_api_config(OpenH264API::from_source(), config)`、尺寸从 YUVSource 读、config 仅调码率/帧率。

**4. RD 处置**:**RD-F1-003 H.264 残件本波回填完毕**(RD 本体 wave.3 已 CLOSED);**RD-F1-004 方向裁决 no-go 维持 OPEN**:扩展单向语义与 VK→D3D12 帧流向冲突,external semaphore 无收益;v1 CPU 块已达标,待帧流向反转场景(如 D3D12 先算→VK 后渲)再评。RD-F1-002(真 LLM 工具循环)open。

**5. not-triggered / deferred**:上游 scripts/_f1w4_upstream_patch8.ps1(external semaphore 基础面)按 no-go 裁决未集成,仅留档可重建;client 侧 H.264 播放接线(WebCodecs 消费进 ViewportCanvas,纯 web 无共享内存场景)非本波验收范围,F2+ 按需立项。

**6. 签署**:Assisted-by: TRAE:Kimi-K3 | 影响范围:crates/engine-host(rpc.rs H264State+rgba_to_i420+encode_frame+viewport_frame format 分支、Cargo.toml openh264 0.6)、crates/mcp/engine-scene-mcp(mcp.rs viewport_frame format 参)、crates/engine-host/tests/f1_h264.rs(新增)、apps/desktop/scripts/h264-decode-main.cjs(新增,WebCodecs 解码证据腿)、scripts/f1-w4-h264-smoke.ps1(新增) | 验证方式:上述命令真实输出 + 冒烟日志 + SPS 独立解析 + WebCodecs 解码 JSON 证据。
