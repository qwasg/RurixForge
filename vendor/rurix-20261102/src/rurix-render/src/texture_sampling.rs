//! RFC-0051: safe host reference for the G35/G42 texture SSBO protocol.
//! RGB transfers and filtering are in linear light; UV origin is upper left.
// Assisted-by: Codex:gpt-6-astra.

/// One tightly packed, upper-left-origin RGBA8 mip level.
#[derive(Clone, Debug, PartialEq)]
pub struct MipLevel {
    pub width: u32,
    pub height: u32,
    pub rgba8: Vec<u8>,
}

impl MipLevel {
    pub fn validate(&self) -> Result<(), String> {
        let size = (self.width as usize)
            .checked_mul(self.height as usize)
            .and_then(|v| v.checked_mul(4))
            .ok_or("texture size overflow")?;
        if self.width == 0 || self.height == 0 || size != self.rgba8.len() {
            return Err("texture dimensions / RGBA8 length mismatch".into());
        }
        Ok(())
    }
}

/// Native glTF enum values, shared with GPU metadata (protocol v1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sampler {
    pub wrap_s: u32,
    pub wrap_t: u32,
    pub min_filter: u32,
    pub mag_filter: u32,
}

impl Default for Sampler {
    fn default() -> Self {
        Self {
            wrap_s: 10497,
            wrap_t: 10497,
            min_filter: 9987,
            mag_filter: 9729,
        }
    }
}

impl Sampler {
    pub fn validate(self) -> Result<(), String> {
        if ![10497, 33648, 33071].contains(&self.wrap_s)
            || ![10497, 33648, 33071].contains(&self.wrap_t)
            || ![9728, 9729, 9984, 9985, 9986, 9987].contains(&self.min_filter)
            || ![9728, 9729].contains(&self.mag_filter)
        {
            return Err("unsupported glTF sampler enum".into());
        }
        Ok(())
    }
}

pub fn srgb_lut() -> [f32; 256] {
    std::array::from_fn(|i| {
        let x = i as f32 / 255.0;
        if x <= 0.04045 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        }
    })
}

