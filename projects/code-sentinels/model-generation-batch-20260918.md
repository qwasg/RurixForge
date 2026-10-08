# Code Sentinels 模型资产 AI 生成与验收 · 2026-09-18

接续 [2026-09-16 批次记录](model-generation-batch-20260916.md)（当时 `meshy` 未配置，未生成任何模型）。本次 Meshy 已在 RurixForge 设置页启用并配置 API Key（`GET /api/forge/gen/backends` 报 `meshy configured=true`），全部生成经 RurixForge `forge-agentd` 的 `POST /api/forge/gen/mesh`（gend `meshy` 适配器）完成，产物经 `mcp__gen-model__gen_accept` 入管线。工作区 `ws_1788669812422_35134176`。

## 结论

- **60 个模型已生成并入库**：`Content/Models/v6-meshy/<id>.glb` + `.meta`（provenance `origin: gen-model`，含 Meshy taskId / aiModel / consumedCredits / prompt），并经 assetd 导入链构建出 `.forge/cache/rxmesh/*.rxmesh`（60 个，`build_state: current`）。
- **验收**：60/60 通过 GLB 结构校验（含贴图、法线、UV、PBR 贴图组、无非常规扩展）；60/60 通过 Blender 5.2 严格 glTF 导入并完成同机位四向渲染；53 个 PASS、7 个 WARN（色调偏离粗模、需人工过目，无几何缺陷）、0 个 FAIL。两个首版几何/贴图不合格的模型（`depot`、`secure-switch`）已重生成并替换，首版留档在 `pipeline/v6/meshy-batch-20260918/rejected/`。
- **额度**：Meshy 余额 1000 → 40 credits。60 个入库模型 900 + 2 个被替换的首版 30 + 2 个因轮询 TLS 瞬断而丢失的任务 30 = 960。
- **未做**：94 个粗模中 30 个按设计保留粗模（15 个结构套件 + 15 个插件卡，理由见下）；4 个与已生成模型几何完全相同的孪生单位（`interceptor`/`anti-air`/`engineer-drone`/`resource-hauler`）因余额不足 60 credits 未生成。粗模及其 8 向 sprite bake 均未改动，本批未重做 sprite bake。

## 范围与取舍（预算 1000 credits）

Meshy 2026 定价：Smart Topology（`meshy-t2`）image-to-3D 带贴图 15 credits/次；Meshy-7 标准档 30 credits/次；text-to-3D preview+refine 30（Meshy-6/7）或 15（T2）。94 个粗模全部按 15 credits 需 1410，超出余额，因此：

| 类别 | 数量 | 处置 | 理由 |
|---|---|---|---|
| 单位（炮台/坦克/飞机/车辆） | 33 | 29 生成，4 孪生未生成 | 主体资产；`interceptor`=`autocannon`、`anti-air`=`aa-turret`、`engineer-drone`=`micro-drone`、`resource-hauler`=`cargo-truck` 粗模几何逐字节相同，可先共用底盘 |
| 设施/道具（建筑、能源、节点、矿脉） | 31 | 31 全部生成 | 主体资产 |
| 结构套件 `foundation/floor/roof/roof-corner/wall/window-wall/column/stairs/elevator/door/cuda-wall/physical-wall/ramp/bridge/moat` | 15 | 保留粗模 | 必须保证 1×1 格精确尺寸与拼接边缘，AI 网格无法保证；平板/墙体粗模本身已是终态 |
| 插件卡 `plugin-{speed,security,algorithm,science,lightweight}-{core,attack,support}` | 15 | 保留粗模 | 3 种几何 × 5 种配色的近似重复件，用作卡牌图标，不值 225 credits |

## 流程

1. **参考图**（`pipeline/v6/render_meshy_refs.py`）：Blender 5.2 后台打开每个 `Content/Models/v6/<id>.blend` 粗模，沿用其中保存的正交 30° bake 相机与灯光，旋转到原 bake 的 `se` 方向（前右 3/4 视角，root z=-90°）渲染 1024px 透明底 PNG → `pipeline/v6/meshy-batch-20260918/refs/`。`vscode`/`pycharm` 渲染时隐藏 `unmodified official icon` 三块品牌面片，避免 AI 重绘官方图标；品牌需在引擎内以贴花方式叠加。总览：`refs-overview.png`。
2. **生成**（`pipeline/v6/meshy_generate.py run`，清单 `pipeline/v6/meshy-manifest.json`）：`POST /api/forge/gen/mesh`，`backend=meshy`，`modelType=smart-topology`（`ai_model=meshy-t2`），`imageDataUrl`=参考图，`targetPolycount` 8000（小道具 6000），`texture=true`、`pbr=true`、`textureResolution=2k`、`timeoutSec=1800`；5 并发，单个任务 85–230 s。预算守卫按已消耗 + 在途 × 15 与实时余额双重钳制。
   - 贴图引导策略：试点与前 14 个模型把清单里的材质描述作为 `texture_prompt`（`prompt` 引导）；其中混凝土底座炮台（`pycharm`、`vscode`、`command-core`）出现整体偏白/偏黑的失真，遂对比试验 `plasma-cannon`/`micro-drone` 改为**不发文本、由 Meshy 直接按参考图取色**（`image` 引导），色调与粗模一致度明显更高，其后 46 个模型全部采用 `image` 引导。两种引导各自对应的模型见结果表「贴图引导」列。
   - 中断恢复：第一批在切换策略时被中止，服务端 5 个在途任务由 forge-agentd 的阻塞任务继续完成并落盘 `.forge/tmp/gen/`，用 `meshy_generate.py recover` 按 sidecar 里的 prompt 匹配认领（`fighter`、`aa-turret`、`missile-truck`），零额度浪费。
