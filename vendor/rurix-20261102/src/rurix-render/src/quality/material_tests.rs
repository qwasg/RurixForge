use super::*;

fn limits() -> PackingLimits {
    PackingLimits {
        max_storage_buffer_range: PAGE_BYTES as u64,
        max_per_stage_descriptor_storage_buffers: 64,
        max_descriptor_set_storage_buffers: 64,
        max_per_stage_resources: 128,
    }
}
fn base(w: u32, h: u32, bytes: Vec<u8>) -> MipLevel {
    MipLevel {
        width: w,
        height: h,
        rgba8: bytes,
    }
}

#[test]
fn hq_color_and_data_mips_have_distinct_domains_and_linear_alpha() {
    let image = base(2, 1, vec![0, 0, 0, 0, 255, 255, 255, 255]);
    let color = complete_mips(image.clone(), Domain::Srgb).unwrap();
    let data = complete_mips(image.clone(), Domain::Linear).unwrap();
    let normal = complete_mips(image, Domain::Normal).unwrap();
    assert_eq!(color[1].rgba8, [188, 188, 188, 128]);
    assert_eq!(data[1].rgba8, [128, 128, 128, 128]);
    assert_eq!(normal, data);
    let mut store = TextureStore::new(limits()).unwrap();
    let a = store.add_image(&color, Domain::Srgb).unwrap();
    let b = store.add_image(&data, Domain::Linear).unwrap();
    assert_ne!(a, b);
    assert_eq!(store.image_chains, 2);
}

#[test]
fn hq_npot_data_mips_include_all_source_texels() {
    let image = base(
        3,
        1,
        vec![0, 30, 60, 0, 100, 90, 80, 90, 200, 150, 100, 180],
    );
    let mips = complete_mips(image, Domain::Linear).unwrap();
    assert_eq!(mips[1].rgba8, [100, 90, 80, 90]);
    validate_chain(&mips).unwrap();
    assert!(validate_chain(&mips[..1]).is_err());
    let mut bad = mips.clone();
    bad.push(mips[1].clone());
    assert!(validate_chain(&bad).is_err());
}

#[test]
fn hq_sampler_uses_linear_normal_rma_and_srgb_base_rgb_with_raw_alpha() {
    let mips = vec![base(1, 1, vec![128, 64, 255, 32])];
    let data = sample_rgba(&mips, Sampler::default(), Domain::Linear, [0.5, 0.5], 0.0).unwrap();
    let color = sample_rgba(&mips, Sampler::default(), Domain::Srgb, [0.5, 0.5], 0.0).unwrap();
    assert!((data[0] - 128.0 / 255.0).abs() < 1e-6);
    assert!((color[0] - 0.2158605).abs() < 1e-6);
    assert_eq!(data[3], color[3]);
    assert_eq!(data[2], 1.0);
    assert!(
        sample_rgba(
            &mips,
            Sampler::default(),
            Domain::Normal,
            [f32::NAN, 0.0],
            0.0
        )
        .is_err()
    );
}

#[test]
fn hq_tbn_inverse_transpose_nonuniform_scale_and_mirror_sign() {
    let (n, t) = transform_tbn(
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0, 1.0],
        [[-2.0, 0.0, 0.0], [0.0, 3.0, 0.0], [0.0, 0.0, 0.5]],
    )
    .unwrap();
    assert_eq!(n, [0.0, 0.0, 1.0]);
    assert_eq!(t, [-1.0, 0.0, 0.0, -1.0]);
    let x = 1.0f32 / 3.0f32.sqrt();
    let (n, t) = transform_tbn(
        [x, x, x],
        [0.70710677, -0.70710677, 0.0, 1.0],
        [[2.0, 0.0, 0.0], [0.0, 4.0, 0.0], [0.0, 0.0, 8.0]],
    )
    .unwrap();
    let e = normalized([0.5, 0.25, 0.125]).unwrap();
    for c in 0..3 {
        assert!((n[c] as f64 - e[c]).abs() < 1e-6);
    }
    assert!((n[0] * t[0] + n[1] * t[1] + n[2] * t[2]).abs() < 1e-6);
    assert!(transform_tbn([0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0], [[0.0; 3]; 3]).is_err());
    assert!(
        transform_tbn(
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0, 0.0],
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
        )
        .is_err()
    );
}

