# 安装与打包说明

这份说明分三部分：普通用户怎么装和用、开发者怎么从源码一键跑起来、维护者怎么构建并发布安装包。

## 一、用安装包（推荐）

### 系统要求

- Windows 10（1809 或更新）/ Windows 11，64 位。
- 约 2 GB 可用磁盘空间（安装目录约 1.1 GB，另加你的项目与生成内容）。
- 支持 Direct3D 12 的显卡驱动（Godot 后端缺省使用 D3D12）；使用 Rurix 后端还需要 Vulkan 驱动，主流显卡驱动都会自带。

不需要事先安装 Node.js、Rust、Visual Studio 或 VC++ 运行库，安装包已经包含运行所需的一切。

### 安装

1. 打开 [Releases 页面](https://github.com/qwasg/RurixForge/releases/latest)，下载 `RurixForge-Setup-<版本>.exe`。
2. 双击运行。欢迎页列出安装位置、快捷方式和数据位置，确认无误点「立即安装」；要调整就点「自定义安装选项」：
   - **安装位置**：缺省为 `%LOCALAPPDATA%\Programs\RurixForge`，可以改到其他盘。选中的文件夹名里没有 RurixForge 时，会在里面新建 `RurixForge` 子文件夹。界面会显示所需空间和该盘的可用空间。卸载和升级时会删除整个安装文件夹，所以只能装进新的、空的或原来的安装文件夹；选中已有其他文件的文件夹（例如源码仓库）时「开始安装」按钮不可用。
   - **数据位置**：项目、会话、设置与生成内容存放的位置，缺省为 `%APPDATA%\RurixForge\data`，可以改到空间更大的磁盘。不能放在安装文件夹里。改位置不会搬动已有的数据。
   - **桌面快捷方式**、**开始菜单快捷方式**、**安装完成后启动**：可以分别关掉。升级时沿用上次的快捷方式选择（删掉的不会再加回来）。

   整个过程不需要管理员权限。
3. 安装完成后，3 秒后自动启动 RurixForge（可以取消），也可以直接点「启动 RurixForge」或「完成」。

安装界面需要 WebView2 运行时：Windows 11 自带，Windows 10 一般已随 Edge 更新装好。系统里没有时，安装程序会自动改用经典安装向导，装出来的结果相同。

再次运行安装包时，界面会显示已安装的版本；更新、重新安装都会保留你的项目与设置。如果 RurixForge 正在运行，界面会先提醒，安装时自动关闭它。

安装包目前没有代码签名。Windows 可能提示「Windows 已保护你的电脑」，点「更多信息」→「仍要运行」即可。如需核对下载的文件，Release 附有 `SHA256SUMS.txt`：

```powershell
Get-FileHash .\RurixForge-Setup-<版本>.exe -Algorithm SHA256
```

### 命令行安装

批量部署或脚本安装时可以不显示界面：

```powershell
.\RurixForge-Setup-<版本>.exe /S --no-desktop-shortcut "--data-dir=E:\RurixForge 数据" /D=E:\Apps\RurixForge
```

| 参数 | 作用 |
| --- | --- |
| `/S` | 静默安装。退出码 0 表示成功；2 表示安装位置不可用（文件夹里有其他文件），或正在运行的 RurixForge 无法关闭 |
| `/D=<目录>` | 安装位置，按原样使用（不会再追加 `RurixForge` 子文件夹）。必须放在最后，路径含空格也不要加引号 |
| `--no-desktop-shortcut` | 不创建桌面快捷方式（升级时删掉已有的） |
| `--no-start-menu-shortcut` | 不创建开始菜单快捷方式（升级时删掉已有的） |
| `--data-dir=<目录>` | 数据位置（与 `/S` 一起用；写进 `launcher.json`） |
| `--classic` | 不用新的安装界面，打开经典安装向导 |

### 首次启动

启动时会先显示一个小窗（Windows 11 上是透出桌面的毛玻璃），依次显示「准备工作区 → 启动 AI 后端 → 启动界面服务 → 打开工作台」，同时在后台启动本地服务（AI 后端 `forge-agentd` 和网页宿主）；第一次启动还会创建默认工作区。随后主窗口打开，在设置里配置模型服务（自带 API Key 的渠道、Codex 或云服务）即可开始使用。

### 文件放在哪里

| 内容 | 位置 |
| --- | --- |
| 程序本体 | 安装目录（缺省 `%LOCALAPPDATA%\Programs\RurixForge`） |
| 会话、设置、密钥、生成记录、新建的项目 | 数据位置：缺省 `%APPDATA%\RurixForge\data`，或安装时另选的文件夹 |
| 默认工作区 | 数据位置下的 `workspaces\default` |
| 日志（`desktop.log`、`agentd.log`、`host.log`、`engine-host-*.log`） | `%APPDATA%\RurixForge\logs` |
| 本机端口记录、安装时另选的数据位置（`dataDir`） | `%APPDATA%\RurixForge\launcher.json` |

卸载和升级都会保留 `%APPDATA%\RurixForge` 和另选的数据文件夹。想彻底清除，卸载后再删除它们。

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
| `FORGE_DATA_DIR` | 把用户数据改放到别的目录（例如另一块磁盘）；优先于安装时选的数据位置 |
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

输出在 `target\installer\`：

- `out\RurixForge-Setup-<版本>.exe`：要发布的安装程序，版本号取自根目录 `package.json`；
- `out\SHA256SUMS.txt`：安装程序的 SHA-256；
- `nsis\win-unpacked\`：免安装的完整程序，可直接运行 `RurixForge.exe` 试用；
- `nsis\RurixForge-Core-<版本>.exe`：内层安装包（单独运行就是经典安装向导），调试用，不要发布。

首次出包需要完整的 release 构建，并下载约 1 GB 的 Godot 导出模板，耗时较长。参数：

| 参数 | 作用 |
| --- | --- |
| `-SkipInstall` | 不重新执行 `pnpm install` / `npm ci` |
| `-SkipBuild` | 不重新构建，直接用现有产物收集出包（只改了安装器配置时用） |
| `-SkipGodot` | 不带 Godot 运行时，只用于调试打包流程 |
| `-Out <目录>` | 输出根目录，缺省 `target\installer` |

安装程序由两部分拼成一个 exe：

- **内层安装包**：`tools/installer/` 里的 electron-builder 生成的 NSIS 安装包，负责解压程序、写卸载信息和快捷方式（钩子在 `installer.nsh`）。这个目录用 npm 单独管理（electron-builder 需要扁平的 `node_modules`），不属于 pnpm 工作区。
- **安装界面**：`tools/installer/setup/`，Rust + WebView2 的小程序（独立的 Cargo 工作区，产物在 `target\forge-setup`，首次构建需要联网下载依赖）。页面在 `ui/`，出包时编进 exe。它运行时把内层安装包解到临时目录、按界面里的选择静默运行，并按安装目录的增长显示进度。`forge-setup-pack.exe` 负责把两者和元数据（产品名、版本、安装后大小、许可证）拼在一起，格式见 `src/payload.rs`。

调整安装界面时不必每次重新出包：设置环境变量 `FORGE_SETUP_UI_DIR` 指向 `tools\installer\setup\ui` 后再运行已有的安装程序，页面会直接读磁盘上的文件，改完重新打开即可；`FORGE_SETUP_DEBUG_PORT=<端口>` 会打开 DevTools 协议端口，便于用 Playwright 等工具自动操作界面。

electron-builder 的安装包原本会把自身完整复制一份到 `%LOCALAPPDATA%\rurixforge-updater\installer.exe`，留给自动更新做差分下载。RurixForge 没有自动更新，`installer.nsh` 会在安装完成后删掉这份副本。

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
  agents\ skills\ registry\ tools\blender\ tools\model\ assets\motion-packs\ projects\demo\   随包资源（只含 git 跟踪的文件）
  templates\default-workspace\   首次启动复制成默认工作区
  data\antigravity-runtime\      可选：Antigravity 桥接（构建机装有 Go 时才包含）
```

### 运行根是怎么定位的

- Rust 侧统一走 `forge_util::roots`：`FORGE_ROOT` 指定运行根，`FORGE_DATA_DIR` 指定数据根，`FORGE_LOG_DIR` 指定日志目录；伙伴程序（MCP 服务器、engine-host）先在当前程序所在目录查找。都没设置时退回编译期的仓库路径，所以源码检出下的行为与以前完全一样。
- 桌面端的 `apps/desktop/src/backend.cjs` 负责安装版的目录约定：设置上述变量、首次启动复制默认工作区、选择端口（上次用的 → 缺省 3080/8103 → 系统分配的空闲端口）、启动 agentd 与网页宿主、退出时回收整棵进程树。agentd 和 host 的标准输入接在桌面端的管道上，桌面端异常退出时它们读到 EOF 也会自行退出。
