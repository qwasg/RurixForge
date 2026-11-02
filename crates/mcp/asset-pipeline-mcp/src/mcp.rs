//! MCP stdio 服务:initialize / tools/list / tools/call。复刻 engine-scene-mcp 骨架。

use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex, MutexGuard};

use assetd::import::import_assets;
use assetd::meta::MetaDoc;
use assetd::project::ForgeProject;
use assetd::{meta_path_for, normalize_rel};
use forge_index::vector::Embedder;
use serde_json::{json, Value};

/// gend::embed::RemoteEmbedder → forge_index Embedder 适配(与 context-mcp 同)。
struct GendEmbedder(gend::embed::RemoteEmbedder);

impl Embedder for GendEmbedder {
    fn model(&self) -> &str {
        &self.0.model
    }
    fn embed(&self, texts: &[String]) -> std::result::Result<Vec<Vec<f32>>, String> {
        self.0.embed_batch(texts)
    }
}

fn resolve_embedder() -> Option<GendEmbedder> {
    gend::embed::resolve_embedder().map(GendEmbedder)
}

/// F10-RAG:简介写后增量进语义索引(词法 + 已配 embedding 则向量)。
/// 返回并入响应的字段;索引未建/失败不阻断写结果,如实标注(indexed/indexError)。
fn index_after_description(project_root: &std::path::Path, asset_path: &str) -> Value {
    let rel = match normalize_rel(asset_path) {
        Ok(r) => r,
        Err(e) => {
            return json!({ "indexed": false, "indexError": format!("[{}] {}", e.code, e.message) })
        }
    };
    let embedder = resolve_embedder();
    match forge_index::build::upsert_asset_docs(
        project_root,
        &rel,
        embedder.as_ref().map(|e| e as &dyn Embedder),
    ) {
        Ok(o) => {
            let mut v = json!({
                "indexed": o.indexed,
                "indexedDocs": o.docs,
                "tier": o.tier,
            });
            if let Some(e) = o.embed_error {
                v["embedError"] = json!(e);
            }
            v
        }
        Err(e) => json!({ "indexed": false, "indexError": format!("[{}] {}", e.code, e.message) }),
    }
}

