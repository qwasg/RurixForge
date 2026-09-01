# 02 · 系统总体架构

## 1. 进程拓扑

```
┌────────────────────────────────────────────────────────────────────┐
│ rurix-forge-ide (Electron: main + renderer, React/TS)               │
│   面板:Viewport Hierarchy Inspector Assets Console Chat 设置        │
└───────┬───────────────────────────────────────────────┬────────────┘
        │ HTTP/SSE (api/forge/*)                        │ 帧流 + 输入转发
        ▼                                               ▼
┌───────────────────┐   HTTP    ┌──────────────────────────────────┐
│ forge-gateway (Go)│──────────▶│ forge-agentd (Rust, :8103)        │
│ :8102 公开端口     │           │  会话/plan/todo/swarm/tools/MCP   │
└───────────────────┘           └───────┬──────────────────────────┘
                                        │ MCP (stdio 优先, streamable-HTTP 备选)
        ┌───────────────────────────────┼────────────────────────────┐
        ▼                               ▼                            ▼
┌────────────────┐            ┌──────────────────┐         ┌──────────────────┐
│ engine-scene-mcp│           │ asset-pipeline-mcp│         │ code-forge-mcp   │
│ (Rust,内嵌于    │  控制通道  │ (Rust,封装 assetd)│         │ (Rust,封装 rx    │
│  engine-host 或 │◀─────────▶│                  │         │  工具链 + LSP)   │
│  伴生进程)      │           └────────┬─────────┘         └──────────────────┘
└───────┬────────┘                     │                      ┌──────────────────┐
        │ 帧通道(共享纹理/流)          │ 构建产物              │ gen-image-mcp /  │
        ▼                              ▼                      │ gen-model-mcp    │
┌────────────────┐            ┌──────────────────┐            │ (预留适配层)      │
│ engine-host    │            │ asset cache      │            └──────────────────┘
│ rurix-render + │            │ (.forge/cache)   │         ┌──────────────────┐
│ rurix-physics  │            └──────────────────┘         │ playtest-mcp     │
└────────────────┘                                          │ project-mcp      │
                                                            └──────────────────┘
```

进程清单与职责:

| 进程 | 技术 | 端口/通道 | 职责 | 崩溃隔离要求 |
|---|---|---|---|---|
| `rurix-forge-ide` | Electron + React + TS + Tailwind | HTTP/SSE 客户端;帧流消费端 | 全部 UI;不直接触碰引擎库/文件真相 | UI 崩溃不丢后端会话 |
| `forge-gateway` | Go | `:8102` 公开 | CORS、JWT 鉴权、反向代理、WS→SSE 桥(照搬 agent-cowork gateway-go) | 无状态,可重启 |
| `forge-agentd` | Rust(axum) | `:8103` 内部 | Agent 内核:会话/plan/todo/swarm/工具/MCP 客户端/记忆/checkpoint | 崩溃不丢事件日志(redb + JSONL) |
| `engine-host` | Rust,链接 rurix-render + rurix-physics | 控制:JSON-RPC over 本地 socket;帧流:共享纹理或编码流;事件:ring buffer | 场景运行时、渲染、物理、PIE | GPU 崩溃不得拖垮 agentd/IDE;看门狗自动重启 |
| `assetd` | Rust,链接 rurix-geom-build 等 | 经 asset-pipeline-mcp 间接口;本身不暴露网络端口 | 导入/构建/缓存/引用图 | 构建失败 = 结构化错误,不 panic |
| MCP servers | Rust 单 crate 多 bin | stdio(默认)/ streamable-HTTP | 领域工具面,见 `05` | 单 server 崩溃不影响其他域 |
| `rx` 工具链 | rurix 既有 | CLI | `.rx` 脚本 build/check/run/fmt/test/doc | code-forge-mcp 子进程调用 |

端口约定:gateway `:8102`、agentd `:8103`(错开 agent-cowork 的 8002/8003 避免共存冲突,D-002)。

## 2. 组件职责矩阵

| 能力 | 唯一负责方 | 其余组件的访问方式 |
|---|---|---|
| 场景运行时事实(实体/组件/变换) | engine-host(内存态)+ `.rxscene`(磁盘事实源) | 只经 engine-scene-mcp;禁止直接改内存 |
| 物理世界 | engine-host 内 `PhysicsWorld` | engine-scene-mcp 查询/命令工具 |
| 渲染帧 | engine-host | 帧通道 → IDE Viewport;截图经 engine-scene-mcp |
| 资产源文件与 `.meta` | 项目目录(磁盘) | asset-pipeline-mcp / project-mcp |
| 资产构建产物与缓存 | assetd(`.forge/cache`) | asset-pipeline-mcp |
| `.rx` 脚本编译/诊断 | `rx` 工具链 | code-forge-mcp |
| 会话/plan/todo/记忆/事件日志 | forge-agentd | IDE 与 MCP 经 HTTP API |
| LLM 渠道与密钥 | forge-agentd(providers + keystore) | 设置页经 API 读写,密钥不回显 |
| 用户设置 | IDE main 端 per-domain JSON store(cindy 模式,`07 §7.3`) | renderer 经 IPC;agent 经 settings API(只读域白名单) |
| 版本控制 | git(project-mcp 封装) | agent 经 project-mcp;破坏性 git 操作走 Proposal |