fn encode_srgb(x: f64) -> u8 {
    let x = x.clamp(0.0, 1.0);
    let s = if x <= 0.0031308 {
        x * 12.92
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round().clamp(0.0, 255.0) as u8
}

pub fn mean_rgb(level: &MipLevel) -> Result<[f32; 3], String> {
    level.validate()?;
    let lut = srgb_lut();
    let mut sum = [0.0f64; 3];
    for p in level.rgba8.chunks_exact(4) {
        for c in 0..3 {
            sum[c] += f64::from(lut[p[c] as usize]);
        }
    }
    let count = f64::from(level.width) * f64::from(level.height);
    Ok(sum.map(|s| (s / count) as f32))
}

/// Complete NPOT mip pyramid. Area weights include partial source cells, so odd
/// rows/columns are never discarded; alpha remains linear and unpremultiplied.
pub fn complete_mips(base: MipLevel) -> Result<Vec<MipLevel>, String> {
    base.validate()?;
    let lut = srgb_lut();
    let mut levels = vec![base];
    while levels.last().unwrap().width > 1 || levels.last().unwrap().height > 1 {
        let src = levels.last().unwrap();
        let (w, h) = ((src.width / 2).max(1), (src.height / 2).max(1));
        let mut dst = MipLevel {
            width: w,
            height: h,
            rgba8: vec![0; w as usize * h as usize * 4],
        };
        for y in 0..h {
            let y0 = f64::from(y) * f64::from(src.height) / f64::from(h);
            let y1 = f64::from(y + 1) * f64::from(src.height) / f64::from(h);
            for x in 0..w {
                let x0 = f64::from(x) * f64::from(src.width) / f64::from(w);
                let x1 = f64::from(x + 1) * f64::from(src.width) / f64::from(w);
                let mut sum = [0.0f64; 4];
                for sy in y0.floor() as u32..(y1.ceil() as u32).min(src.height) {
                    let wy = (y1.min(f64::from(sy + 1)) - y0.max(f64::from(sy))).max(0.0);
                    for sx in x0.floor() as u32..(x1.ceil() as u32).min(src.width) {
                        let wx = (x1.min(f64::from(sx + 1)) - x0.max(f64::from(sx))).max(0.0);
                        let p = (sy as usize * src.width as usize + sx as usize) * 4;
                        for c in 0..3 {
                            sum[c] += f64::from(lut[src.rgba8[p + c] as usize]) * wx * wy;
                        }
                        sum[3] += f64::from(src.rgba8[p + 3]) * wx * wy;
                    }
                }
                let area = (x1 - x0) * (y1 - y0);
                let o = (y as usize * w as usize + x as usize) * 4;
                for c in 0..3 {
                    dst.rgba8[o + c] = encode_srgb(sum[c] / area);
                }
                dst.rgba8[o + 3] = (sum[3] / area).round().clamp(0.0, 255.0) as u8;
            }
        }
        levels.push(dst);
    }
    Ok(levels)
}

fn wrap_tap(i: i64, size: u32, mode: u32) -> usize {
    let n = i64::from(size);
    match mode {
        33071 => i.clamp(0, n - 1) as usize,
        33648 => {
            let q = i.rem_euclid(n * 2);
            if q < n {
                q as usize
            } else {
                (n * 2 - q - 1) as usize
            }
        }
        _ => i.rem_euclid(n) as usize,
    }
}

fn sample_level(
    level: &MipLevel,
    s: Sampler,
    uv: [f32; 2],
    linear: bool,
    lut: &[f32; 256],
) -> [f32; 3] {
    // Reducing UV first also prevents integer overflow on large finite UVs.
    let reduced = |v: f32, mode: u32| match mode {
        33071 => v.clamp(0.0, 1.0),
        33648 => v.rem_euclid(2.0),
        _ => v.rem_euclid(1.0),
    };
    let x = reduced(uv[0], s.wrap_s) * level.width as f32 - if linear { 0.5 } else { 0.0 };
    let y = reduced(uv[1], s.wrap_t) * level.height as f32 - if linear { 0.5 } else { 0.0 };
    let (ix, iy) = (x.floor() as i64, y.floor() as i64);
    let tap = |dx, dy| {
        let p = (wrap_tap(iy + dy, level.height, s.wrap_t) * level.width as usize
            + wrap_tap(ix + dx, level.width, s.wrap_s))
            * 4;
        std::array::from_fn::<_, 3, _>(|c| lut[level.rgba8[p + c] as usize])
    };
    if !linear {
        return tap(0, 0);
    }
    let (fx, fy) = (x - x.floor(), y - y.floor());
    let (a, b, c, d) = (tap(0, 0), tap(1, 0), tap(0, 1), tap(1, 1));
    std::array::from_fn(|i| {
        (a[i] * (1.0 - fx) + b[i] * fx) * (1.0 - fy) + (c[i] * (1.0 - fx) + d[i] * fx) * fy
    })
}

/// CPU reference of the shader's full glTF sampling rules at a fixed lambda.
pub fn sample_rgb(
    levels: &[MipLevel],
    s: Sampler,
    uv: [f32; 2],
    lambda: f32,
) -> Result<[f32; 3], String> {
    s.validate()?;
    if levels.is_empty() || !uv.iter().all(|v| v.is_finite()) || !lambda.is_finite() {
        return Err("sampling requires mips and finite UV / lambda".into());
    }
    for l in levels {
        l.validate()?;
    }
    let lut = srgb_lut();
    let f = if lambda <= 0.0 {
        s.mag_filter
    } else {
        s.min_filter
    };
    let linear = [9729, 9985, 9987].contains(&f);
    let lod = lambda.max(0.0).min((levels.len() - 1) as f32);
    let (l0, l1, blend) = match f {
        9984 | 9985 => {
            let l = (lod + 0.5).floor() as usize;
            (l, l, 0.0)
        }
        9986 | 9987 => {
            let l = lod.floor() as usize;
            (l, (l + 1).min(levels.len() - 1), lod - l as f32)
        }
        _ => (0, 0, 0.0),
    };
    let a = sample_level(&levels[l0], s, uv, linear, &lut);
    let b = sample_level(&levels[l1], s, uv, linear, &lut);
    Ok(std::array::from_fn(|i| a[i] * (1.0 - blend) + b[i] * blend))
}

/// Maximum singular value of the world-tangent-to-texel Jacobian.
pub fn k_tex(
    world: [[f32; 3]; 3],
    uv: [[f32; 2]; 3],
    width: u32,
    height: u32,
) -> Result<f32, String> {
    if width == 0
        || height == 0
        || !world
            .iter()
            .flatten()
            .chain(uv.iter().flatten())
            .all(|v| v.is_finite())
    {
        return Err("k_tex requires finite geometry / UV and nonzero dimensions".into());
    }
    let a: [f64; 3] = std::array::from_fn(|i| f64::from(world[1][i]) - f64::from(world[0][i]));
    let b: [f64; 3] = std::array::from_fn(|i| f64::from(world[2][i]) - f64::from(world[0][i]));
    let dot = |x: [f64; 3], y: [f64; 3]| x.into_iter().zip(y).map(|(u, v)| u * v).sum::<f64>();
    let ax = dot(a, a).sqrt();
    if ax == 0.0 {
        return Ok(0.0);
    }
    let bx = dot(a, b) / ax;
    // Area detects exactly collinear edges without projection cancellation
    // leaving a tiny nonzero tangent height and an enormous spurious density.
    let cross = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    let by = dot(cross, cross).sqrt() / ax;
    if by == 0.0 {
        return Ok(0.0);
    }
    let u1 = (f64::from(uv[1][0]) - f64::from(uv[0][0])) * f64::from(width);
    let u2 = (f64::from(uv[2][0]) - f64::from(uv[0][0])) * f64::from(width);
    let v1 = (f64::from(uv[1][1]) - f64::from(uv[0][1])) * f64::from(height);
    let v2 = (f64::from(uv[2][1]) - f64::from(uv[0][1])) * f64::from(height);
    let j = [
        u1 / ax,
        (u2 - u1 * bx / ax) / by,
        v1 / ax,
        (v2 - v1 * bx / ax) / by,
    ];
    let aa = j[0] * j[0] + j[2] * j[2];
    let bb = j[0] * j[1] + j[2] * j[3];
    let cc = j[1] * j[1] + j[3] * j[3];
    let k = ((aa + cc + ((aa - cc).powi(2) + 4.0 * bb * bb).sqrt()) * 0.5).sqrt() as f32;
    if !k.is_finite() {
        return Err("k_tex exceeds finite f32 range".into());
    }
    Ok(k)
}

pub fn ray_lod(distance: f32, k_pix: f32, k_tex: f32, normal_dot_ray: f32) -> f32 {
    (distance * k_pix * k_tex / normal_dot_ray.abs().max(1e-4))
        .max(1e-6)
        .log2()
}

/// Multiplies the linear sample; contractual Le alone controls intensity.
pub fn emissive_multiplier(
    mean: [f32; 3],
    factor: [f32; 3],
    le: [f32; 3],
) -> Result<[f32; 3], String> {
    let mut result = [0.0; 3];
    for c in 0..3 {
        if ![mean[c], factor[c], le[c]]
            .into_iter()
            .all(|x| x.is_finite() && x >= 0.0)
        {
            return Err("emission mean/factor/Le must be finite and nonnegative".into());
        }
        if le[c] == 0.0 {
            continue;
        }
        let q_mean = f64::from(mean[c]) * f64::from(factor[c]);
        if q_mean == 0.0 {
            return Err(format!(
                "positive contractual Le has zero emissive q mean in channel {c}"
            ));
        }
        result[c] = (f64::from(factor[c]) * f64::from(le[c]) / q_mean) as f32;
        if !result[c].is_finite() {
            return Err("emissive normalization overflows f32".into());
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<MipLevel> {
        complete_mips(MipLevel {
            width: 2,
            height: 2,
            rgba8: vec![
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
            ],
        })
        .unwrap()
    }
    #[test]
    fn linear_light_and_upper_left() {
        let m = fixture();
        let mut s = Sampler {
            mag_filter: 9728,
            ..Sampler::default()
        };
        assert_eq!(
            sample_rgb(&m, s, [0.25, 0.25], -1.0).unwrap(),
            [1.0, 0.0, 0.0]
        );
        assert_eq!(
            sample_rgb(&m, s, [0.25, 0.75], -1.0).unwrap(),
            [0.0, 0.0, 1.0]
        );
        s.mag_filter = 9729;
        assert_eq!(sample_rgb(&m, s, [0.5, 0.5], 0.0).unwrap(), [0.5; 3]);
        assert!((mean_rgb(&m[1]).unwrap()[0] - 0.5).abs() < 0.004);
    }
    #[test]
    fn independent_wrap_negative_seams_all_filters() {
        let m = fixture();
        for ws in [10497, 33648, 33071] {
            for wt in [10497, 33648, 33071] {
                for min in [9728, 9729, 9984, 9985, 9986, 9987] {
                    for mag in [9728, 9729] {
                        let s = Sampler {
                            wrap_s: ws,
                            wrap_t: wt,
                            min_filter: min,
                            mag_filter: mag,
                        };
                        for uv in [[-0.25, 0.25], [0.0, 1.0], [1.25, -1.25], [0.5, 0.5]] {
                            for lod in [-1.0, 0.0, 0.25, 0.75, 3.0] {
                                let rgb = sample_rgb(&m, s, uv, lod).unwrap();
                                assert!(rgb.into_iter().all(|x| (0.0..=1.0).contains(&x)));
                            }
                        }
                    }
                }
            }
        }
        let s = Sampler {
            mag_filter: 9728,
            ..Sampler::default()
        };
        assert_eq!(
            sample_rgb(&m, s, [-0.25, 0.25], 0.0).unwrap(),
            [0.0, 1.0, 0.0]
        );
        assert_eq!(
            sample_rgb(&m, Sampler { wrap_s: 33648, ..s }, [-0.25, 0.25], 0.0).unwrap(),
            [1.0, 0.0, 0.0]
        );
        assert_eq!(
            sample_rgb(&m, Sampler { wrap_s: 33071, ..s }, [-1.25, 0.75], 0.0).unwrap(),
            [0.0, 0.0, 1.0]
        );
    }
    #[test]
    fn mip_filter_selection() {
        let mut m = fixture();
        m[1].rgba8 = vec![255, 255, 255, 255];
        for filter in [9984, 9985] {
            let s = Sampler {
                min_filter: filter,
                mag_filter: 9728,
                ..Sampler::default()
            };
            assert_eq!(
                sample_rgb(&m, s, [0.25, 0.25], 0.49).unwrap(),
                [1.0, 0.0, 0.0]
            );
            assert_eq!(sample_rgb(&m, s, [0.25, 0.25], 0.51).unwrap(), [1.0; 3]);
        }
        for filter in [9986, 9987] {
            let s = Sampler {
                min_filter: filter,
                ..Sampler::default()
            };
            assert_eq!(
                sample_rgb(&m, s, [0.25, 0.25], 0.25).unwrap(),
                [1.0, 0.25, 0.25]
            );
        }
    }
    #[test]
    fn npot_linear_repeat_blends_across_last_first_texels() {
        // RFC-0051 regression: a 7-texel axis reaches tap 7 near its seam.
        // Wrapping must fetch texel 0, not clamp a rounded float remainder to 6.
        for (w, h, uv) in [(1, 7, [0.5, 0.96875]), (7, 1, [0.96875, 0.5])] {
            let mut rgba8 = vec![0; (w * h * 4) as usize];
            rgba8[..4].copy_from_slice(&[0, 0, 255, 255]);
            rgba8[24..28].copy_from_slice(&[255, 0, 0, 255]);
            let levels = vec![MipLevel { width: w, height: h, rgba8 }];
            for min_filter in [9729, 9985, 9987] {
                let sampler = Sampler { min_filter, ..Sampler::default() };
                for lambda in [-2.0, 0.0, 0.125, 9.0] {
                    assert_eq!(
                        sample_rgb(&levels, sampler, uv, lambda).unwrap(),
                        [0.71875, 0.0, 0.28125]
                    );
                }
            }
        }
    }

    #[test]
    fn integer_wrapping_matches_biased_shader_taps() {
        // Reduced UV bounds each tap to [-1, 2*n]; bias preserves repeat and
        // mirror periods and makes the unsigned device remainder well-defined.
        for n in [1u32, 3, 5, 7, 13, 1023] {
            for i in -1..=2 * i64::from(n) {
                let biased = (i + 2 * i64::from(n)) as usize;
                assert_eq!(wrap_tap(i, n, 10497), biased % n as usize);
                let period = 2 * n as usize;
                let mirror = biased % period;
                let actual = if mirror < n as usize { mirror } else { period - 1 - mirror };
                assert_eq!(wrap_tap(i, n, 33648), actual);
            }
        }
    }

    #[test]
    fn npot_includes_every_row_column() {
        for (w, h) in [(1, 7), (7, 1), (3, 5)] {
            let mut rgba8 = vec![0; w * h * 4];
            for (i, p) in rgba8.chunks_exact_mut(4).enumerate() {
                p.copy_from_slice(&[if i + 1 == w * h { 255 } else { 0 }, 0, 0, 255]);
            }
            let levels = complete_mips(MipLevel {
                width: w as u32,
                height: h as u32,
                rgba8,
            })
            .unwrap();
            assert_eq!(
                (levels.last().unwrap().width, levels.last().unwrap().height),
                (1, 1)
            );
            assert!(
                (mean_rgb(levels.last().unwrap()).unwrap()[0] - 1.0 / (w * h) as f32).abs() < 0.004
            );
        }
    }
    #[test]
    fn jacobian_and_reflection_cone() {
        let world = [[0., 0., 0.], [2., 0., 0.], [0., 1., 0.]];
        let uv = [[0., 0.], [1., 0.], [0., 1.]];
        assert_eq!(k_tex(world, uv, 64, 64).unwrap(), 64.0);
        assert_eq!(k_tex([[0.; 3]; 3], uv, 64, 64).unwrap(), 0.0);
        assert!((ray_lod(4., 0.01, 64., 0.5) - ray_lod(2., 0.01, 64., 0.5) - 1.0).abs() < 1e-6);
        assert!((ray_lod(2., 0.01, 64., 0.5) - ray_lod(2., 0.01, 64., 1.0) - 1.0).abs() < 1e-6);
    }
    #[test]
    fn emission_contract_zero_and_factor() {
        let m = emissive_multiplier([0.5, 0.25, 0.], [0.3, 0.7, 0.], [2., 3., 0.]).unwrap();
        assert_eq!(m, [4., 12., 0.]);
        assert!(emissive_multiplier([0.; 3], [1.; 3], [1.; 3]).is_err());
        assert!(emissive_multiplier([0.5; 3], [0.; 3], [1.; 3]).is_err());
    }

    #[test]
    fn every_minification_filter_has_analytic_lod_selection() {
        // Solid levels make expected mip choice independent of spatial taps.
        let levels = [
            (4, [255, 0, 0, 255]),
            (2, [0, 255, 0, 255]),
            (1, [0, 0, 255, 255]),
        ]
        .into_iter()
        .map(|(n, p)| MipLevel {
            width: n,
            height: n,
            rgba8: p.repeat((n * n) as usize),
        })
        .collect::<Vec<_>>();
        for filter in [9728, 9729, 9984, 9985, 9986, 9987] {
            let s = Sampler {
                min_filter: filter,
                ..Sampler::default()
            };
            for (lod, nearest, blend) in [
                (0.25, [1., 0., 0.], [0.75, 0.25, 0.]),
                (0.75, [0., 1., 0.], [0.25, 0.75, 0.]),
                (1.25, [0., 1., 0.], [0., 0.75, 0.25]),
                (8., [0., 0., 1.], [0., 0., 1.]),
            ] {
                let expected = match filter {
                    9728 | 9729 => [1., 0., 0.],
                    9984 | 9985 => nearest,
                    _ => blend,
                };
                assert_eq!(
                    sample_rgb(&levels, s, [-1.5, 2.5], lod).unwrap(),
                    expected,
                    "filter {filter}, lod {lod}"
                );
            }
            assert_eq!(
                sample_rgb(&levels, s, [0.5, 0.5], 0.).unwrap(),
                [1., 0., 0.]
            );
        }
    }

    #[test]
    fn one_dimensional_npot_wrap_and_linear_alpha_mips() {
        let image = MipLevel {
            width: 1,
            height: 3,
            rgba8: vec![255, 0, 0, 0, 0, 255, 0, 120, 0, 0, 255, 240],
        };
        let levels = complete_mips(image).unwrap();
        assert_eq!(levels[1].rgba8[3], 120);
        let s = Sampler {
            mag_filter: 9728,
            ..Sampler::default()
        };
        assert_eq!(
            sample_rgb(&levels, s, [-100., -1. / 6.], 0.).unwrap(),
            [0., 0., 1.]
        );
        assert_eq!(
            sample_rgb(
                &levels,
                Sampler { wrap_t: 33648, ..s },
                [100., -1. / 6.],
                0.
            )
            .unwrap(),
            [1., 0., 0.]
        );
        assert_eq!(
            sample_rgb(&levels, Sampler { wrap_t: 33071, ..s }, [100., 2.], 0.).unwrap(),
            [0., 0., 1.]
        );
        let s = Sampler {
            mag_filter: 9729,
            ..s
        };
        assert_eq!(
            sample_rgb(&levels, s, [0., 0.], 0.).unwrap(),
            [0.5, 0., 0.5]
        );
        assert_eq!(
            sample_rgb(&levels, Sampler { wrap_t: 33648, ..s }, [0., 0.], 0.).unwrap(),
            [1., 0., 0.]
        );
    }

    #[test]
    fn jacobian_collinear_shear_scale_and_uv_rotation() {
        let uv = [[0., 0.], [1., 0.], [0., 1.]];
        assert_eq!(
            k_tex([[0., 0., 0.], [1., 1., 0.], [2., 2., 0.]], uv, 64, 64).unwrap(),
            0.
        );
        let world = [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]];
        let sheared = [[0., 0.], [1., 0.], [1., 1.]];
        let expected = (3.0f32 + 5.0f32.sqrt()).sqrt() / 2.0f32.sqrt();
        assert!((k_tex(world, sheared, 1, 1).unwrap() - expected).abs() < 1e-6);
        let turned = [[0., 0.], [0., 1.], [-1., 0.]];
        assert_eq!(k_tex(world, turned, 64, 64).unwrap(), 64.);
        assert_eq!(
            k_tex([[0., 0., 0.], [2., 0., 0.], [0., 2., 0.]], uv, 64, 64).unwrap(),
            32.
        );
    }

    #[test]
    fn sampling_rejects_invalid_input_and_emission_overflow() {
        let levels = fixture();
        let s = Sampler::default();
        assert!(
            sample_rgb(
                &levels,
                Sampler {
                    mag_filter: 9987,
                    ..s
                },
                [0.; 2],
                0.
            )
            .is_err()
        );
        assert!(sample_rgb(&[], s, [0.; 2], 0.).is_err());
        assert!(sample_rgb(&levels, s, [f32::NAN, 0.], 0.).is_err());
        assert!(sample_rgb(&levels, s, [0.; 2], f32::INFINITY).is_err());
        assert!(
            complete_mips(MipLevel {
                width: 3,
                height: 5,
                rgba8: vec![0; 59]
            })
            .is_err()
        );
        assert!(k_tex([[0.; 3]; 3], [[0.; 2]; 3], 0, 1).is_err());
        assert!(emissive_multiplier([f32::NAN; 3], [1.; 3], [0.; 3]).is_err());
        assert!(emissive_multiplier([0.5; 3], [-1.; 3], [0.; 3]).is_err());
        assert!(emissive_multiplier([f32::from_bits(1); 3], [1.; 3], [1.; 3]).is_err());
    }
}
