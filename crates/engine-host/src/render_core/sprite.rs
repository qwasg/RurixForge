//! 精灵解析、渲染态变换与可见性判据(02 §3.3),自 viewport.rs 逐字搬来。
//! `sprite_render_transform` 渲染与点选共用;`blend` 返回中立的 `SpriteBlend`,
//! 到 `rex::BlendMode` 的映射留在 rurix 侧(viewport.rs)。

use forge_scene::Transform;
use serde_json::Value;

use super::assets::{load_tex_static_cached, sprite_doc_cached, TexGpu};
use super::math::{m3_apply, quat_to_mat3, v3_add};

/// 实体是否参与视口渲染/点选(enabled MeshRenderer / Sprite / Text)。
pub(crate) fn is_renderable(e: &forge_scene::Entity) -> bool {
    e.components
        .iter()
        .any(|c| (c.ctype == "MeshRenderer" || c.ctype == "Sprite" || c.ctype == "Text") && c.enabled)
}

/// 启用态 2D 绘制组件:Sprite(F-GAME-3)优先,其次 Text(D-045,光栅化成贴图后同走精灵腿)。
pub(crate) fn sprite_component(e: &forge_scene::Entity) -> Option<&forge_scene::Component> {
    e.components
        .iter()
        .find(|c| c.ctype == "Sprite" && c.enabled)
        .or_else(|| e.components.iter().find(|c| c.ctype == "Text" && c.enabled))
}

/// 是否文字组件(D-045:恒 alpha 混合、无色键)。
pub(crate) fn is_text(c: &forge_scene::Component) -> bool {
    c.ctype == "Text"
}

/// 该 2D 组件是否关闭品红色键(Text 恒关:字形边缘的紫粉色不能被抠掉)。
pub(crate) fn chroma_none(c: &forge_scene::Component) -> bool {
    is_text(c) || c.props.get("chromaKey").and_then(Value::as_str) == Some("none")
}

/// Sprite 排序键(sortingOrder;缺省/非 Sprite = 0.0,与旧贴图 quad 行为一致)。
#[cfg_attr(not(feature = "backend-rurix"), allow(dead_code))]
pub(crate) fn sprite_sorting_order(e: &forge_scene::Entity) -> f64 {
    sprite_component(e)
        .and_then(|c| c.props.get("sortingOrder").and_then(Value::as_f64))
        .unwrap_or(0.0)
}

/// Sprite 数值字段读取(带缺省,与 forge-scene 注册表缺省一致)。
fn sprite_num(c: &forge_scene::Component, key: &str, default: f64) -> f64 {
    c.props.get(key).and_then(Value::as_f64).unwrap_or(default)
}

