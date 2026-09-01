# 05 · MCP 工程集(核心设计点)

> 全部重复性/批量性/可程序化操作的**唯一执行面**(不变量 I-2,红线 R-1/R-2)。
> agent 经 `mcp__{server}__{tool}` 调用;人类用户的 UI 操作在底层也复用同一工具面
> (UI → gateway → 同一 handler),保证行为一致、可审计。

## 1. 总约定

### 1.1 Server 清单

| server | bin | 职责域 | 底层 |
|---|---|---|---|
| `engine-scene` | `engine-scene-mcp`(可与 engine-host 同进程内嵌或伴生) | 场景/实体/组件/物理查询/PIE/视口 | engine-host 控制通道(`03 §5.2`) |
| `asset-pipeline` | `asset-pipeline-mcp` | 素材导入/构建/缓存/引用图/批处理 | assetd(`08 §1`) |
| `code-forge` | `code-forge-mcp` | `.rx` 脚本构建/诊断/格式化/测试/LSP 结构化编辑 | `rx` CLI + rurixc LSP |
| `project` | `project-mcp` | 项目/文件/git/设置 | 文件系统 + git2 |
| `playtest` | `playtest-mcp` | 无头仿真/输入注入/断言/录像 | engine-host `sim.*` |
| `gen-image`(预留) | `gen-image-mcp` | 图像/贴图生成 | 可插拔后端(`08 §6.2`) |
| `gen-model`(预留) | `gen-model-mcp` | 3D 模型生成与后处理 | 可插拔后端(`08 §6.3`) |

### 1.2 `mcp.json` 声明与 autoStart

```json
{
  "servers": {
    "engine-scene": { "command": "target/debug/engine-scene-mcp.exe",
      "args": ["--project", "projects/demo"], "autoStart": true,
      "healthCheck": { "tool": "host_ping", "intervalMs": 5000 } },
    "asset-pipeline": { "command": "target/debug/asset-pipeline-mcp.exe", "args": ["--project", "projects/demo"] },
    "code-forge": { "command": "target/debug/code-forge-mcp.exe", "args": ["--project", "projects/demo"] },
    "project": { "command": "target/debug/project-mcp.exe", "args": ["--root", "projects/demo"] },
    "playtest": { "command": "target/debug/playtest-mcp.exe", "args": ["--project", "projects/demo"] },
    "gen-image": { "command": "target/debug/gen-image-mcp.exe", "enabled": false },
    "gen-model": { "command": "target/debug/gen-model-mcp.exe", "enabled": false }
  }
}
```

`autoStart: true` 的 server 由 agent-mcp 管理器拉起并看护;`enabled: false` 不拉起(预留接口默认关)。

### 1.3 工具元数据约定

每个工具的 schema 描述中必须含:

- `idempotent`: true/false。true = 同参数重复调用效果等同一次(agent 可安全重试)。
- `mutates`: `scene` | `assets` | `code` | `project` | `none`。
- `permissionTier`: `read` | `write` | `destructive`(对接权限系统,`12 §2`)。
- `bulk`: true/false(bulk 工具自动附带「先 checkpoint」语义,§2.4)。
- 错误返回:统一 `{ error: { code, message, details? } }`;错误码表见 `11 §5`。

### 1.4 命名

工具名 = snake_case 动词_名词,如 `entity_create`、`asset_reimport`;与同域 HTTP API(`11 §4`)同名同参,一一对应。

## 2. engine-scene(server 名:`engine-scene`)

### 2.1 场景

| 工具 | 参数 | 返回 | tier/幂等 |
|---|---|---|---|
| `scene_new` | `{ name, template? }` | `{ scenePath }` | write / 幂等(重名报错) |
| `scene_load` | `{ path }` | `{ sceneSummary }` | write(切当前场景)/ 幂等 |
| `scene_save` | `{ path? }` | `{ saved, path }` | write / 幂等 |
| `scene_diff` | `{ }` | `{ dirtyEntities[], added[], removed[] }` | read / 幂等 |
| `scene_summary` | `{ }` | `{ entityCount, componentCounts, renderSettings }` | read / 幂等 |

### 2.2 实体与组件

