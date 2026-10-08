//! Stage 6 精灵 CPU 合约测试。GPU 场景测试由父会话统一串行执行。
use engine_host::{ItemBody, SpriteBlend, SpriteDraw, TexData};

fn sprite_body() -> ItemBody {
    ItemBody::Sprite(SpriteDraw {
        tex: TexData { guid: "test".into(), w: 1, h: 1, rgba: &[255, 255, 255, 255] },
        graph: None,
        uv_rect: [0., 0., 1., 1.],
        frame_px: [1., 1.],
        pivot: [0.5, 0.88],
        tint: [1.; 4],
        flip: [true, false],
        blend: SpriteBlend::Alpha,
        chroma_key: false,
        sorting_order: 2.,
        selected: false,
    })
}

#[test]
fn normal_sprite_has_a_dedicated_render_body() {
    let ItemBody::Sprite(draw) = sprite_body() else { panic!("Sprite fell through to mesh body") };
    assert_eq!(draw.tex.guid, "test");
    assert_eq!(draw.pivot, [0.5, 0.88]);
    assert_eq!(draw.flip, [true, false]);
    assert_eq!(draw.blend, SpriteBlend::Alpha);
    assert_eq!(draw.sorting_order, 2.0);
}

#[test]
fn sprite_blend_modes_are_explicit_and_distinct() {
    assert_ne!(SpriteBlend::Opaque, SpriteBlend::Alpha);
    assert_ne!(SpriteBlend::Alpha, SpriteBlend::Additive);
    assert_ne!(SpriteBlend::Opaque, SpriteBlend::Additive);
}

#[test]
fn sprite_body_keeps_chroma_key_and_additive_semantics_in_one_record() {
    let ItemBody::Sprite(mut draw) = sprite_body() else { unreachable!() };
    draw.chroma_key = true;
    draw.blend = SpriteBlend::Additive;
    assert!(draw.chroma_key);
    assert_eq!(draw.blend, SpriteBlend::Additive);
}