/// Sprite 布尔字段读取(缺省 false)。
#[cfg_attr(not(feature = "backend-rurix"), allow(dead_code))]
pub(crate) fn sprite_bool(c: &forge_scene::Component, key: &str) -> bool {
    c.props.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// 整图 uv_rect(offset 0 + 全尺寸;texture 直贴模式 / 消隐槽用)。
pub(crate) const FULL_UV_RECT: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

/// Sprite.blendMode 三值;与 `rex::BlendMode` 一一映射(映射在 rurix 侧 viewport.rs)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(feature = "backend-rurix"), allow(dead_code))]
pub enum SpriteBlend {
    Opaque,
    Alpha,
    Additive,
}

/// 自 viewport::sprite_blend 搬来,判据不变,返回中立的 `SpriteBlend`(原返回 `rex::BlendMode`)。
#[cfg_attr(not(feature = "backend-rurix"), allow(dead_code))]
pub(crate) fn blend(e: &forge_scene::Entity) -> SpriteBlend {
    if sprite_component(e).is_some_and(is_text) {
        return SpriteBlend::Alpha;
    }
    match sprite_component(e).and_then(|s| s.props.get("blendMode")).and_then(Value::as_str) {
        Some("alpha") => SpriteBlend::Alpha,
        Some("additive") => SpriteBlend::Additive,
        _ => SpriteBlend::Opaque,
    }
}

/// 自 viewport 逐字搬来:compositing = [chroma≠none, blend≠opaque, 0, 0]。
#[cfg_attr(not(feature = "backend-rurix"), allow(dead_code))]
pub(crate) fn sprite_compositing(e: &forge_scene::Entity) -> [f32; 4] {
    let keyed = sprite_component(e).is_some_and(|s| !chroma_none(s));
    [if keyed { 1. } else { 0. },
     if blend(e) == SpriteBlend::Opaque { 0. } else { 1. }, 0., 0.]
}

/// Sprite 实体的渲染解析结果(texture 直贴 / .rxsprite 图集两模式统一出口)。
pub struct SpriteRenderInfo {
    pub tex: &'static TexGpu,
    /// 图集子矩形(offset.xy + scale.zw,0..1 贴图空间;整图 = FULL_UV_RECT)。
    pub uv_rect: [f32; 4],
    /// 当前帧像素宽高(世界尺寸 = scale × 帧像素/ppu)。
    pub frame_px: [f32; 2],
    /// 图像空间锚点(0..1,y 向下;texture 直贴恒 [0.5,0.5] 居中 = F-GAME-3 行为不变)。
    pub pivot: [f32; 2],
}

/// 解析 Sprite 组件当前帧(F-GAME-4):
/// - `sprite`(.rxsprite GUID)非空 → 图集模式:clip 非空取 clip 第 frame 帧
///   (钳制到末帧);clip 空则 frame 为 frames 键序下标;frames 为空 → 整图 + 文档 pivot;
/// - 否则 `texture` 非空 → 整图直贴(居中锚,行为与 F-GAME-3 一致);
/// - 两者皆空/解析失败 → None(实体回退 cube 占位腿,与既有坏引用行为同形)。
/// - `project_root`:项目根。中立版改为参数(02 §3.3),rurix 包装(viewport.rs)传 `rpc::project_root()`,值与搬迁前在此处取的相同。
pub(crate) fn resolve_sprite_render(c: &forge_scene::Component, project_root: &std::path::Path) -> Option<SpriteRenderInfo> {
    if is_text(c) {
        return super::text::resolve_text_render(c, project_root);
    }
    let packed_frame = sprite_num(c, "frame", 0.0).max(0.0) as usize;
    let variants = c.props.get("spriteVariants").and_then(Value::as_array).filter(|a| !a.is_empty());
    let (sprite_guid, frame_idx) = if let Some(variants) = variants {
        let stride = sprite_num(c, "variantStride", 0.0);
        if !stride.is_finite() || stride < 1. || stride.fract() != 0. { return None; }
        let stride = stride as usize;
        (variants.get(packed_frame / stride)?.as_str()?, packed_frame % stride)
    } else {
        (c.props.get("sprite").and_then(Value::as_str).unwrap_or(""), packed_frame)
    };
    if !sprite_guid.is_empty() {
        let doc = sprite_doc_cached(sprite_guid)?;
        let tex = load_tex_static_cached(project_root, &doc.texture)?;
        let clip_name = c.props.get("clip").and_then(Value::as_str).unwrap_or("");
        let frame = if !clip_name.is_empty() {
            doc.clips.get(clip_name).and_then(|clip| {
                let idx = frame_idx.min(clip.frames.len().saturating_sub(1));
                let name = clip.frames.get(idx)?;
                doc.frames.get(name).map(|f| (name.clone(), f.clone()))
            })
        } else {
            let keys: Vec<&String> = doc.frames.keys().collect();
            keys.get(frame_idx.min(keys.len().saturating_sub(1)))
                .map(|k| ((*k).clone(), doc.frames[*k].clone()))
        };
        return Some(match frame {
            Some((name, f)) => {
                let (tw, th) = (tex.w.max(1) as f32, tex.h.max(1) as f32);
                let bbox = f.bbox;
                SpriteRenderInfo {
                    tex,
                    uv_rect: [
                        bbox[0] as f32 / tw,
                        bbox[1] as f32 / th,
                        bbox[2] as f32 / tw,
                        bbox[3] as f32 / th,
                    ],
                    frame_px: [bbox[2] as f32, bbox[3] as f32],
                    pivot: doc.resolve_pivot(&name),
                }
            }
            // frames 为空(新建未切帧):整图 + 文档 pivot(诚实可见,编辑器可继续切)。
            None => SpriteRenderInfo {
                tex,
                uv_rect: FULL_UV_RECT,
                frame_px: [tex.w as f32, tex.h as f32],
                pivot: doc.pivot,
            },
        });
    }
    let tex_guid = c.props.get("texture").and_then(Value::as_str).unwrap_or("");
    if tex_guid.is_empty() {
        return None;
    }
    let tex = load_tex_static_cached(project_root, tex_guid)?;
    Some(SpriteRenderInfo {
        tex,
        uv_rect: FULL_UV_RECT,
        frame_px: [tex.w as f32, tex.h as f32],
        pivot: [0.5, 0.5],
    })
}

/// Sprite 实体渲染态等效变换(F-GAME-4:帧尺寸缩放 + pivot 锚定平移),
/// 渲染模型矩阵与点选 OBB 共用,保证画面与点选一致。返回 None = 非 Sprite 实体。
/// 锚定:图像空间 pivot(y 向下)→ 单位 quad 局部偏移 (0.5-px, py-0.5),
/// 经旋转与有效缩放折入 translation,使锚点恰落在实体 translation 上。
pub(crate) fn sprite_render_transform(e: &forge_scene::Entity) -> Option<Transform> {
    let c = sprite_component(e)?;
    let ppu = sprite_num(c, "pixelsPerUnit", 100.0).max(1.0) as f32;
    let info = resolve_sprite_render(c, &crate::rpc::project_root());
    let (fw, fh, pivot) = match &info {
        Some(i) => (i.frame_px[0], i.frame_px[1], i.pivot),
        // 贴图未解析:回退 1×1 居中(与旧 quad 腿同形)。
        None => (ppu, ppu, [0.5, 0.5]),
    };
    let s = e.transform.scale;
    let scale = [s[0] * fw / ppu, s[1] * fh / ppu, s[2].max(1e-3)];
    let offset_local = [0.5 - pivot[0], pivot[1] - 0.5, 0.0];
    let rot = quat_to_mat3(e.transform.rotation);
    let world_off = m3_apply(
        rot,
        [
            offset_local[0] * scale[0],
            offset_local[1] * scale[1],
            0.0,
        ],
    );
    Some(Transform {
        translation: v3_add(e.transform.translation, world_off),
        rotation: e.transform.rotation,
        scale,
    })
}

/// 实体网格引用(MeshRenderer.props.mesh;缺省/空 = 内置 cube)。
#[cfg_attr(not(feature = "backend-rurix"), allow(dead_code))]
pub(crate) fn entity_mesh_ref(e: &forge_scene::Entity) -> String {
    e.components
        .iter()
        .find(|c| c.ctype == "MeshRenderer" && c.enabled)
        .and_then(|c| c.props.get("mesh").and_then(Value::as_str))
        .filter(|s| !s.is_empty())
        .unwrap_or("cube")
        .to_string()
}