fn tool_list() -> Value {
    let mut out = json!({
        "tools": [
            {"name":"shader_graph_get","description":"Read a Shader Graph document, stable IDs and sourceHash for optimistic concurrency","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"reference":{"type":"string"}}}},
            {"name":"shader_graph_save","description":"Save a Shader Graph draft atomically. Existing graphs require expectedHash; saving never publishes runtime changes.","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"graph":{"type":"object"},"expectedHash":{"type":"string"}},"required":["path","graph"]}},
            {"name":"shader_graph_compile","description":"Compile a typed Shader Graph DAG to validated Rurix SPIR-V and Godot source with node diagnostics. Runtime preview verifies actual backend pipelines.","inputSchema":{"type":"object","properties":{"graph":{"type":"object"},"path":{"type":"string"},"reference":{"type":"string"}}}},
            {"name":"shader_graph_list_nodes","description":"Discover Shader Graph domains, typed nodes, ports and budgets progressively","inputSchema":{"type":"object","properties":{}}},
            {"name":"shader_material_create","description":"Create rxmat v2 binding a Shader Graph GUID to typed parameters and texture GUIDs","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"shaderGraph":{"type":"string"},"params":{"type":"object"},"textures":{"type":"object"}},"required":["path","shaderGraph"]}},
            {
                "name": "asset_import",
                "description": "导入源文件到 Content/目录;幂等(覆盖源文件但保留 .meta GUID)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "sourcePaths": { "type": "array", "items": { "type": "string" }, "description": "源文件路径列表(绝对或相对项目根)" },
                        "destFolder": { "type": "string", "description": "目标文件夹(相对 Content/,如 \"Meshes\")" },
                        "importSettings": { "type": "object", "description": "导入选项覆盖(可选)" }
                    },
                    "required": ["sourcePaths", "destFolder"]
                }
            },
            {
                "name": "font_list",
                "description": "D-045:列可用字体——项目已入库字体(含 GUID,Text 组件直接引用)+ 系统字体目录里的候选(未入库,需 font_import)。返回族名/子族/字形数/是否含中文字形",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "includeSystem": { "type": "boolean", "description": "是否列系统字体(缺省 true)" },
                        "query": { "type": "string", "description": "按族名或文件名过滤(不区分大小写)" },
                        "cjkOnly": { "type": "boolean", "description": "只列含中文字形的字体" },
                        "limit": { "type": "integer", "description": "系统字体条数上限(缺省 60,最大 300)" }
                    }
                }
            },
            {
                "name": "font_import",
                "description": "D-045:把字体文件(.ttf/.otf/.ttc,常为 font_list 列出的系统字体)复制进 Content/Fonts/ 入库,返回 GUID 供 Text.font 引用。provenance 记来源与授权提醒:系统字体仅供本地制作,发行前须确认授权",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "sourcePath": { "type": "string", "description": "字体文件绝对路径或项目相对路径" },
                        "destFolder": { "type": "string", "description": "相对 Content/,缺省 \"Fonts\"" }
                    },
                    "required": ["sourcePath"]
                }
            },
            {
                "name": "asset_list",
                "description": "列出 Content/ 下全部资产及元信息",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "asset_get_meta",
                "description": "读 .meta 内容",
                "inputSchema": {
                    "type": "object",
                    "properties": { "assetPath": { "type": "string" } },
                    "required": ["assetPath"]
                }
            },
            {
                "name": "asset_build_status",
                "description": "查构建状态(current/stale/building/failed);空参数 = 全项目",
                "inputSchema": {
                    "type": "object",
                    "properties": { "assetPaths": { "type": "array", "items": { "type": "string" } } }
                }
            },
            {
                "name": "asset_refs",
                "description": "查资产引用边(refs=出边 / referencedBy=入边)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "assetPath": { "type": "string" },
                        "direction": { "type": "string", "enum": ["refs", "referencedBy"] },
                        "depth": { "type": "integer", "description": "递归深度(缺省 1)" }
                    },
                    "required": ["assetPath", "direction"]
                }
            },
            {
                "name": "asset_delete",
                "description": "删除资产(默认引用阻断;force=true 跳过阻断,但 MCP 层须先 Proposal)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "assetPaths": { "type": "array", "items": { "type": "string" } },
                        "force": { "type": "boolean", "description": "跳过引用阻断(危险)" }
                    },
                    "required": ["assetPaths"]
                }
            },
            {
                "name": "asset_move",
                "description": "移动资产到新目录(自动留 redirector;GUID 引用不断链);可选 newName 同步改名(清洗命名)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "assetPath": { "type": "string" },
                        "destFolder": { "type": "string" },
                        "newName": { "type": "string", "description": "新文件名(可选;清洗命名用)" }
                    },
                    "required": ["assetPath", "destFolder"]
                }
            },
            {
                "name": "asset_fix_redirectors",
                "description": "收敛 redirector(重写路径引用并清除)",
                "inputSchema": {
                    "type": "object",
                    "properties": { "folder": { "type": "string", "description": "限定目录(可选)" } }
                }
            },
            {
                "name": "asset_reimport",
                "description": "重新构建资产(改 importSettings 后)",
                "inputSchema": {
                    "type": "object",
                    "properties": { "assetPaths": { "type": "array", "items": { "type": "string" } } },
                    "required": ["assetPaths"]
                }
            },
            {
                "name": "asset_set_meta",
                "description": "打补丁到 .meta importSettings",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "assetPath": { "type": "string" },
                        "patch": { "type": "object" }
                    },
                    "required": ["assetPath", "patch"]
                }
            },
            {
                "name": "asset_set_description",
                "description": "写入 .meta semantic 段(description/tags/source);缺 .meta 时自动补建。写后自动增量进 RAG 语义索引(词法即时;索引为 hybrid 档时向量同步顶量。响应 indexed/tier 如实标注;索引未建则跳过,需 context_index_build 首建)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "assetPath": { "type": "string" },
                        "description": { "type": "string", "description": "文字简介" },
                        "tags": { "type": "array", "items": { "type": "string" }, "description": "标签列表" },
                        "source": { "type": "string", "enum": ["human", "agent-vision", "agent-facts"], "description": "描述来源(I-7 溯源)" },
                        "model": { "type": "string", "description": "生成模型名(可选)" },
                        "contentHash": { "type": "string", "description": "内容摘要 hash(可选,用于 stale 判定)" }
                    },
                    "required": ["assetPath", "description", "source"]
                }
            },
            {
                "name": "asset_thumbnail",
                "description": "贴图缩略图(原图直出 data URL,前端 CSS 缩放);非贴图 → NO_THUMBNAIL(网格离屏渲染 = RD-F2-002)",
                "inputSchema": {
                    "type": "object",
                    "properties": { "assetPath": { "type": "string" } },
                    "required": ["assetPath"]
                }
            },
            {
                "name": "material_create",
                "description": "创建材质资产(.rxmat JSON:version/shader/params/textures);textures 槽位值须为已存在资产的 GUID,建 material→texture 引用边",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "材质名(不含扩展名/路径分隔符)" },
                        "destFolder": { "type": "string", "description": "目标文件夹(缺省 Materials)" },
                        "shader": { "type": "string", "description": "shader/closure id(缺省 pbr-default)" },
                        "params": { "type": "object", "description": "材质参数(baseColor/roughness/metallic 等)" },
                        "textures": { "type": "object", "description": "纹理槽位 → 贴图资产 GUID" }
                    },
                    "required": ["name"]
                }
            },
            {
                "name": "texture_process",
                "description": "贴图处理(image crate 解码):ops.resize={width,height} 精确 | {max} 等比缩小;ops.format=png|jpg 转格式;输出同目录 <stem>@<w>x<h>.<ext> 新资产(幂等),原资产不动",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "assetPath": { "type": "string" },
                        "ops": {
                            "type": "object",
                            "properties": {
                                "resize": { "type": "object", "description": "{width,height} 或 {max}" },
                                "format": { "type": "string", "enum": ["png", "jpg"] }
                            }
                        }
                    },
                    "required": ["assetPath", "ops"]
                }
            },
            {
                "name": "mesh_inspect",
                "description": "网格统计(读缓存 .rxmesh 真实字节):vertices/triangles/meshlets/lods/materials/bounds;缓存缺失 → MESH_NOT_BUILT",
                "inputSchema": {
                    "type": "object",
                    "properties": { "assetPath": { "type": "string" } },
                    "required": ["assetPath"]
                }
            },
            {
                "name": "inspect_3d_asset",
                "description": "Deterministic pure CPU 3D model inspection (.glb, .gltf, .rxmodel, .rxmesh): extracts polygon count, dimensions, bounding box, material list, and renders/caches a 4-panel composite showcase sheet (perspective + front/side/top orthographic views) returning multimodal base64 dataUrl.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Asset path relative to Content/ or project root (e.g. 'Content/Models/tower.glb', 'Models/bld_tavern/model.rxmodel', or '.forge/tmp/gen/mesh_01.glb')"
                        },
                        "assetPath": {
                            "type": "string",
                            "description": "Alias for path"
                        },
                        "generateThreeView": {
                            "type": "boolean",
                            "description": "Whether to render and return the 4-panel composite three-view sheet (default: true)"
                        },
                        "panelSize": {
                            "type": "integer",
                            "description": "Quadrant panel size in pixels (default: 256; 64-512 supported)"
                        }
                    },
                    "required": ["path"]
                }
            },
            {
                "name": "asset_cleanup_scan",
                "description": "asset-cleanup dryRun:扫描全项目,产出整理提案(misplaced 错放/naming 命名混乱/orphan 孤儿),不写任何文件;执行经 asset_move(移动/改名)或 asset_delete(孤儿,须 Proposal)",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "sprite_create",
                "description": "创建精灵图集资产(.rxsprite:单张贴图 + 帧 bbox + pivot 级联 + 动画 clip + 可选 animator 状态机);texture 须为已存在贴图 GUID;autoslice=true 时自动切帧(连通域检测,品红族/alpha 背景判定与视口色键同规则)填充 frames",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "精灵名(不含扩展名/路径分隔符)" },
                        "texture": { "type": "string", "description": "图集贴图资产 GUID" },
                        "destFolder": { "type": "string", "description": "目标文件夹(缺省 Sprites)" },
                        "pivot": { "type": "array", "items": { "type": "number" }, "description": "文档级锚点 [x,y](0..1,y 向下;缺省 [0.5,1] 脚底锚;VFX/飞行体用 [0.5,0.5])" },
                        "frames": { "type": "object", "description": "帧名 → { bbox: [x,y,w,h], pivot?: [x,y] }(与 autoslice 二选一)" },
                        "clips": { "type": "object", "description": "clip 名 → { frames: [帧名], fps?: 数, duration?: 总秒, loop?: 布尔, onFinish?: hold|first }" },
                        "animator": { "type": "object", "description": "可选状态机 { defaultState, parameters, states, transitions }" },
                        "autoslice": { "type": "boolean", "description": "自动切帧填充 frames(命名 frame_<i>,行序从上到下、行内从左到右)" },
                        "minArea": { "type": "integer", "description": "autoslice 连通域像素数下限(缺省 16)" }
                    },
                    "required": ["name", "texture"]
                }
            },
            {
                "name": "sprite_get",
                "description": "读 .rxsprite 文档(解析+校验后的规范形态)",
                "inputSchema": {
                    "type": "object",
                    "properties": { "assetPath": { "type": "string" } },
                    "required": ["assetPath"]
                }
            },
            {
                "name": "sprite_set",
                "description": "整文档覆盖写 .rxsprite(先校验:clip 引用帧存在、animator 引用 clip/参数存在,坏文档拒绝;GUID 稳定不变)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "assetPath": { "type": "string" },
                        "doc": { "type": "object", "description": "完整 .rxsprite JSON 文档(version/texture/pivot/frames/clips/animator)" }
                    },
                    "required": ["assetPath", "doc"]
                }
            },
            {
                "name": "sprite_autoslice",
                "description": "对贴图做自动切帧(不写文件):连通域检测出紧致 bbox 列表,行带分组排序(从上到下、行内从左到右);背景判定 = alpha 过低或品红族(g < 0.5*min(r,b),与视口色键同规则)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "assetPath": { "type": "string", "description": "贴图资产路径(相对 Content/)" },
                        "minArea": { "type": "integer", "description": "连通域像素数下限(缺省 16;噪点多时调高)" },
                        "alphaThreshold": { "type": "integer", "description": "alpha 背景阈值 0-255(缺省 5)" }
                    },
                    "required": ["assetPath"]
                }
            }
        ]
    });
    out["tools"].as_array_mut().unwrap().extend(rig_tool_list());
    out
}

