# Blender → RurixForge

这条管线使用独立 Codex 的原生 computer-use 制作资产；Forge 负责固定导出、验证、模型构建、模板与自动同步。`blender-bridge-mcp` 不提供桌面操作，也不会调用 `open-computer-use`。

## 使用

1. 在素材创作中打开“3D 模型”，选择“使用 Blender 制作地图／角色”，填写需求和用途。
2. 点击“在 Codex 制作”。Forge 准备项目 MCP 配置和 `blender-production` 技能，打开 Codex 并预填任务；在 Codex 点击一次发送后才会领取任务。
3. Codex 按任务提供的绝对路径保存 `.blend`。首次发布成功后，保存 Blender 源或关联贴图即可触发同步。
4. 素材创作中的模型预览来自 Forge GPU 渲染，可旋转和采样动画。点击“加入场景”，或从 Assets 拖入 prefab；层级面板可以选择子物体。
5. 实例位置、子节点改动、附加组件和材质参数覆盖保留。需要清除实例修改时，在属性面板使用“恢复整个实例的模板默认值”。

首次必须能调用 Codex 原生 computer-use。服务的能力状态是执行器领取时的能力声明，不等于系统安装检测；没有原生桌面工具时任务应保持等待状态。官方深链接只预填，不自动发送：[Codex deep links](https://learn.chatgpt.com/docs/reference/commands#deep-links)。

Blender 插件位于 `rurix_forge/__init__.py`；项目 setup 会复制为 `.forge/blender/addons/rurix_forge.py`，技能指导 Codex 在 Blender 中安装启用。插件负责持久化对象身份和保存状态，不承担通用远程 Python 执行。

## 构建与运行

Windows 需要 Rust、Node/pnpm 和 Visual Studio C++ Build Tools。当前 Rurix 锁定依赖漏打包了一个 Jolt 构建文件，首次构建先执行校验脚本；它仅补齐 Cargo 依赖缓存，并校验确切的上游提交和 SHA-256。

```powershell
.\scripts\bootstrap-rurix-physics.ps1
cargo build -p forge-agentd -p engine-host -p engine-scene-mcp -p asset-pipeline-mcp -p blender-bridge-mcp
pnpm build
```

Windows 正在运行的 `.exe` 无法覆盖。正常退出旧服务后可构建默认目录；与旧服务并行验证时，为上述 cargo 命令追加 `--target-dir target/blender-validation`，运行新 agentd 时将 `FORGE_ENGINE_HOST_BIN`、`FORGE_ENGINE_SCENE_MCP_BIN`、`FORGE_ASSET_PIPELINE_MCP_BIN` 指向相同构建目录。MCP bridge 默认从 agentd 可执行文件的同目录发现。

Blender 路径优先采用项目配置或 `FORGE_BLENDER_EXECUTABLE`，界面也可填写。首个实测版本是 Blender 5.2.1 LTS。

## 接口

REST 根：`/api/forge/blender`。GET 使用 `workspaceId` 查询参数，POST 使用同名请求字段；默认项目必须显式填写 `default`。

- `GET status`、`POST config {executablePath}`、`POST setup`：检测与项目接入。
- `GET jobs`、`POST jobs {name,prompt,kind,idleClip?,walkClip?}`：列出／创建任务，kind 为 `prop`、`map` 或 `character`。
- `GET jobs/{id}`：实际状态、源文件、发布版本、模板与诊断。
- `POST jobs/{id}/claim {executorId,capabilities:{computerUse:true}}`：领取并返回 leaseToken；在实际检测到原生 computer-use 可用后调用。
- `POST jobs/{id}/progress {leaseToken,message?,heartbeat?}`：汇报与续租。
- `POST jobs/{id}/bind {leaseToken,sourcePath,autoSync?}`：绑定项目内已保存的源。
- `POST jobs/{id}/publish {leaseToken}`：异步导出与提交，轮询 GET job 获取结果。
- `POST jobs/{id}/retry`、`POST jobs/{id}/cancel`：恢复或取消，保留已发布资产。
- `POST jobs/{id}/preview {width?,height?,yaw?,clip?,time?}`：同一项目引擎中的真实 RGBA 预览。

独立 Codex 通过项目中的 `rurix-blender` STDIO MCP 调用对应 `blender_*` 工具。MCP 的项目由 `--workspace-id` 固定，不能由工具参数切换；仅连接显式端口的本机 HTTP 服务。首次也可调用 REST setup 准备配置后重新打开 Codex 项目，再直接使用 `blender_job_create`。

引擎 MCP 新增 `prefab_instantiate`、`prefab_revert`、`asset_reload`、`animation_control` 和 `template_preview`。场景操作复用当前 Forge 的 engine-host，不另外启动一套孤立场景。

## 数据与同步语义

- 可编辑源：`Sources/Blender/<sourceId>/`；导出暂存与任务记录：`.forge/blender/`。
- 交换产物：嵌入 PNG/JPEG 的 GLB + manifest；程序材质先烘焙。单位为米，导出 Y-up，稳定节点标识位于 glTF extras。
- `.rxmodel` 保留几何、UV、法线、切线、PBR、多材质、纹理、节点、蒙皮和动画；`.rxprefab` 为真实实体子树。旧 `.rxmesh` 与 2D Sprite 保留原路径。
- 角色必须有有效骨骼权重和独立 Idle／Walk 动作，可用 manifest 显式映射名称；不自动以第一个动作替代缺失动作。
- 地图优先采用 `rurixCollision=true` 的节点；未标记时使用可见静态网格。角色胶囊控制器使用固定步扫掠、滑移、重力和地面检测。
- GUID 基于稳定源与子资产身份，revision 为数字，内容哈希另存。连续保存合并处理；源在导出期间变化会拒绝该次导出并重试。
- 发布先验证、后事务提交；崩溃恢复和失败保留上一成功版本。引擎在安全更新点切换资源和碰撞。
- `.rxmodel` 是完整版本快照，不能通过修改派生 PNG／JSON 代替源同步。Assets 对 Blender 来源资产的“重新导入”会触发源工程重新发布。
- 实例材质参数覆盖：`ModelRenderer.materialOverrides` 的键为材质槽索引，值支持 `baseColor`、`metallic`、`roughness`、`emissive`、`normalScale`、`occlusionStrength`、`alphaMode`、`alphaCutoff`、`doubleSided`。覆盖不会改动导入源。
- 动画采用 CPU 采样／蒙皮与 Vulkan 动态顶点上传；3D 模型视口经 GPU readback/帧流呈现，桌面共享纹理子窗口对这类场景隐藏，避免遮住实时画面。

不包含双向回写、IK、动画重定向、根运动、复杂动作状态机、碰撞台阶算法或跨项目分发。

## 验证

```powershell
pnpm typecheck
pnpm test
cargo test -p assetd
cargo test -p engine-host
cargo test -p engine-scene-mcp
cargo test -p blender-bridge-mcp
cargo test -p forge-agentd blender::
python -X utf8 tools/e2e/blender_pipeline.py --bin-dir target/blender-validation/debug --engine-bin-dir target/debug
```

端到端测试在隔离项目／端口中生成固定 Blender 回归样例，验证发布、动作差异、同 GUID 更新、同尺寸纹理变更及失败恢复，并保存 RGBA 预览 PNG 和 JSON 结果。仅清理测试启动的进程，源文件和日志保留在输出目录。它明确记录 `fixtureOnly=true`、`nativeComputerUseVerified=false`：固定脚本测试不能替代 Codex 原生 computer-use 的现场制作验收。