## 3. 通信协议

### 3.1 IDE ↔ agentd

- 传输:HTTP(JSON)+ SSE(事件流),经 gateway 代理,契约见 `11_API_CONTRACTS.md`。
- 协议族沿用 agent-cowork `/api/agent-debug/*` 形态,更名 `/api/forge/*`(D-002)。

### 3.2 agentd ↔ MCP servers

- 照搬 agent-cowork `agent-mcp` 客户端:长连接、`mcp.json` 声明、stdio 与 streamable-HTTP 双传输、工具发现后注入工具循环,命名 `mcp__{server}__{tool}`。
- `mcp.json` 增加字段 `autoStart`(engine-host/assetd 这类本地伴生进程的拉起命令与健康检查),见 `05 §1.2`。

### 3.3 engine-host 三通道(`03 §5` 详述)

| 通道 | 方向 | 内容 | 传输 |
|---|---|---|---|
| 控制通道 | 双向 | JSON-RPC 2.0:场景 CRUD、Play/Pause/Step、查询、命令回执 | 本地命名管道(Windows)或 loopback TCP,长度前缀帧 |
| 帧通道 | host→IDE | 视口渲染帧:D3D12 共享纹理句柄(同进程 GPU 零拷贝优先)或 H.264/RAW 回退 | 共享句柄 + 事件通知;回退走 loopback 流 |
| 事件通道 | host→订阅者 | 接触事件、流送事件、诊断、帧统计 | 有界 ring + 拉取;语义对齐 `ContactEvent` 规范序 |

### 3.4 数据流三条主线

1. **编辑流**:UI/agent → MCP → engine-host/assetd → 磁盘事实源 → 事件 → IDE 刷新。
2. **运行流**:PIE 启动 → engine-host 加载 `.rxscene` + 构建产物 → 物理固定步 → 渲染帧 → 帧通道 → Viewport;输入反向转发。
3. **agent 回合流**:用户消息 → forge-agentd 会话 → (plan 引擎) → 工具循环(MCP 调用)→ SSE 事件 → IDE Chat/Workbench。

## 4. 仓库布局(monorepo)

```
rurix-forge/
├── Cargo.toml                 # Rust workspace: agentd + engine-host + assetd + mcp servers
├── forge.toml.example         # 项目模板(非引擎自身配置)
├── apps/
│   └── ide/                   # Electron + React 前端(07 全篇)
│       ├── src/main/          #   Electron main:窗口、IPC、settings-store
│       ├── src/renderer/      #   React 面板
│       └── package.json
├── crates/
│   ├── forge-protocol/        # wire 契约 DTO/事件信封/错误码(11)
│   ├── forge-agentd/          # agent 内核七 crate(04 §1),由 agent-cowork 移植
│   │   ├── agent-config/  agent-protocol/  agent-store/  agent-providers/
│   │   ├── agent-mcp/     agent-tools/     agent-core/
│   ├── engine-host/           # 03 §5
│   ├── assetd/                # 08 §1
│   ├── forge-scene/           # 场景/实体/组件/prefab 模型与序列化(09)
│   ├── forge-nodagraph/       # 节点图模型与 lowering(10)
│   └── mcp/
│       ├── engine-scene-mcp/  asset-pipeline-mcp/  code-forge-mcp/
│       ├── project-mcp/       playtest-mcp/
│       ├── gen-image-mcp/     gen-model-mcp/
├── gateway-go/                # forge-gateway
├── skills/                    # 06 全篇
├── data/
│   ├── agents/                # subagent profile *.md(04 §6)
│   └── mcp.json               # MCP server 声明(05 §1.2)
├── projects/                  # 用户游戏项目根(每项目一个 forge.toml,08 §3.1)
└── scripts/                   # start-stack.ps1 等
```

引擎对 rurix 的依赖方式:Cargo path/git 依赖指向 rurix workspace 的
`rurix-render` / `rurix-physics` / `rurix-geom-build` / `rurix-geometry` / `image-io`,
版本锚定 rurix release tag(起步 `v1.0.1-dist` 系;D-003)。
