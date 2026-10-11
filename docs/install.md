# 安装与打包说明

这份说明分三部分：普通用户怎么装和用、开发者怎么从源码一键跑起来、维护者怎么构建并发布安装包。

## 一、用安装包（推荐）

### 系统要求

- Windows 10（1809 或更新）/ Windows 11，64 位。
- 约 2 GB 可用磁盘空间（安装目录约 1.2 GB，另加你的项目与生成内容）。
- 支持 Direct3D 12 的显卡驱动（Godot 后端缺省使用 D3D12）；使用 Rurix 后端还需要 Vulkan 驱动，主流显卡驱动都会自带。

不需要事先安装 Node.js、Rust、Visual Studio 或 VC++ 运行库，安装包已经包含运行所需的一切。

### 安装

1. 打开 [Releases 页面](https://github.com/qwasg/RurixForge/releases/latest)，下载 `RurixForge-Setup-<版本>.exe`。
2. 双击运行。安装向导只有一步：选择安装位置（缺省为 `%LOCALAPPDATA%\Programs\RurixForge`，可以改到其他盘），点「安装」。整个过程不需要管理员权限。卸载和升级时会删除整个安装文件夹，所以安装位置必须是新的或空的文件夹；选中已有其他文件的文件夹（例如源码仓库）时「安装」按钮会变灰。
3. 安装完成后会自动启动 RurixForge，并在桌面和开始菜单创建快捷方式。

安装包目前没有代码签名。Windows 可能提示「Windows 已保护你的电脑」，点「更多信息」→「仍要运行」即可。如需核对下载的文件，Release 附有 `SHA256SUMS.txt`：

```powershell
Get-FileHash .\RurixForge-Setup-<版本>.exe -Algorithm SHA256
```

### 首次启动

启动时会先显示一个小窗，同时在后台启动本地服务（AI 后端 `forge-agentd` 和网页宿主）；第一次启动还会创建默认工作区。随后主窗口打开，在设置里配置模型服务（自带 API Key 的渠道、Codex 或云服务）即可开始使用。

### 文件放在哪里

| 内容 | 位置 |
| --- | --- |
| 程序本体 | 安装目录（缺省 `%LOCALAPPDATA%\Programs\RurixForge`） |
| 会话、设置、密钥、生成记录、新建的项目 | `%APPDATA%\RurixForge\data` |
| 默认工作区 | `%APPDATA%\RurixForge\data\workspaces\default` |
| 日志（`desktop.log`、`agentd.log`、`host.log`、`engine-host-*.log`） | `%APPDATA%\RurixForge\logs` |
| 本机端口记录 | `%APPDATA%\RurixForge\launcher.json` |

卸载和升级都会保留 `%APPDATA%\RurixForge`。想彻底清除，卸载后再删除这个文件夹。

### 升级与卸载

- 升级：直接运行新版安装包。安装程序会先关闭正在运行的 RurixForge 及其后台进程，再覆盖安装。
- 卸载：「设置 → 应用 → 已安装的应用」里找到 RurixForge 卸载，或运行安装目录里的 `Uninstall RurixForge.exe`。

### 可选组件

这些组件不随安装包附带，需要对应功能时再装：

- **Microsoft Edge 或 Google Chrome**：UltraPlan 用它实际操作、检查 Web 试玩版。Windows 10/11 自带 Edge。
- **ffmpeg**：视频截帧。把 `ffmpeg.exe` 放到 `%APPDATA%\RurixForge\data\tools\`，或加入 PATH，或用环境变量 `FORGE_FFMPEG` 指定。
- **Blender**：建模、贴图、骨骼动画，见 [Blender 接入步骤](../tools/blender/README.md)。
- **Codex**：本机 PATH 上能找到 `codex` 命令时可以使用 Codex 引擎。

### 高级设置（环境变量）

| 变量 | 作用 |
| --- | --- |
| `FORGE_DATA_DIR` | 把用户数据改放到别的目录（例如另一块磁盘） |
| `FORGE_AGENTD_WORKSPACE_ROOT` | 指定默认工作区目录 |
| `FORGE_HOST_PORT` / `FORGE_AGENTD_ADDR` | 固定网页宿主端口 / AI 后端地址（如 `127.0.0.1:8103`） |

### 故障排查

- **启动失败**：弹窗里点「打开日志文件夹」，查看 `desktop.log`、`agentd.log`、`host.log`。反馈问题时请附上这几个文件。
- **端口被占用**：缺省端口是 3080（网页宿主）和 8103（AI 后端）。被其他程序占用时会自动换成空闲端口，并记在 `launcher.json` 里，下次继续使用。
- **安全软件拦截**：后台服务都只监听本机回环地址（127.0.0.1）。如果安全软件拦截了安装目录 `resources\forge\bin` 下的程序，请把安装目录加入信任。
- **画面不显示**：先确认显卡驱动是最新版本；Godot 后端需要 D3D12，Rurix 后端需要 Vulkan。

## 二、从源码运行（开发者）

在仓库根目录执行：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\setup.ps1
```

脚本会依次：

1. 检查 Git、Node.js 22+、Rust（rustup）、Visual Studio C++ 构建工具，缺少的经 winget 安装（逐项询问；加 `-Yes` 不再询问）。Visual Studio 构建工具约需 5 GB，安装时会弹出 UAC 确认。随后用 npm 全局安装 `package.json` 里声明的 pnpm 版本。
2. `pnpm install`、`cargo fetch`，并补齐上游物理库缺失的构建文件（按来源和哈希校验）。
3. 构建 Rust 服务（debug）与网页端。
4. 下载并按官方 SHA-512 校验 Godot 导出模板，生成 Godot 运行时。

可以重复运行，已就绪的步骤会跳过或增量完成。其他参数：`-SkipGodot`（只用 Rurix 后端）、`-NoBuild`（只装环境和依赖）、`-SkipPrerequisites`（不检查、不安装构建工具）。

完成后执行 `pnpm dev:desktop` 打开桌面应用。桌面端发现 AI 后端没有运行时，会自动启动 `target\debug\forge-agentd.exe`，退出时一并关闭；如果你已经手动运行了 agentd，就直接复用它。

## 三、构建安装包

```powershell
pnpm package:win
# 或者
powershell -ExecutionPolicy Bypass -File scripts\package-windows.ps1
```

输出在 `target\installer\out\`：

- `RurixForge-Setup-<版本>.exe`：安装程序，版本号取自根目录 `package.json`；
- `SHA256SUMS.txt`：安装程序的 SHA-256；
- `win-unpacked\`：免安装的完整程序，可直接运行 `RurixForge.exe` 试用。

首次出包需要完整的 release 构建，并下载约 1 GB 的 Godot 导出模板，耗时较长。参数：

| 参数 | 作用 |
| --- | --- |
| `-SkipInstall` | 不重新执行 `pnpm install` / `npm ci` |
| `-SkipBuild` | 不重新构建，直接用现有产物收集出包（只改了安装器配置时用） |
| `-SkipGodot` | 不带 Godot 运行时，只用于调试打包流程 |
| `-Out <目录>` | 输出根目录，缺省 `target\installer` |

安装器本身由 `tools/installer/` 里的 electron-builder 生成。这个目录用 npm 单独管理（electron-builder 需要扁平的 `node_modules`），不属于 pnpm 工作区。

发布到 GitHub Releases：

```powershell
gh release create v<版本> target\installer\out\RurixForge-Setup-<版本>.exe target\installer\out\SHA256SUMS.txt --title "RurixForge <版本>" --notes-file <说明文件>
```

### 安装后的目录结构

安装目录下的 `resources\forge` 按仓库结构收纳运行所需的部分，也就是安装版的运行根（`FORGE_ROOT`）：

```text
resources\forge\
  bin\                     forge-agentd、engine-host、viewport-presenter、各 MCP 服务器、VC++ 运行库
  godot-runtime\           官方 Godot 导出模板 + release 版 godot_host.dll（带 runtime-manifest.json 校验）
  ultraplan-runtime\       Node + playwright-core，UltraPlan 检查 Web 试玩版用
  packages\host\           网页宿主（由 Electron 自带的 Node 运行）
  packages\client\dist\    界面
  agents\ skills\ registry\ tools\blender\ projects\demo\   随包资源（只含 git 跟踪的文件）
  templates\default-workspace\   首次启动复制成默认工作区
  data\antigravity-runtime\      可选：Antigravity 桥接（构建机装有 Go 时才包含）
```

### 运行根是怎么定位的

- Rust 侧统一走 `forge_util::roots`：`FORGE_ROOT` 指定运行根，`FORGE_DATA_DIR` 指定数据根，`FORGE_LOG_DIR` 指定日志目录；伙伴程序（MCP 服务器、engine-host）先在当前程序所在目录查找。都没设置时退回编译期的仓库路径，所以源码检出下的行为与以前完全一样。
- 桌面端的 `apps/desktop/src/backend.cjs` 负责安装版的目录约定：设置上述变量、首次启动复制默认工作区、选择端口（上次用的 → 缺省 3080/8103 → 系统分配的空闲端口）、启动 agentd 与网页宿主、退出时回收整棵进程树。agentd 和 host 的标准输入接在桌面端的管道上，桌面端异常退出时它们读到 EOF 也会自行退出。
