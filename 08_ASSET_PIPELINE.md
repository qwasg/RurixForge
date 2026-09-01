# 08 · 素材处理管线

> 设计参考:UE5 Content Browser(导入/组织/引用修复/拖放)与 Unity Project window
> (文件系统视图 + Inspector 导入设置 + `.meta` sidecar)的交集。
> 执行面 = `asset-pipeline-mcp`(`05 §3`)+ assetd;UI 只做预览/挑拣/单项操作(`07 §4`)。

## 1. assetd 服务

Rust 库 + 被 asset-pipeline-mcp 内嵌。职责:

- 导入器:fbx / gltf(+glb)/ obj(网格);png / jpg / tga / exr / hdr(贴图);wav / ogg(音频)。
- 构建:网格 → rurix-geom-build(meshlet DAG 或标准 GPU 布局,`03 §4`);贴图 → 尺寸/格式/压缩处理;音频 → 透传 + 转码占位。
- 缓存:`.forge/cache/`(构建产物 + 缩略图 + 引用图索引)。
- 引用图:资产间引用(场景→prefab→材质→贴图/网格)的持久化索引,支撑 `asset_refs` 与删除防护。

## 2. 从 UE5/Unity 取用的设计决策

| 设计点 | 来源 | 本引擎决策 |
|---|---|---|
| 导入即入内容库,统一资产视图 | UE Content Browser | Assets 面板 = 项目 `Content/` 目录视图;源文件与 `.meta` 成对 |
| 文件系统即真相,`.meta` sidecar 存导入设置与 GUID | Unity | 逐字采用:每资产一个 `<file>.meta`(YAML),含 `guid` / `type` / `importSettings` / `provenance` |
| 导入设置随资产,改设置 → 重导入 | Unity Inspector | Inspector 编辑 `.meta.importSettings` → `asset_reimport` |
| 移动资产留 redirector,定期 Fix Up | UE | `asset_move` 自动写 redirector 记录;`asset_fix_redirectors` 收敛;场景/prefab 引用一律 GUID,移动不断链 |
| 删除前引用检查,Force Delete 危险 | UE | `asset_delete` 默认引用阻断;`force=true` 必须 Proposal(`12 §3`) |
| 拖放到视口 = 实例化 | UE/Unity | Assets → Viewport 拖拽创建实体(网格/prefab) |
| 引用矩阵/批量改 | UE Property Matrix | **不采用**(UI 不做批量);批量 → `entity_batch_apply` / agent |

## 3. 项目目录与文件契约

### 3.1 布局

```
<project>/
├── forge.toml            # 项目清单:名称、引擎版本、rurix 依赖锚、入口场景、目录映射
├── Content/              # 全部资产(源文件 + .meta)
│   ├── Meshes/  Textures/  Materials/  Prefabs/  Scenes/  Scripts/  Audio/
├── .forge/
│   ├── cache/            # 构建产物(hash 寻址)、缩略图、引用图
│   └── tmp/              # 截图、playtest 产物、生成候选暂存
└── .gitignore            # 默认忽略 .forge/
```

`forge.toml` 示例:

```toml
[project]
name = "demo"
engine-version = "0.1.0"
rurix-ref = "v1.0.1-dist"
entry-scene = "Content/Scenes/Main.rxscene"

[dirs]
content = "Content"
scripts = "Content/Scripts"
```

### 3.2 `.meta` 文件(每资产一个,YAML)

```yaml
guid: 3f6c2ab8-…            # 创建即定,永不复用;引用一律用 GUID
type: mesh                   # mesh|texture|material|prefab|scene|script|audio
importer: gltf               # 导入器 id
importSettings:              # 全字段进入构建缓存键
  geometryPath: virtualized  # virtualized|standard(03 §4)
  meshletBudget: auto
  generateLods: [0.5, 0.25]
  normals: import            # import|recompute
  materialSlots: [mat_wood, mat_metal]
provenance:                  # I-7;人工导入 = origin: user-import
  origin: user-import        # user-import|gen-image|gen-model
  detail: null               # 生成类:backendId/prompt/seed/parentRef
```

## 4. 导入与构建

### 4.1 导入流程(`asset_import`)

1. 源文件复制进 `Content/<destFolder>/`(源在外部时)或直接登记(已在 Content 内)。
2. 生成 `.meta`(GUID 新建;`importSettings` = 参数 ∪ 类型默认)。
3. 入构建队列:资产类型 → 构建器(§4.2);失败 → `.meta` 标 `buildState: failed` + 诊断,进 Problems 面板。
4. 建引用边(网格→材质→贴图;导入器解析)。

### 4.2 构建器

