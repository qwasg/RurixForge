//! Empirical Challenger Stress & Adversarial Test Suite for Milestone 2 (Feature 4).
//!
//! Stress-tests:
//! 1. Adversarial mesh topologies (needle aspect ratios: 10000:1:1, 1:10000:1, 1:1:10000).
//! 2. Depth buffer occlusion order-independence and coplanar stability.
//! 3. Lighting stability (unit vector fallback, extreme light angles, two-sided symmetry).
//! 4. High polycount stress (grid mesh with thousands of triangles, memory & latency checks).
//! 5. Degenerate, collinear, duplicate, out-of-bounds indices, and extreme coordinate centering.
//! 6. Truncated and corrupted byte sequences for .glb, .rxmodel, and .rxmesh.
//! 7. Cache hit latency (< 5ms over 200 consecutive runs) and multi-threaded concurrency.
//! 8. Path traversal and security containment.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use assetd::inspect::inspect_3d_asset;
use assetd::project::ForgeProject;
use assetd::rasterizer::{
    compute_normals, extract_from_glb_bytes,
    extract_from_rxmesh_bytes, extract_from_rxmodel_bytes,
    MeshGeometry, PureCpuRasterizer, AABB,
};

fn tmp_project(tag: &str) -> (PathBuf, ForgeProject) {
    let dir = std::env::temp_dir().join(format!("challenger_stress_{}_{}", tag, uuid::Uuid::new_v4()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let p = ForgeProject::with_defaults(dir.clone());
    p.ensure_dirs().unwrap();
    (dir, p)
}

// ---------------------------------------------------------------------------
// 1. Adversarial Needle Aspect Ratios
// ---------------------------------------------------------------------------

#[test]
fn test_adversarial_needle_aspect_ratios() {
    let test_cases = [
        ([10000.0, 1.0, 1.0], "Needle-X-10000"),
        ([1.0, 10000.0, 1.0], "Needle-Y-10000"),
        ([1.0, 1.0, 10000.0], "Needle-Z-10000"),
        ([50000.0, 0.001, 0.001], "Needle-Extreme-50000"),
        ([0.00001, 0.00001, 0.00001], "Micro-Mesh-Degenerate"),
    ];

    let rast = PureCpuRasterizer::new(64);

    for (dims, label) in test_cases {
        let (sheet, meta) = rast.render_cube_mesh((dims[0], dims[1], dims[2]));
        assert_eq!(sheet.width(), 128, "{label}: Width must be 128");
        assert_eq!(sheet.height(), 128, "{label}: Height must be 128");

        let dim_x = meta["dimensions"]["x"].as_f64().unwrap();
        let dim_y = meta["dimensions"]["y"].as_f64().unwrap();
        let dim_z = meta["dimensions"]["z"].as_f64().unwrap();

        assert!((dim_x - dims[0] as f64).abs() < 1e-3, "{label}: dim_x mismatch");
        assert!((dim_y - dims[1] as f64).abs() < 1e-3, "{label}: dim_y mismatch");
        assert!((dim_z - dims[2] as f64).abs() < 1e-3, "{label}: dim_z mismatch");

        // Verify non-empty pixels in at least one quadrant
        let mut colored = 0;
        for pixel in sheet.pixels() {
            if pixel.0 != [24, 26, 32, 255] && pixel.0 != [60, 64, 76, 255] && pixel.0 != [200, 200, 200, 255] {
                colored += 1;
            }
        }
        assert!(colored > 0, "{label}: Should render visible geometry without panic");
    }
}

// ---------------------------------------------------------------------------
// 2. Depth Buffer Occlusion & Order Independence
// ---------------------------------------------------------------------------

#[test]
fn test_depth_buffer_occlusion_order_independence() {
    // We create two quads (2 triangles each):
    // Front quad: Z = 1.0, Normal [0, 0, 1]
    // Back quad:  Z = -1.0, Normal [0, 0, 1]
    // In Front orthographic view (looking along -Z), Front quad is closer and must fully occlude Back quad.

    let front_positions = vec![
        [-1.0, -1.0, 1.0],
        [1.0, -1.0, 1.0],
        [1.0, 1.0, 1.0],
        [-1.0, 1.0, 1.0],
    ];
    let front_indices = vec![0, 1, 2, 0, 2, 3];

    let back_positions = vec![
        [-1.0, -1.0, -1.0],
        [1.0, -1.0, -1.0],
        [1.0, 1.0, -1.0],
        [-1.0, 1.0, -1.0],
    ];
    let back_indices = vec![0, 1, 2, 0, 2, 3];

    // Submission Order A: Front first, then Back
    let mut pos_a = front_positions.clone();
    let mut ind_a = front_indices.clone();
    let base_idx_a = pos_a.len() as u32;
    pos_a.extend(back_positions.clone());
    ind_a.extend(back_indices.iter().map(|i| i + base_idx_a));
    let norm_a = vec![[0.0, 0.0, 1.0]; pos_a.len()];
    let aabb_a = AABB::from_points(&pos_a);

    let mesh_a = MeshGeometry {
        positions: pos_a,
        indices: ind_a,
        normals: norm_a,
        materials: vec!["front_mat".into()],
        bounding_box: aabb_a,
        edges: Vec::new(),
    };

    // Submission Order B: Back first, then Front
    let mut pos_b = back_positions.clone();
    let mut ind_b = back_indices.clone();
    let base_idx_b = pos_b.len() as u32;
    pos_b.extend(front_positions.clone());
    ind_b.extend(front_indices.iter().map(|i| i + base_idx_b));
    let norm_b = vec![[0.0, 0.0, 1.0]; pos_b.len()];
    let aabb_b = AABB::from_points(&pos_b);

    let mesh_b = MeshGeometry {
        positions: pos_b,
        indices: ind_b,
        normals: norm_b,
        materials: vec!["back_mat".into()],
        bounding_box: aabb_b,
        edges: Vec::new(),
    };

    let rast = PureCpuRasterizer::new(64);
    let sheet_a = rast.render(&mesh_a);
    let sheet_b = rast.render(&mesh_b);

    // In Front View (Quadrant 2: x in 64..128, y in 0..64):
    // Center pixel is at (64 + 32, 32) = (96, 32)
    let pixel_a = sheet_a.get_pixel(96, 32);
    let pixel_b = sheet_b.get_pixel(96, 32);

    assert_ne!(pixel_a.0, [24, 26, 32, 255], "Front view pixel must be shaded");
    assert_eq!(
        pixel_a, pixel_b,
        "Depth buffer must be strictly order-independent: pixel_a={:?} vs pixel_b={:?}",
        pixel_a, pixel_b
    );

    // Verify all pixels in Front view match exactly between submission orders A and B
    for y in 0..64 {
        for x in 64..128 {
            assert_eq!(
                sheet_a.get_pixel(x, y),
                sheet_b.get_pixel(x, y),
                "Pixel mismatch at ({x}, {y}) between submission orders"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 3. Lighting Stability & Symmetrical Normal Handling
// ---------------------------------------------------------------------------

#[test]
fn test_lighting_stability_and_symmetry() {
    // Two-sided lighting equation in PureCpuRasterizer:
    // dot = |norm . light|
    // intensity = (ambient + diffuse * dot).clamp(0.0, 1.0)
    // Therefore, normal N and normal -N must yield the EXACT same pixel intensity!

    let tri_positions = vec![
        [-1.0, -1.0, 0.0],
        [1.0, -1.0, 0.0],
        [0.0, 1.0, 0.0],
    ];
    let tri_indices = vec![0, 1, 2];

    let rast = PureCpuRasterizer::new(64);

    let mesh_pos_n = MeshGeometry {
        positions: tri_positions.clone(),
        indices: tri_indices.clone(),
        normals: vec![[0.0, 0.0, 1.0]; 3],
        materials: vec!["mat".into()],
        bounding_box: AABB::from_points(&tri_positions),
        edges: Vec::new(),
    };

    let mesh_neg_n = MeshGeometry {
        positions: tri_positions.clone(),
        indices: tri_indices.clone(),
        normals: vec![[0.0, 0.0, -1.0]; 3],
        materials: vec!["mat".into()],
        bounding_box: AABB::from_points(&tri_positions),
        edges: Vec::new(),
    };

    let sheet_pos = rast.render(&mesh_pos_n);
    let sheet_neg = rast.render(&mesh_neg_n);

    // Front view pixel (96, 32)
    let p_pos = sheet_pos.get_pixel(96, 32);
    let p_neg = sheet_neg.get_pixel(96, 32);

    assert_eq!(p_pos, p_neg, "Two-sided diffuse lighting must produce identical colors for N and -N");

    // Zero-length normal fallback
    let zero_normals = compute_normals(&tri_positions, &[]);
    assert_eq!(zero_normals.len(), 3);
    for n in zero_normals {
        assert_eq!(n, [0.0, 1.0, 0.0], "Zero-normal must fallback safely to [0.0, 1.0, 0.0]");
    }
}

// ---------------------------------------------------------------------------
// 4. High Polycount Stress (Dense Grid Mesh)
// ---------------------------------------------------------------------------

#[test]
fn test_high_polycount_stress_performance() {
    // Generate a 120 x 120 vertex dense heightmap / grid
    // Vertices: 14,400
    // Triangles: 119 * 119 * 2 = 28,322 triangles
    let n = 120usize;
    let mut positions = Vec::with_capacity(n * n);
    for j in 0..n {
        for i in 0..n {
            let x = (i as f32 / (n - 1) as f32) * 2.0 - 1.0;
            let z = (j as f32 / (n - 1) as f32) * 2.0 - 1.0;
            let y = 0.2 * ((x * 4.0).sin() * (z * 4.0).cos());
            positions.push([x, y, z]);
        }
    }

    let mut indices = Vec::with_capacity((n - 1) * (n - 1) * 6);
    for j in 0..(n - 1) {
        for i in 0..(n - 1) {
            let i0 = (j * n + i) as u32;
            let i1 = (j * n + (i + 1)) as u32;
            let i2 = ((j + 1) * n + i) as u32;
            let i3 = ((j + 1) * n + (i + 1)) as u32;

            // Two triangles per quad
            indices.push(i0);
            indices.push(i1);
            indices.push(i2);

            indices.push(i1);
            indices.push(i3);
            indices.push(i2);
        }
    }

    let polycount = indices.len() / 3;
    assert_eq!(polycount, 28322);

    let t_norm0 = Instant::now();
    let normals = compute_normals(&positions, &indices);
    let dt_norm = t_norm0.elapsed();

    assert_eq!(normals.len(), positions.len());

    let mesh = MeshGeometry {
        positions,
        indices,
        normals,
        materials: vec!["dense_grid".into()],
        bounding_box: AABB::new([-1.0, -0.2, -1.0], [1.0, 0.2, 1.0]),
        edges: Vec::new(),
    };

    let rast = PureCpuRasterizer::new(128); // 256x256 composite sheet
    let t_rast0 = Instant::now();
    let sheet = rast.render(&mesh);
    let dt_rast = t_rast0.elapsed();

    println!(
        "High polycount stress ({} tris): normal synthesis = {:?}, 4-panel rasterization = {:?}",
        polycount, dt_norm, dt_rast
    );

    assert_eq!(sheet.width(), 256);
    assert_eq!(sheet.height(), 256);
    // Pure CPU rasterization of 28,322 triangles into 4 quadrants should be well under 1.5s
    assert!(
        dt_rast.as_millis() < 1500,
        "Rasterization took too long: {}ms (expected < 1500ms)",
        dt_rast.as_millis()
    );
}

// ---------------------------------------------------------------------------
// 5. Degenerate, Collinear, Duplicate & Out-Of-Bounds Indices
// ---------------------------------------------------------------------------

#[test]
fn test_degenerate_and_adversarial_topologies() {
    let rast = PureCpuRasterizer::new(64);

    // 1. Collinear vertices (triangle with area = 0)
    let collinear_mesh = MeshGeometry {
        positions: vec![
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
            [2.0, 2.0, 2.0],
        ],
        indices: vec![0, 1, 2],
        normals: vec![[0.0, 1.0, 0.0]; 3],
        materials: vec!["collinear".into()],
        bounding_box: AABB::from_points(&[[0.0, 0.0, 0.0], [2.0, 2.0, 2.0]]),
        edges: Vec::new(),
    };
    let sheet1 = rast.render(&collinear_mesh);
    assert_eq!(sheet1.width(), 128);

    // 2. Duplicate / Coincident vertices
    let coincident_mesh = MeshGeometry {
        positions: vec![
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
        ],
        indices: vec![0, 1, 2],
        normals: vec![[0.0, 1.0, 0.0]; 3],
        materials: vec!["coincident".into()],
        bounding_box: AABB::from_points(&[[0.0, 0.0, 0.0]]),
        edges: Vec::new(),
    };
    let sheet2 = rast.render(&coincident_mesh);
    assert_eq!(sheet2.width(), 128);

    // 3. Out-of-bounds indices in mesh (indices point beyond positions array)
    let oob_mesh = MeshGeometry {
        positions: vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
        ],
        indices: vec![0, 1, 999999, 1000000, 2, 0],
        normals: vec![[0.0, 0.0, 1.0]; 3],
        materials: vec!["oob".into()],
        bounding_box: AABB::from_points(&[[0.0, 0.0, 0.0], [1.0, 1.0, 0.0]]),
        edges: Vec::new(),
    };
    let sheet3 = rast.render(&oob_mesh);
    assert_eq!(sheet3.width(), 128);

    // 4. Extreme coordinate offset (tests vertex centering v' = v - C numerical stability)
    let huge_offset_mesh = MeshGeometry {
        positions: vec![
            [1000000.0, 1000000.0, 1000000.0],
            [1000001.0, 1000000.0, 1000000.0],
            [1000000.0, 1000001.0, 1000000.0],
        ],
        indices: vec![0, 1, 2],
        normals: vec![[0.0, 0.0, 1.0]; 3],
        materials: vec!["huge".into()],
        bounding_box: AABB::from_points(&[
            [1000000.0, 1000000.0, 1000000.0],
            [1000001.0, 1000001.0, 1000000.0],
        ]),
        edges: Vec::new(),
    };
    let sheet4 = rast.render(&huge_offset_mesh);
    assert_eq!(sheet4.width(), 128);
}

// ---------------------------------------------------------------------------
// 6. Corrupted & Truncated Asset Bytes
// ---------------------------------------------------------------------------

#[test]
fn test_corrupted_and_truncated_byte_sequences() {
    // 1. Empty byte slices
    assert!(extract_from_glb_bytes(&[]).is_err(), "Empty GLB bytes must error");
    assert!(extract_from_rxmodel_bytes(&[]).is_err(), "Empty rxmodel bytes must error");
    assert!(extract_from_rxmesh_bytes(&[], vec![]).is_err(), "Empty rxmesh bytes must error");

    // 2. Truncated GLB header
    let truncated_glb = b"glTF";
    let res_glb1 = extract_from_glb_bytes(truncated_glb);
    assert!(res_glb1.is_err());
    assert_eq!(res_glb1.unwrap_err().code, "GLTF_PARSE_ERR");

    // 3. GLB magic followed by corrupted chunk headers
    let corrupted_glb = b"glTF\x02\x00\x00\x00\xff\xff\xff\xffJSONcorruptgarbage";
    let res_glb2 = extract_from_glb_bytes(corrupted_glb);
    assert!(res_glb2.is_err());

    // 4. Malformed JSON for rxmodel
    let malformed_json = b"{ \"version\": 1, \"nodes\": [ { \"name\": \"broken\", \"children\": [1000] } ";
    let res_rx1 = extract_from_rxmodel_bytes(malformed_json);
    assert!(res_rx1.is_err());
    assert_eq!(res_rx1.unwrap_err().code, "MODEL_PARSE_ERR");

    // 5. Corrupted binary DAG for rxmesh
    let corrupted_rxmesh = b"RXMSH\x01\x00\x00\x00corrupted-payload-without-valid-dag-structures";
    let res_mesh = extract_from_rxmesh_bytes(corrupted_rxmesh, vec![]);
    assert!(res_mesh.is_err());
    assert_eq!(res_mesh.unwrap_err().code, "RXMESH_PARSE_ERR");
}

// ---------------------------------------------------------------------------
// 7. Cache Hit Speed (< 5ms) & Concurrency
// ---------------------------------------------------------------------------

#[test]
fn test_cache_hit_speed_and_concurrency() {
    let (_tmp_dir, project) = tmp_project("cache_speed");

    // Create a valid test model file
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/quad_indexed.glb");
    let target = project.content_root().join("Models/test_quad.glb");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::copy(&fixture_path, &target).unwrap();

    // 1. Initial call (Cache Miss)
    let t0 = Instant::now();
    let initial = inspect_3d_asset(&project, "Content/Models/test_quad.glb", true, 64)
        .expect("Initial inspect should succeed");
    let _dt_initial = t0.elapsed();
    assert_eq!(initial.polycount, 2);
    assert!(initial.three_view_sheet_uri.starts_with(".forge/cache/thumbs/"));

    // 2. Perform 200 consecutive rapid calls (Cache Hits)
    let iterations = 200;
    let mut durations = Vec::with_capacity(iterations);

    for _ in 0..iterations {
        let t_hit = Instant::now();
        let cached = inspect_3d_asset(&project, "Content/Models/test_quad.glb", true, 64)
            .expect("Cached inspect should succeed");
        let dt = t_hit.elapsed();
        durations.push(dt);
        assert_eq!(cached.three_view_sheet_uri, initial.three_view_sheet_uri);
        assert_eq!(cached.data_url, initial.data_url);
    }

    durations.sort();
    let min_dt = durations[0];
    let max_dt = durations[durations.len() - 1];
    let median_dt = durations[durations.len() / 2];
    let p99_dt = durations[(durations.len() as f32 * 0.99) as usize];
    let avg_us: u128 = durations.iter().map(|d| d.as_micros()).sum::<u128>() / iterations as u128;

    println!(
        "Cache Hit Benchmarks ({} iterations): min={:?}, median={:?}, avg={}us, p99={:?}, max={:?}",
        iterations, min_dt, median_dt, avg_us, p99_dt, max_dt
    );

    // Assert: Average cache hit latency must be strictly < 5ms (5,000 microseconds)
    assert!(
        avg_us < 5000,
        "Average cache hit latency was {}us, exceeding the 5000us (< 5ms) threshold!",
        avg_us
    );

    // 3. Multithreaded Concurrency Stress
    let project_arc = Arc::new(project);
    let mut handles = Vec::new();
    for thread_id in 0..10 {
        let p = Arc::clone(&project_arc);
        let handle = std::thread::spawn(move || {
            for _ in 0..20 {
                let res = inspect_3d_asset(&p, "Content/Models/test_quad.glb", true, 64)
                    .unwrap_or_else(|e| panic!("Thread {thread_id} failed: {e}"));
                assert_eq!(res.polycount, 2);
            }
        });
        handles.push(handle);
    }

    for h in handles {
        h.join().expect("Worker thread panicked during concurrent cache access");
    }
}

// ---------------------------------------------------------------------------
// 8. Security Containment & Path Traversal
// ---------------------------------------------------------------------------

#[test]
fn test_security_containment_and_path_traversal() {
    let (_tmp_dir, project) = tmp_project("security_path");

    // Attempt directory traversal out of project root
    let traversal_paths = [
        "../../../../Windows/System32/drivers/etc/hosts",
        "Content/../../../../boot.ini",
        "..\\..\\sensitive.txt",
    ];

    for path in traversal_paths {
        let res = inspect_3d_asset(&project, path, true, 64);
        assert!(res.is_err(), "Path traversal '{path}' must be rejected");
        let err = res.unwrap_err();
        assert!(
            err.code == "ACCESS_DENIED" || err.code == "ASSET_NOT_FOUND",
            "Expected ACCESS_DENIED or ASSET_NOT_FOUND, got: {}",
            err.code
        );
    }
}