| 工具 | 参数 | 返回 | tier/幂等 |
|---|---|---|---|
| `entity_create` | `{ name?, components?, transform?, parentId?, prefabRef? }` | `{ entityId }` | write / 否(返回新 id) |
| `entity_destroy` | `{ entityIds[] }` | `{ destroyed[] }` | destructive / 幂等 |
| `entity_reparent` | `{ entityId, newParentId?, keepWorldTransform=true }` | `{}` | write / 幂等 |
| `entity_rename` | `{ entityId, name }` | `{}` | write / 幂等 |
| `entity_get` | `{ entityId }` | `{ entity }`(组件全量) | read / 幂等 |
| `entity_list` | `{ filter?: { nameGlob?, componentType?, parentId?, folder? } }` | `{ entities[] }` | read / 幂等 |
| `entity_batch_apply` | `{ filter, patch: { componentType, fields }, dryRun? }` | `{ matched, changed, preview? }` | write(bulk)/ 幂等(dryRun 纯读) |
| `component_add` | `{ entityId, type, fields? }` | `{}` | write / 幂等(已有则合并报错) |
| `component_remove` | `{ entityId, type }` | `{}` | write / 幂等 |
| `component_set` | `{ entityId, type, fields }`(RFC7396 merge) | `{}` | write / 幂等 |
| `component_get` | `{ entityId, type }` | `{ fields }` | read / 幂等 |
| `component_list_types` | `{ }` | 组件注册表:`[{ type, fields:[{name,type,default,range?}], category }]` | read / 幂等 |
| `transform_set` | `{ entityId, translation?, rotation?, scale?, space=local|world }` | `{}` | write / 幂等 |
| `transform_batch_set` | `{ items: [{ entityId, ...trs }] }` | `{ applied }` | write(bulk)/ 幂等 |

### 2.3 物理查询与播放

| 工具 | 参数 | 返回 | tier/幂等 |
|---|---|---|---|
| `physics_cast_ray` | `{ origin, dir, maxDist, filter? }` | `{ hits: [{ t, entityId, bodyId, point, normal }] }`(规范序) | read / 幂等 |
| `physics_cast_shape` | `{ shape, from, to, filter? }` | `{ hits[] }` | read / 幂等 |
| `physics_overlap` | `{ shape, transform, filter? }` | `{ overlaps: [{ entityId, bodyId }] }` | read / 幂等 |
| `physics_impulse` | `{ entityId, impulse }` | `{}` | write / 否 |
| `play_enter` / `play_pause` / `play_resume` / `play_step` / `play_exit` | `{}` / `{ frames=1 }`(step) | `{ state }` | write / 幂等(状态机语义) |
| `play_state` | `{}` | `{ mode: edit\|play\|paused, simTime, frame }` | read / 幂等 |

### 2.4 视口与诊断

| 工具 | 参数 | 返回 | tier/幂等 |
|---|---|---|---|
| `viewport_screenshot` | `{ width?, height?, camera? }` | `{ imageFileRef }`(落 `.forge/tmp/`,文件引用非字节) | read / 幂等 |
| `viewport_pick` | `{ x, y }`(视口像素) | `{ entityId? , point? }` | read / 幂等 |
| `camera_set_editor` / `camera_get_editor` | TRS + fov | `{}` / `{ camera }` | write / 幂等 |
| `render_get_settings` / `render_set_settings` | `{}` / `{ patch }` | `{ settings }` | read / write,幂等 |
| `render_graph_dump` | `{ format: text\|json }` | `{ dump }` | read / 幂等 |
| `events_drain` | `{ kinds?: [contact\|streaming\|diagnostic\|frameStats], maxN? }` | `{ events[] }`(规范序) | read / 否(消费语义) |
| `host_ping` / `host_restart` | `{}` | `{ ok, uptimeMs }` / `{ restarted }` | read / write |
| `scene_checkpoint_create` | `{ label? }` | `{ checkpointId }` | write / 否 |
| `scene_checkpoint_restore` | `{ checkpointId }` | `{}` | destructive / 幂等 |

**bulk 前置规则**:任何 `bulk: true` 工具被调用时,server 先自动执行
`scene_checkpoint_create(label=auto:bulk:<tool>)`,失败则拒绝执行(I-5;对接 `04 §9`/`12 §4`)。

## 3. asset-pipeline(server 名:`asset-pipeline`)

