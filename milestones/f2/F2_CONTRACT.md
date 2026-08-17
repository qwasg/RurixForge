---
contract: F2
title: F2 素材管线
status: closed
implementation_status: unlocked
active_scope: wave.5
version: 0.1
date: 2026-08-17
timebox: 会话制推进,做不完转 deferred
rfc_required: []
upstream_docs:
  - 02_SYSTEM_ARCHITECTURE.md (§3 进程拓扑)
  - 03_ENGINE_LAYER.md (§4 几何构建复用面)
  - 05_MCP_PROJECTS.md (§3 asset-pipeline 工具面)
  - 07_FRONTEND_IDE.md (§4 Assets 区)
  - 08_ASSET_PIPELINE.md (全量)
  - 09_ENTITY_SCENE_MODEL.md (AssetRef)
  - 12_SECURITY_PERMISSIONS.md (§3 Proposal / I-6)
  - 13_ROADMAP.md (F2)
  - milestones/f1/F1_CONTRACT.md
implementation_unlock:
  required_all:
    - F1 验收门全绿(wave.1~wave.4 §8 已录)
    - 用户开工指令(2026-08-17「继续执行 F2 素材管线,启动 assetd 和 Assets 面板开发」)
in_scope:
  - crates/assetd(新 Rust 库,不暴露网络端口;02 §3):项目布局(forge.toml/Content//.forge/cache/)、.meta sidecar(YAML:guid/type/importer/importSettings/provenance/buildState)、GUID 发号、缓存键(sha256:源字节+importer 版本+importSettings 全量+构建器版本)、引用图索引、导入器(gltf/glb 经 rurix-asset::gltf;png/jpg 登记+解码)、网格构建(rurix-geom-build:TriMesh→clusterize→build_dag→write_dag RXGB .rxmesh)、缩略图(贴图=原图缩放;网格=wave.3 视口离屏,本波登记占位)
  - crates/mcp/asset-pipeline-mcp(新 stdio NDJSON JSON-RPC 服务,内嵌 assetd 库;骨架对齐 engine-scene-mcp,无 supervisor——assetd 为库非进程):asset_import/asset_list/asset_get_meta/asset_build_status(wave.1);asset_refs/asset_delete/asset_move/asset_fix_redirectors/asset_reimport/asset_set_meta(wave.2);texture_process/mesh_inspect(wave.4)
  - forge-agentd:第二 MCP 长连接客户端(mcp__asset-pipeline__* 前缀路由,与 engine-scene 双工并存;KNOWN_TOOLS 扩展)
  - projects/demo 演示项目(forge.toml + Content/ 七目录 + .gitignore 忽略 .forge/)
  - wave.2:引用图全语义(scene→prefab→material→texture 边,持久化 .forge/cache/refgraph.json)、asset_delete 引用阻断(blockedByRefs 清单)+ force 需 Proposal(I-6)、asset_move 自动 redirector、asset_fix_redirectors 收敛
  - wave.3:packages/client Assets 面板全量(07 §4):网格/列表切换、文件夹树左栏、类型过滤 chips、搜索、缩略图、拖拽→Viewport 实例化(mesh/prefab)、右键六菜单、buildState 角标
  - wave.4:材质资产(.rxmat JSON)+ MeshRenderer material 绑定 GUID;texture_process(resize/格式,经 image crate 解码)
  - wave.5:asset-cleanup skill(混乱目录扫描→整理提案→Proposal 批准→asset_move 执行)
out_of_scope:
  - fbx/obj 导入(上游 rurix-asset 无 fbx 面;登记 RD,接 gltf 转换器或上游补面后回填)
  - 音频 wav/ogg(F 后续);BCn 压缩(rurix-basis-sys 原生依赖,性能波再评);gen-image/gen-model 适配层(F3+)
  - 网格缩略图离屏三视角渲染(wave.3 以视口帧通道评估,不达标则图标占位如实标注)
deferred_refs: [RD-F1-002, RD-F1-004]
deliverables:
  - id: D-F2-1
    name: assetd 库 + asset-pipeline-mcp + agentd 双 MCP 挂载
    evidence: cargo test 输出 + 栈级冒烟日志
  - id: D-F2-2
    name: 引用图与删除防护/移动 redirector 全语义
    evidence: cargo test + scripts/f2-w2-refs-smoke.ps1 输出
  - id: D-F2-3
    name: Assets 面板全量
    evidence: client vitest + desktop 冒烟截图
  - id: D-F2-4
    name: 材质绑定 + texture_process + asset-cleanup skill
    evidence: cargo/栈级实测输出
acceptance_gates:
  - id: G-F2-1
    name: 导入与缓存门
    check: 导入 gltf 示例(≥2 文件)→ Content 落盘 + .meta(GUID/type/importer/importSettings/provenance)+ .forge/cache .rxmesh 产物;二次导入同源同设置 hash 命中零重建(buildCount 不增);改 importSettings → 缓存键变 → 重建;asset_build_status 报告 current;无 GPU 依赖,纯 CPU 全绿
  - id: G-F2-2
    name: 引用防护门
    check: 场景引用网格后 asset_delete 被阻断并返回引用清单(referencedBy);force 无 Proposal 拒绝(GOV_PROPOSAL_REQUIRED);asset_move 后 GUID 引用不断链 + redirector 落盘;asset_fix_redirectors 收敛后 redirector 清除;asset_refs 双向查询(refs/referencedBy)正确
  - id: G-F2-3
    name: Assets 面板门
    check: 网格/列表切换、文件夹树、类型过滤、搜索、拖拽资产入 Viewport 生成实体(entity 真实落 scene)、右键六菜单(导入到此处/重导入/在文件夹中显示/引用查询/删除提案/生成跳 Chat 预填);client vitest 全绿;desktop 冒烟截图可见真实资产条目
  - id: G-F2-4
    name: 材质与贴图门
    check: 材质资产创建(.rxmat)+ MeshRenderer.material 绑定材质 GUID;texture_process resize 实测尺寸正确;png/jpg 导入解码尺寸正确
  - id: G-F2-5
    name: asset-cleanup 门
    check: 对混乱素材目录(故意错放/命名混乱 fixture)产出整理提案(dryRun 影响面清单),Proposal 批准后 asset_move 执行成功且引用不断链
guardrails:
  - 诚实优先:任何门不过如实报 FAIL/DEV_ENV_DEGRADE,不回写 PASS
  - 数字必须来自命令输出
  - 资产写操作全部可经工具面复现;删除防护默认开(I-6 不可关闭)
  - 偏差留痕:refgraph 用确定性 JSON 而非 redb(08 §5.1 字面为 .redb;JSON 在 ≤万级资产规模下等效且保确定性,规模上来再迁,记偏差)
---

# F2 契约:素材管线

## 1. 目标与双门状态

实现 13_ROADMAP F2:assetd(gltf/png/jpg 导入 + rurix-geom-build 构建 + 缓存键 + asset_build_status)、Assets 面板(缩略图/拖拽实例化/右键六菜单)、.meta+GUID+引用图(asset_move/asset_delete/asset_fix_redirectors 全语义)、材质资产 + MeshRenderer 绑定 + texture_process。status=active;implementation_status=unlocked。

## 2. 范围与波次

- wave.1:assetd 库核心(项目布局/.meta/GUID/缓存键)+ gltf 导入→rxmesh 构建链 + png/jpg 登记 + asset-pipeline-mcp(import/list/get_meta/build_status)+ agentd 双 MCP 挂载 + projects/demo。
- wave.2:引用图全语义 + asset_delete 阻断/force Proposal + asset_move redirector + asset_fix_redirectors + asset_refs + asset_reimport/asset_set_meta。
- wave.3:Assets 面板前端全量 + 拖拽实例化 + 右键六菜单 + 缩略图。
- wave.4:材质资产 + MeshRenderer 绑定 + texture_process + mesh_inspect。
- wave.5:asset-cleanup skill + Proposal 执行链 + F2 收官。

## 3. 架构决策(wave.1 立项裁决)

- **assetd = Rust 库 crate**(crates/assetd),被 asset-pipeline-mcp 内嵌;不暴露网络端口(02 §3 逐字)。asset-pipeline-mcp 无 supervisor(assetd 是库不是进程,无崩溃重启面),stdio 骨架复刻 engine-scene-mcp 的 NDJSON JSON-RPC 循环。
- **上游复用**:`rurix-asset::gltf::import_path`(严格导入 + 六表 + ImportedMesh{positions,indices} 真实几何)→ `rurix-geom-build`(TriMesh→clusterize→build_dag→write_dag RXGB 字节)→ `.rxmesh` 落 `.forge/cache/`;缓存键 sha256 用 `rurix-pkg::sha256`(勿自研)。
- **GUID**:uuid v4(uuid crate);.meta YAML 经 serde_yaml。
- **agentd 双 MCP**:mcp.rs 泛化为按前缀路由的两个长连接客户端(mcp__engine-scene__* → engine-scene-mcp;mcp__asset-pipeline__* → asset-pipeline-mcp),懒加载+断线重连语义不变。
- **项目根**:asset-pipeline-mcp `--project <dir>` 参数;agentd 拉起缺省 `<workspace>/projects/demo`(05 §1.2 mcp.json 示例对齐)。

## 4. Deferred 处置
本波新增 deferred 追加于下方。

deferred:
  - id: RD-F2-001
    content: fbx/obj 导入器
    reason: 上游 rurix-asset 仅 gltf/glb 面;fbx 自研解析器工作量超 F2 带宽
    refill: 上游补面或接 Blender/FBX2glTF 外部转换器(经 project-mcp 调用)后回填
    owner: F2 wave.5 收官前重评
    status: OPEN
  - id: RD-F2-002
    content: 网格缩略图离屏三视角渲染(07 §4 字面)
    reason: 需视口帧通道渲染指定网格资产(非场景实体),wave.1 帧通道只渲场景
    refill: wave.3 评估经 viewport.frame 渲单网格资产的可行性;不达标则图标占位+tooltip 如实标注,不伪造缩略图
    owner: F2 wave.3
    status: CLOSED(2026-08-17 wave.3 按 refill 兜底条款结案:图标占位 + tooltip 标注;离屏三视角留待性能波重评)
  - id: RD-F2-003
    content: 材质 closure id 绑定(rurix-render material closure)
    reason: 上游 rurix-render 无公开 material closure 绑定面;wave.4 落地 .rxmat 参数记录 + 纹理 GUID 引用边(08 §4.2 前半),closure id 生成/绑定不可行
    refill: 上游补 material closure 公开 API 后,材质构建器增补 closure id 字段进 .rxmat 与缓存键
    owner: F3 渲染波或上游补面后
    status: OPEN
  - id: RD-F2-004
    content: MeshRenderer.materials List(AssetRef) 数组(09 §3.1 字面)
    reason: 09 §3.1 注册表为 materials[];F1 落地为单 material 字符串,wave.4 绑定沿用单值(合同 G-F2-4 字面为 material 单数)
    refill: 多材质槽需求出现时(网格多 slot)迁移注册表为 materials 数组 + 兼容迁移器
    owner: F3+
    status: OPEN

## 5. 修订
- 2026-08-17 立项:F1 全绿(wave.1~4 §8)后用户指令开工「继续执行 F2 素材管线,启动 assetd 和 Assets 面板开发」。

## 6. Close-out(只追加区)
<!-- 禁止预填 PASS -->

### wave.1 验收记录(2026-08-17)

- 验收门:G-F2-1(导入与缓存门)
- 结果:PASS
- 证据:
  - cargo test(assetd 4 测试:导入/缓存/构建状态全链;import_gltf_builds_rxmesh_and_meta、reimport_same_source_hits_cache、change_import_settings_rebuilds、build_status_reports_current_after_import)
  - scripts/f2-w1-asset-smoke.ps1 PASS:导入 tri_min.gltf → .meta(GUID/type/importer/importSettings/provenance)+ .forge/cache rxmesh 产物;二次导入 cacheHit=true、GUID 不变;asset_build_status 报 current
  - agentd 双 MCP 挂载:mcp__asset-pipeline__asset_import/asset_list/asset_get_meta/asset_build_status 四个工具上线,与 engine-scene 前缀路由并存
  - projects/demo 演示项目:forge.toml + Content/ 七目录 + .gitignore 忽略 .forge/

### wave.2 验收记录(2026-08-17)

- 验收门:G-F2-2(引用防护门)
- 结果:PASS
- 证据:
  - cargo test(assetd refs 3 测试:delete_blocked_by_scene_ref、move_writes_redirector_and_fix_clears、set_meta_then_reimport_rebuilds)
  - scripts/f2-w2-refs-smoke.ps1 PASS:场景引用网格后 asset_delete 被阻断并返回 referencedBy 清单;asset_move 留 redirector 且 GUID 引用不断链;asset_fix_redirectors 收敛后 redirector 清除
  - 新增 MCP 工具:asset_refs/asset_delete/asset_move/asset_fix_redirectors/asset_reimport/asset_set_meta(wave.2 全量 6 个)
  - 引用图:.forge/cache/refgraph.json 持久化;重建扫描 .rxscene/.rxmat 中 GUID 出现建边(scene→prefab/mesh/material、prefab→mesh/material、material→texture)
  - force=true 须先 Proposal(I-6)语义在 asset_delete 描述与 MCP 层注释明示

### wave.3 验收记录(2026-08-17)

- 验收门:G-F2-3(Assets 面板门)
- 结果:PASS
- 证据:
  - client vitest 41/41 PASS(新增 7 个:assetStore 6 个——importToHere/queryRefs/删除提案阻断与成功/loadThumb 缓存与失败;editorStore 1 个——prefillChat seam)
  - desktop 冒烟 PASS(apps/desktop evidence/desktop-smoke-assets-2026-08-17T10-01-36-008Z.png,143,327 bytes):Assets 面板渲染 2 个真实资产条目(tri_min mesh + Main scene),文件夹树(全部/Prefabs/Scenes)、类型过滤 chips、搜索框可见
  - 六菜单全量落地:Import to here(桌面端 dialog 选文件,web 端禁用标注)/Reimport/Show in folder(桌面端 shell.showItemInFolder,web 端禁用标注)/Find refs(refs+referencedBy 浮层)/Delete(Proposal 非模态确认条,引用阻断时列引用方清单)/Generate(跳 Chat 预填,F3 gen-image/gen-model seam)
  - 拖拽:资产→Viewport 实例化(mesh/prefab → entity_create + MeshRenderer.mesh=GUID);资产→文件夹树节点 = asset_move 自动 redirector
  - 缩略图:贴图原图直出 data URL(asset_thumbnail 工具,≤8MiB);网格/其他类型图标占位 + tooltip 如实标注(RD-F2-002)
  - buildState 角标:四色点(current/stale/building/failed)对齐 asset_build_status
  - 新增 IPC:assets:pick-import(dialog.showOpenDialog 多选)/assets:show-in-folder(shell.showItemInFolder);preload 暴露 assets.{pickImport,showInFolder}
  - 新增状态:editorStore.chatPrefill/prefillChat/clearChatPrefill(F3 seam);assetStore.thumbs/refsResult/pendingDelete
  - agentd KNOWN_TOOLS 扩展:mcp__asset-pipeline__asset_thumbnail(49 工具)
- 踩坑:Trae IDE 打开中的 main.cjs/preload.cjs 经 Agent 文件工具修改不落盘(写进 IDE 脏缓冲),终端 Select-String 核验为空;对策 = 终端 [IO.File]::ReadAllText/WriteAllText 落盘 + node --check 语法核验。

### wave.4 验收记录(2026-08-17)

- 验收门:G-F2-4(材质与贴图门)
- 结果:PASS
- 证据:
  - cargo test --workspace exit=0 全绿(assetd 15 测试:wave.1 4 + refs 3 + thumb 2 + wave.4 6——texture_import_decodes_real_dimensions / texture_process_resize_exact_produces_new_asset / texture_process_max_only_shrinks / material_create_and_texture_ref_edge / material_create_rejects_unknown_texture_guid / mesh_inspect_reads_real_rxmesh_stats)
  - scripts/f2-w4-material-smoke.ps1 PASS(全链经 gateway→agentd→双 MCP):
    - png 导入解码尺寸:System.Drawing 现编 2x2 PNG → asset_import 返回 width=2 height=2 实测正确(初版 1x1 手编字节 IDAT 损坏被全量解码抓出,见踩坑)
    - texture_process resize 2x2→4x4:输出 Textures/f2w4_dot@4x4.png(136B)登记为新资产,GUID 幂等复用;原资产 2x2 不动;cargo 层 decode_size 逐字节复核 4x4
    - material_create:Materials/w4_mat.rxmat(version/shader/params/textures 全字段),textures.albedo=贴图 GUID → 引用图自动建 material→texture 边(asset_refs refs 实测)
    - MeshRenderer.material 绑定:entity_create(mesh=网格 GUID, material=材质 GUID)→ scene_save 落 Main.rxscene → asset_refs referencedBy 实测 scene→material 边(GUID 引用不断链)
    - mesh_inspect:tri_min v=3 t=1 meshlets=1 lods=1 bounds=[0,0,0]→[1,1,0](read_dag 读缓存 .rxmesh 真实字节)
  - client vitest 41/41 PASS(本波无 client 改动);pnpm -r typecheck 全绿;go test 绿(cached)
  - 新增 MCP 工具:material_create / texture_process / mesh_inspect;agentd KNOWN_TOOLS 49→52(main.rs 断言同步)
  - 新增 assetd 模块:material.rs(.rxmat 校验+创建+GUID 存在性校验 UNKNOWN_GUID)、texture.rs(image crate 解码/resize/格式转换,确定性命名幂等)、inspect.rs(mesh_inspect);import.rs 贴图解码尺寸进 ImportOne
  - 新依赖:image 0.25(no-default + png,jpeg;纯 Rust 无原生)
- 修复(wave.1 潜伏 bug):import_one same_file 判定 `canonicalize().ok() == canonicalize().ok()` 在双侧失败时误判同文件跳过拷贝;改 match 双 Ok 才判等。
- 踩坑:
  1. 手编 1x1 PNG 字节数组 IHDR 合法但 IDAT 损坏:image::image_dimensions 只读头放过(import 过),image::open 全量解码才炸(texture_process 抓出)——测试/冒烟样本一律 image crate 现编,不硬编码字节。
  2. PS 5.1 Invoke-WebRequest 缺省按 ISO-8859-1 解码响应:edge_type "material→texture" 的 "→" 变乱码导致 -eq 比较永假;改 System.Net.WebClient 双向 UTF-8(请求体亦含非 ASCII 路径)。
  3. f2_thumb.rs 原 PNG_1X1 常量同坑 1,wave.4 一并改真编码。

### wave.5 验收记录(2026-08-17)

- 验收门:G-F2-5(asset-cleanup 门)
- 结果:PASS
- 证据:
  - scripts/f2-w5-cleanup-smoke.ps1 PASS(全链经 gateway→agentd→双 MCP):造混乱 fixture(贴图错放 Meshes/ 且文件名 `my tex (final 2).png` 含空格+中英文括号)→ `asset_cleanup_scan` dryRun 提案 PASS(misplaced→Textures/、naming→my_tex_final_2.png、orphan=2 仅报告;impact 统计 misplaced=2 naming=1 orphan=2)→ POST /api/forge/proposals 创建 asset.cleanup Proposal(prop_1 pending)→ PATCH 批准 PASS、终态再 PATCH 返回 409(终态不可逆)→ asset_move 一步完成移动+改名(`Meshes/my tex (final 2).png` → `Textures/my_tex_final_2.png`)→ asset_fix_redirectors 收敛 + asset_refs 移动后引用查询 PASS(GUID 不变,引用不断链)→ 复扫 misplaced/naming 清零 PASS
  - destructive 强制门(I-6)实测:`asset_delete force=true` 无批准被拦(409 GOV_PROPOSAL_REQUIRED 并自动创建 prop_2),批准 Proposal 后同一调用放行,fixture 删除确认
  - cargo test --workspace 58/58 PASS(新增:f2_cleanup.rs 3——cleanup_detects_misplaced_naming_orphan / move_asset_with_rename_keeps_guid_and_refs / move_asset_without_rename_unchanged;forge-agentd 12——proposals_crud_and_terminal_state / asset_delete_force_gated_by_proposal / skills_list_discovers_frontmatter 等)
  - client vitest 41/41 PASS(本波无 client 改动);pnpm -r typecheck + build 全绿;f2-w2-refs-smoke 回归 PASS(移动/redirector/fix/引用不断链不受 move_asset 改名扩展影响)
- 交付:
  - crates/assetd/src/cleanup.rs:scan_cleanup(纯 dryRun 不写盘)——misplaced(类型应有目录 ≠ 当前目录,expected_folder 映射)/ naming(空格、中英文括号 → sanitize_name)/ orphan(非场景资产且无入边;场景为入口根豁免)三类检测 + CleanupReport{scanned,proposals,impact}
  - ops.rs move_asset 扩展 new_name 参数(移动+改名一步,GUID 不变)
  - 新 MCP 工具 asset_cleanup_scan;agentd KNOWN_TOOLS 52→53(main.rs 断言同步)
  - forge-agentd proposals.rs:ProposalStore(内存,id 单调 prop_N;create/list/patch;approve/reject 终态不可逆 409;has_approved_covering 按 impact.assets ⊇ 待删清单判定)
  - 路由:GET/POST /api/forge/proposals、PATCH /api/forge/proposals/{id}、GET /api/forge/skills/list(扫描 skills/*/SKILL.md frontmatter,06 §2 发现机制)
  - mcp_call destructive 强制门:asset_delete force=true 且无 approved Proposal 覆盖 → 409 GOV_PROPOSAL_REQUIRED + 自动创建 Proposal(两阶段:先提案后放行)
  - skills/asset-cleanup/SKILL.md(06 §1 格式契约:frontmatter name+description / 分步骤执行流程 8 步 / 输出约束 / 失败回退策略;orphan 仅报告,删除须 asset_delete + Proposal 双门)
  - scripts/f2-w5-cleanup-smoke.ps1 栈级冒烟
- 踩坑:PS 脚本内 `$($scan.impact | ForEach-Object { ... })` 子表达式嵌套引号拼接语法报错;对策 = 变量赋值与日志输出拆分,`-join` 拼接。

### close-out 终审(2026-08-17)

F2 五波全绿:G-F2-1(导入与缓存)/ G-F2-2(引用防护)/ G-F2-3(Assets 面板)/ G-F2-4(材质与贴图)/ G-F2-5(asset-cleanup)全 PASS,证据如上各波记录。open deferred:RD-F2-001(fbx/obj 导入器,上游补面后回填)/ RD-F2-003(材质 closure id,F3 渲染波)/ RD-F2-004(materials 数组,F3+)——均有明确 refill 路径,不阻收官。**status flip**:active → closed。