fn rig_tool_list() -> Vec<Value> {
    json!([
        // ---- rig_*(F-GAME-5 / D-050:Live2D 风格 2D 绑骨 .rxrig;assetPath 与 guid 二选一) ----
        {
            "name": "rig_create",
            "description": "创建 2D 绑骨资产(.rxrig):空白文档,可合并 bust(半身)/fullbody(全身)模板的标准参数/骨骼/物理/自动眨眼呼吸;texture 须为已存在贴图 GUID(全部部件共用一张图集);canvasSize 缺省 = 贴图像素尺寸,origin = 底边中点",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "rig 名(不含扩展名/路径分隔符/点)" },
                    "texture": { "type": "string", "description": "图集贴图资产 GUID" },
                    "canvasSize": { "type": "array", "items": { "type": "number" }, "description": "画布 [w,h] 像素(缺省 = 贴图尺寸)" },
                    "template": { "type": "string", "enum": ["bust", "fullbody"], "description": "合并模板骨架" },
                    "destFolder": { "type": "string", "description": "目标文件夹(相对 Content/,缺省 Rigs)" },
                    "origin": { "type": "string", "description": "provenance 来源(缺省 user-import;由生图组装时写 gen 来源)" }
                },
                "required": ["name", "texture"]
            }
        },
        {
            "name": "rig_get",
            "description": "读 .rxrig(解析+校验后的规范形态)。缺省省略大数组防上下文爆炸:parts[].mesh → {vertexCount, triangleCount, uvBounds},parts[].weights → {count},keyforms[].forms[].offsets → {count};includeMesh=true 返回全量。sections 只取指定段",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "assetPath": { "type": "string", "description": "rig 资产路径(相对 Content/,如 Rigs/hero.rxrig)" },
                    "guid": { "type": "string", "description": "rig GUID(与 assetPath 二选一)" },
                    "sections": { "type": "array", "items": { "type": "string", "enum": ["parameters", "bones", "deformers", "parts", "keyforms", "physics", "auto", "clips", "animator", "canvas"] } },
                    "includeMesh": { "type": "boolean", "description": "返回完整顶点/UV/三角形/权重/偏移数组(缺省 false)" }
                }
            }
        },
        {
            "name": "rig_set",
            "description": "整文档覆盖写 .rxrig(先语义校验 + 贴图 GUID 解析,坏文档拒绝 RIG_INVALID;GUID 稳定不变)。局部修改优先用 rig_patch",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "assetPath": { "type": "string" },
                    "guid": { "type": "string" },
                    "doc": { "type": "object", "description": "完整 .rxrig JSON 文档" }
                },
                "required": ["doc"]
            }
        },
        {
            "name": "rig_patch",
            "description": "细粒度原子编辑 .rxrig:ops 全部成功才落盘(任一失败整体回滚,错误带 ops[i] 路径);dryRun=true 只校验不写。op ∈ add_part/set_part/remove_part、add_bone/set_bone/remove_bone、set_weights{part, weights|bone}、add_parameter/set_parameter/remove_parameter、add_deformer/set_deformer/remove_deformer、set_keyform{target, params, keys?, at, form}、set_keyform_group/remove_keyform_group、set_physics/remove_physics、set_auto、set_clip/remove_clip、set_animator、set_canvas、merge_scaffold{template, overwrite?};set_* 的 fields 为 JSON merge-patch。返回 destructive = 删除类 op 下标(须走 Proposal 门)",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "assetPath": { "type": "string" },
                    "guid": { "type": "string" },
                    "ops": { "type": "array", "items": { "type": "object" }, "description": "PatchOp 数组(每项带 op 字段)" },
                    "dryRun": { "type": "boolean" }
                },
                "required": ["ops"]
            }
        },
        {
            "name": "rig_mesh_auto",
            "description": "按图集轮廓自动网格化部件(alpha/品红色键 → 轮廓 → 简化 → 内部撒点 → 约束 Delaunay),保持部件画布摆放;顶点变化后原 weights 与指向该部件的 keyform 顶点偏移被清空(返回 weightsCleared / keyformsCleared)",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "assetPath": { "type": "string" },
                    "guid": { "type": "string" },
                    "parts": { "type": "array", "items": { "type": "string" }, "description": "部件 id(缺省全部)" },
                    "maxError": { "type": "number", "description": "轮廓简化容差像素(缺省 2)" },
                    "interiorSpacing": { "type": "number", "description": "内部撒点间距像素(缺省 24;脸/身体 16–24,细小部件 32+)" },
                    "alphaThreshold": { "type": "number", "description": "不透明阈值 0..1(缺省 0.05)" },
                    "maxVertices": { "type": "integer", "description": "单部件顶点上限(缺省 1500,超出自动放大间距)" }
                }
            }
        },
        {
            "name": "rig_weights_auto",
            "description": "自动蒙皮权重:顶点到骨段距离反比(1/d^falloff),每顶点 ≤4 影响并归一化,可选一次平滑;写 weights 并清除刚性 bone。未指定 parts 时跳过已刚性绑定的部件(skippedRigid)",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "assetPath": { "type": "string" },
                    "guid": { "type": "string" },
                    "parts": { "type": "array", "items": { "type": "string" } },
                    "bones": { "type": "array", "items": { "type": "string" }, "description": "只考虑这些骨骼(缺省全部)" },
                    "falloff": { "type": "number", "description": "距离衰减指数(缺省 2)" },
                    "smooth": { "type": "boolean", "description": "沿网格平滑一次(缺省 true)" }
                }
            }
        },
        {
            "name": "rig_evaluate",
            "description": "求一帧形变(只读):参数缺省取 default,超范围夹取;可叠加 clip@time。返回画布像素坐标(Y 向下)的部件顶点位置、骨骼首尾点与包围盒;未识别的参数/clip/部件列入 unknown",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "assetPath": { "type": "string" },
                    "guid": { "type": "string" },
                    "params": { "type": "object", "description": "参数 id → 值" },
                    "clip": { "type": "string" },
                    "time": { "type": "number", "description": "clip 时间秒(缺省 0)" },
                    "parts": { "type": "array", "items": { "type": "string" }, "description": "只返回这些部件" }
                }
            }
        },
        {
            "name": "rig_preview",
            "description": "CPU 光栅预览图(只读,PNG 写 .forge/tmp/,返回 fileRef):可叠 overlay(mesh 网格 / bones 骨骼 / weights:<boneId> 权重热力图);compareTo 给参考图(项目相对路径,尺寸视为 canvas 尺寸)时返回剪影 silhouetteIoU;grid 给 1–2 个参数的取值(每轴 ≤5)出参数网格拼图",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "assetPath": { "type": "string" },
                    "guid": { "type": "string" },
                    "params": { "type": "object" },
                    "clip": { "type": "string" },
                    "time": { "type": "number" },
                    "size": { "type": "integer", "description": "方形边长 64..2048(缺省 512;grid 时为单格边长)" },
                    "overlay": { "type": "string", "description": "none | mesh | bones | weights:<boneId>" },
                    "compareTo": { "type": "string", "description": "参考图项目相对路径(如 .forge/tmp/gen/ref.png 或 Content/Textures/ref.png)" },
                    "grid": { "type": "object", "description": "参数 id → 取值数组,如 {\"ParamAngleX\":[-30,0,30]}" }
                }
            }
        },
        {
            "name": "rig_import_psd",
            "description": "PSD 分层导入为 rig:仅 8-bit RGB(PSB/16-bit/CMYK/解析失败 → RIG_PSD_UNSUPPORTED);每个可见像素图层 = 一个矩形部件(id = 组路径/图层名,非法字符替换为 _,重名追加 _2;drawOrder = 自底向上序 × 10);图层 shelf 打包进 ≤4096² 图集 Textures/<name>_atlas.png(超出 RIG_TOO_LARGE);canvas = PSD 尺寸",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "psdRef": { "type": "string", "description": "PSD 文件项目相对路径" },
                    "name": { "type": "string", "description": "rig 名(同时决定图集名 <name>_atlas.png)" },
                    "destFolder": { "type": "string", "description": "rig 目标文件夹(缺省 Rigs)" }
                },
                "required": ["psdRef", "name"]
            }
        },
        {
            "name": "rig_parts_from_sheet",
            "description": "用部件表组装 rig:texture 为品红底/透明底部件表贴图 GUID,每部件给图集 bbox(通常来自 sprite_autoslice)与画布左上角位置 position,生成矩形网格部件(drawOrder 缺省 = 序号 × 10);之后用 rig_preview{compareTo: 定妆参考图} 核对剪影",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "texture": { "type": "string", "description": "部件表贴图 GUID" },
                    "name": { "type": "string" },
                    "canvasSize": { "type": "array", "items": { "type": "number" }, "description": "画布 [w,h](通常 = 参考图尺寸)" },
                    "parts": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string" },
                                "bbox": { "type": "array", "items": { "type": "integer" } },
                                "position": { "type": "array", "items": { "type": "number" } },
                                "drawOrder": { "type": "integer" }
                            },
                            "required": ["name", "bbox", "position"]
                        }
                    },
                    "destFolder": { "type": "string" },
                    "origin": { "type": "string", "description": "provenance 来源(缺省 user-import)" }
                },
                "required": ["texture", "name", "canvasSize", "parts"]
            }
        },
        {
            "name": "rig_validate",
            "description": "校验 .rxrig(只读):给 assetPath|guid 校验磁盘文件,或给 doc 校验未落盘文档;含语义校验与贴图 GUID 解析。返回 {ok, error?:{code, message}}",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "assetPath": { "type": "string" },
                    "guid": { "type": "string" },
                    "doc": { "type": "object" }
                }
            }
        }
    ])
    .as_array()
    .cloned()
    .unwrap_or_default()
}