3. **结构校验**（`meshy_generate.py validate`）：解析 GLB 头/JSON 块，判据：≥1 个网格与图元；三角面 ≥500 且不超目标 3 倍；含 `NORMAL` 与 `TEXCOORD_0`；≥1 个材质带 `baseColorTexture`；内嵌贴图 ≥1 张且 ≥10 KB；`metallicRoughnessTexture` 与 `normalTexture` 齐备（否则 WARN）；包围盒非退化、长宽比 ≤25；`extensionsRequired` 不含未知扩展；Meshy 四视图预览齐全。结果：三角面 5454–8885（中位数 8156），每模型 1 材质 4 张 2K 贴图，GLB 平均 5.4 MB（合计 326 MB），无 `extensionsRequired`。
4. **引擎入库**（`gen_accept`，默认 assetd 导入链）：`Content/Models/v6-meshy/<id>.glb` + `.meta`（provenance 含 `backendId: meshy`、`taskId`、`aiModel: meshy-t2`、`modelType: smart-topology`、`consumedCredits: 15`、`targetPolycount`、prompt）+ `.rxmesh` 构建产物；`asset_list` 识别为 `type: mesh` 并分配 GUID。单个入库 1.3–2.5 s。
5. **独立导入 + 同机位对比渲染**（`pipeline/v6/render_meshy_iso.py`）：用 Blender 严格 glTF 导入器导入每个入库 GLB（60/60 成功，材质与 4 张贴图均被识别），按粗模最大水平占地归一化尺寸、落地到 z=0，用与 `bake_models.py` 完全相同的相机与灯光渲染 s/w/n/e 四向 → `iso/<id>.png`（左侧为粗模 `se` bake，右侧四格为 AI 模型）。**Meshy 输出为单位化尺寸**（最大边 ≈1.0），与粗模匹配所需缩放系数 1.10–2.73 逐模型记录在 `iso/iso-index.json`（`normalizeScale`），进引擎场景时需按此缩放。
6. **色调偏差指标**（`meshy_generate.py palette`）：同灯光下粗模四向 bake 与 AI 模型四向渲染的 alpha 加权平均色比较。ΔL（亮度差）|ΔL|>0.15 记 WARN「palette drift, review」，>0.25 记「strong palette drift」；覆盖比（剪影像素占比 AI/粗模）超出 0.6–1.7 记 WARN。全批 |ΔL| 平均 0.08。
7. **目视抽检**：每模型一张验收拼图 `sheets/<id>.png`（粗模参考 + Meshy front/right/back/left + 同机位对比条 + 校验数据），总览 `overview-front-views.png`。逐张查看了 light-tank、vscode、pycharm、command-core、autocannon、pulse-turret、fighter、aa-turret、missile-truck、scout-buggy、plasma-cannon、micro-drone、rail-tank、cargo-aircraft、strategic-node、launchpad、airfield、depot、secure-switch、drainage-pump、drilling-rig、wind-power、network-defense、generator，其余按总览图核对轮廓。

## 目视结论与重试

- 几何整体忠实于粗模：底盘/炮塔/炮管/履带/机翼/发动机舱/塔架等主要部件齐全，T2 输出的分件干净。
- **重生成 2 个（各 15 credits）**：
  - `depot` v1 把 2×2 四个补给箱融合成两个偏大的浅蓝箱体 → 参考图按 alpha 包围盒裁切放大到 1024（`cropRef`），v2 得到四个独立带铜带的箭体，已替换。
  - `secure-switch` v1 只是一个无细节的浅色盒子（`se` 视角看到的是空白背面）→ 改用 `nw` 视角（`refAngle=-270`）并裁切，v2 出现刀片、蓝色加密端口与铜色防拆锁，已替换。
  - 首版 GLB 与四视图留在 `rejected/`，结果 JSON 存为 `results/<id>.v1.json`。