| 工具 | 参数 | 返回 | tier/幂等 |
|---|---|---|---|
| `asset_import` | `{ sourcePaths[] \| sourceDir, destFolder, importSettings? }` | `{ imported: [{ assetPath, guid, type }], failed[] }` | write / 幂等(同源同设置覆盖构建产物) |
| `asset_reimport` | `{ assetPaths[] }` | `{ rebuilt[], failed[] }` | write(bulk)/ 幂等 |
| `asset_list` | `{ folder?, type?, nameGlob? }` | `{ assets: [{ path, guid, type, size, deps }][] }` | read / 幂等 |
| `asset_get_meta` | `{ assetPath }` | `{ meta }`(导入设置 + provenance) | read / 幂等 |
| `asset_set_meta` | `{ assetPath, patch }` | `{}` | write / 幂等 |
| `asset_move` | `{ assetPath, destFolder }` | `{ moved, redirector }` | write / 幂等(自动留 redirector,`08 §5.3`) |
| `asset_delete` | `{ assetPaths[], force=false }` | `{ deleted[], blockedByRefs[] }` | destructive / 幂等 |
| `asset_fix_redirectors` | `{ folder? }` | `{ fixed[] }` | write(bulk)/ 幂等 |
| `asset_refs` | `{ assetPath, direction: refs\|referencedBy, depth? }` | `{ edges[] }` | read / 幂等 |
| `asset_build_status` | `{ assetPaths[]? }` | `{ items: [{ path, state: current\|stale\|building\|failed, hash }] }` | read / 幂等 |
| `texture_process` | `{ assetPath, ops: { resize?, compress?, normalFromHeight?, channelPack? } }` | `{ outputAssetPath }` | write / 幂等 |
| `mesh_inspect` | `{ assetPath }` | `{ vertices, triangles, meshlets, lods, materials[], bounds }` | read / 幂等 |
| `mesh_lod_generate` | `{ assetPath, levels: [{ ratio }] }` | `{ lods }` | write / 幂等 |
| `asset_batch_rename` | `{ folder, pattern, replacement, dryRun? }` | `{ matched, renamed[], preview? }` | write(bulk)/ dryRun 幂等 |

缓存键与确定性:见 `08 §4.3`;`asset_import` 的 `importSettings` 全字段进入缓存键。

## 4. code-forge(server 名:`code-forge`)

| 工具 | 参数 | 返回 | tier/幂等 |
|---|---|---|---|
| `rx_check` | `{ file? \| project }` | `{ diagnostics: [{ code, severity, file, span, message, suggestion? }] }` | read / 幂等 |
| `rx_build` | `{ file \| project, emit?: exe\|ptx\|dxil }` | `{ artifact?, diagnostics[] }` | write(产物)/ 幂等 |
| `rx_run` | `{ file \| project, args?, timeoutMs? }` | `{ exitCode, stdoutRef, stderrRef }` | write(副作用)/ 否 |
| `rx_fmt` | `{ file? \| project, checkOnly? }` | `{ formatted[] , diff?}` | write / 幂等 |
| `rx_test` | `{ filter?, timeoutMs? }` | `{ passed, failed, failures[] }` | read(语义)/ 幂等 |
| `rx_doc` | `{ root, out }` | `{ outDir }` | write / 幂等 |
| `code_structured_edit` | `{ file, edits: [{ kind: replace\|insert\|delete, span \| symbolQuery, content? }] }` | `{ applied, newDiagnostics? }` | write / 否 |
| `code_symbol_search` | `{ query, kinds? }` | `{ symbols: [{ name, kind, file, span }] }` | read / 幂等 |
| `code_references` | `{ symbolQuery }` | `{ refs[] }` | read / 幂等 |

实现:`rx_*` 子进程包 `rx` CLI(超时/输出截断/结构化 JSON 解析);`code_*` 走 rurixc LSP 会话(常驻,复用 `src/rurixc` LSP 能力)。

## 5. project(server 名:`project`)

