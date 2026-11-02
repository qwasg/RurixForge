//! Stage 7 Live2D-style 2D 骨骼绑定 (RigMesh) CPU 合约测试。
use std::sync::Arc;
use engine_host::{
    rig_capacity, ItemBody, RigDraw, SpriteBlend, TexData, RIG_MIN_CAPACITY, RIG_VERTEX_F32,
};

fn sample_rig_draw(verts: Vec<f32>) -> RigDraw {
    let vertex_count = (verts.len() / RIG_VERTEX_F32) as u32;
    let capacity = rig_capacity(vertex_count);
    RigDraw {
        tex: TexData { guid: "rig-atlas".into(), w: 64, h: 64, rgba: &[255, 255, 255, 255] },
        verts: Arc::new(verts),
        vertex_count,
        capacity,
        tint: [1.0, 0.9, 0.8, 1.0],
        flip: [false, false],
        blend: SpriteBlend::Alpha,
        sorting_order: 3.5,
        selected: false,
    }
}

#[test]
fn rig_mesh_has_dedicated_render_body_and_bucket_capacity() {
    let verts = vec![
        -10.0, -10.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, //
        10.0, -10.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, //
        10.0, 10.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0,
    ];
    let body = ItemBody::RigMesh(sample_rig_draw(verts));
    let ItemBody::RigMesh(draw) = body else { panic!("RigMesh fell through to another body") };
    assert_eq!(draw.tex.guid, "rig-atlas");
    assert_eq!(draw.vertex_count, 3);
    assert_eq!(draw.capacity, RIG_MIN_CAPACITY);
    assert_eq!(draw.sorting_order, 3.5);
    assert_eq!(draw.tint, [1.0, 0.9, 0.8, 1.0]);
    assert_eq!(draw.blend, SpriteBlend::Alpha);
}

#[test]
fn rig_vertex_deformation_preserves_static_equality_and_reports_vertex_change() {
    let v0 = vec![
        -10.0, -10.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, //
        10.0, -10.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, //
        10.0, 10.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0,
    ];
    let v1 = vec![
        -5.0, -10.0, 0.0, 0.0, 0.0, 0.75, 0.0, 0.0, //
        15.0, -10.0, 0.0, 0.0, 0.0, 0.75, 1.0, 0.0, //
        15.0, 10.0, 0.0, 0.0, 0.0, 0.75, 1.0, 1.0,
    ];
    let draw_a = sample_rig_draw(v0);
    let draw_b = sample_rig_draw(v1);

    // Static signature matches so session/instance is NOT torn down:
    assert_eq!(draw_a, draw_b);
    assert_eq!(ItemBody::RigMesh(draw_a.clone()), ItemBody::RigMesh(draw_b.clone()));
    // But vertex comparison detects pose deformation:
    assert!(!draw_a.same_vertices(&draw_b));
    assert!(draw_a.same_vertices(&draw_a.clone()));
}