| 类型 | 构建器 | 产物 |
|---|---|---|
| mesh | rurix-geom-build | `.rxmesh`(meshlet DAG + LOD + 材质槽)+ `mesh_inspect` 统计 |
| texture | assetd 纹理器 | 压缩/重尺寸/格式转换产物(BCn 候选;mip 链) |
| material | 参数校验 + rurix-render material closure 绑定 | 材质记录(closure id + 参数 + 纹理 GUID 引用) |
| prefab/scene | 校验器(引用 GUID 存在性、组件注册表校验) | 校验报告;无中间产物 |
| script | `rx_check`(经 code-forge) | 诊断;产物编译延迟到 Play/打包 |

### 4.3 缓存与确定性

- 缓存键 = `hash(源文件字节, importer 版本, importSettings 全量, 构建器版本)`;命中即跳过。
- rurix-geom-build 输出确定性(同输入 hash → 同产物 hash,`03 §4`),使缓存键可跨机器复用。
- `asset_build_status` 报告 `current|stale|building|failed` 供 UI 角标与 agent 决策。

## 5. 引用管理

### 5.1 引用图

- 边类型:`scene→prefab`、`prefab→mesh/material`、`material→texture`、`script→(无资产边)`。
- 索引持久化 `.forge/cache/refgraph.redb`,资产变更增量更新;`asset_refs` 提供 refs / referencedBy 双向查询(带深度)。

### 5.2 删除防护

`asset_delete` 默认:`referencedBy` 非空 → `blockedByRefs` 返回并拒绝;`force=true` → 仍须 Proposal 确认(I-6)。agent 的 `asset-cleanup` skill 必须先出引用报告再提案。

### 5.3 redirector(移动/重命名)

`asset_move`:更新资产路径 → 写 redirector 记录(旧 GUID→新路径;因引用走 GUID,redirector 仅服务路径级引用与旧场景文件)→ 触发引用方重校验。`asset_fix_redirectors`:扫描并落实路径引用重写,清除 redirector(UE Fix Up Redirectors 对齐)。

## 6. 生成式 API 预留(I-7)

### 6.1 定位

`gen-image-mcp` / `gen-model-mcp` 为**适配层**:对 agent 暴露统一工具面(`05 §7/§8`),
后端可插拔;未配置 = `GEN_BACKEND_NOT_CONFIGURED`(I-5)。密钥走 agentd keystore(R-5)。

### 6.2 gen-image 后端接口(适配器需实现)

```text
capabilities() -> { kinds: [text2img, img2img, texture-set], sizes[], maxBatch }
generate(request) -> [candidate { imageBytes|fileRef, seed, backendId }]
```

首发适配器占位:`remote-openai-compatible`(任意兼容端点)、`local-diffusers`(本地推理服务,后置)。
产物一律先落 `.forge/tmp/gen/`,用户/agent 挑拣后 `gen_accept` 正式导入(写 provenance)。

### 6.3 gen-model 后端接口(适配器需实现)

```text
capabilities() -> { kinds: [text2mesh, img2mesh], polyBudgetRange }
generate(request) -> [candidate { meshFileRef(gltf), stats }]
```

`gen_accept` 后走与人工导入完全相同的构建链(meshlet 化/LOD/材质槽),保证生成资产与人工资产同权同管(D-009)。后处理(retopo/减面/LOD)用 rurix-geom-build 简化 DAG,不引第三方拓扑库。

### 6.4 provenance(强制)

生成资产 `.meta.provenance`:`origin: gen-image|gen-model`,`detail: { backendId, prompt, negativePrompt?, seed, sourceRefs[], generatedAt }`。删除/导出/打包时 provenance 随资产;许可风险由用户承担,引擎在 About/导出清单中可汇总生成资产列表。

## 7. 打包(本期最小)

`project-pack`(F6,`13 §F6`):收集入口场景引用闭包 → 复制产物 + 源 → 独立目录 + 启动器(engine-host `--game` 模式,无编辑面)。资产加密/压缩打包不在本期。

## Errata(只追加区)

- **E-08-001(2026-08-25,F11 / D-025)**:§3.2 与 §6.4 的 `provenance.origin` 枚举 `user-import|gen-image|gen-model` 扩一值 **`store-install`**——资产商店安装的资产在原枚举下无合法取值,而 I-7 要求 provenance 强制写。`origin: store-install` 时 `detail` 形态为 `{ sourceId, sourceName, packageId, packageVersion, fileSha256, license, publisher, installedAt }`,许可证与发布者随资产落盘,导出/打包时一并携带(与生成资产同纪律:许可风险由用户承担,引擎在 About/导出清单中可汇总)。商店安装走与人工导入**完全相同**的构建链(`asset_import`,同 D-009 对生成资产的处置),不设旁路;安装前逐文件 sha256 校验,不符即拒(`STORE_CHECKSUM_MISMATCH`)不静默接受。个人资产库(`data/store/library/`)是跨项目的内容寻址收藏层,与项目 `Content/` 经显式双向操作流转,**不做自动同步**——保持「文件系统即真相」的单一事实源(裁决见 D-F11-B)。