fn ok(id: Value, result: Value) -> Value { json!({ "jsonrpc": "2.0", "id": id, "result": result }) }
fn err(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn tool_wrap(v: &Value, is_error: bool) -> Value {
    let text = serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string());
    let mut out = json!({ "content": [{ "type": "text", "text": text }] });
    if is_error { out["isError"] = json!(true); }
    out
}

fn lock<'a>(m: &'a Mutex<ForgeProject>) -> MutexGuard<'a, ForgeProject> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn resolve_rig_arg(p: &ForgeProject, args: &Value) -> Result<(String, String), Result<Value, Value>> {
    let asset_path = args.get("assetPath").and_then(Value::as_str).filter(|s| !s.is_empty());
    let guid = args.get("guid").and_then(Value::as_str).filter(|s| !s.is_empty());
    if asset_path.is_none() && guid.is_none() {
        return Err(Err(err(Value::Null, -32602, "invalid params: 须提供 assetPath 或 guid")));
    }
    assetd::rig::resolve_rig(p, asset_path, guid)
        .map_err(|e| Ok(json!({ "error": e.code, "message": e.message })))
}

fn call_tool(proj: &Arc<Mutex<ForgeProject>>, params: &Value) -> Result<Value, Value> {
    let name = params.get("name").and_then(Value::as_str)
        .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺工具名"))?;
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    if !args.is_object() { return Err(err(Value::Null, -32602, "invalid params: arguments 须为对象")); }

    match name {
        "shader_graph_list_nodes"=>Ok(assetd::shader::catalog()),
        "shader_graph_get"|"shader_graph_save"|"shader_graph_compile"|"shader_material_create"=>{
            let p=lock(proj);let reference=args.get("reference").or_else(||args.get("path")).and_then(Value::as_str).unwrap_or("");
            let result=match name{
                "shader_graph_get"=>assetd::shader::load(&p,reference),
                "shader_graph_compile"=>if let Some(graph)=args.get("graph"){assetd::shader::compile_document(graph)}else{assetd::shader::compile(&p,reference)},
                "shader_graph_save"=>serde_json::from_value::<assetd::shader::GraphDoc>(args["graph"].clone()).map_err(|e|assetd::AssetError::new("SHADER_PARSE",e.to_string())).and_then(|g|assetd::shader::save(&p,reference,&g,args["expectedHash"].as_str())),
                _=>assetd::shader::create_material(&p,reference,args["shaderGraph"].as_str().unwrap_or(""),args.get("params").unwrap_or(&json!({})),args.get("textures").unwrap_or(&json!({}))),
            };Ok(match result{Ok(mut v)=>{if name=="shader_graph_save"{let indexed=index_after_description(&p.root,reference);v["index"]=indexed;}v},Err(e)=>json!({"ok":false,"error":e.code,"message":e.message})})
        },
        "asset_import" => {
            let sources: Vec<String> = args.get("sourcePaths")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let dest = args.get("destFolder").and_then(Value::as_str).unwrap_or("");
            let settings: Option<&serde_json::Map<String, Value>> = args.get("importSettings")
                .and_then(|v| v.as_object());
            let p = lock(proj);
            match import_assets(&p, &sources, dest, settings) {
                Ok(out) => Ok(json!({
                    "imported": out.imported.iter().map(|i| json!({
                        "assetPath": i.asset_path, "guid": i.guid,
                        "type": i.atype.as_str(), "cacheHit": i.cache_hit,
                        "artifact": i.artifact, "vertexCount": i.vertex_count,
                        "triangleCount": i.triangle_count,
                        "width": i.width, "height": i.height
                    })).collect::<Vec<_>>(),
                    "failed": out.failed.iter().map(|f| json!({
                        "source": f.source, "error": f.error
                    })).collect::<Vec<_>>()
                })),
                Err(e) => Ok(json!({ "error": e.message, "code": e.code })),
            }
        }
        "font_list" => {
            let include_system = args.get("includeSystem").and_then(Value::as_bool).unwrap_or(true);
            let query = args.get("query").and_then(Value::as_str).unwrap_or("").to_lowercase();
            let cjk_only = args.get("cjkOnly").and_then(Value::as_bool).unwrap_or(false);
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(60).clamp(1, 300) as usize;
            let keep = |path: &str, info: &assetd::font::FontInfo| {
                (!cjk_only || info.has_cjk)
                    && (query.is_empty()
                        || info.family.to_lowercase().contains(&query)
                        || path.to_lowercase().contains(&query))
            };
            let p = lock(proj);
            let mut project_fonts = Vec::new();
            for rel in p.scan_content().map_err(|e| err(Value::Null, -32000, &e.message))? {
                let abs = p.content_root().join(&rel);
                if !assetd::font::is_font_path(&abs) {
                    continue;
                }
                let meta_path = meta_path_for(&p.content_root(), &rel);
                let Ok(meta) = MetaDoc::load(&meta_path) else { continue };
                let Ok(info) = assetd::font::probe(&abs) else { continue };
                if keep(&rel, &info) {
                    project_fonts.push(json!({ "assetPath": rel, "guid": meta.guid, "font": info }));
                }
            }
            drop(p);
            let mut system = Vec::new();
            if include_system {
                // 先多取再过滤,过滤后再截到 limit。
                for (path, info) in assetd::font::list_fonts(&assetd::font::system_font_dirs(), 2000) {
                    let ps = path.to_string_lossy().to_string();
                    if keep(&ps, &info) {
                        system.push(json!({ "path": ps, "font": info }));
                        if system.len() >= limit {
                            break;
                        }
                    }
                }
            }
            Ok(json!({ "projectFonts": project_fonts, "systemFonts": system }))
        }
        "font_import" => {
            let source = args.get("sourcePath").and_then(Value::as_str).unwrap_or("").trim().to_string();
            if source.is_empty() {
                return Ok(json!({ "error": "缺参数: sourcePath", "code": "INVALID_PARAMS" }));
            }
            let dest = args.get("destFolder").and_then(Value::as_str).filter(|s| !s.trim().is_empty()).unwrap_or("Fonts");
            let p = lock(proj);
            let src_abs = if std::path::Path::new(&source).is_absolute() {
                std::path::PathBuf::from(&source)
            } else {
                p.root.join(&source)
            };
            if !assetd::font::is_font_path(&src_abs) {
                return Ok(json!({ "error": format!("不是字体文件(.ttf/.otf/.ttc): {source}"), "code": "FONT_INVALID" }));
            }
            let info = match assetd::font::probe(&src_abs) {
                Ok(i) => i,
                Err(e) => return Ok(json!({ "error": e.message, "code": e.code })),
            };
            match import_assets(&p, &[source.clone()], dest, None) {
                Ok(out) => {
                    if let Some(f) = out.failed.first() {
                        return Ok(json!({ "error": f.error, "code": "IMPORT_FAILED" }));
                    }
                    let Some(i) = out.imported.first() else {
                        return Ok(json!({ "error": "导入无结果", "code": "IMPORT_FAILED" }));
                    };
                    let rel = normalize_rel(&i.asset_path).unwrap_or_else(|_| i.asset_path.clone());
                    let meta_path = meta_path_for(&p.content_root(), &rel);
                    let system_dirs = assetd::font::system_font_dirs();
                    let from_system = system_dirs.iter().any(|d| src_abs.starts_with(d));
                    if let Ok(mut meta) = MetaDoc::load(&meta_path) {
                        meta.provenance = Some(assetd::meta::Provenance {
                            origin: "user-import".into(),
                            detail: Some(json!({
                                "source": source,
                                "kind": "font",
                                "family": info.family,
                                "licenseNote": if from_system {
                                    "系统字体:仅供本地制作,发行前须确认字体授权"
                                } else {
                                    "外部字体:发行前须确认字体授权"
                                },
                            })),
                        });
                        let _ = meta.save(&meta_path);
                    }
                    Ok(json!({ "assetPath": i.asset_path, "guid": i.guid, "font": info }))
                }
                Err(e) => Ok(json!({ "error": e.message, "code": e.code })),
            }
        }
        "asset_list" => {
            let p = lock(proj);
            let items = p.scan_content()
                .map_err(|e| err(Value::Null, -32000, &e.message))?
                .into_iter()
                .map(|rel| {
                    let meta_path = meta_path_for(&p.content_root(), &rel);
                    let (guid, atype, description, tags) = if meta_path.is_file() {
                        match MetaDoc::load(&meta_path) {
                            Ok(m) => {
                                let description = m.description().unwrap_or("").to_string();
                                let tags = m.tags().to_vec();
                                (m.guid, m.atype, description, tags)
                            }
                            Err(_) => (String::new(), "unknown".into(), String::new(), Vec::new()),
                        }
                    } else {
                        (String::new(), "unknown".into(), String::new(), Vec::new())
                    };
                    let size = p.content_root().join(&rel).metadata().ok().map(|m| m.len()).unwrap_or(0);
                    json!({
                        "path": rel,
                        "guid": guid,
                        "type": atype,
                        "size": size,
                        "description": description,
                        "tags": tags
                    })
                })
                .collect::<Vec<_>>();
            Ok(json!({ "assets": items }))
        }
        "asset_get_meta" => {
            let asset_path = args.get("assetPath").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 assetPath"))?;
            let p = lock(proj);
            let rel = normalize_rel(asset_path).map_err(|e| err(Value::Null, -32602, &e.message))?;
            let meta_path = meta_path_for(&p.content_root(), &rel);
            if !meta_path.is_file() {
                return Ok(json!({ "error": "NO_META", "message": format!("缺 .meta: {rel}") }));
            }
            match MetaDoc::load(&meta_path) {
                Ok(m) => Ok(json!({ "meta": serde_json::to_value(m).unwrap_or(Value::Null) })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "asset_build_status" => {
            let paths: Vec<String> = args.get("assetPaths")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let p = lock(proj);
            match assetd::status::build_status(&p, &paths) {
                Ok(items) => Ok(json!({
                    "items": items.iter().map(|i| json!({
                        "path": i.path, "state": i.state.as_str(), "hash": i.hash
                    })).collect::<Vec<_>>()
                })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "asset_refs" => {
            let asset_path = args.get("assetPath").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 assetPath"))?;
            let direction = args.get("direction").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 direction"))?;
            let p = lock(proj);
            let rel = normalize_rel(asset_path).map_err(|e| err(Value::Null, -32602, &e.message))?;
            let meta_path = meta_path_for(&p.content_root(), &rel);
            if !meta_path.is_file() {
                return Ok(json!({ "error": "NO_META", "message": format!("缺 .meta: {rel}") }));
            }
            let meta = MetaDoc::load(&meta_path)
                .map_err(|e| err(Value::Null, -32000, &e.message))?;
            // 查询前实时重建引用图(小项目性能可接受;大项目后续改增量索引)。
            let graph = assetd::refs::RefGraph::rebuild(&p)
                .map_err(|e| err(Value::Null, -32000, &e.message))?;
            let edges = if direction == "refs" {
                graph.refs(&meta.guid)
            } else {
                graph.referenced_by(&meta.guid)
            };
            Ok(json!({
                "edges": edges.iter().map(|e| json!({
                    "from": e.from_guid, "to": e.to_guid, "type": e.edge_type
                })).collect::<Vec<_>>()
            }))
        }
        "asset_delete" => {
            let paths: Vec<String> = args.get("assetPaths")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let force = args.get("force").and_then(Value::as_bool).unwrap_or(false);
            let p = lock(proj);
            match assetd::ops::delete_assets(&p, &paths, force) {
                Ok(out) => Ok(json!({
                    "deleted": out.deleted,
                    "blockedByRefs": out.blocked_by_refs.iter().map(|(p, refs)| json!({
                        "assetPath": p, "referencedBy": refs
                    })).collect::<Vec<_>>()
                })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "asset_move" => {
            let asset_path = args.get("assetPath").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 assetPath"))?;
            let dest = args.get("destFolder").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 destFolder"))?;
            let new_name = args.get("newName").and_then(Value::as_str);
            let p = lock(proj);
            match assetd::ops::move_asset(&p, asset_path, dest, new_name) {
                Ok(out) => {
                    let red = out.redirector.map(|(g, old, new)| json!({
                        "guid": g, "oldPath": old, "newPath": new
                    }));
                    Ok(json!({ "moved": out.moved, "redirector": red }))
                }
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "asset_fix_redirectors" => {
            let folder = args.get("folder").and_then(Value::as_str);
            let p = lock(proj);
            match assetd::ops::fix_redirectors(&p, folder) {
                Ok(fixed) => Ok(json!({ "fixed": fixed })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "asset_reimport" => {
            let paths: Vec<String> = args.get("assetPaths")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let p = lock(proj);
            match assetd::ops::reimport_assets(&p, &paths) {
                Ok(rebuilt) => Ok(json!({ "rebuilt": rebuilt })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "asset_set_meta" => {
            let asset_path = args.get("assetPath").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 assetPath"))?;
            let patch = args.get("patch").and_then(|v| v.as_object())
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 patch"))?;
            let p = lock(proj);
            match assetd::ops::set_meta(&p, asset_path, patch) {
                Ok(()) => Ok(json!({})),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "asset_set_description" => {
            let asset_path = args.get("assetPath").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 assetPath"))?;
            let description = args.get("description").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 description"))?;
            let source = args.get("source").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 source"))?;
            let tags: Vec<String> = args.get("tags")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let model = args.get("model").and_then(Value::as_str);
            let content_hash = args.get("contentHash").and_then(Value::as_str);
            let p = lock(proj);
            match assetd::ops::set_description(
                &p, asset_path, description, &tags, source, model, content_hash,
            ) {
                Ok(()) => {
                    // F10-RAG:写后即增量进检索(索引未建/失败如实标注,不阻断写)。
                    let mut resp = json!({ "ok": true });
                    resp.as_object_mut().unwrap().extend(
                        index_after_description(&p.root, asset_path)
                            .as_object()
                            .cloned()
                            .unwrap_or_default(),
                    );
                    Ok(resp)
                }
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "asset_thumbnail" => {
            let asset_path = args.get("assetPath").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 assetPath"))?;
            let p = lock(proj);
            match assetd::thumb::thumbnail_data_url(&p, asset_path) {
                Ok((url, bytes)) => Ok(json!({ "dataUrl": url, "bytes": bytes })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "material_create" => {
            let name = args.get("name").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 name"))?;
            let dest = args.get("destFolder").and_then(Value::as_str).unwrap_or("");
            let shader = args.get("shader").and_then(Value::as_str);
            let params = args.get("params").and_then(|v| v.as_object());
            let textures = args.get("textures").and_then(|v| v.as_object());
            let p = lock(proj);
            match assetd::material::create_material(&p, dest, name, shader, params, textures) {
                Ok(c) => Ok(json!({
                    "assetPath": c.asset_path, "guid": c.guid,
                    "textureRefs": c.texture_refs.iter().map(|(s, g)| json!({ "slot": s, "guid": g })).collect::<Vec<_>>()
                })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "texture_process" => {
            let asset_path = args.get("assetPath").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 assetPath"))?;
            let ops = args.get("ops").cloned().unwrap_or(json!({}));
            if !ops.is_object() { return Err(err(Value::Null, -32602, "invalid params: ops 须为对象")); }
            let p = lock(proj);
            match assetd::texture::process_texture(&p, asset_path, &ops) {
                Ok(o) => Ok(json!({
                    "outputAssetPath": o.output_rel, "guid": o.guid,
                    "width": o.width, "height": o.height, "bytes": o.bytes
                })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "mesh_inspect" => {
            let asset_path = args.get("assetPath").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 assetPath"))?;
            let p = lock(proj);
            match assetd::inspect::inspect_mesh(&p, asset_path) {
                Ok(r) => Ok(json!({
                    "vertices": r.vertices, "triangles": r.triangles,
                    "meshlets": r.meshlets, "lods": r.lods,
                    "materials": r.materials,
                    "bounds": r.bounds.map(|(mn, mx)| json!({ "min": mn, "max": mx })),
                    "artifact": r.artifact
                })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "inspect_3d_asset" => {
            let path = args.get("path")
                .or_else(|| args.get("assetPath"))
                .and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 path 或 assetPath"))?;
            let generate_three_view = args.get("generateThreeView")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let panel_size = args.get("panelSize")
                .and_then(Value::as_u64)
                .unwrap_or(256) as u32;

            let p = lock(proj);
            match assetd::inspect::inspect_3d_asset(&p, path, generate_three_view, panel_size) {
                Ok(res) => Ok(json!({
                    "path": res.path,
                    "polycount": res.polycount,
                    "dimensions": {
                        "x": res.dimensions.x,
                        "y": res.dimensions.y,
                        "z": res.dimensions.z
                    },
                    "boundingBox": {
                        "min": res.bounding_box.min,
                        "max": res.bounding_box.max
                    },
                    "materials": res.materials,
                    "threeViewSheetUri": res.three_view_sheet_uri,
                    "dataUrl": res.data_url
                })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "asset_cleanup_scan" => {
            let p = lock(proj);
            match assetd::cleanup::scan_cleanup(&p) {
                Ok(r) => Ok(json!({
                    "scanned": r.scanned,
                    "proposals": r.proposals.iter().map(|p| json!({
                        "assetPath": p.asset_path, "guid": p.guid, "issue": p.issue,
                        "destFolder": p.dest_folder, "newName": p.new_name, "reason": p.reason
                    })).collect::<Vec<_>>(),
                    "impact": r.impact.iter().map(|(k, n)| json!({ "issue": k, "count": n })).collect::<Vec<_>>()
                })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        // ---- sprite_*(F-GAME-4:精灵图集资产面) ----
        "sprite_create" => {
            let name = args.get("name").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 name"))?;
            let texture = args.get("texture").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 texture"))?;
            let dest = args.get("destFolder").and_then(Value::as_str).unwrap_or("");
            let pivot: Option<[f32; 2]> = args.get("pivot")
                .and_then(|v| serde_json::from_value(v.clone()).ok());
            let clips = args.get("clips").and_then(|v| v.as_object());
            let animator = args.get("animator");
            let autoslice = args.get("autoslice").and_then(Value::as_bool).unwrap_or(false);
            let p = lock(proj);
            // frames 来源:显式传入 > autoslice 检出 > 空(编辑器后续补)。
            let frames_owned: Option<serde_json::Map<String, Value>> = if autoslice {
                let mut opts = assetd::sprite::SliceOptions::default();
                if let Some(m) = args.get("minArea").and_then(Value::as_u64) {
                    opts.min_area = m as u32;
                }
                // GUID → 相对路径(autoslice 面接受路径;此处按 GUID 反查)。
                let rel = p.scan_content()
                    .ok()
                    .and_then(|rels| rels.into_iter().find(|r| {
                        let mp = meta_path_for(&p.content_root(), r);
                        mp.is_file() && MetaDoc::load(&mp).map(|m| m.guid == texture).unwrap_or(false)
                    }));
                match rel {
                    Some(rel) => match assetd::sprite::autoslice_texture(&p, &rel, opts) {
                        Ok(out) => Some(assetd::sprite::frames_from_boxes("frame", &out.boxes)),
                        Err(e) => return Ok(json!({ "error": e.code, "message": e.message })),
                    },
                    None => return Ok(json!({ "error": "UNKNOWN_GUID", "message": format!("texture GUID 不存在: {texture}") })),
                }
            } else {
                args.get("frames").and_then(|v| v.as_object()).cloned()
            };
            match assetd::sprite::create_sprite(
                &p, dest, name, texture, pivot, frames_owned.as_ref(), clips, animator,
            ) {
                Ok(c) => Ok(json!({
                    "assetPath": c.asset_path, "guid": c.guid,
                    "textureGuid": c.texture_guid,
                    "frameCount": c.frame_count, "clipCount": c.clip_count
                })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "sprite_get" => {
            let asset_path = args.get("assetPath").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 assetPath"))?;
            let p = lock(proj);
            let rel = normalize_rel(asset_path).map_err(|e| err(Value::Null, -32602, &e.message))?;
            let abs = p.content_root().join(&rel);
            match assetd::sprite::load_rxsprite(&abs) {
                Ok(doc) => {
                    let meta_path = meta_path_for(&p.content_root(), &rel);
                    let guid = MetaDoc::load(&meta_path).map(|m| m.guid).unwrap_or_default();
                    Ok(json!({ "guid": guid, "doc": serde_json::to_value(&doc).unwrap_or(Value::Null) }))
                }
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "sprite_set" => {
            let asset_path = args.get("assetPath").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 assetPath"))?;
            let doc = args.get("doc")
                .filter(|v| v.is_object())
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 doc 对象"))?;
            let p = lock(proj);
            match assetd::sprite::write_sprite_doc(&p, asset_path, doc) {
                Ok(parsed) => Ok(json!({
                    "ok": true,
                    "frameCount": parsed.frames.len(),
                    "clipCount": parsed.clips.len()
                })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "sprite_autoslice" => {
            let asset_path = args.get("assetPath").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 assetPath"))?;
            let mut opts = assetd::sprite::SliceOptions::default();
            if let Some(m) = args.get("minArea").and_then(Value::as_u64) {
                opts.min_area = m as u32;
            }
            if let Some(a) = args.get("alphaThreshold").and_then(Value::as_u64) {
                opts.alpha_threshold = a.min(255) as u8;
            }
            let p = lock(proj);
            match assetd::sprite::autoslice_texture(&p, asset_path, opts) {
                Ok(out) => Ok(json!({
                    "width": out.width, "height": out.height,
                    "boxes": out.boxes.iter().map(|b| json!([b[0], b[1], b[2], b[3]])).collect::<Vec<_>>()
                })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        // ---- rig_*(F-GAME-5 / D-050:Live2D 风格 2D 绑骨 .rxrig) ----
        "rig_create" => {
            let name = args.get("name").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 name"))?;
            let texture = args.get("texture").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 texture"))?;
            let canvas_size: Option<[f32; 2]> = args.get("canvasSize")
                .and_then(|v| serde_json::from_value(v.clone()).ok());
            let template = args.get("template").and_then(Value::as_str);
            let dest = args.get("destFolder").and_then(Value::as_str).unwrap_or("");
            let origin = args.get("origin").and_then(Value::as_str);
            let p = lock(proj);
            match assetd::rig::create_rig(&p, dest, name, texture, canvas_size, template, origin) {
                Ok(c) => Ok(json!({
                    "assetPath": c.asset_path,
                    "guid": c.guid,
                    "textureGuid": c.texture_guid,
                    "stats": c.stats
                })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "rig_get" => {
            let sections: Option<Vec<String>> = match args.get("sections") {
                Some(v) => Some(
                    serde_json::from_value(v.clone())
                        .map_err(|_| err(Value::Null, -32602, "invalid params: sections 须为字符串数组"))?,
                ),
                None => None,
            };
            let include_mesh = args.get("includeMesh").and_then(Value::as_bool).unwrap_or(false);
            let p = lock(proj);
            let (rel, guid) = match resolve_rig_arg(&p, &args) {
                Ok(x) => x,
                Err(r) => return r,
            };
            match assetd::rig::load_rel(&p, &rel) {
                Ok(doc) => match assetd::rig::doc_view(&doc, sections.as_deref(), include_mesh) {
                    Ok(view) => Ok(json!({
                        "assetPath": rel,
                        "guid": guid,
                        "stats": doc.stats(),
                        "doc": view
                    })),
                    Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
                },
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "rig_set" => {
            let doc = args.get("doc")
                .filter(|v| v.is_object())
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 doc 对象"))?;
            let p = lock(proj);
            let (rel, guid) = match resolve_rig_arg(&p, &args) {
                Ok(x) => x,
                Err(r) => return r,
            };
            match assetd::rig::write_rig_doc(&p, &rel, doc) {
                Ok(parsed) => Ok(json!({
                    "assetPath": rel,
                    "guid": guid,
                    "stats": parsed.stats()
                })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "rig_patch" => {
            let ops = args.get("ops")
                .filter(|v| v.is_array())
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 ops 数组"))?;
            let dry_run = args.get("dryRun").and_then(Value::as_bool).unwrap_or(false);
            let p = lock(proj);
            let (rel, _guid) = match resolve_rig_arg(&p, &args) {
                Ok(x) => x,
                Err(r) => return r,
            };
            match assetd::rig::patch_rig(&p, &rel, ops, dry_run) {
                Ok(out) => Ok(json!({
                    "applied": out.applied,
                    "dryRun": out.dry_run,
                    "stats": out.stats,
                    "destructive": out.destructive
                })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "rig_mesh_auto" => {
            let parts: Option<Vec<String>> = match args.get("parts") {
                Some(v) => Some(
                    serde_json::from_value(v.clone())
                        .map_err(|_| err(Value::Null, -32602, "invalid params: parts 须为字符串数组"))?,
                ),
                None => None,
            };
            let mut opts = assetd::rig::MeshAutoOpts::default();
            if let Some(v) = args.get("maxError").and_then(Value::as_f64) {
                opts.max_error = v as f32;
            }
            if let Some(v) = args.get("interiorSpacing").and_then(Value::as_f64) {
                opts.interior_spacing = v as f32;
            }
            if let Some(v) = args.get("alphaThreshold").and_then(Value::as_f64) {
                opts.alpha_threshold = v as f32;
            }
            if let Some(v) = args.get("maxVertices").and_then(Value::as_u64) {
                opts.max_vertices = v as usize;
            }
            let p = lock(proj);
            let (rel, _guid) = match resolve_rig_arg(&p, &args) {
                Ok(x) => x,
                Err(r) => return r,
            };
            match assetd::rig::mesh_auto_parts(&p, &rel, parts.as_deref(), &opts) {
                Ok(out) => Ok(json!({
                    "parts": out.parts.iter().map(|pt| json!({
                        "id": pt.id,
                        "vertices": pt.vertices,
                        "triangles": pt.triangles
                    })).collect::<Vec<_>>(),
                    "weightsCleared": out.weights_cleared,
                    "keyformsCleared": out.keyforms_cleared,
                    "stats": out.stats
                })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "rig_weights_auto" => {
            let parts: Option<Vec<String>> = match args.get("parts") {
                Some(v) => Some(
                    serde_json::from_value(v.clone())
                        .map_err(|_| err(Value::Null, -32602, "invalid params: parts 须为字符串数组"))?,
                ),
                None => None,
            };
            let mut opts = assetd::rig::WeightOpts::default();
            if let Some(v) = args.get("falloff").and_then(Value::as_f64) {
                opts.falloff = v as f32;
            }
            if let Some(v) = args.get("smooth").and_then(Value::as_bool) {
                opts.smooth = v;
            }
            if let Some(v) = args.get("bones") {
                opts.bones = Some(
                    serde_json::from_value(v.clone())
                        .map_err(|_| err(Value::Null, -32602, "invalid params: bones 须为字符串数组"))?,
                );
            }
            let p = lock(proj);
            let (rel, _guid) = match resolve_rig_arg(&p, &args) {
                Ok(x) => x,
                Err(r) => return r,
            };
            match assetd::rig::weights_auto_parts(&p, &rel, parts.as_deref(), &opts) {
                Ok(out) => Ok(json!({
                    "parts": out.parts.iter().map(|pt| json!({
                        "id": pt.id,
                        "dominantBones": pt.dominant_bones
                    })).collect::<Vec<_>>(),
                    "skippedRigid": out.skipped_rigid,
                    "stats": out.stats
                })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "rig_evaluate" => {
            let params_map: std::collections::BTreeMap<String, f32> = match args.get("params") {
                Some(v) => serde_json::from_value(v.clone())
                    .map_err(|_| err(Value::Null, -32602, "invalid params: params 须为 {id: number}"))?,
                None => std::collections::BTreeMap::new(),
            };
            let clip = args.get("clip").and_then(Value::as_str).filter(|s| !s.is_empty());
            let time = args.get("time").and_then(Value::as_f64).unwrap_or(0.0) as f32;
            let parts: Option<Vec<String>> = match args.get("parts") {
                Some(v) => Some(
                    serde_json::from_value(v.clone())
                        .map_err(|_| err(Value::Null, -32602, "invalid params: parts 须为字符串数组"))?,
                ),
                None => None,
            };
            let p = lock(proj);
            let (rel, _guid) = match resolve_rig_arg(&p, &args) {
                Ok(x) => x,
                Err(r) => return r,
            };
            match assetd::rig::load_rel(&p, &rel) {
                Ok(doc) => match assetd::rig::evaluate_json(&doc, &params_map, clip, time, parts.as_deref()) {
                    Ok(v) => Ok(v),
                    Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
                },
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "rig_preview" => {
            let req: assetd::rig::PreviewRequest = serde_json::from_value(args.clone())
                .map_err(|e| err(Value::Null, -32602, &format!("invalid params: {e}")))?;
            let p = lock(proj);
            let (rel, guid) = match resolve_rig_arg(&p, &args) {
                Ok(x) => x,
                Err(r) => return r,
            };
            match assetd::rig::preview_rig(&p, &rel, &guid, &req) {
                Ok(out) => {
                    let mut resp = json!({
                        "fileRef": out.file_ref,
                        "width": out.width,
                        "height": out.height
                    });
                    if let Some(iou) = out.silhouette_iou {
                        resp["silhouetteIoU"] = json!(iou);
                    }
                    if !out.unknown.is_empty() {
                        resp["unknown"] = json!(out.unknown);
                    }
                    Ok(resp)
                }
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "rig_import_psd" => {
            let psd_ref = args.get("psdRef").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 psdRef"))?;
            let name = args.get("name").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 name"))?;
            let dest = args.get("destFolder").and_then(Value::as_str).unwrap_or("");
            let p = lock(proj);
            let psd_abs = match assetd::rig::resolve_project_file(&p, psd_ref) {
                Ok(abs) => abs,
                Err(e) => return Ok(json!({ "error": e.code, "message": e.message })),
            };
            match assetd::rig::import_psd(&p, &psd_abs, name, dest) {
                Ok(out) => {
                    let mut resp = json!({
                        "assetPath": out.asset_path,
                        "guid": out.guid,
                        "atlasGuid": out.atlas_guid,
                        "atlasPath": out.atlas_path,
                        "parts": out.parts.iter().map(|pt| json!({
                            "id": pt.id,
                            "drawOrder": pt.draw_order,
                            "canvasRect": pt.canvas_rect
                        })).collect::<Vec<_>>()
                    });
                    if !out.warnings.is_empty() {
                        resp["warnings"] = json!(out.warnings);
                    }
                    Ok(resp)
                }
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "rig_parts_from_sheet" => {
            let texture = args.get("texture").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 texture"))?;
            let name = args.get("name").and_then(Value::as_str)
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 name"))?;
            let canvas_size: [f32; 2] = args.get("canvasSize")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 canvasSize [w,h]"))?;
            let parts_val = args.get("parts")
                .filter(|v| v.is_array())
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 parts 数组"))?;
            let parts: Vec<assetd::rig::SheetPart> = serde_json::from_value(parts_val.clone())
                .map_err(|e| err(Value::Null, -32602, &format!("invalid params: parts 格式错误: {e}")))?;
            let dest = args.get("destFolder").and_then(Value::as_str).unwrap_or("");
            let origin = args.get("origin").and_then(Value::as_str);
            let p = lock(proj);
            match assetd::rig::parts_from_sheet(&p, texture, name, canvas_size, &parts, dest, origin) {
                Ok(c) => Ok(json!({
                    "assetPath": c.asset_path,
                    "guid": c.guid,
                    "textureGuid": c.texture_guid,
                    "stats": c.stats
                })),
                Err(e) => Ok(json!({ "error": e.code, "message": e.message })),
            }
        }
        "rig_validate" => {
            let p = lock(proj);
            if let Some(doc) = args.get("doc") {
                return match assetd::rig::validate_value(&p, doc) {
                    Ok(parsed) => Ok(json!({ "ok": true, "stats": parsed.stats() })),
                    Err(e) => Ok(json!({ "ok": false, "error": { "code": e.code, "message": e.message } })),
                };
            }
            let asset_path = args.get("assetPath").and_then(Value::as_str).filter(|s| !s.is_empty());
            let guid = args.get("guid").and_then(Value::as_str).filter(|s| !s.is_empty());
            if asset_path.is_none() && guid.is_none() {
                return Err(err(Value::Null, -32602, "invalid params: 须提供 doc、assetPath 或 guid"));
            }
            let check = assetd::rig::resolve_rig(&p, asset_path, guid)
                .and_then(|(rel, _)| assetd::rig::load_rel(&p, &rel))
                .and_then(|doc| {
                    assetd::rig::texture_rel(&p, &doc.texture)?;
                    Ok(doc)
                });
            match check {
                Ok(doc) => Ok(json!({ "ok": true, "stats": doc.stats() })),
                Err(e) => Ok(json!({ "ok": false, "error": { "code": e.code, "message": e.message } })),
            }
        }
        _ => Err(err(Value::Null, -32602, &format!("未知工具:{name}"))),
    }
}

pub fn serve_stdio(proj: Arc<Mutex<ForgeProject>>) {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = match line { Ok(l) => l, Err(_) => break };
        if line.trim().is_empty() { continue; }
        let req: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let resp = err(Value::Null, -32700, &format!("parse error: {e}"));
                let _ = writeln!(stdout, "{resp}");
                let _ = stdout.flush();
                continue;
            }
        };
        let id = req.get("id").cloned();
        let method = req.get("method").and_then(Value::as_str).unwrap_or("");
        let resp: Option<Value> = match method {
            "initialize" => id.map(|i| ok(i, json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "asset-pipeline-mcp", "version": env!("CARGO_PKG_VERSION") }
            }))),
            "notifications/initialized" | "notifications/cancelled" => None,
            "ping" => id.map(|i| ok(i, json!({}))),
            "tools/list" => id.map(|i| ok(i, tool_list())),
            "tools/call" => id.map(|i| {
                let params = req.get("params").cloned().unwrap_or(Value::Null);
                match call_tool(&proj, &params) {
                    Ok(result) => ok(i, tool_wrap(&result, false)),
                    Err(mut e) => { e["id"] = i.clone(); e }
                }
            }),
            "" => id.map(|i| err(i, -32600, "invalid request: 缺 method")),
            other => id.map(|i| err(i, -32601, &format!("method not found: {other}"))),
        };
        if let Some(r) = resp {
            let _ = writeln!(stdout, "{r}");
            let _ = stdout.flush();
        }
    }
}
