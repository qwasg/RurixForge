//! Adversarial Boundary & Stress Test Suite for Five-Stage Objective Verification Gates (Gate 1~5)
//!
//! Validates fail-closed properties, numeric precision limits, and boundary error handling
//! in `crates/gend/src/gates.rs` as specified in Milestone 2 Challenger mandate.

use gend::gates::{
    build_synthetic_glb, create_test_image_bytes,
    validate_gate_1_concept_art_bytes,
    validate_gate_2_raw_glb_bytes,
    validate_gate_3_polycount_bytes,
    validate_gate_4_rigging_bytes,
    validate_gate_5_motion_and_publish_bytes,
    SyntheticGlbConfig,
    GATE1_IMAGE_INVALID, GATE2_MODEL_INVALID,
    GATE3_POLYCOUNT_EXCEEDED, GATE3_DEGENERATE_MESH,
    GATE4_RIGGING_INVALID, GATE5_ANIMATION_INVALID, GATE5_PUBLISH_FAILED,
};
use serde_json::json;
use std::path::PathBuf;

fn make_temp_dir(suffix: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("adv-gate-test-{suffix}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// ============================================================================
// GATE 1 ADVERSARIAL STRESS TESTS
// ============================================================================

#[test]
fn test_adv_g1_sub_256_resolutions() {
    // 255x255: Just one pixel short on both axes
    let b_255_255 = create_test_image_bytes(255, 255, "silhouette");
    let res = validate_gate_1_concept_art_bytes(&b_255_255);
    assert!(res.is_err(), "255x255 must be rejected");
    assert_eq!(res.unwrap_err().error_code, GATE1_IMAGE_INVALID);

    // 256x255: Width meets threshold, height is 1 pixel short
    let b_256_255 = create_test_image_bytes(256, 255, "silhouette");
    let res = validate_gate_1_concept_art_bytes(&b_256_255);
    assert!(res.is_err(), "256x255 must be rejected");
    assert_eq!(res.unwrap_err().error_code, GATE1_IMAGE_INVALID);

    // 255x256: Height meets threshold, width is 1 pixel short
    let b_255_256 = create_test_image_bytes(255, 256, "silhouette");
    let res = validate_gate_1_concept_art_bytes(&b_255_256);
    assert!(res.is_err(), "255x256 must be rejected");
    assert_eq!(res.unwrap_err().error_code, GATE1_IMAGE_INVALID);

    // 1x1: Degenerate micro image
    let b_1_1 = create_test_image_bytes(1, 1, "silhouette");
    let res = validate_gate_1_concept_art_bytes(&b_1_1);
    assert!(res.is_err(), "1x1 must be rejected");
    assert_eq!(res.unwrap_err().error_code, GATE1_IMAGE_INVALID);

    // 256x256: Minimum accepted boundary
    let b_256_256 = create_test_image_bytes(256, 256, "silhouette");
    let res = validate_gate_1_concept_art_bytes(&b_256_256);
    assert!(res.is_ok(), "256x256 silhouette must be accepted: {:?}", res.err());
}

#[test]
fn test_adv_g1_transparent_and_coverage_boundaries() {
    // 1. Completely transparent PNG
    let b_transparent = create_test_image_bytes(256, 256, "transparent");
    let res = validate_gate_1_concept_art_bytes(&b_transparent);
    assert!(res.is_err());
    let err = res.unwrap_err();
    assert_eq!(err.error_code, GATE1_IMAGE_INVALID);
    assert!(err.message.contains("empty/transparent"));

    // 2. Sub-1% coverage boundary (e.g. 0.3% coverage: 200 non-transparent pixels in 256x256 = 65536)
    let mut img = image::RgbaImage::new(256, 256);
    for i in 0..200 {
        img.put_pixel(i % 256, i / 256, image::Rgba([200, 100, 50, 255]));
    }
    let mut bytes = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png).unwrap();
    let res = validate_gate_1_concept_art_bytes(&bytes);
    assert!(res.is_err(), "Sub-1% coverage must be rejected");
    assert_eq!(res.unwrap_err().error_code, GATE1_IMAGE_INVALID);

    // 3. Above-1% coverage boundary (e.g. 2% coverage: 1400 non-transparent pixels in 256x256)
    let mut img2 = image::RgbaImage::new(256, 256);
    for i in 0..1400 {
        img2.put_pixel(i % 256, i / 256, image::Rgba([200, 100, 50, 255]));
    }
    let mut bytes2 = Vec::new();
    img2.write_to(&mut std::io::Cursor::new(&mut bytes2), image::ImageFormat::Png).unwrap();
    let res2 = validate_gate_1_concept_art_bytes(&bytes2);
    assert!(res2.is_ok(), "Above 1% coverage must pass: {:?}", res2.err());
}

#[test]
fn test_adv_g1_solid_color_canvases() {
    // Solid white canvas (contrast variance = 0 < 10)
    let mut img_white = image::RgbaImage::new(256, 256);
    for p in img_white.pixels_mut() {
        *p = image::Rgba([255, 255, 255, 255]);
    }
    let mut b_white = Vec::new();
    img_white.write_to(&mut std::io::Cursor::new(&mut b_white), image::ImageFormat::Png).unwrap();
    let res = validate_gate_1_concept_art_bytes(&b_white);
    assert!(res.is_err(), "Solid white canvas must be rejected");
    assert_eq!(res.unwrap_err().error_code, GATE1_IMAGE_INVALID);

    // Solid black canvas (contrast variance = 0 < 10)
    let mut img_black = image::RgbaImage::new(256, 256);
    for p in img_black.pixels_mut() {
        *p = image::Rgba([0, 0, 0, 255]);
    }
    let mut b_black = Vec::new();
    img_black.write_to(&mut std::io::Cursor::new(&mut b_black), image::ImageFormat::Png).unwrap();
    let res = validate_gate_1_concept_art_bytes(&b_black);
    assert!(res.is_err(), "Solid black canvas must be rejected");
    assert_eq!(res.unwrap_err().error_code, GATE1_IMAGE_INVALID);

    // Solid red canvas (contrast variance = 0 < 10)
    let mut img_red = image::RgbaImage::new(256, 256);
    for p in img_red.pixels_mut() {
        *p = image::Rgba([255, 0, 0, 255]);
    }
    let mut b_red = Vec::new();
    img_red.write_to(&mut std::io::Cursor::new(&mut b_red), image::ImageFormat::Png).unwrap();
    let res = validate_gate_1_concept_art_bytes(&b_red);
    assert!(res.is_err(), "Solid red canvas must be rejected");
    assert_eq!(res.unwrap_err().error_code, GATE1_IMAGE_INVALID);
}

// ============================================================================
// GATE 2 ADVERSARIAL STRESS TESTS
// ============================================================================

#[test]
fn test_adv_g2_truncated_buffers() {
    // 0 bytes
    let res = validate_gate_2_raw_glb_bytes(&[]);
    assert!(res.is_err(), "0-byte buffer must be rejected");
    assert_eq!(res.unwrap_err().error_code, GATE2_MODEL_INVALID);

    // 4 bytes: partial header
    let res = validate_gate_2_raw_glb_bytes(b"glTF");
    assert!(res.is_err(), "4-byte buffer must be rejected");
    assert_eq!(res.unwrap_err().error_code, GATE2_MODEL_INVALID);

    // 11 bytes: 1 byte short of 12-byte glTF header
    let res = validate_gate_2_raw_glb_bytes(&[0x67, 0x6c, 0x54, 0x46, 2, 0, 0, 0, 100, 0, 0]);
    assert!(res.is_err(), "11-byte buffer must be rejected");
    assert_eq!(res.unwrap_err().error_code, GATE2_MODEL_INVALID);

    // 12 bytes: declared length 1000, actual 12 bytes -> Truncated buffer
    let mut header = Vec::new();
    header.extend_from_slice(&0x4654_6C67u32.to_le_bytes()); // "glTF"
    header.extend_from_slice(&2u32.to_le_bytes()); // version 2
    header.extend_from_slice(&1000u32.to_le_bytes()); // declared 1000 bytes
    let res = validate_gate_2_raw_glb_bytes(&header);
    assert!(res.is_err(), "Truncated buffer must be rejected");
    let err = res.unwrap_err();
    assert_eq!(err.error_code, GATE2_MODEL_INVALID);
    assert!(err.message.contains("truncated"));
}

#[test]
fn test_adv_g2_corrupt_magic_signatures() {
    let magics: [&[u8]; 4] = [
        b"glTX\x02\x00\x00\x00\x20\x00\x00\x00",
        b"RIFF\x02\x00\x00\x00\x20\x00\x00\x00",
        b"\x89PNG\x02\x00\x00\x00\x20\x00\x00\x00",
        b"\x00\x00\x00\x00\x02\x00\x00\x00\x20\x00\x00\x00",
    ];
    for m in magics {
        let res = validate_gate_2_raw_glb_bytes(m);
        assert!(res.is_err(), "Corrupt magic signature must be rejected");
        assert_eq!(res.unwrap_err().error_code, GATE2_MODEL_INVALID);
    }
}

#[test]
fn test_adv_g2_non_triangles_draw_modes() {
    // Mode 0: POINTS, Mode 1: LINES, Mode 2: LINE_LOOP, Mode 3: LINE_STRIP, Mode 5: TRIANGLE_STRIP, Mode 6: TRIANGLE_FAN
    let invalid_modes = [0u32, 1, 2, 3, 5, 6];
    for mode in invalid_modes {
        let mut cfg = SyntheticGlbConfig::default();
        cfg.target_mode = mode;
        let glb = build_synthetic_glb(&cfg);
        let res = validate_gate_2_raw_glb_bytes(&glb);
        assert!(res.is_err(), "Draw mode {mode} must be rejected");
        let err = res.unwrap_err();
        assert_eq!(err.error_code, GATE2_MODEL_INVALID);
        assert!(err.message.contains("TRIANGLES"));
    }
}

#[test]
fn test_adv_g2_missing_texcoord_0() {
    let mut cfg = SyntheticGlbConfig::default();
    cfg.omit_uv0 = true;
    let glb = build_synthetic_glb(&cfg);
    let res = validate_gate_2_raw_glb_bytes(&glb);
    assert!(res.is_err(), "Model missing TEXCOORD_0 must be rejected");
    let err = res.unwrap_err();
    assert_eq!(err.error_code, GATE2_MODEL_INVALID);
    assert!(err.message.contains("TEXCOORD_0"));
}

// ============================================================================
// GATE 3 ADVERSARIAL STRESS TESTS
// ============================================================================

#[test]
fn test_adv_g3_polycount_budget_extremes() {
    let mut cfg = SyntheticGlbConfig::default();
    cfg.triangle_count = 150;
    let glb = build_synthetic_glb(&cfg);

    // Budget = 149 (1 less than actual 150) -> Must reject
    let res_over = validate_gate_3_polycount_bytes(&glb, 149);
    assert!(res_over.is_err(), "Exceeding budget by 1 must be rejected");
    assert_eq!(res_over.unwrap_err().error_code, GATE3_POLYCOUNT_EXCEEDED);

    // Budget = 150 (exact match) -> Must accept
    let res_exact = validate_gate_3_polycount_bytes(&glb, 150);
    assert!(res_exact.is_ok(), "Exact budget match must be accepted");

    // Budget = 151 (1 more than actual 150) -> Must accept
    let res_under = validate_gate_3_polycount_bytes(&glb, 151);
    assert!(res_under.is_ok(), "Under budget must be accepted");
}

#[test]
fn test_adv_g3_duplicate_indices_variations() {
    let mut cfg = SyntheticGlbConfig::default();
    cfg.duplicate_triangle_indices = true;
    let glb = build_synthetic_glb(&cfg);
    let res = validate_gate_3_polycount_bytes(&glb, 1000);
    assert!(res.is_err(), "Duplicate vertex indices within triangle must be rejected");
    let err = res.unwrap_err();
    assert_eq!(err.error_code, GATE3_DEGENERATE_MESH);
    assert!(err.message.contains("degenerate"));
}

#[test]
fn test_adv_g3_collinear_zero_area_triangles() {
    let mut cfg = SyntheticGlbConfig::default();
    cfg.zero_area_triangles = true; // All x and z set to 0.0 -> collinear on y-axis
    let glb = build_synthetic_glb(&cfg);
    let res = validate_gate_3_polycount_bytes(&glb, 1000);
    assert!(res.is_err(), "Collinear zero area triangles must be rejected");
    let err = res.unwrap_err();
    assert_eq!(err.error_code, GATE3_DEGENERATE_MESH);
    assert!(err.message.contains("degenerate"));
}

// ============================================================================
// GATE 4 ADVERSARIAL STRESS TESTS
// ============================================================================

#[test]
fn test_adv_g4_unskinned_mesh_rejection() {
    let mut cfg = SyntheticGlbConfig::default();
    cfg.include_skin = false;
    let glb = build_synthetic_glb(&cfg);
    let res = validate_gate_4_rigging_bytes(&glb);
    assert!(res.is_err(), "Unskinned mesh must be rejected by Gate 4");
    assert_eq!(res.unwrap_err().error_code, GATE4_RIGGING_INVALID);
}

#[test]
fn test_adv_g4_mismatched_ibms_and_joints() {
    let mut cfg = SyntheticGlbConfig::default();
    cfg.include_skin = true;
    cfg.mismatch_joints_ibms = true;
    let glb = build_synthetic_glb(&cfg);
    let res = validate_gate_4_rigging_bytes(&glb);
    assert!(res.is_err(), "Mismatched IBMs and joints must be rejected");
    let err = res.unwrap_err();
    assert_eq!(err.error_code, GATE4_RIGGING_INVALID);
    assert!(err.message.contains("does not match IBM count"));
}

#[test]
fn test_adv_g4_unnormalized_weights_precision_boundary() {
    // 1. Coarsely unnormalized (sum = 0.5 != 1.0)
    let mut cfg = SyntheticGlbConfig::default();
    cfg.include_skin = true;
    cfg.unnormalized_weights = true;
    let glb = build_synthetic_glb(&cfg);
    let res = validate_gate_4_rigging_bytes(&glb);
    assert!(res.is_err(), "Unnormalized weights must be rejected");
    let err = res.unwrap_err();
    assert_eq!(err.error_code, GATE4_RIGGING_INVALID);
    assert!(err.message.contains("must equal 1.0"));

    // 2. Strict numerical epsilon test around 1e-4 boundary (|sum - 1.0| > 1e-4):
    let valid_glb = build_synthetic_glb(&SyntheticGlbConfig {
        include_skin: true,
        ..Default::default()
    });
    assert!(validate_gate_4_rigging_bytes(&valid_glb).is_ok());

    let target_pattern = 0.85f32.to_le_bytes();
    let pos = valid_glb.windows(4).position(|w| w == target_pattern).expect("0.85f32 weight found in GLB");

    // Case A: sum = 1.0 + 2e-4 (0.8502 + 0.15 = 1.0002, excess = 2e-4 > 1e-4) -> Must reject
    let mut glb_over = valid_glb.clone();
    glb_over[pos..pos + 4].copy_from_slice(&0.8502f32.to_le_bytes());
    let res_over = validate_gate_4_rigging_bytes(&glb_over);
    assert!(res_over.is_err(), "Weights sum 1.0002 (> 1e-4) must be rejected");
    assert_eq!(res_over.unwrap_err().error_code, GATE4_RIGGING_INVALID);

    // Case B: sum = 1.0 + 5e-5 (0.85005 + 0.15 = 1.00005, delta = 5e-5 <= 1e-4) -> Must accept
    let mut glb_within = valid_glb.clone();
    glb_within[pos..pos + 4].copy_from_slice(&0.85005f32.to_le_bytes());
    let res_within = validate_gate_4_rigging_bytes(&glb_within);
    assert!(res_within.is_ok(), "Weights sum 1.00005 (<= 1e-4) must be accepted: {:?}", res_within.err());

    // Case C: Negative weight (-0.01) -> Must reject
    let mut glb_neg = valid_glb.clone();
    glb_neg[pos..pos + 4].copy_from_slice(&(-0.01f32).to_le_bytes());
    let res_neg = validate_gate_4_rigging_bytes(&glb_neg);
    assert!(res_neg.is_err(), "Negative skin weight must be rejected");
    assert_eq!(res_neg.unwrap_err().error_code, GATE4_RIGGING_INVALID);
}

#[test]
fn test_adv_g4_joint_index_palette_overflow() {
    let mut cfg = SyntheticGlbConfig::default();
    cfg.include_skin = true;
    cfg.palette_overflow = true; // Assigns joint index 99 when skeleton only has 3 joints
    let glb = build_synthetic_glb(&cfg);
    let res = validate_gate_4_rigging_bytes(&glb);
    assert!(res.is_err(), "Palette overflow must be rejected");
    let err = res.unwrap_err();
    assert_eq!(err.error_code, GATE4_RIGGING_INVALID);
    assert!(err.message.contains("palette"));
}

// ============================================================================
// GATE 5 ADVERSARIAL STRESS TESTS
// ============================================================================

#[test]
fn test_adv_g5_missing_idle_or_walk_tracks() {
    // Missing walk track
    let mut cfg_no_walk = SyntheticGlbConfig::default();
    cfg_no_walk.include_skin = true;
    cfg_no_walk.include_animations = true;
    cfg_no_walk.include_idle = true;
    cfg_no_walk.include_walk = false;
    let glb = build_synthetic_glb(&cfg_no_walk);
    let res = validate_gate_5_motion_and_publish_bytes(&glb, 0.01, None);
    assert!(res.is_err(), "Missing Walk track must be rejected");
    assert_eq!(res.unwrap_err().error_code, GATE5_ANIMATION_INVALID);

    // Missing idle track
    let mut cfg_no_idle = SyntheticGlbConfig::default();
    cfg_no_idle.include_skin = true;
    cfg_no_idle.include_animations = true;
    cfg_no_idle.include_idle = false;
    cfg_no_idle.include_walk = true;
    let glb2 = build_synthetic_glb(&cfg_no_idle);
    let res2 = validate_gate_5_motion_and_publish_bytes(&glb2, 0.01, None);
    assert!(res2.is_err(), "Missing Idle track must be rejected");
    assert_eq!(res2.unwrap_err().error_code, GATE5_ANIMATION_INVALID);
}

#[test]
fn test_adv_g5_non_monotonic_animation_timestamps() {
    let mut cfg = SyntheticGlbConfig::default();
    cfg.include_skin = true;
    cfg.include_animations = true;
    cfg.non_monotonic_anim = true;
    let glb = build_synthetic_glb(&cfg);
    let res = validate_gate_5_motion_and_publish_bytes(&glb, 0.01, None);
    assert!(res.is_err(), "Non-monotonic timestamps must be rejected");
    let err = res.unwrap_err();
    assert_eq!(err.error_code, GATE5_ANIMATION_INVALID);
    assert!(err.message.contains("non-monotonic"));
}

#[test]
fn test_adv_g5_stationary_vertex_animation() {
    let mut cfg = SyntheticGlbConfig::default();
    cfg.include_skin = true;
    cfg.include_animations = true;
    cfg.static_motion = true; // amplitude 0.00001
    let glb = build_synthetic_glb(&cfg);
    let res = validate_gate_5_motion_and_publish_bytes(&glb, 0.01, None);
    assert!(res.is_err(), "Stationary animation with delta < 0.01 must be rejected");
    let err = res.unwrap_err();
    assert_eq!(err.error_code, GATE5_ANIMATION_INVALID);
    assert!(err.message.contains("below threshold"));

    // Active motion GLB:
    let mut cfg_active = SyntheticGlbConfig::default();
    cfg_active.include_skin = true;
    cfg_active.include_animations = true;
    cfg_active.static_motion = false; // Real walk animation
    let glb_active = build_synthetic_glb(&cfg_active);

    // With epsilon = 0.01: delta is well above 0.01 -> Must pass
    let res_active = validate_gate_5_motion_and_publish_bytes(&glb_active, 0.01, None);
    assert!(res_active.is_ok(), "Active animation must pass at epsilon 0.01");
    let m = res_active.unwrap();
    assert!(m.max_vertex_displacement >= 0.01);

    // If epsilon is set higher than actual delta (e.g. 10.0 meters) -> Must reject
    let res_too_strict = validate_gate_5_motion_and_publish_bytes(&glb_active, 10.0, None);
    assert!(res_too_strict.is_err(), "Deformation below 10.0m threshold must be rejected");
    assert_eq!(res_too_strict.unwrap_err().error_code, GATE5_ANIMATION_INVALID);
}

#[test]
fn test_adv_g5_publication_package_validation() {
    let mut cfg = SyntheticGlbConfig::default();
    cfg.include_skin = true;
    cfg.include_animations = true;
    let glb = build_synthetic_glb(&cfg);

    // 1. Missing publication directory files
    let dir = make_temp_dir("adv_g5_pub_missing");
    let res = validate_gate_5_motion_and_publish_bytes(&glb, 0.01, Some(&dir));
    assert!(res.is_err());
    assert_eq!(res.unwrap_err().error_code, GATE5_PUBLISH_FAILED);

    // 2. Present files, but missing CharacterController in prefab
    std::fs::write(dir.join("model.rxmodel"), b"{}").unwrap();
    std::fs::write(
        dir.join("template.rxprefab"),
        serde_json::to_vec(&json!({
            "entities": [{
                "id": 1,
                "components": {
                    "Animator": { "type": "Animator" }
                    // Missing CharacterController
                }
            }]
        })).unwrap(),
    ).unwrap();
    let res2 = validate_gate_5_motion_and_publish_bytes(&glb, 0.01, Some(&dir));
    assert!(res2.is_err());
    assert_eq!(res2.unwrap_err().error_code, GATE5_PUBLISH_FAILED);

    // 3. Complete bundle with Animator and CharacterController
    std::fs::write(
        dir.join("template.rxprefab"),
        serde_json::to_vec(&json!({
            "entities": [{
                "id": 1,
                "components": {
                    "Animator": { "type": "Animator" },
                    "CharacterController": { "type": "CharacterController" }
                }
            }]
        })).unwrap(),
    ).unwrap();
    let res3 = validate_gate_5_motion_and_publish_bytes(&glb, 0.01, Some(&dir));
    assert!(res3.is_ok(), "Complete valid publication bundle must pass: {:?}", res3.err());

    std::fs::remove_dir_all(&dir).ok();
}