| 工具 | 参数 | 返回 | tier/幂等 |
|---|---|---|---|
| `project_create` | `{ name, dir, template? }` | `{ projectRoot }` | write / 幂等 |
| `project_info` | `{}` | `{ name, engineVersion, rurixVersion, scenes[], settings }` | read / 幂等 |
| `fs_read` / `fs_write` / `fs_list` / `fs_glob` / `fs_grep` | 常规 | 常规 | read/write / 幂等(写=覆盖) |
| `git_status` | `{}` | `{ branch, dirty[], aheadBehind }` | read / 幂等 |
| `git_commit` | `{ message, paths? }` | `{ commitId }` | write / 否 |
| `git_branch` / `git_checkout` | `{ name }` / `{ ref }` | `{}` | write / checkout 非幂等 |
| `settings_get` / `settings_set` | `{ domain, key? }` / `{ domain, patch }` | `{ settings }` | read / write,幂等(写仅限白名单域,`12 §2`) |

`fs_*` 根限定项目目录;越界 = `PROJECT_OUT_OF_ROOT`(I-5)。删除文件一律走 `asset_delete` 或 proposal,`fs_write` 不接受 `delete` 语义。

## 6. playtest(server 名:`playtest`)

| 工具 | 参数 | 返回 | tier/幂等 |
|---|---|---|---|
| `test_run` | `{ scenePath, script: [ { op: waitFrames\|injectInput\|assertState\|assertScreenshot\|setVar, ... } ], headless=true, timeoutMs? }` | `{ result: pass\|fail, steps: [{ op, ok, detail }], artifactsRef }` | write(临时)/ 幂等(确定性,I-5) |
| `test_assert_library` | `{}` | 内建断言清单(实体存在/属性区间/截图 SSIM≥阈值/接触事件发生/帧率下限) | read / 幂等 |
| `capture_video` | `{ scenePath, frames, fps?, script? }` | `{ videoFileRef }` | write(临时)/ 幂等 |
| `replay_session` | `{ sessionId }` | `{ result }` | read / 幂等 |

截图断言用 rurix-render `temporal::ssim`;确定性依赖 `03 §6` 物理确定性红线。
并发:多 host headless 实例,上限 `FORGE_AGENT_MAX_HEADLESS_HOSTS`(`04 §5.3`)。

## 7. gen-image(预留,server 名:`gen-image`)

> 接口面冻结,后端可插拔;未配置后端时所有工具返回 `GEN_BACKEND_NOT_CONFIGURED`(I-5),不静默失败。

| 工具 | 参数 | 返回 |
|---|---|---|
| `gen_backends_list` | `{}` | `{ backends: [{ id, kind: local\|remote, configured, capabilities[] }] }` |
| `gen_image` | `{ prompt, negativePrompt?, size?, styleRefAssetPath?, seed?, n=1..4, backend? }` | `{ candidates: [{ imageFileRef, seed, backendId }] }` |
| `gen_texture_set` | `{ prompt, materialKind: pbr\|unlit, maps: [albedo,normal,roughness,ao?], size?, seamless=true }` | `{ textureAssets: [{ map, assetPath }] }`(自动入管线) |
| `gen_accept` | `{ imageFileRef, destFolder, name }` | `{ assetPath, guid }`(导入 + provenance 写入) |
| `gen_variations` | `{ sourceImageRef, prompt?, strength, n }` | `{ candidates[] }` |

## 8. gen-model(预留,server 名:`gen-model`)

| 工具 | 参数 | 返回 |
|---|---|---|
| `gen_mesh` | `{ prompt \| imageRef, targetPolyBudget?, styleRefAssetPath?, backend? }` | `{ candidates: [{ meshFileRef, stats }] }` |
| `gen_mesh_refine` | `{ meshFileRef, ops: { retopo?: { targetFaces }, unwrap?, rigPreview? } }` | `{ refinedMeshFileRef, stats }` |
| `gen_accept` | `{ meshFileRef, destFolder, name, importSettings? }` | `{ assetPath, guid }`(走 asset_import 同一构建链,含 meshlet 化) |

后处理(retopo/LOD)默认走 rurix-geom-build 简化 DAG 派生 LOD,不另引第三方库(决策 D-009)。

## 9. 工具面扩展规则

1. 新工具必须登记本章表格 + JSON Schema + 错误码,PR 引用条款号(对标 rurix 规范条款↔conformance↔PR 三角)。
2. 禁止「万能工具」(如 `engine_eval` 任意脚本执行):agent 能力扩张走新工具而非 eval 后门(安全,`12 §2`)。
3. 任何 UI 新操作先问「对应工具是哪个」;没有就先补工具(P-2)。