- **7 个 WARN 仅为色调偏离**（几何合格，建议人工过目决定是否用剩余额度重出）：`drilling-rig`（+0.29，底座变浅灰）、`drainage-pump`（+0.20）、`wind-power`（+0.20，塔身浅灰 vs 粗模深色机舱）、`generator`（+0.18）、`extractor`（+0.15）、`network-defense`（−0.17）、`logistics-belt`（−0.15）。
- `prompt` 引导的 14 个模型里，`pycharm`（+0.10，炮塔偏白）与 `vscode`（−0.09，炮塔偏黑高光）色调与统一调色板差距最明显，但仍在阈值内，未标 WARN。
- 参考视角带来的局限：细节面朝 −y 的设施（`rack` 的 GPU 刀片、`network-defense`/`generator`/`missile-silo` 的状态面板）在 `se` 视角下不可见，AI 补出的背面为素面；如需这些细节，可像 `secure-switch` 一样以 `refAngle=-270` 重出。

## 结果表

ΔL / ΔRGB / 覆盖比为同机位对比指标（见流程第 6 步）；「Blender 导入」列的 `×N` 为归一化到粗模占地所需缩放。

| # | 模型 | 分组 | 状态 | 校验 | 三角面 | 顶点 | 贴图 | GLB | 贴图引导 | Blender 导入 | ΔL | ΔRGB | 覆盖比 | credits | Meshy task | 入库路径 | 备注 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | `light-tank` | unit | 已入库 | PASS | 6237 | 5960 | 4 | 6.2 MB | prompt | OK 6237 tris/4 img, ×2.4357 | -0.02 | 0.04 | 1.148 | 15 | `01a0b455-8b16-76cd-bd14-8a4673a17017` | `Content/Models/v6-meshy/light-tank.glb` |  |
| 2 | `command-core` | facility | 已入库 | PASS | 8042 | 7954 | 4 | 5.2 MB | prompt | OK 8042 tris/4 img, ×1.5 | -0.05 | 0.09 | 0.955 | 15 | `01a0b459-132d-7620-93fb-1a84e6a235b6` | `Content/Models/v6-meshy/command-core.glb` |  |
| 3 | `vscode` | unit | 已入库 | PASS | 8730 | 7688 | 4 | 5.7 MB | prompt | OK 8730 tris/4 img, ×2.0707 | -0.09 | 0.17 | 1.317 | 15 | `01a0b459-1339-74ab-9b6f-ad570fc28a02` | `Content/Models/v6-meshy/vscode.glb` | 品牌面片已隐藏，需引擎内贴花 |
| 4 | `pycharm` | unit | 已入库 | PASS | 8530 | 7891 | 4 | 5.3 MB | prompt | OK 8530 tris/4 img, ×1.5827 | +0.10 | 0.17 | 1.21 | 15 | `01a0b459-1338-7780-82c8-a4df154b62c3` | `Content/Models/v6-meshy/pycharm.glb` | 品牌面片已隐藏；炮塔偏白 |
| 5 | `autocannon` | unit | 已入库 | PASS | 8673 | 8764 | 4 | 5.5 MB | prompt | OK 8673 tris/4 img, ×2.0707 | -0.09 | 0.15 | 1.41 | 15 | `01a0b459-132c-75e0-8299-df9e7bd354cc` | `Content/Models/v6-meshy/autocannon.glb` |  |
| 6 | `mortar` | unit | 已入库 | PASS | 7574 | 7706 | 4 | 5.8 MB | prompt | OK 7574 tris/4 img, ×1.5827 | -0.03 | 0.06 | 1.054 | 15 | `01a0b459-133f-726a-9e5f-020e8c29ad77` | `Content/Models/v6-meshy/mortar.glb` |  |
| 7 | `pulse-turret` | unit | 已入库 | PASS | 8202 | 8111 | 4 | 6.4 MB | prompt | OK 8202 tris/4 img, ×1.9 | -0.09 | 0.15 | 1.217 | 15 | `01a0b45b-ef62-7300-98fb-7a024082d7c9` | `Content/Models/v6-meshy/pulse-turret.glb` |  |
| 8 | `shield-tank` | unit | 已入库 | PASS | 6339 | 6364 | 4 | 6.4 MB | prompt | OK 6339 tris/4 img, ×2.4357 | +0.04 | 0.07 | 1.157 | 15 | `01a0b45c-0ce9-72a7-a7f1-4843086ff81c` | `Content/Models/v6-meshy/shield-tank.glb` |  |
| 9 | `artillery` | unit | 已入库 | PASS | 7506 | 7419 | 4 | 7.2 MB | prompt | OK 7506 tris/4 img, ×1.9791 | -0.03 | 0.05 | 0.888 | 15 | `01a0b45c-42f3-75c1-9599-f334d19c21d4` | `Content/Models/v6-meshy/artillery.glb` |  |
| 10 | `laser-tank` | unit | 已入库 | PASS | 7653 | 7707 | 4 | 6.1 MB | prompt | OK 7653 tris/4 img, ×2.265 | +0.03 | 0.04 | 1.102 | 15 | `01a0b45c-781c-724c-aba6-395667c38a1c` | `Content/Models/v6-meshy/laser-tank.glb` |  |
| 11 | `scout-buggy` | unit | 已入库 | PASS | 7180 | 6629 | 4 | 4.8 MB | prompt | OK 7180 tris/4 img, ×2.3457 | -0.07 | 0.13 | 1.317 | 15 | `01a0b45c-9b39-76f3-a27b-541a9564b1fb` | `Content/Models/v6-meshy/scout-buggy.glb` |  |
| 12 | `fighter` | unit | 已入库 | PASS | 7864 | 8237 | 4 | 6.6 MB | prompt | OK 7864 tris/4 img, ×2.4 | -0.07 | 0.12 | 1.105 | 15 | `01a0b45e-6aa0-7010-9d2a-fffcd2244990` | `Content/Models/v6-meshy/fighter.glb` | recover 认领 |
| 13 | `aa-turret` | unit | 已入库 | PASS | 7827 | 7864 | 4 | 5.8 MB | prompt | OK 7827 tris/4 img, ×1.6636 | -0.07 | 0.13 | 1.014 | 15 | `01a0b45e-be0b-7424-94db-a21dc0de9f21` | `Content/Models/v6-meshy/aa-turret.glb` | recover 认领 |
| 14 | `missile-truck` | unit | 已入库 | PASS | 8000 | 7640 | 4 | 4.9 MB | prompt | OK 8000 tris/4 img, ×1.96 | -0.02 | 0.05 | 0.871 | 15 | `01a0b45e-da9a-75b5-92d7-2c4fee772261` | `Content/Models/v6-meshy/missile-truck.glb` | recover 认领 |
| 15 | `plasma-cannon` | unit | 已入库 | PASS | 8267 | 8152 | 4 | 5.5 MB | image | OK 8267 tris/4 img, ×2.08 | -0.11 | 0.19 | 1.189 | 15 | `01a0b462-dd68-745d-852c-3918699f998f` | `Content/Models/v6-meshy/plasma-cannon.glb` | image 引导对比试验 |
| 16 | `micro-drone` | unit | 已入库 | PASS | 8427 | 6853 | 4 | 6.5 MB | image | OK 8427 tris/4 img, ×1.88 | -0.09 | 0.16 | 1.019 | 15 | `01a0b462-dd69-7009-aaf2-72f973000739` | `Content/Models/v6-meshy/micro-drone.glb` | image 引导对比试验 |
| 17 | `rail-tank` | unit | 已入库 | PASS | 8490 | 8076 | 4 | 7.3 MB | image | OK 8490 tris/4 img, ×2.545 | -0.06 | 0.12 | 1.129 | 15 | `01a0b466-766b-75e8-919c-e55a9712959d` | `Content/Models/v6-meshy/rail-tank.glb` |  |
| 18 | `fortress-tank` | unit | 已入库 | PASS | 7722 | 7590 | 4 | 7.3 MB | image | OK 7722 tris/4 img, ×1.9791 | +0.07 | 0.12 | 0.876 | 15 | `01a0b466-767b-71f1-9894-3a633004543e` | `Content/Models/v6-meshy/fortress-tank.glb` |  |
| 19 | `siege-launcher` | unit | 已入库 | PASS | 8063 | 8243 | 4 | 6.1 MB | image | OK 8063 tris/4 img, ×1.6571 | +0.00 | 0.01 | 1.037 | 15 | `01a0b466-7668-7614-bea9-d08274964877` | `Content/Models/v6-meshy/siege-launcher.glb` |  |
| 20 | `particle-cannon` | unit | 已入库 | PASS | 8136 | 7999 | 4 | 6.6 MB | image | OK 8136 tris/4 img, ×2.17 | -0.04 | 0.07 | 1.187 | 15 | `01a0b466-7654-76df-b83f-cd5e0b2e7aad` | `Content/Models/v6-meshy/particle-cannon.glb` |  |
| 21 | `loiter-drone` | unit | 已入库 | PASS | 8413 | 8632 | 4 | 6.1 MB | image | OK 8413 tris/4 img, ×2.4 | -0.14 | 0.24 | 1.124 | 15 | `01a0b466-7658-7784-85ef-514c7505be6e` | `Content/Models/v6-meshy/loiter-drone.glb` |  |
| 22 | `aerospace-fighter` | unit | 已入库 | PASS | 6319 | 7004 | 4 | 6.4 MB | image | OK 6319 tris/4 img, ×2.4 | -0.02 | 0.06 | 1.078 | 15 | `01a0b467-cd01-7234-8212-26d3c0a00e71` | `Content/Models/v6-meshy/aerospace-fighter.glb` |  |
| 23 | `aegis-array` | unit | 已入库 | PASS | 8330 | 6477 | 4 | 6.1 MB | image | OK 8330 tris/4 img, ×1.608 | -0.11 | 0.20 | 0.971 | 15 | `01a0b467-fd16-720c-8d34-dfe12cf2151a` | `Content/Models/v6-meshy/aegis-array.glb` |  |
| 24 | `precision-strike` | unit | 已入库 | PASS | 7377 | 6996 | 4 | 5.8 MB | image | OK 7377 tris/4 img, ×1.6571 | -0.02 | 0.03 | 1.075 | 15 | `01a0b468-101a-76b5-af00-f50443549ec8` | `Content/Models/v6-meshy/precision-strike.glb` |  |
| 25 | `orbital-lance` | unit | 已入库 | PASS | 6593 | 6326 | 4 | 6.2 MB | image | OK 6593 tris/4 img, ×1.5564 | -0.10 | 0.18 | 0.94 | 15 | `01a0b468-3395-764e-a49a-6a0944d8209d` | `Content/Models/v6-meshy/orbital-lance.glb` |  |
| 26 | `distributed-array` | unit | 已入库 | PASS | 8657 | 7658 | 4 | 5.9 MB | image | OK 8657 tris/4 img, ×1.88 | -0.12 | 0.21 | 1.114 | 15 | `01a0b468-3c4a-759f-bf6e-f2ddbf39d7fc` | `Content/Models/v6-meshy/distributed-array.glb` |  |
| 27 | `stealth-wing` | unit | 已入库 | PASS | 7683 | 8984 | 4 | 5.1 MB | image | OK 7683 tris/4 img, ×2.4 | +0.05 | 0.07 | 1.048 | 15 | `01a0b469-2ee6-731b-8cde-fe330a178201` | `Content/Models/v6-meshy/stealth-wing.glb` |  |
| 28 | `cargo-truck` | unit | 已入库 | PASS | 8731 | 7242 | 4 | 3.7 MB | image | OK 8731 tris/4 img, ×2.0595 | -0.07 | 0.12 | 0.929 | 15 | `01a0b469-fb9c-734b-a470-e40dacaabf5e` | `Content/Models/v6-meshy/cargo-truck.glb` |  |
| 29 | `cargo-aircraft` | unit | 已入库 | PASS | 6338 | 6381 | 4 | 5.7 MB | image | OK 6338 tris/4 img, ×2.7342 | -0.04 | 0.07 | 1.08 | 15 | `01a0b46a-0edf-757f-85fd-f9a8bb84b9f9` | `Content/Models/v6-meshy/cargo-aircraft.glb` |  |
| 30 | `breach-tank` | unit | 已入库 | PASS | 7973 | 7625 | 4 | 6.9 MB | image | OK 7973 tris/4 img, ×1.9791 | +0.07 | 0.11 | 0.857 | 15 | `01a0b46a-2577-7734-8213-d3a6dad28727` | `Content/Models/v6-meshy/breach-tank.glb` |  |
| 31 | `rack` | facility | 已入库 | PASS | 6544 | 7894 | 4 | 4.4 MB | image | OK 6544 tris/4 img, ×1.4477 | -0.08 | 0.13 | 1.083 | 15 | `01a0b46a-c3aa-731b-be80-1c090e1a1715` | `Content/Models/v6-meshy/rack.glb` | GPU 刀片面朝 −y，参考图未见 |
| 32 | `research-console` | facility | 已入库 | PASS | 5909 | 6159 | 4 | 4.4 MB | image | OK 5909 tris/4 img, ×1.2421 | -0.08 | 0.14 | 1.02 | 15 | `01a0b46c-4062-728d-a0f6-c421973ee294` | `Content/Models/v6-meshy/research-console.glb` |  |
| 33 | `depot` | facility | 已入库 | PASS | 6386 | 6819 | 4 | 4.1 MB | image | OK 6386 tris/4 img, ×1.12 | +0.05 | 0.10 | 0.81 | 15 | `01a0b47f-903e-7293-8124-0db1cc552443` | `Content/Models/v6-meshy/depot.glb` | v2（裁切参考图）；v1 见 rejected/ |
| 34 | `generator` | facility | 已入库 | WARN | 8624 | 10143 | 4 | 4.7 MB | image | OK 8624 tris/4 img, ×1.2 | +0.18 | 0.31 | 0.777 | 15 | `01a0b46c-cf00-77e2-b2dd-a352d36e83a2` | `Content/Models/v6-meshy/generator.glb` | palette drift +0.18 luminance (lighter than authored palette, review) |
| 35 | `wind-power` | facility | 已入库 | WARN | 8176 | 8373 | 4 | 4.3 MB | image | OK 8176 tris/4 img, ×1.9713 | +0.20 | 0.34 | 0.695 | 15 | `01a0b46d-3b10-734e-badd-222384df1a88` | `Content/Models/v6-meshy/wind-power.glb` | palette drift +0.20 luminance (lighter than authored palette, review) |
| 36 | `hydro-power` | facility | 已入库 | PASS | 8408 | 7042 | 4 | 4.6 MB | image | OK 8408 tris/4 img, ×1.4 | -0.03 | 0.06 | 0.732 | 15 | `01a0b46d-4212-76b5-bf17-8868837ae311` | `Content/Models/v6-meshy/hydro-power.glb` |  |
| 37 | `coal-power` | facility | 已入库 | PASS | 8744 | 6988 | 4 | 5.0 MB | image | OK 8744 tris/4 img, ×1.5533 | -0.10 | 0.17 | 0.825 | 15 | `01a0b46d-d8cc-7726-8a8d-430fd0b598ff` | `Content/Models/v6-meshy/coal-power.glb` |  |
| 38 | `nuclear-power` | facility | 已入库 | PASS | 8399 | 7907 | 4 | 5.3 MB | image | OK 8399 tris/4 img, ×1.38 | -0.00 | 0.01 | 0.693 | 15 | `01a0b46e-6bf9-74f0-92e7-d5235693e14e` | `Content/Models/v6-meshy/nuclear-power.glb` |  |
| 39 | `extractor` | facility | 已入库 | WARN | 8433 | 6604 | 4 | 3.4 MB | image | OK 8433 tris/4 img, ×1.2164 | +0.15 | 0.27 | 1.005 | 15 | `01a0b470-60f4-76c6-b10e-7613d97212b5` | `Content/Models/v6-meshy/extractor.glb` | palette drift +0.15；首次任务因 TLS 瞬断丢失后重建 |
| 40 | `mobile-relay` | unit | 已入库 | PASS | 7473 | 6806 | 4 | 4.3 MB | image | OK 7473 tris/4 img, ×2.0595 | -0.11 | 0.19 | 0.885 | 15 | `01a0b46e-b7ff-7510-b759-65378abe2157` | `Content/Models/v6-meshy/mobile-relay.glb` |  |
| 41 | `factory` | facility | 已入库 | PASS | 8885 | 8804 | 4 | 5.1 MB | image | OK 8885 tris/4 img, ×1.12 | -0.07 | 0.12 | 0.949 | 15 | `01a0b46f-10b5-7784-88be-af421d7bb830` | `Content/Models/v6-meshy/factory.glb` |  |
| 42 | `network-defense` | facility | 已入库 | WARN | 8395 | 7336 | 4 | 4.3 MB | image | OK 8395 tris/4 img, ×1.3516 | -0.17 | 0.30 | 0.906 | 15 | `01a0b470-60bb-7672-8e63-80bf95ec9cfe` | `Content/Models/v6-meshy/network-defense.glb` | palette drift -0.17；首次任务因 TLS 瞬断丢失后重建 |
| 43 | `energy-defense` | facility | 已入库 | PASS | 6924 | 4839 | 4 | 5.0 MB | image | OK 6924 tris/4 img, ×1.2034 | +0.01 | 0.07 | 0.875 | 15 | `01a0b470-30c4-7553-a9e1-b55622716307` | `Content/Models/v6-meshy/energy-defense.glb` |  |
| 44 | `repair-bay` | facility | 已入库 | PASS | 8826 | 8000 | 4 | 5.4 MB | image | OK 8826 tris/4 img, ×1.7 | +0.04 | 0.07 | 0.659 | 15 | `01a0b470-9bae-7299-97c4-9c16ea43c9fe` | `Content/Models/v6-meshy/repair-bay.glb` |  |
| 45 | `orbital-control` | facility | 已入库 | PASS | 8570 | 8947 | 4 | 5.7 MB | image | OK 8570 tris/4 img, ×1.8308 | -0.10 | 0.19 | 1.133 | 15 | `01a0b471-16af-763d-a024-b67d40f00325` | `Content/Models/v6-meshy/orbital-control.glb` |  |
| 46 | `airfield` | facility | 已入库 | PASS | 8539 | 6388 | 4 | 4.9 MB | image | OK 8539 tris/4 img, ×2.5 | +0.07 | 0.13 | 0.994 | 15 | `01a0b472-070f-7304-9c16-7a39e6dd3700` | `Content/Models/v6-meshy/airfield.glb` |  |
| 47 | `missile-silo` | facility | 已入库 | PASS | 8476 | 6182 | 4 | 4.0 MB | image | OK 8476 tris/4 img, ×1.6525 | +0.07 | 0.12 | 0.913 | 15 | `01a0b472-30d8-75ed-b13a-0f4a6df84587` | `Content/Models/v6-meshy/missile-silo.glb` |  |
| 48 | `ammunition-workshop` | facility | 已入库 | PASS | 8463 | 7376 | 4 | 5.2 MB | image | OK 8463 tris/4 img, ×1.2 | -0.13 | 0.23 | 0.972 | 15 | `01a0b472-4ce7-7794-9273-d7b66d78bcaa` | `Content/Models/v6-meshy/ammunition-workshop.glb` |  |
| 49 | `drainage-pump` | facility | 已入库 | WARN | 8254 | 7873 | 4 | 4.8 MB | image | OK 8254 tris/4 img, ×1.52 | +0.20 | 0.34 | 1.043 | 15 | `01a0b472-759c-722f-8417-2b069ea8750c` | `Content/Models/v6-meshy/drainage-pump.glb` | palette drift +0.20 luminance (lighter than authored palette, review) |
| 50 | `drilling-rig` | facility | 已入库 | WARN | 7786 | 8256 | 4 | 4.4 MB | image | OK 7786 tris/4 img, ×1.1 | +0.29 | 0.51 | 0.713 | 15 | `01a0b472-fe60-7399-81cd-24af9e701d8a` | `Content/Models/v6-meshy/drilling-rig.glb` | strong palette drift +0.29 luminance (lighter than authored palette) |
| 51 | `radar-array` | facility | 已入库 | PASS | 7697 | 6797 | 4 | 3.9 MB | image | OK 7697 tris/4 img, ×1.4582 | -0.11 | 0.19 | 0.898 | 15 | `01a0b473-c80b-708b-bbe0-d6a25ab0411f` | `Content/Models/v6-meshy/radar-array.glb` |  |
| 52 | `logistics-belt` | facility | 已入库 | WARN | 8713 | 8109 | 4 | 6.2 MB | image | OK 8713 tris/4 img, ×1.48 | -0.15 | 0.25 | 1.109 | 15 | `01a0b473-da03-7212-9420-c5f34b436119` | `Content/Models/v6-meshy/logistics-belt.glb` | palette drift -0.15 luminance (darker than authored palette, review) |
| 53 | `secure-switch` | facility | 已入库 | PASS | 5454 | 6053 | 4 | 4.4 MB | image | OK 5454 tris/4 img, ×1.3674 | +0.07 | 0.14 | 0.925 | 15 | `01a0b47f-9057-7777-afee-85ac6570fd7a` | `Content/Models/v6-meshy/secure-switch.glb` | v2（nw 视角 + 裁切）；v1 见 rejected/ |
| 54 | `fire-control` | facility | 已入库 | PASS | 7434 | 7489 | 4 | 5.9 MB | image | OK 7434 tris/4 img, ×1.2133 | -0.08 | 0.13 | 0.918 | 15 | `01a0b475-0008-76de-8eed-0ebe87af56f6` | `Content/Models/v6-meshy/fire-control.glb` |  |
| 55 | `particle-foundry` | facility | 已入库 | PASS | 8834 | 7362 | 4 | 6.1 MB | image | OK 8834 tris/4 img, ×1.11 | +0.12 | 0.20 | 0.767 | 15 | `01a0b475-6d3a-7155-a425-64de87a4fa21` | `Content/Models/v6-meshy/particle-foundry.glb` |  |
| 56 | `modular-workshop` | facility | 已入库 | PASS | 8428 | 8384 | 4 | 5.5 MB | image | OK 8428 tris/4 img, ×1.2 | -0.03 | 0.05 | 0.833 | 15 | `01a0b476-3196-7601-a4fb-0e9edd0d8596` | `Content/Models/v6-meshy/modular-workshop.glb` |  |
| 57 | `launchpad` | facility | 已入库 | PASS | 8794 | 9100 | 4 | 5.7 MB | image | OK 8794 tris/4 img, ×2.0 | +0.09 | 0.16 | 0.922 | 15 | `01a0b476-a260-7480-af7e-f5f7a4328563` | `Content/Models/v6-meshy/launchpad.glb` |  |
| 58 | `strategic-node` | facility | 已入库 | PASS | 8367 | 7469 | 4 | 5.7 MB | image | OK 8367 tris/4 img, ×1.86 | +0.02 | 0.07 | 0.931 | 15 | `01a0b476-b343-7427-b7e4-0903b40a0960` | `Content/Models/v6-meshy/strategic-node.glb` |  |
| 59 | `ore-node` | facility | 已入库 | PASS | 5899 | 6056 | 4 | 5.9 MB | image | OK 5899 tris/4 img, ×2.047 | -0.04 | 0.07 | 1.129 | 15 | `01a0b477-34e3-7631-8f53-ad7ce324acca` | `Content/Models/v6-meshy/ore-node.glb` |  |
| 60 | `coal-node` | facility | 已入库 | PASS | 6607 | 5693 | 4 | 4.4 MB | image | OK 6607 tris/4 img, ×1.9681 | +0.14 | 0.23 | 1.086 | 15 | `01a0b477-ae2e-73a5-9149-fe926af06ca6` | `Content/Models/v6-meshy/coal-node.glb` |  |
| 61 | `interceptor` | unit | 未运行 |  |  |  |  |  |  |  |  |  |  |  |  |  | 与 `autocannon` 同几何；余额不足 |
| 62 | `anti-air` | unit | 未运行 |  |  |  |  |  |  |  |  |  |  |  |  |  | 与 `aa-turret` 同几何；余额不足 |
| 63 | `engineer-drone` | unit | 未运行 |  |  |  |  |  |  |  |  |  |  |  |  |  | 与 `micro-drone` 同几何；余额不足 |
| 64 | `resource-hauler` | unit | 未运行 |  |  |  |  |  |  |  |  |  |  |  |  |  | 与 `cargo-truck` 同几何；余额不足 |

