# RurixForge

**AI-first 游戏制作引擎** — 以 rurix 渲染器/物理引擎为运行时内核，以 agent 集群（coding 模式 + swarm）为第一公民操作者，以 MCP 工程为全部重复性工作的执行面，人类只经极简前端做必须的人工确认与直观调整。

本仓库是 RurixForge 的完整工程实现：Rust 引擎与服务层 + React 前端 IDE + Go 边缘网关 + 示例游戏项目。

## 仓库结构

| 模块 | 目录 | 说明 |
|---|---|---|
| 引擎宿主 | `crates/engine-host` | JSON-RPC 2.0 over TCP 控制通道、rurix-physics 固定步后台线程、soft-raster CPU 渲染 / Vulkan 视口（D3D12 共享纹理 + H.264 帧流）、PIE |
| 素材管线 | `crates/assetd` | glTF 严格导入、纹理/材质处理、`.meta`+GUID、缓存键、引用图、缩略图、清理 |
| Agent 内核 | `crates/forge-agentd` | 会话、plan/todo、swarm 集群、subagent、proposal 权限确认、checkpoint、多 LLM provider（含 openai-compat 通用渠道） |
| 生成服务 | `crates/gend` | 图像/模型/媒体生成后端抽象（mock + 远程渠道；密钥经 DPAPI keystore，不落明文） |
| 代码索引 | `crates/forge-index` | 词法/向量索引、文档提取、检索 |
| 逻辑内核 | `crates/forge-logic` | 节点图（Blueprint-lite）解释器、`.rx` 脚本、call_function DLL 运行时 |
| 共享层 | `crates/forge-util` 等 | 工具库 / 场景模型（`forge-scene`）/ 技能与包仓库（`forge-store`） |
| MCP 工程 | `crates/mcp/*` | 七个领域 MCP server：engine-scene、asset-pipeline、context、code-forge、gen-image、gen-model、store |
| 前端 IDE | `packages/client` | React + TypeScript：三栏壳、聊天时间线、Composer、Workbench tabs、节点图、Inspector、设置页、明暗主题 |
| 宿主服务 | `packages/host` | Node 宿主：静态服务、HTTP/SSE、forge 代理、会话 |
| 协议 | `packages/protocol` | 前后端共享 TS 类型 |
| 桌面壳 | `apps/desktop` | Electron 桌面应用 |
| 边缘网关 | `gateway-go` | Go：CORS/JWT/反向代理/WS→SSE 桥 |
| 示例项目 | `projects/demo` | demo 游戏项目（场景/材质/贴图/脚本/节点图） |
| 技能库 | `skills/` | SKILL.md 形式的可复用操作规程（素材导入、场景搭建、材质调优、回归验证等） |
| 设计文档 | `00_MASTER_INDEX.md` ~ `14_DECISION_LOG.md` | 冻结级架构设计文档集 |

## 构建与测试

前置要求：Node.js ≥ 22、pnpm 11.5、Rust ≥ 1.80、Go；视口 D3D12 共享纹理部分为 Windows only。

```bash
pnpm install            # 前端依赖
pnpm build              # 构建全部 JS/TS 包
pnpm test               # 前端测试
cargo test --workspace  # Rust 测试
go -C gateway-go test ./...
```

> **上游依赖说明**：`crates/*` 以本地 path 依赖引用独立的 rurix 上游仓库（`rurix-rt` / `rurix-physics` / `rurix-asset` / `rurix-geom-build` / `rurix-pkg` / `soft-raster`，对账时点记录见 `RURIX_PIN.json`）。上游仓库未随本仓库发布，`cargo build` 需本地存在对应 rurix 源码树，并按各 `Cargo.toml` 中的 path 指向放置。

## 设计文档

| 文档 | 内容 |
|---|---|
| [00_MASTER_INDEX.md](00_MASTER_INDEX.md) | 主索引：文档地图、术语表、全局不变量 |
| [01_PRODUCT_VISION.md](01_PRODUCT_VISION.md) | 产品定位与设计原则 |
| [02_SYSTEM_ARCHITECTURE.md](02_SYSTEM_ARCHITECTURE.md) | 系统总体架构 |
| [03_ENGINE_LAYER.md](03_ENGINE_LAYER.md) | 引擎层（rurix 内核复用面） |
| [04_AGENT_BACKEND.md](04_AGENT_BACKEND.md) | Agent 后端 |
| [05_MCP_PROJECTS.md](05_MCP_PROJECTS.md) | MCP 工程集 |
| [06_SKILLS_LIBRARY.md](06_SKILLS_LIBRARY.md) | Skill 体系 |
| [07_FRONTEND_IDE.md](07_FRONTEND_IDE.md) | 前端 IDE |
| [08_ASSET_PIPELINE.md](08_ASSET_PIPELINE.md) | 素材处理管线 |
| [09_ENTITY_SCENE_MODEL.md](09_ENTITY_SCENE_MODEL.md) | 实体与场景模型 |
| [10_INTERACTION_LOGIC.md](10_INTERACTION_LOGIC.md) | 交互逻辑（节点图 + `.rx` 双轨） |
| [11_API_CONTRACTS.md](11_API_CONTRACTS.md) | API 与数据契约 |
| [12_SECURITY_PERMISSIONS.md](12_SECURITY_PERMISSIONS.md) | 安全与权限 |
| [13_ROADMAP.md](13_ROADMAP.md) | 里程碑路线图 |
| [14_DECISION_LOG.md](14_DECISION_LOG.md) | 决策日志 |

## License

[Apache-2.0](LICENSE)
