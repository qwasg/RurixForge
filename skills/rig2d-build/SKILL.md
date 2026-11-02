---
name: rig2d-build
description: 2D 角色绑骨(Live2D 风格):把分层立绘做成可动的 .rxrig——拆部件、网格化、骨骼、蒙皮权重、参数与关键形状(keyform)、头发物理、自动眨眼呼吸。当任务涉及「绑骨 / 骨骼绑定 / Live2D / 2D 骨骼动画 / 立绘动起来 / 网格变形 / 蒙皮权重 / PSD 拆层绑定 / 部件表组装」且项目为 2D 模式时使用。
tags: [2d, rig, skeletal, live2d-style]
---

# rig2d-build · 2D 角色绑骨规程

## 目标
产出一个通过校验、在参数全范围内无撕裂穿帮的 `.rxrig` 资产(`Content/Rigs/<name>.rxrig`):
部件网格贴合轮廓、骨骼摆在关节处、权重平滑、核心参数(眨眼 / 张嘴 / 头部转向 / 身体 / 呼吸 / 头发)都有关键形状,
物理与自动眨眼呼吸可用。只产出资产;挂进场景与逻辑驱动见 `rig2d-animate`。

## 必须遵守
- **先确认模式**:`mcp__engine-scene__scene_summary` 的 mode 须为 `2d`;否则如实告知并停下。
- **先查询后修改**:`mcp__asset-pipeline__asset_list` 核对素材;已有 rig 先 `mcp__asset-pipeline__rig_get`
  (`sections` 按需取,默认不含网格数组),在其上增量修改,不覆盖用户已有工作。
- **几何一律来自工具**:网格用 `mcp__asset-pipeline__rig_mesh_auto`,权重用 `mcp__asset-pipeline__rig_weights_auto`,
  骨架用 `rig_patch` 的 `merge_scaffold`;禁止在 ops 里手写大段顶点或权重数组。
- **dryRun 优先**:每批 `mcp__asset-pipeline__rig_patch` 先 `dryRun: true` 看 `stats` 与 `destructive`,再正式提交;
  含删除类 op(remove_*)且非用户明确要求时必须先征得确认。
- **每阶段自检**:阶段末用 `mcp__asset-pipeline__rig_preview` 出图并亲眼核对;不合格不进入下一阶段。
- 坐标约定:画布像素、Y 向下;骨骼 `position` / `rotation` 是**绝对**静止姿态(度,Y 向下时正角度视觉上顺时针;
  朝上的骨骼 rotation = -90);keyform 与 clip 对骨骼的改动都是**相对静止的增量**。
- 命名:部件 id 用英文小写下划线(`face`、`eye_l_white`、`eye_l_iris`、`mouth`、`hair_front`、`hair_back`、`body`、`arm_l`);
  参数沿用模板标准名(`ParamAngleX`、`ParamEyeLOpen`、`ParamMouthOpenY`…),下游逻辑按名驱动。

