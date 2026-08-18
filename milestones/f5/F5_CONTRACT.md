---
contract: F5
title: F5 生成接入(gen-image/gen-model)
status: closed
implementation_status: unlocked
active_scope: wave.1
version: 0.1
date: 2026-08-18
timebox: 会话制推进,做不完转 deferred
rfc_required: []
upstream_docs:
  - 05_MCP_PROJECTS.md (§7 gen-image 五工具 / §8 gen-model 三工具 / §9 扩展规则)
  - 06_SKILLS_LIBRARY.md (§3 gen-asset-fill 行)
  - 07_FRONTEND_IDE.md (§4 Assets 右键六项 / §7.2 generation 设置 tab)
  - 08_ASSET_PIPELINE.md (§2 .meta 字段 / §6.2 gen-image 后端接口 / §6.3 gen-model 后端接口 / §6.4 provenance 强制)
  - 11_API_CONTRACTS.md (§2.2 /api/forge/gen/* / §4 GEN_* 错误码)
  - 12_SECURITY_PERMISSIONS.md (§5 R-5 密钥 keystore / I-5 / I-7)
  - 13_ROADMAP.md (F5)
  - milestones/f4/F4_CONTRACT.md
implementation_unlock:
  required_all:
    - F4 验收门全绿(wave.1~wave.5 §6 已录,status=closed)
    - 用户开工指令(2026-08-18「启动 F5 生成接入开发,推进 gen-image/gen-model 与 Assets 右键集成」)
in_scope:
  - gen-image-mcp(crates/mcp/gen-image-mcp,server 名 gen-image):gen_backends_list/gen_image/gen_texture_set/gen_accept/gen_variations 五工具(05 §7 逐字);后端注册表可插拔(capabilities/generate 接口,08 §6.2);产物先落 .forge/tmp/gen/(08 §6.2 逐字),gen_accept 正式导入 Content/ 写 provenance
  - gen-model-mcp(crates/mcp/gen-model-mcp,server 名 gen-model):gen_mesh/gen_mesh_refine/gen_accept 三工具(05 §8 逐字);gen_accept 走 asset_import 同一构建链(meshlet 化/LOD,08 §6.3 D-009)
  - 首发远程适配器 remote-openai-compatible(08 §6.2 首发占位:任意兼容端点;HTTP POST prompt/size/n/seed → imageBytes;超时/重试/结构化错误 GEN_RATE_LIMITED/GEN_BACKEND_ERROR);本地确定性适配器 local-mock(CI 恒绿腿,见 §3 D-F5-A)
  - keystore(12 §5 R-5):后端密钥保管 data/keystore.json(.gitignore 覆盖;env FORGE_GEN_API_KEY 优先);密钥不进前端/项目文件/日志/事件——前端只见 configured 布尔,gen_* 日志脱敏
  - provenance 强制(08 §6.4):origin: gen-image|gen-model;detail: { backendId, prompt, negativePrompt?, seed, sourceRefs[], generatedAt };assetd MetaDoc Provenance 结构扩展(detail 由 string 升级为结构化)
  - 设置页 generation tab(07 §7.2):backends 清单(id/kind/configured/capabilities)+ 端点/密钥配置(写 data/gen-backends.json + keystore;前端不见密钥值)
  - Assets 右键「生成」真实入口(07 §4 六项之生成,F2 wave.3 prefillChat seam 替换):生成对话框(prompt/negativePrompt/size/n=1..4/backend 选择)→ gen_image → 候选挑选网格(modal,缩略图 + seed + Accept)→ gen_accept(destFolder=当前文件夹)入管线
  - gen-asset-fill skill seam 摘除(06 §3:列缺口清单 → 生成候选 → 用户挑拣 → gen_accept)
  - agentd 第四/五 ServerKind 挂载(mcp__gen-image__ / mcp__gen-model__ 前缀);host forgeProxy 扩 /api/forge/gen REST 面(gen_backends_list 与配置写读经 agentd REST,gateway 路由族 11 §2.2 已有)
  - scripts/f5-w*-*.ps1 栈级冒烟
out_of_scope:
  - local-diffusers 本地推理适配器(08 §6.2 标注「后置」)
  - gen_mesh 的 text2mesh/img2mesh 真实远程生成(首个远程适配器仅图像;gen_mesh 无配置后端时 GEN_BACKEND_NOT_CONFIGURED 显式,gen_accept 链不受影响)
  - 生成资产许可汇总 About/导出清单(08 §6.4 尾句,后续波)
  - 密钥系统级加密存储(OS keychain/DPAPI;F5 为 data/keystore.json 文件,登记 deferred)
  - img2img styleRefAssetPath 真实传入远程适配器(接口面保留,适配器 v1 不消费,如实标注)
deferred_refs: [RD-F4-004, RD-F1-002]
deliverables:
  - id: D-F5-1
    name: gen-image-mcp + 后端注册表 + keystore + GEN_BACKEND_NOT_CONFIGURED 门
    evidence: cargo test + scripts/f5-w1-gen-backends-smoke.ps1 输出
  - id: D-F5-2
    name: gen_texture_set/gen_variations + gen-model-mcp + provenance 结构化
    evidence: cargo test + scripts/f5-w2-gen-pipeline-smoke.ps1 输出
  - id: D-F5-3
    name: Assets 生成对话框 + 候选挑选 + generation 设置 tab + gen-asset-fill 摘 seam
    evidence: client vitest + desktop 冒烟截图 + scripts/f5-w3-gen-ui-smoke.ps1 输出
acceptance_gates:
  - id: G-F5-1
    name: 后端配置门
    check: 「无后端配置时全工具面 GEN_BACKEND_NOT_CONFIGURED 显式错误(I-5)」(13_ROADMAP F5 逐字)。机验:清空 gen-backends.json → gen_image/gen_texture_set/gen_variations/gen_mesh/gen_mesh_refine 全返 GEN_BACKEND_NOT_CONFIGURED(gen_backends_list/gen_accept 除外——list 返 backends[] configured=false,accept 不依赖后端);gen_backends_list 如实列两适配器 configured 状态;密钥不出现于任何工具返回/日志/事件(断言扫描)
  - id: G-F5-2
    name: 生成管线门
    check: 配通一个后端后(local-mock 配置写入)→ gen_image n=4 产 4 候选(落 .forge/tmp/gen/,种子确定性:同 seed 同 prompt 复跑字节一致)→ gen_accept 1 张 → Content/Textures/ 落资产 + .meta provenance 全字段(origin=gen-image, detail 含 backendId/prompt/seed/generatedAt)→ gen_texture_set 自动入管线(maps 各槽 assetPath);gen-model gen_accept 走 asset_import 同链(网格资产 + meshlet 构建产物)
  - id: G-F5-3
    name: 前端门
    check: 「生成 4 张候选木纹 → 接受 1 张 → 材质引用 → 场景可见」全链路走通,provenance 完整(13_ROADMAP F5 逐字)。机验:desktop 冒烟——Assets 右键生成 → 对话框 prompt=木纹 n=4 → 候选网格 4 张 → Accept 1 张 → Assets 列表出现资产(缩略图)→ material_create 引用其 GUID → 实体 MeshRenderer 绑材质 → viewport_frame 断言非零像素(场景可见);.meta provenance 断言;client vitest 全绿;设置页 generation tab 配置写入后端状态反映
guardrails:
  - 诚实优先:任何门不过如实报 FAIL/DEV_ENV_DEGRADE,不回写 PASS;无远程端点环境不伪造 remote 调用成功
  - 数字必须来自命令输出
  - 密钥红线 R-5:密钥值不落前端/项目文件/日志/事件;任何工具返回含密钥 = FAIL
  - provenance 强制 I-7:gen_accept 资产 .meta provenance 全字段,缺一 = FAIL
  - 双状态机:status/implementation_status 严格分离;契约 §6 只追加
---

# F5 契约:生成接入

## 1. 目标与双门状态

实现 13_ROADMAP F5:gen-image-mcp/gen-model-mcp 适配层 + 首个远程适配器;gen_accept 入管线 + provenance;Assets 右键「生成」入口 + 候选挑选 UX;设置页 generation tab;gen-asset-fill skill。status=active;implementation_status=unlocked。

## 2. 范围与波次

- wave.1:gen-image-mcp 骨架 + 后端注册表(local-mock + remote-openai-compatible)+ keystore + gen_backends_list/gen_image/gen_accept + f5-w1 冒烟 → G-F5-1。
- wave.2:gen_texture_set/gen_variations + gen-model-mcp 三工具(gen_accept 走 asset_import 链)+ provenance 结构化全字段 + f5-w2 冒烟 → G-F5-2。
- wave.3:Assets 生成对话框 + 候选挑选 modal + 设置页 generation tab + gen-asset-fill 摘 seam + desktop 冒烟 → G-F5-3。
- wave.4:全量回归 + close-out。

## 3. 架构决策(wave.1 立项裁决)

- **D-F5-A(local-mock 确定性适配器)**:开发/CI 环境无真实远程图像端点;验收门「配通一个后端」需要 CI 可复现的已配置后端。落地 local-mock 适配器:确定性 PNG 生成(seed + prompt 哈希驱动噪声/木纹条纹算法,同 seed 同 prompt 复跑字节一致),configured 恒 true(无需密钥),capabilities: text2img/texture-set/variations,sizes 256/512/1024。remote-openai-compatible 为真实 HTTP 适配面(端点+密钥配置后真实调用;未配置 = configured=false)。mock 不伪装远程:backendId=local-mock 如实进 provenance。
- **D-F5-B(双 server 双 crate)**:05 §7/§8 server 名两个(gen-image/gen-model);照 agentd ServerKind 族一 server 一 crate:gen-image-mcp 五工具,gen-model-mcp 三工具;后端注册表/keystore/临时产物目录逻辑进共享库 crate crates/gend(两 mcp 内嵌,照 assetd 双 mcp 先例——assetd 被 asset-pipeline-mcp 内嵌)。
- **D-F5-C(keystore 形态)**:12 §5 R-5「keystore 保管」未指定形态;落地 data/keystore.json(仅本地,.gitignore 已盖;env FORGE_GEN_API_KEY 优先于文件);文件权限加固(DPAPI/0600)登记 deferred;密钥值永不进工具返回(配置接口只回 configured: true)。
- **D-F5-D(provenance detail 结构化)**:assetd MetaDoc.Provenance 现 detail: Option<String>(人工导入/派生事实文本);08 §6.4 要求 detail { backendId, prompt, negativePrompt?, seed, sourceRefs[], generatedAt } 结构化。扩展 Provenance::Gen { backendId, prompt, negativePrompt, seed, sourceRefs, generatedAt } 与 Provenance::Text(String) 双形态(serde untagged 向后兼容已有 .meta);gen_accept 写 Gen 形态,人工导入/texture_process 沿用文本形态。
- **D-F5-E(gen-model gen_mesh seam)**:首个远程适配器仅图像(13 F5「首个」单数);gen_mesh/gen_mesh_refine 工具面落地但无可用后端(注册表不含 text2mesh 适配器)→ GEN_BACKEND_NOT_CONFIGURED 显式;gen_accept(meshFileRef 来自用户自备 gltf 放 .forge/tmp/gen/ 或后续真实后端)走 asset_import 同一构建链接地。

## 4. Deferred 处置
本波新增 deferred 追加于下方。

deferred:
  - id: RD-F5-001
    content: keystore 文件权限加固(DPAPI/0600)与 OS keychain 接入
    reason: F5 为 data/keystore.json 明文本地文件(.gitignore 覆盖,env 优先);R-5 满足(不进前端/项目/日志)但静态加固未做
    refill: Windows DPAPI 加密 keystore;macOS Keychain/Linux secret-service 适配
    owner: 安全加固波
    status: OPEN
  - id: RD-F5-002
    content: local-diffusers 本地推理适配器(08 §6.2 标注后置);img2img/styleRefAssetPath 真实消费
    reason: 首个远程适配器仅 remote-openai-compatible;local 推理面后置
    refill: 本地推理服务接入波;styleRef 消费随 img2img 适配器
    owner: 生成扩展波
    status: OPEN

## 5. 修订
- 2026-08-18 立项:F4 全绿(wave.1~5 §6,status=closed)后用户指令开工「启动 F5 生成接入开发,推进 gen-image/gen-model 与 Assets 右键集成」。现状盘点:assetd MetaDoc 有 provenance: Option<Provenance{origin,detail}> 人工导入已填 user-import;Assets 右键生成 = F2 wave.3 prefillChat seam;设置页 generation tab 占位(TAB_Landed F5);无 gen 任何代码痕迹;KNOWN_TOOLS 67。

## 6. Close-out(只追加区)

### wave.1 验收记录(2026-08-18)

- 验收门:G-F5-1(后端配置门)
- 结果:PASS
- 证据:
  - scripts/f5-w1-gen-backends-smoke.ps1 PASS(经 gateway→agentd→gen-image-mcp):**删除 gen-backends.json → gen_image/gen_texture_set/gen_variations 全返 GEN_BACKEND_NOT_CONFIGURED(I-5 逐字)**;gen_backends_list 两适配器(local-mock/remote-openai-compatible)configured=false 如实列;gen_accept 不依赖后端(fixture png → Textures/f5w1_fixture.png + guid);**配置写入 local-mock 热生效**(configured=true,kinds=text2img/texture-set/variations,sizes=256/512/1024);gen_image 木纹 n=4 seed=42 → 4 候选(seed=42..45 派生)+ **复跑 SHA256 逐字节一致**(D-F5-A 确定性);gen_accept → Textures/f5w1_wood.png,provenance 五字段全(origin=gen-image/backendId=local-mock/prompt 含木纹/seed=42/generatedAt);**密钥红线 R-5**:remote 条目 + keystore 假密钥 sk-test-SECRET-12345 → gen_backends_list 响应全文扫描无密钥子串;假端点 127.0.0.1:1 → 如实 GEN_BACKEND_ERROR(连接拒绝不伪装成功,错误串无密钥)
  - cargo test:gend 21(mock 确定性同 seed 字节一致/异 seed 异字节/config 判定三态/keystore env 优先/tmpstore/accept 全链临时项目)+ gen-image-mcp 3 + forge-agentd 35(KNOWN_TOOLS 67→72 + 五工具存在断言);workspace **172/172 全绿**(基线 148 + 新增 24)
- 交付:
  - crates/gend 共享库:backends.rs(GenBackend trait + 注册表)/mock.rs(确定性木纹条纹 PNG,注释如实标注非 AI 模型)/remote.rs(ureq POST /v1/images/generations,30s 超时,b64_json/url[];429→GEN_RATE_LIMITED,余→GEN_BACKEND_ERROR 只带状态码不回显密钥)/keystore.rs(env FORGE_GEN_API_KEY 优先;无 Serialize 出口,Debug 脱敏)/config.rs(gen-backends.json,**条目缺失=configured=false 诚实缺省**;FORGE_GEN_DATA_DIR 测试隔离)/tmpstore.rs(.forge/tmp/gen/ + sidecar 生成上下文)/accept.rs(staging 命名副本 → assetd import_assets 同链 → provenance origin=gen-image + 结构化 detail)/timeutil.rs(零依赖 ISO8601 UTC)
  - crates/mcp/gen-image-mcp:五工具逐字(05 §7);styleRefAssetPath 接受不消费(如实注释);texture-set 能力门(remote v1 仅 text2img 如实拒);variations 远程如实 seam
  - agentd:ServerKind::GenImage 第四槽 + mcp__gen-image__ 前缀 + env FORGE_GEN_IMAGE_MCP_BIN
  - scripts/f5-w1-gen-backends-smoke.ps1
- 踩坑:
  1. gend 跨模块测试各有 ENV_LOCK 互踩 PoisonError——lib.rs 单一 TEST_ENV_LOCK + 中毒容忍。
  2. PS Set-Content 改写 remote.rs 引入 BOM + 险 GBK 损坏中文——[IO.File] 无 BOM 重写 + Select-String 核验(IDE 坑变体,PS 写文件同样不守 BOM 纪律会炸)。
  3. accept staging 命名副本残留 tmp——冒烟 finally 补 f5w1_* 清理模式,复跑零残留。
- 如实登记:
  - gen_accept assetPath 采 assetd 族约定(相对 Content/,如 Textures/f5w1_wood.png)。
  - mock seamless=true 接受但 v1 不保证真无缝(schema 标注);remote variations/texture-set 为 seam(wave.2/RD-F5-002 承接)。

### wave.2 验收记录(2026-08-18)

- 验收门:G-F5-2(生成管线门)
- 结果:PASS
- 证据:
  - scripts/f5-w2-gen-pipeline-smoke.ps1 PASS(连跑两遍幂等;经 gateway→agentd→gen-image-mcp/gen-model-mcp):**gen_texture_set 自动入管线**——{prompt:"wood 木纹",pbr,maps:[albedo,normal,roughness],256} → 3 textureAssets 落 Content/Textures/wood_*,provenance origin=gen-image + detail.map 逐槽位;**gen_mesh 门**——{prompt:"a chair"} → GEN_BACKEND_NOT_CONFIGURED 显式不伪造(注册表无 text2mesh 适配器,D-F5-E),双空 GEN_BAD_PARAMS 参数校验先行;**gen-model gen_accept 走 asset_import 同一构建链(08 §6.3 D-009)**——tri_min.gltf 模拟生成产物 → Meshes/f5w2_chair.gltf + guid + **artifact=.forge/cache/rxmesh/69cac1…c85.rxmesh 真实落盘** + .meta 全字段(origin=gen-model/backendId=user-provided 如实标注/sourceRefs 含 meshFileRef/import_settings.generateLods 透传进缓存键);**确定性复跑**——f5w2_chair2:guid 不同(创建即定)但 cacheHit=true + 同 .rxmesh 产物(08 §4.3 缓存键)
  - cargo test:gend 22(+mesh 全链)+ gen-model-mcp 4(schema/双空 BAD_PARAMS/NOT_CONFIGURED/accept 全链)+ forge-agentd 35(KNOWN_TOOLS 72→75 + 三工具抽查);workspace **177/177 全绿**(基线 172 + 新增 5)
- 交付:
  - gend:accept.rs 泛化 accept_asset(origin 参数化,AcceptedAsset + artifact/cacheHit;gen_accept 保留 origin=gen-image 兼容包装);tmpstore.rs resolve_gen_ref 统一三标签(imageFileRef/meshFileRef/fileRef)
  - crates/mcp/gen-model-mcp:gen_mesh(双空 BAD_PARAMS → NOT_CONFIGURED)/gen_mesh_refine(meshFileRef 存在性 → NOT_CONFIGURED;retopo/LOD 注明走 gen_accept 构建链 DAG 派生 D-009)/gen_accept(origin=gen-model,backendId=user-provided 如实,sourceRefs=[meshFileRef],importSettings 透传,返回 {assetPath,guid,artifact,cacheHit} 超集)
  - agentd:ServerKind::GenModel 第五槽 + mcp__gen-model__ 前缀 + env FORGE_GEN_MODEL_MCP_BIN
  - scripts/f5-w2-gen-pipeline-smoke.ps1;fixture 留档:Content/Meshes/f5w2_chair(2).gltf + Content/Textures/wood_*(wave.3 场景可见复用)
- 踩坑:
  1. PS `"$m:"` 被当驱动器限定变量 → `${m}:`。
  2. tmp 清理模式:staging 副本为下划线命名(f5w2_chair.gltf),首版 f5w2-* 漏配 → 四模式补齐,复跑 tmp/gen 清零。

### wave.3 验收记录(2026-08-18)

- 验收门:G-F5-3(前端门)
- 结果:PASS
- 证据:
  - scripts/f5-w3-gen-ui-smoke.ps1 PASS(连跑 3 次幂等;经 gateway→agentd→gen-image-mcp):**「生成 4 张候选木纹 → 接受 1 张 → 材质引用 → 场景可见」全链走通**(13_ROADMAP F5 逐字)——gen_image {prompt:木纹,n:4} → 4 候选落 .forge/tmp/gen/ → gen_accept 1 张 → Content/Textures/wood-9425964566131575000.png + guid + .meta provenance 全字段断言(origin=gen-image/backendId=local-mock/prompt/seed/generatedAt)→ Assets 列表出现(缩略图)→ material_create 引用其 GUID(f5w3_gen_mat.rxmat)→ 实体 MeshRenderer 绑材质 → **viewport_frame nonZeroPixels=413>0(场景可见)**;desktop 冒烟截图留档 desktop-smoke-gen-2026-08-18T02-07-13-435Z.png(396376 B);**设置页 generation tab 配置写入 → 后端状态反映**(configure 写经 REST,backends 清单 configured=true)
  - pnpm -r test:**97/97 全绿**(client 73 + host 19 + protocol 5;F4 基线 87 + 新增 10:genStore 5 + assetsPanel 2 + settingsView generation + forgeProxy gen 前缀断言)
- 交付:
  - agentd gen REST 面(11 §2.2):GET /api/forge/gen/backends(密钥脱敏只回 configured 布尔)+ POST /api/forge/gen/backends/configure(写 data/gen-backends.json + keystore;api_key 只进 keystore 不落 config/响应/日志,R-5)
  - host forgeProxy PROXY_PREFIXES + /api/forge/gen(client 单源 3080;F3 settings 冒烟代理缺先例同型)
  - client:genStore + GenerateDialog(prompt/negativePrompt/size 256/512/1024/n=1..4/backend 下拉只列 configured)+ CandidatesModal(候选网格缩略图 + seed + Accept → gen_accept destFolder=当前文件夹)+ AssetsPanel 右键「生成」替换 F2 wave.3 prefillChat seam + SettingsView generation tab 真实化(backends 清单 id/kind/configured/capabilities + 端点/密钥配置表单,前端不见密钥值)
  - skills/gen-asset-fill 摘 seam(列缺口清单 → 生成候选 → 用户挑拣 → gen_accept 真实工具面,06 §3)
  - scripts/f5-w3-gen-ui-smoke.ps1
- 踩坑:
  1. host 未重建致 /api/forge/gen 404——pnpm --filter @forge/host build 后冒烟通过(desktop 经 host 代理;w1/w2 栈级冒烟直连 gateway 未暴露此缺,F3 同型先例)。
  2. demo 项目 F4 留档 .rxgraph 缺 .meta(door_opener/f4w3_probe)→ 补齐两 sidecar,资产扫描零 UNKNOWN_TYPE。

### wave.4 验收记录(2026-08-18):全量回归 + close-out 终审

- 全量回归数字(均来自命令输出):
  - cargo test --workspace **179/179 全绿**(exit=0;forge-agentd 37 / code-forge-mcp 30 / forge-logic 28 / engine-host 23 / gend 22 / assetd 18 / forge-scene 9 / gen-model-mcp 4 / gen-image-mcp 3 / 余 crate 套件 5;F4 基线 148 + F5 新增 31)
  - pnpm -r typecheck 全绿;pnpm -r test **97/97**(client 73 + host 19 + protocol 5);pnpm -r build 全绿
  - go build + go test(gateway-go)exit=0
  - 冒烟三链复跑全 PASS:f5-w1-gen-backends-smoke(G-F5-1)/ f5-w2-gen-pipeline-smoke(G-F5-2,复跑幂等 cacheHit=true 同 rxmesh 产物 69cac1…c85)/ f5-w3-gen-ui-smoke(G-F5-3,新截图 desktop-smoke-gen-2026-08-18T02-16-26-152Z.png 396,376 B;UI 候选 4;viewport_frame nonZeroPixels=413)
  - desktop 冒烟:home PASS(75,681 B)+ settings PASS(167,764 B,skills 13 条;前置 agentd 8103,F3 面回归)+ gen(f5-w3 脚本内)
- 三门终审:
  - G-F5-1 后端配置门:PASS(wave.1 §6)——无配置全工具面 GEN_BACKEND_NOT_CONFIGURED 显式(I-5);gen_backends_list 两适配器如实列 configured;密钥红线 R-5 响应/日志/事件扫描无密钥;假端点如实 GEN_BACKEND_ERROR 不伪装成功
  - G-F5-2 生成管线门:PASS(wave.2 §6)——local-mock n=4 确定性(同 seed 复跑 SHA256 逐字节一致,D-F5-A);gen_accept provenance 五字段全(I-7);gen_texture_set 三 map 自动入管线逐槽位 provenance;gen-model gen_accept 走 asset_import 同一构建链(rxmesh 真实落盘 + 缓存键命中,08 §6.3 D-009);KNOWN_TOOLS 67→75
  - G-F5-3 前端门:PASS(wave.3 §6)——「生成 4 张候选木纹 → 接受 1 张 → 材质引用 → 场景可见」全链走通(13_ROADMAP F5 逐字);.meta provenance 断言;设置页 generation tab 配置写读闭环(REST 经 host 代理单源 3080);client vitest 73/73
- 结论:**F5 三门全绿,里程碑 close-out。status: active → closed。**
- open RD 不阻收官(均有 refill 路径):RD-F5-001(keystore 权限加固 DPAPI/0600 + OS keychain)/ RD-F5-002(local-diffusers 本地推理适配器 + img2img/styleRef 消费);承前 deferred:RD-F4-004(call_function 互绑——下一 code-forge 波第一优先)/ RD-F1-002(真 LLM 工具循环)。
- 下一里程碑可选:**F6 试玩回归与打包**(13_ROADMAP F6)或 RD-F4-004 回填波或消化 open RD。
