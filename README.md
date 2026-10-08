# RurixForge

**一个让你和 AI 一起做游戏的桌面工作台。**

你描述想做的游戏，AI 帮你拆任务、写代码、准备素材和搭场景。你可以在同一个窗口里看进度、改细节、运行游戏，再决定下一步怎么做。

它把聊天、代码编辑、场景编辑、素材管理和试玩放在了一起。项目仍在开发中，目前主要在 Windows 上开发和验证，适合愿意从源码运行、一起完善工具的开发者。

## 现在可以做什么

- **边聊边制作**：让 AI 读项目、改文件、执行工具；也可以先列计划，再分工完成任务。需要确认的操作会在界面里提出。
- **从想法做到可试玩的版本**：UltraPlan 会先帮你梳理需求，做一个 Web 小样供你试玩；确认玩法和计划后，再进入正式制作与验收。
- **先看设计稿，再搭场景**：Design 模式先生成场景或界面草图。选定后，把画面拆成可编辑的场景元素，文字也能继续修改。
- **管理游戏内容**：查看场景层级、调整对象属性、编辑代码和节点逻辑，导入图片、材质、模型与动画。
- **接入制作工具**：已有 Blender 接入，以及图像、视频、音频、模型生成接口；实际可用能力取决于你配置的工具和服务。
- **选择运行后端**：支持 Rurix 和 Godot。新建 2D 项目默认使用 Godot，普通新建 3D 项目默认使用 Rurix；UltraPlan 会让你确认具体选择。
- **使用自己的模型服务**：可配置自带 API Key 的渠道或 Codex；仓库也包含可自行部署的云端账号、同步和模型网关服务。

这里的“AI 帮你做”仍然需要检查和试玩。自动检查通过，不代表游戏的玩法、美术和体验已经符合你的预期。

## 从源码启动

下面以 **Windows + PowerShell** 为例。请在仓库根目录执行命令。

### 1. 准备环境

- Git、Node.js 22 或更新版本，以及 pnpm。仓库在 `package.json` 中声明的 pnpm 版本为 **11.5.0**。
- Rust / rustup。`rust-toolchain.toml` 固定了 **Rust 1.94.1**，进入项目后由 rustup 使用该版本。
- Visual Studio C++ 构建工具、Windows SDK 和 CMake，用于编译原生依赖。
- 需要云服务、Go 网关或 Antigravity 桥接时，再安装 **Go 1.26 或更新版本**。

首次构建会下载依赖并产生较多编译缓存，请预留足够磁盘空间。Windows 原生视口需要相应的图形驱动；其他系统尚未完成同等范围的桌面验证。

### 2. 下载并构建

```powershell
git clone https://github.com/qwasg/RurixForge.git
cd RurixForge

pnpm install --frozen-lockfile
cargo fetch --locked

# 补齐当前固定版本的上游物理库构建文件；脚本会校验来源和哈希
.\scripts\bootstrap-rurix-physics.ps1

# 先构建默认 Rurix 后端和各服务；Godot 在下一步单独准备
cargo build --workspace --exclude godot-host --locked

# 按顺序构建桌面端需要的网页和宿主服务
pnpm --filter @forge/protocol build
pnpm --filter @forge/host build
pnpm --filter @forge/client build
pnpm --filter @forge/desktop build
```

Rurix 内核来自 [qwasg/Rurix](https://github.com/qwasg/Rurix)，具体版本固定在 `Cargo.toml` 和 `Cargo.lock` 中，无须另建本地源码目录。仓库内还保留了一份小范围的渲染修补，见 [修补说明](vendor/rurix/FORGE_BLEND_PATCH.md)。

如果要使用 Godot（包括默认的新建 2D 项目），继续执行：

```powershell
.\scripts\godot-fetch.ps1 -Templates
.\scripts\godot-runtime.ps1 -Build
```

Godot 版本与依赖见 [GODOT_PIN.json](GODOT_PIN.json)，接入范围与现有限制见 [Godot 说明](docs/godot-backend/03-completion.md)。不使用 Godot 时，可以先用 Rurix 后端。

### 3. 打开应用

在第一个终端启动 AI 后端，保持它运行：

```powershell
.\target\debug\forge-agentd.exe
```

再打开一个终端，进入同一个仓库目录：

```powershell
pnpm dev:desktop
```

桌面应用会启动网页宿主服务。首次进入后，可以登录你配置的云服务，也可以选择自带密钥，在设置里配置模型渠道；Codex 的可用能力取决于本机安装、登录状态和上游支持。

想在浏览器里开发界面时，保持 AI 后端运行，分别执行 `pnpm dev:host` 和 `pnpm dev:client`，再打开 Vite 输出的地址。默认宿主端口为 `3080`，AI 后端端口为 `8103`。

## 按需启用的功能

- **UltraPlan**：需要可用的模型服务和系统 Edge / Chrome，用来实际操作、检查 Web 试玩版。见 [使用说明](docs/ultraplan.md) 和 [验证范围](docs/ultraplan-validation.md)。
- **Design 与素材生成**：先配置支持对应能力的生成服务；视频截帧还需要 ffmpeg。见 [Design 说明](docs/design-mode.md)。
- **Blender**：用于建模、贴图、骨骼动画和模板同步。见 [接入步骤](tools/blender/README.md)。
- **云服务**：账号、资料同步和模型网关均可自行部署。开发依赖使用 PostgreSQL、Redis 和 Docker，`pnpm dev:cloud` 只启动这些依赖；完整配置见 [云服务文档](15_CLOUD_SERVICE.md)。本地自带密钥的用法不要求部署整套云服务。

## 开发时常用的检查

```powershell
pnpm typecheck                 # TypeScript 类型检查
pnpm test                      # 前端与 Node 宿主测试
cargo test --workspace         # Rust 工作区测试
go -C gateway-go test ./...     # Go 网关测试
go -C cloud test ./...          # 云服务测试
```

需要真实 GPU、Godot、浏览器或外部模型服务的检查，要先准备对应环境。上面的命令是运行入口，不代表所有检查都已在当前机器上通过。

## 想改代码，从哪里看

- `packages/client`：用户看到的界面，使用 React 和 TypeScript。
- `apps/desktop`、`packages/host`：Electron 桌面窗口和本地网页服务。
- `crates/forge-agentd`：AI 会话、任务计划、工具调用与权限处理。
- `crates/engine-host`、`crates/godot-host`：场景运行与两种渲染后端。
- `crates/assetd`、`crates/gend`：素材导入、处理和生成服务。
- `crates/mcp`：供 AI 调用的场景、素材、代码等工具。
- `cloud`、`gateway-go`：可选的云服务和网关。
- `projects/demo`、`projects/code-sentinels`：示例项目与游戏制作代码。
- `agents`、`skills`：任务角色和可复用的制作步骤。

详细设计从 [文档索引](00_MASTER_INDEX.md) 进入。日常上手先看这份 README，需要查接口或实现细节时再翻设计文档。

## 当前状态

这是开发中的源码仓库，还没有提供完整的 RurixForge 一键安装包。不同模型、图形后端和外部工具的能力并不完全相同，具体支持范围以对应文档和实际运行结果为准。

仓库保留源码、示例运行资源和必要说明。模型密钥、登录状态、本机配置、生成服务原始记录、测试截图及历史打包副本留在本地，不随源码提交；部分历史验证文档引用的是本地证据文件。

## 许可证

项目代码采用 [Apache-2.0](LICENSE)。第三方依赖和示例素材请同时查看各自保留的许可证与来源说明。