#[test]
fn hq_normal_import_y_sign_is_explicit_and_applied_once() {
    let n = [0.0, 0.0, 1.0];
    let t = [1.0, 0.0, 0.0, 1.0];
    let a = apply_normal_map(n, t, [0.5, 0.75, 1.0], 1.0, 1.0).unwrap();
    let b = apply_normal_map(n, t, [0.5, 0.75, 1.0], 1.0, -1.0).unwrap();
    assert!(a[1] > 0.0 && b[1] < 0.0);
    assert_eq!(a[1], -b[1]);
    assert_eq!(a[2], b[2]);
    assert_eq!(
        apply_normal_map(n, t, [0.2, 0.8, 1.0], 0.0, 1.0).unwrap(),
        n
    );
    assert!(apply_normal_map(n, t, [0.5; 3], 1.0, 1.0).is_err());
    assert!(apply_normal_map(n, t, [0.5, 0.75, 1.0], 1.0, 0.0).is_err());
}

#[test]
fn hq_full_texture_pages_reconstruct_and_only_equal_domain_payloads_share() {
    let image = base(
        256,
        256,
        (0..256usize * 256)
            .flat_map(|i| (i as u32).to_le_bytes())
            .collect(),
    );
    let mips = complete_mips(image, Domain::Linear).unwrap();
    let mut store = TextureStore::new(limits()).unwrap();
    let chain = store.add_image(&mips, Domain::Linear).unwrap();
    let bytes = store.logical_bytes;
    store.add_image(&mips, Domain::Linear).unwrap();
    assert_eq!(store.logical_bytes, bytes);
    store
        .add_binding(chain, Sampler::default(), Domain::Linear)
        .unwrap();
    assert!(store.segments.len() >= 3);
    assert!(store.segments.iter().all(|s| s.len() <= PAGE_BYTES));
    let mut expected = vec![255; 4];
    for m in &mips {
        expected.extend_from_slice(&m.rgba8);
    }
    assert_eq!(store.segments.concat(), expected);
    assert_eq!(
        store.logical_sha256(),
        rurix_pkg::sha256::hex_digest(&expected)
    );
    let meta = store.metadata().unwrap();
    validate_texture_packet(&meta, &store.segments, limits()).unwrap();
    let words: Vec<u32> = meta
        .chunks_exact(4)
        .map(|x| u32::from_le_bytes(x.try_into().unwrap()))
        .collect();
    assert_eq!(words[3], 3);
    assert_eq!(words[6 + 8 + 6], Domain::Linear as u32);
    assert_eq!(words[5] as usize, expected.len().div_ceil(PAGE_BYTES));
    let mut changed = mips.clone();
    changed[0].rgba8[3] ^= 1;
    store.add_image(&changed, Domain::Linear).unwrap();
    assert!(store.logical_bytes > bytes);
    assert!(
        store
            .add_binding((u32::MAX, 0), Sampler::default(), Domain::Linear)
            .is_err()
    );
}

#[test]
fn hq_metadata_tampering_and_empty_mip_chains_are_rejected() {
    let store = TextureStore::new(limits()).unwrap();
    let meta = store.metadata().unwrap();
    for (word, value) in [(3, 2u32), (7, 0), (6, 1), (12, 3), (14, 1)] {
        let mut bad = meta.clone();
        bad[word * 4..word * 4 + 4].copy_from_slice(&value.to_le_bytes());
        assert!(
            validate_texture_packet(&bad, &store.segments, limits()).is_err(),
            "word {word}"
        );
    }
}

#[test]
fn hq_descriptor_range_and_complete_residency_limits_fail_closed() {
    for bad in [
        PackingLimits {
            max_storage_buffer_range: PAGE_BYTES as u64 - 1,
            ..limits()
        },
        PackingLimits {
            max_per_stage_descriptor_storage_buffers: 23,
            ..limits()
        },
        PackingLimits {
            max_descriptor_set_storage_buffers: 23,
            ..limits()
        },
        PackingLimits {
            max_per_stage_resources: 23,
            ..limits()
        },
    ] {
        assert!(TextureStore::new(bad).is_err());
    }
    let mut store = TextureStore::new(limits()).unwrap();
    assert!(store.append(&[1, 2, 3]).is_err());
    assert!(store.append(&vec![0; PAGE_BYTES * MAX_SEGMENTS]).is_err());
    assert!(limits().check_buffer(PAGE_BYTES + 4, "test").is_err());
}
