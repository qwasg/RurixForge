---
contract: F1
title: F1 场景编辑闭环
status: active
implementation_status: unlocked
active_scope: wave.1
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
out_of_scope:
  - Viewport 帧通道共享纹理/H.264 流(RD-F1-### 承接,Viewport 先以占位+帧统计呈现)
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
guardrails:
  - 诚实优先:任何门不过如实报 FAIL/DEV_ENV_DEGRADE,不回写 PASS
  - 数字必须来自命令输出
  - UI 风格回归基准:cursor+Claude 系(同 wave.1 截图基准)
---

# F1 契约:场景编辑闭环

## 1. 目标与双门状态
实现 13_ROADMAP F1:Entity/组件模型 + 注册表、.rxscene 读写、Hierarchy/Inspector、Undo/Redo、PIE 双态、七区骨架。status=active;implementation_status=unlocked。

## 2. 范围与波次
- wave.1(本波):front matter in_scope 全量。
- wave.2+:Viewport 真实帧通道(共享纹理)、viewport gizmo 实操、Assets 面板(F1 尾/F2 衔接)。

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
  - id: RD-F1-002
    content: Chat 自然语言→工具映射(真 LLM 工具循环)
    reason: forge-agentd 为最小骨架 + mock provider seam,无 LLM key;DEV_ENV_DEGRADE 不充绿
    refill: RD-F0-003 七 crate 移植带回真 providers 后回填
    owner: F3 前置波

## 5. 修订
- 2026-08-16 立项:F0 全绿后用户指令开工。

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