## 分步骤执行流程
1. **部件来源(二选一)**
   - A. PSD:`mcp__asset-pipeline__rig_import_psd { psdRef, name }` → 核对返回 `parts`(图层 = 部件,
     位置即 PSD 内位置,初始为矩形网格)。图层名不规范时用 `set_part` 不能改 id——记下映射,后续按实际 id 操作。
   - B. 生图:
     1. `mcp__gen-image__gen_image` 先出**全身定妆参考图**(纯品红底 #FF00FF,正面,无透视),`gen_accept` 入库记 GUID。
     2. 再出**部件表**:同一角色、同一比例,每个部件单独完整画出(**被遮挡区也要画全**,如被头发盖住的额头、
        被手臂挡住的躯干),部件之间留纯品红隔离带、互不接触;列明部件清单(脸、左/右眼白、左/右瞳孔、眉、嘴、
        前发、后发、身体、左/右臂)。`gen_accept` 入库。
     3. `mcp__asset-pipeline__sprite_autoslice { assetPath }` 取各部件 bbox;对照参考图把每个 bbox 认成部件名,
        并估计它在参考图画布上的左上角位置。
     4. `mcp__asset-pipeline__rig_parts_from_sheet { texture, name, canvasSize: 参考图尺寸, parts:[{name,bbox,position,drawOrder}] }`。
        drawOrder 由后到前:后发 100 → 身体 200 → 手臂 250 → 脸 300 → 眼白 400 → 瞳孔 410 → 眉/嘴 450 → 前发 500。
     5. `rig_preview { compareTo: 参考图 fileRef }`:`silhouetteIoU < 0.9` 时按错位方向 `set_part` 平移该部件
        (改 mesh.vertices 整体平移;此时网格仍是矩形,4 个顶点)后重试,最多 3 轮。
2. **骨架**:`rig_patch [{op:"merge_scaffold", template:"bust"|"fullbody"}]`(半身立绘用 bust)。
   然后 `rig_preview { overlay:"bones" }` 看骨骼与部件是否对齐,用 `set_bone` 把 `head` 关节挪到下巴/脖子交界、
   `body` 到胸口、`arm_*` 到肩、`hair_*` 根到头顶发旋,`length` 覆盖到对应部件末端。
3. **网格**:`rig_mesh_auto`——脸/身体/头发 `interiorSpacing: 16–24`,眼/嘴/眉等小部件 `interiorSpacing: 8–12`。
   `rig_preview { overlay:"mesh" }` 检查:三角形贴合轮廓、无大片空白、头发尖端有足够顶点。
4. **权重**:
   - 刚性部件直接 `set_weights { part, bone }`:瞳孔/眼白/眉/嘴/脸 → `head`;前发 → `hair_front`;后发 → `hair_back`。
   - 需弯曲的部件(身体、手臂、长发)用 `rig_weights_auto { parts, bones }` 限定相关骨骼(如手臂只给 `body` + `arm_l`)。
   - `rig_preview { overlay:"weights:<骨骼>" }` 抽检关节处过渡是否平滑。
5. **参数与关键形状**(按优先级逐个做;每个都要出图验证):
   1. 眨眼 `ParamEyeLOpen` / `ParamEyeROpen`(0/1):在 0 处把眼白和瞳孔压成一条缝——
      `rig_evaluate` 取当前顶点,计算让上下眼睑合拢的偏移,`set_keyform { target:"part:eye_l_white", params:["ParamEyeLOpen"],
      keys:[[0,1]], at:{ParamEyeLOpen:0}, form:{offsets:[...]} }`;瞳孔同理,或在 0 处 `form:{opacity:0}` 直接隐藏。
   2. 张嘴 `ParamMouthOpenY`(0/1):在 1 处下唇顶点下移。
   3. 头部转向 `ParamAngleX` / `ParamAngleY`(±30,三点 [-30,0,30]):优先给头部部件挂一个 warp 形变器
      (`add_deformer { id:"face_warp", type:"warp", rect: 头部包围盒, grid:[3,3] }`,脸/眼/嘴/眉/前发 `set_part { deformer:"face_warp" }`),
      再对 `deformer:face_warp` 写 keyform:向一侧转时整体平移约 8% 头宽,近侧控制点拉宽、远侧压窄。
      `ParamAngleZ` 已由模板映射到 head 骨骼旋转,无需再做。
   4. 身体 `ParamBodyAngleX`:身体部件 keyform 小幅水平偏移(±4% 体宽)。
   5. 呼吸、头发摆动已由模板的骨骼 keyform 与物理提供;只需确认 `hair_*` 部件绑在对应骨骼上。
   每个参数做完后 `rig_preview { grid:{ "<参数>": [min, default, max] } }` 出三联图核对。
6. **物理与自动项**:模板已含 `hair_front` / `hair_back` 摆锤链与自动眨眼 / 呼吸。头发偏长就 `set_physics` 加段、
   调 `damping`(0.05–0.2,越小越飘)与 `stiffness`(10–40,越大越快回正)。
7. **收尾**:`mcp__asset-pipeline__rig_validate` 必须 `ok:true`;最后出一张
   `rig_preview { grid:{ "ParamAngleX":[-30,0,30], "ParamAngleY":[-30,0,30] } }` 3×3 总览图给用户确认。

## 输出约束
- 报告:rig 路径与 GUID、图集 GUID、部件数 / 顶点总数 / 骨骼数、参数清单(id、范围、已做 keyform 的轴)、
  物理链与自动项、最终预览图 fileRef(及 silhouetteIoU,如有)。
- 效果不佳或未完成的部位(撕裂、穿帮、缺被遮挡区)单独列「问题清单」,不混入「完成」叙述。
- 结论只来自真实工具返回与预览图;没出图核对过的不写成已完成。

## 失败回退策略
- `rig_patch` 是原子的:失败时按错误消息里的 `ops[i](op名)` 与 JSON 路径修正该条后整批重试,不要逐条盲试。
- `RIG_MESH_FAILED`(区域无像素 / 顶点超限):检查部件 UV 区域是否落在透明区、调大 `interiorSpacing` 或 `maxError`。
- `RIG_TOO_LARGE`:顶点总数超 8192,对大部件调大 `interiorSpacing` 重新 `rig_mesh_auto`。
- `RIG_PSD_UNSUPPORTED`:请用户导出 8-bit RGB 的 PSD(非 PSB / CMYK / 16-bit)。
- `RIG_FEATURE_UNSUPPORTED`(剪贴蒙版 mask):v1 不支持,移除 mask,改用 drawOrder 与部件裁切达成效果。
- 连续 3 轮 silhouetteIoU 不达标或 keyform 效果始终不对:停下,说明现状并请用户在 IDE 绑骨编辑器里手调,
  不要反复覆盖。不伪造预览,不删除用户已有部件(删除须经确认)。