被替换的首版（额度已消耗，产物留档）：`depot` v1 task `01a0b46c-c776-71c3-a14c-6220d23e3233`，`secure-switch` v1 task `01a0b474-3306-7614-bccd-3380a1952048`。因轮询 TLS 瞬断而丢失（Meshy 侧已建任务并计费，产物未取回）：`extractor` task `01a0b46e-a811-7089-ba06-76af7f3f2072`，`network-defense` task `01a0b46f-a9ca-702f-84d8-c862510c5251`；两者均可在 Meshy 控制台查看。

## 产物与证据

| 路径 | 内容 |
|---|---|
| `Content/Models/v6-meshy/<id>.glb` + `.meta` | 60 个入库模型（provenance origin=gen-model） |
| `.forge/cache/rxmesh/*.rxmesh` | 引擎网格构建产物 |
| `.forge/tmp/gen/gen-*.glb` + `.json` + `.{front,right,back,left}.png` | 原始候选、sidecar、Meshy 四视图（含未被采用的首版） |
| `pipeline/v6/meshy-manifest.json` | 64 模型清单：分组、优先级、材质描述、贴图引导、参考视角/裁切覆盖、跳过清单与理由 |
| `pipeline/v6/meshy_generate.py` | 生成/恢复/校验/入库/色调指标/拼图/报告脚本（`balance` 子命令查余额，密钥不落日志） |
| `pipeline/v6/render_meshy_refs.py` | Blender 参考图渲染 |
| `pipeline/v6/render_meshy_iso.py` | Blender 严格导入 + 同机位四向验收渲染 |
| `pipeline/v6/meshy-batch-20260918/refs/` | 64 张 1024px 参考图（+2 张裁切版）、`refs-index.json` |
| `pipeline/v6/meshy-batch-20260918/results/<id>.json` | 每模型请求（参考图脱敏）、响应、GLB 统计、校验、入库、色调指标 |
| `pipeline/v6/meshy-batch-20260918/results.json` / `results-table.md` | 汇总 |
| `pipeline/v6/meshy-batch-20260918/sheets/<id>.png` | 验收拼图；`overview-front-views.png`、`refs-overview.png` 总览 |
| `pipeline/v6/meshy-batch-20260918/iso/` | 四向验收渲染帧与拼条、`iso-index.json`（尺寸、缩放、Blender 导入统计） |
| `pipeline/v6/meshy-batch-20260918/rejected/` | 被替换的 v1 GLB 与四视图 |
| `pipeline/v6/meshy-batch-20260918/*.log` | 试点、两批主跑、对比试验、重试、参考图与验收渲染日志 |

