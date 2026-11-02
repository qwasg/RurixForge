//! Integration tests for Milestone 2: Pure CPU 4-Panel Software Rasterizer,
//! Unified Geometry Extraction (.glb, .rxmodel, .rxmesh), and SHA-256 Thumbnail Caching.

use std::path::PathBuf;

use assetd::inspect::inspect_3d_asset;
use assetd::project::ForgeProject;
use assetd::rasterizer::{
    compute_normals, extract_from_glb_bytes, extract_from_model_bundle,
    MeshGeometry, PureCpuRasterizer, AABB,
};
use base64::Engine as _;

fn tmp_project(tag: &str) -> (PathBuf, ForgeProject) {
    let dir = std::env::temp_dir().join(format!("raster_test_{}_{}", tag, uuid::Uuid::new_v4()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let p = ForgeProject::with_defaults(dir.clone());
    p.ensure_dirs().unwrap();
    (dir, p)
}

#[test]
fn test_cube_rasterizer_quadrants_non_empty() {
    let rast = PureCpuRasterizer::new(64);
    let (sheet, meta) = rast.render_cube_mesh((2.0, 2.0, 2.0));
    assert_eq!(sheet.width(), 128);
    assert_eq!(sheet.height(), 128);

    // Q1: Perspective Showcase (0, 0 to 64, 64)
    let mut q1_colored = 0;
    for y in 0..64 {
        for x in 0..64 {
            if sheet.get_pixel(x, y).0 != [24, 26, 32, 255] {
                q1_colored += 1;
            }
        }
    }
    assert!(q1_colored > 30, "Q1 should have rendered pixels, got {q1_colored}");

    // Q2: Front (64, 0 to 128, 64)
    let mut q2_colored = 0;
    for y in 0..64 {
        for x in 64..128 {
            if sheet.get_pixel(x, y).0 != [24, 26, 32, 255] {
                q2_colored += 1;
            }
        }
    }
    assert!(q2_colored > 30, "Q2 Front should have rendered pixels, got {q2_colored}");

    // Q3: Side (0, 64 to 64, 128)
    let mut q3_colored = 0;
    for y in 64..128 {
        for x in 0..64 {
            if sheet.get_pixel(x, y).0 != [24, 26, 32, 255] {
                q3_colored += 1;
            }
        }
    }
    assert!(q3_colored > 30, "Q3 Side should have rendered pixels, got {q3_colored}");

    // Q4: Top (64, 64 to 128, 128)
    let mut q4_colored = 0;
    for y in 64..128 {
        for x in 64..128 {
            if sheet.get_pixel(x, y).0 != [24, 26, 32, 255] {
                q4_colored += 1;
            }
        }
    }
    assert!(q4_colored > 30, "Q4 Top should have rendered pixels, got {q4_colored}");

    assert_eq!(meta["polycount"], 12);
}

#[test]
fn test_bounding_box_and_dimensions() {
    let rast = PureCpuRasterizer::new(64);
    let (_, meta) = rast.render_cube_mesh((4.0, 2.5, 6.0));
    assert_eq!(meta["dimensions"]["x"], 4.0);
    assert_eq!(meta["dimensions"]["y"], 2.5);
    assert_eq!(meta["dimensions"]["z"], 6.0);
    assert_eq!(meta["boundingBox"]["min"][0], -2.0);
    assert_eq!(meta["boundingBox"]["max"][0], 2.0);
}

#[test]
fn test_zero_dimension_mesh_safety() {
    let rast = PureCpuRasterizer::new(64);
    let (sheet, meta) = rast.render_cube_mesh((0.0, 0.0, 0.0));
    assert_eq!(sheet.width(), 128);
    assert_eq!(sheet.height(), 128);
    assert_eq!(meta["dimensions"]["x"], 0.0);
}

#[test]
fn test_needle_aspect_ratio() {
    let rast = PureCpuRasterizer::new(64);
    let (sheet, meta) = rast.render_cube_mesh((100.0, 0.1, 0.1));
    assert_eq!(sheet.width(), 128);
    assert_eq!(sheet.height(), 128);
    assert_eq!(meta["dimensions"]["x"], 100.0);
}

#[test]
fn test_large_coordinate_bounds() {
    let aabb = AABB::new([10000.0, 10000.0, 10000.0], [10010.0, 10010.0, 10010.0]);
    assert_eq!(aabb.dimensions(), [10.0, 10.0, 10.0]);
    assert_eq!(aabb.center(), [10005.0, 10005.0, 10005.0]);
}

#[test]
fn test_data_url_format_and_png_roundtrip() {
    let rast = PureCpuRasterizer::new(64);
    let (sheet, _) = rast.render_cube_mesh((2.0, 2.0, 2.0));
    let url = rast.export_data_url(&sheet);
    assert!(url.starts_with("data:image/png;base64,"));
    assert!(url.len() > 100);

    let raw_b64 = url.strip_prefix("data:image/png;base64,").unwrap();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(raw_b64)
        .expect("Base64 decode must succeed");

    let loaded = image::load_from_memory(&bytes).expect("PNG load must succeed");
    assert_eq!(loaded.width(), 128);
    assert_eq!(loaded.height(), 128);
}

#[test]
fn test_depth_buffer_occlusion() {
    // Front triangle at z=1.0 and back triangle at z=0.0
    let positions = vec![
        // Front triangle: z = 1.0
        [-1.0, -1.0, 1.0],
        [1.0, -1.0, 1.0],
        [0.0, 1.0, 1.0],
        // Back triangle: z = 0.0
        [-1.0, -1.0, 0.0],
        [1.0, -1.0, 0.0],
        [0.0, 1.0, 0.0],
    ];
    let indices = vec![0, 1, 2, 3, 4, 5];
    let normals = vec![[0.0, 0.0, 1.0]; 6];
    let bounding_box = AABB::from_points(&positions);

    let mesh = MeshGeometry {
        positions,
        indices,
        normals,
        materials: vec!["mat_test".into()],
        bounding_box,
        edges: Vec::new(),
    };

    let rast = PureCpuRasterizer::new(64);
    let sheet = rast.render(&mesh);

    // In Front view (Q2: x in 64..128, y in 0..64), center pixel must be shaded
    let center_pixel = sheet.get_pixel(96, 32);
    assert_ne!(center_pixel.0, [24, 26, 32, 255]);
}

#[test]
fn test_normal_synthesis_unit_length() {
    let positions = vec![
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
    ];
    let indices = vec![0, 1, 2];
    let normals = compute_normals(&positions, &indices);
    assert_eq!(normals.len(), 3);
    for n in normals {
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        assert!((len - 1.0).abs() < 1e-4, "Normal should have unit length: len={len}");
    }
}

#[test]
fn test_gltf_fixture_extraction() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/quad_indexed.glb");
    if fixture.is_file() {
        let bytes = std::fs::read(&fixture).unwrap();
        let mesh = extract_from_glb_bytes(&bytes).expect("Extraction should succeed");
        assert_eq!(mesh.positions.len(), 4);
        assert_eq!(mesh.indices.len(), 6);
        assert_eq!(mesh.normals.len(), 4);
        assert_eq!(mesh.bounding_box.dimensions()[0], 1.0);
    }
}

#[test]
fn test_rxmodel_bundle_extraction_with_transforms() {
    let bundle = assetd::model::ModelBundle {
        version: 1,
        guid: "test-guid".into(),
        revision: 1,
        name: "TestModel".into(),
        source_id: "test-src".into(),
        source_hash: "test-hash".into(),
        kind: "prop".into(),
        roots: vec![0],
        primitives: vec![assetd::model::ModelPrimitive {
            id: "p0".into(),
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            tangents: vec![],
            uv0: vec![],
            indices: vec![0, 1, 2],
            joints: vec![],
            weights: vec![],
            material: Some(0),
        }],
        nodes: vec![assetd::model::ModelNode {
            id: "n0".into(),
            name: "RootNode".into(),
            children: vec![],
            primitives: vec![0],
            translation: [10.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [2.0, 2.0, 2.0],
            matrix: None,
            skin: None,
            collision: false,
        }],
        materials: vec![assetd::model::ModelMaterial {
            guid: "mat-guid".into(),
            name: "stone_wall".into(),
            base_color: [0.8, 0.8, 0.8, 1.0],
            metallic: 0.1,
            roughness: 0.8,
            emissive: [0.0, 0.0, 0.0],
            base_color_texture: None,
            normal_texture: None,
            metallic_roughness_texture: None,
            occlusion_texture: None,
            emissive_texture: None,
            normal_scale: 1.0,
            occlusion_strength: 1.0,
            double_sided: false,
            alpha_mode: "OPAQUE".into(),
            alpha_cutoff: 0.5,
            unlit: false,
        }],
        textures: vec![],
        skins: vec![],
        animations: vec![],
        idle_clip: "".into(),
        walk_clip: "".into(),
    };

    let mesh = extract_from_model_bundle(&bundle).expect("ModelBundle extraction should succeed");
    assert_eq!(mesh.positions.len(), 3);
    // Scaled by 2 and translated by 10 on X
    assert_eq!(mesh.positions[0], [10.0, 0.0, 0.0]);
    assert_eq!(mesh.positions[1], [12.0, 0.0, 0.0]);
    assert_eq!(mesh.positions[2], [10.0, 2.0, 0.0]);
    assert_eq!(mesh.materials, vec!["stone_wall"]);
}

#[test]
fn test_inspect_3d_asset_end_to_end_caching() {
    let (_dir, project) = tmp_project("inspect_e2e");
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/quad_indexed.glb");
    if fixture.is_file() {
        let rel = "Meshes/quad.glb";
        let target = project.content_root().join(rel);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::copy(&fixture, &target).unwrap();

        // 1. Initial Inspection (Cache Miss -> Renders & Caches)
        let inspection1 = inspect_3d_asset(&project, rel, true, 64).expect("Inspect should succeed");
        assert_eq!(inspection1.polycount, 2);
        assert!(inspection1.data_url.starts_with("data:image/png;base64,"));
        assert!(inspection1.three_view_sheet_uri.starts_with(".forge/cache/thumbs/"));

        let cached_file = project.root.join(&inspection1.three_view_sheet_uri);
        assert!(cached_file.is_file(), "Cached PNG file must exist on disk");

        // 2. Second Inspection (Cache Hit -> Returns immediately)
        let inspection2 = inspect_3d_asset(&project, rel, true, 64).expect("Inspect should succeed");
        assert_eq!(inspection2.polycount, 2);
        assert_eq!(inspection2.three_view_sheet_uri, inspection1.three_view_sheet_uri);
        assert_eq!(inspection2.data_url, inspection1.data_url);

        // 3. Inspection with generateThreeView: false
        let inspection3 = inspect_3d_asset(&project, rel, false, 64).expect("Inspect should succeed");
        assert_eq!(inspection3.polycount, 2);
        assert!(inspection3.three_view_sheet_uri.is_empty());
        assert!(inspection3.data_url.is_empty());
    }
}