## 已知问题与后续

- **尺寸**：Meshy GLB 为单位化尺寸，接入场景/重做 8 向 sprite bake 时按 `iso-index.json` 的 `normalizeScale` 缩放（或在 `.meta` import_settings 里固化）。本批未替换 `Content/UI/v6/model-bakes/`，游戏运行时仍使用粗模 bake。
- **孪生单位**（60 credits）与 **7 个 WARN 色调复审**（如需重出每个 15 credits）：余额 40，需充值后用 `python meshy_generate.py run --ids ...`（孪生）或 `run --ids ... --redo`（重出，自动归档旧版）继续。
- **RurixForge 侧可改进**：① `gend::media::meshy_poll` 遇 TLS 瞬断直接报错，任务已计费却丢产物（本批 30 credits），应像预览图下载一样对瞬时传输错误重试；② REST `gen/mesh` 的 sidecar `kind` 恒为 `text2mesh`，图生 3D 时应为 `image2mesh`（`meta.mode` 正确）；③ REST 面缺 `textureImageUrl`（MCP 的 `styleRefAssetPath` 有），无法从 REST 指定独立贴图参考图。
- `vscode`/`pycharm` 的官方图标未进入 AI 贴图，须以引擎贴花叠加原始 `Content/Models/v6/branding/*.png`（sha256 见 `Content/UI/v6/model-bakes/<id>/bake.json`）。
