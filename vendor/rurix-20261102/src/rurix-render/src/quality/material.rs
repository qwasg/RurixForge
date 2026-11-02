//! RFC-0052 §4.3: HQ material math, color domains and complete resident texture pages.
//! Independent HQ protocol v3; does not change RFC-0051/G35 buffers or shaders.
//! Assisted-by: Codex:gpt-6-astra.

use crate::texture_sampling::{MipLevel, Sampler};
use std::collections::BTreeMap;

pub const HQ_TRI_WORDS: usize = 64;
pub const HQ_FACTOR_WORDS: usize = 32;
pub const HQ_MATERIAL_PROTOCOL: u32 = 3;
pub const MAX_SEGMENTS: usize = 16;
pub const PAGE_BYTES: usize = 128 * 1024;
pub const MATERIAL_PASS_BUFFERS: u32 = 24;
pub const NORMAL_MIN_LENGTH: f64 = 1e-12;
pub const TRI_NORMAL_OFFSET: usize = 4;
pub const TRI_TANGENT_OFFSET: usize = 13;
pub const TRI_DENSITY_OFFSET: usize = 25;
pub const TRI_UV_OFFSET: usize = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalMode {
    Full,
    GeometricControl,
}
impl NormalMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::GeometricControl => "geometric_control",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Domain {
    Srgb = 0,
    Linear = 1,
    Normal = 2,
}

#[derive(Debug, Clone, Copy)]
pub struct PackingLimits {
    pub max_storage_buffer_range: u64,
    pub max_per_stage_descriptor_storage_buffers: u32,
    pub max_descriptor_set_storage_buffers: u32,
    pub max_per_stage_resources: u32,
}
impl PackingLimits {
    pub fn check_descriptors(self, count: u32) -> Result<(), String> {
        if count > self.max_per_stage_descriptor_storage_buffers
            || count > self.max_descriptor_set_storage_buffers
            || count > self.max_per_stage_resources
        {
            return Err(format!(
                "HQ descriptor limit: need {count}, stage={}, set={}, total={}",
                self.max_per_stage_descriptor_storage_buffers,
                self.max_descriptor_set_storage_buffers,
                self.max_per_stage_resources
            ));
        }
        Ok(())
    }
    pub fn segment_capacity(self) -> Result<usize, String> {
        self.check_descriptors(MATERIAL_PASS_BUFFERS)?;
        let cap = self.max_storage_buffer_range / PAGE_BYTES as u64 * PAGE_BYTES as u64;
        if cap < PAGE_BYTES as u64 {
            return Err("HQ storage-buffer range is smaller than one page".into());
        }
        usize::try_from(cap).map_err(|_| "HQ storage-buffer range exceeds address space".into())
    }
    pub fn check_buffer(self, bytes: usize, label: &str) -> Result<(), String> {
        if bytes == 0 || bytes % 4 != 0 || bytes as u64 > self.max_storage_buffer_range {
            return Err(format!(
                "HQ {label} invalid size {bytes} for range {}",
                self.max_storage_buffer_range
            ));
        }
        Ok(())
    }
}

/// Mip chains retain all authored bytes when the producer declares their domain.
/// Missing raster tails are generated in that domain, never silently reused from
/// a cache entry for another domain.
pub fn complete_mips(base: MipLevel, domain: Domain) -> Result<Vec<MipLevel>, String> {
    if domain == Domain::Srgb {
        return crate::texture_sampling::complete_mips(base);
    }
    base.validate()?;
    let mut levels = vec![base];
    while levels.last().unwrap().width > 1 || levels.last().unwrap().height > 1 {
        let src = levels.last().unwrap();
        let (w, h) = ((src.width / 2).max(1), (src.height / 2).max(1));
        let size = (w as usize)
            .checked_mul(h as usize)
            .and_then(|x| x.checked_mul(4))
            .ok_or("HQ mip size overflow")?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(size)
            .map_err(|_| "HQ mip allocation failed")?;
        bytes.resize(size, 0);
        for y in 0..h {
            for x in 0..w {
                let (x0, x1) = (
                    f64::from(x) * f64::from(src.width) / f64::from(w),
                    f64::from(x + 1) * f64::from(src.width) / f64::from(w),
                );
                let (y0, y1) = (
                    f64::from(y) * f64::from(src.height) / f64::from(h),
                    f64::from(y + 1) * f64::from(src.height) / f64::from(h),
                );
                let mut sum = [0.0f64; 4];
                for sy in y0.floor() as u32..(y1.ceil() as u32).min(src.height) {
                    let wy = (y1.min(f64::from(sy + 1)) - y0.max(f64::from(sy))).max(0.0);
                    for sx in x0.floor() as u32..(x1.ceil() as u32).min(src.width) {
                        let wx = (x1.min(f64::from(sx + 1)) - x0.max(f64::from(sx))).max(0.0);
                        let p = (sy as usize * src.width as usize + sx as usize) * 4;
                        for c in 0..4 {
                            sum[c] += f64::from(src.rgba8[p + c]) * wx * wy;
                        }
                    }
                }
                let area = (x1 - x0) * (y1 - y0);
                let offset = (y as usize * w as usize + x as usize) * 4;
                for c in 0..4 {
                    bytes[offset + c] = (sum[c] / area).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
        levels.push(MipLevel {
            width: w,
            height: h,
            rgba8: bytes,
        });
    }
    Ok(levels)
}

pub fn validate_chain(mips: &[MipLevel]) -> Result<(), String> {
    if mips.is_empty() {
        return Err("HQ image has no mip levels".into());
    }
    for (i, mip) in mips.iter().enumerate() {
        mip.validate()?;
        if i > 0 {
            let p = &mips[i - 1];
            if (p.width == 1 && p.height == 1)
                || mip.width != (p.width / 2).max(1)
                || mip.height != (p.height / 2).max(1)
            {
                return Err("HQ image has invalid mip order or dimensions".into());
            }
        }
    }
    if mips.last().unwrap().width != 1 || mips.last().unwrap().height != 1 {
        return Err("HQ image mip chain is incomplete".into());
    }
    Ok(())
}

fn wrapped(i: i64, n: u32, wrap: u32) -> usize {
    let n = i64::from(n);
    match wrap {
        33071 => i.clamp(0, n - 1) as usize,
        33648 => {
            let p = i.rem_euclid(2 * n);
            if p < n {
                p as usize
            } else {
                (2 * n - 1 - p) as usize
            }
        }
        _ => i.rem_euclid(n) as usize,
    }
}
fn sample_level(
    m: &MipLevel,
    sampler: Sampler,
    domain: Domain,
    uv: [f32; 2],
    linear: bool,
) -> [f32; 4] {
    let reduce = |v: f32, wrap| match wrap {
        33071 => v.clamp(0.0, 1.0),
        33648 => v.rem_euclid(2.0),
        _ => v.rem_euclid(1.0),
    };
    let x = reduce(uv[0], sampler.wrap_s) * m.width as f32 - if linear { 0.5 } else { 0.0 };
    let y = reduce(uv[1], sampler.wrap_t) * m.height as f32 - if linear { 0.5 } else { 0.0 };
    let (bx, by) = (x.floor() as i64, y.floor() as i64);
    let (fx, fy) = if linear {
        (x - x.floor(), y - y.floor())
    } else {
        (0.0, 0.0)
    };
    let lut = crate::texture_sampling::srgb_lut();
    let get = |dx, dy, c| {
        let p = (wrapped(by + dy, m.height, sampler.wrap_t) * m.width as usize
            + wrapped(bx + dx, m.width, sampler.wrap_s))
            * 4
            + c;
        if domain == Domain::Srgb && c < 3 {
            lut[m.rgba8[p] as usize]
        } else {
            m.rgba8[p] as f32 / 255.0
        }
    };
    std::array::from_fn(|c| {
        let a = get(0, 0, c) * (1.0 - fx) + get(1, 0, c) * fx;
        let b = get(0, 1, c) * (1.0 - fx) + get(1, 1, c) * fx;
        a * (1.0 - fy) + b * fy
    })
}
pub fn sample_rgba(
    mips: &[MipLevel],
    sampler: Sampler,
    domain: Domain,
    uv: [f32; 2],
    lambda: f32,
) -> Result<[f32; 4], String> {
    validate_chain(mips)?;
    sampler.validate()?;
    if !uv.iter().chain([&lambda]).all(|x| x.is_finite()) {
        return Err("HQ sample has non-finite coordinates or LOD".into());
    }
    let filter = if lambda > 0.0 {
        sampler.min_filter
    } else {
        sampler.mag_filter
    };
    let last = mips.len() - 1;
    let mut level = 0.0;
    if lambda > 0.0 {
        if [9984, 9985].contains(&filter) {
            level = (lambda + 0.5).floor().clamp(0.0, last as f32);
        }
        if [9986, 9987].contains(&filter) {
            level = lambda.clamp(0.0, last as f32);
        }
    }
    let lo = level.floor() as usize;
    let hi = (lo + 1).min(last);
    let blend = level - level.floor();
    let linear = [9729, 9985, 9987].contains(&filter);
    let a = sample_level(&mips[lo], sampler, domain, uv, linear);
    let b = sample_level(&mips[hi], sampler, domain, uv, linear);
    Ok(std::array::from_fn(|c| a[c] * (1.0 - blend) + b[c] * blend))
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn normalized(v: [f64; 3]) -> Result<[f64; 3], String> {
    let len = dot(v, v).sqrt();
    if !len.is_finite() || len <= NORMAL_MIN_LENGTH {
        return Err("HQ zero/non-finite normal or tangent".into());
    }
    Ok(v.map(|x| x / len))
}
pub fn transform_tbn(
    normal: [f32; 3],
    tangent: [f32; 4],
    linear: [[f64; 3]; 3],
) -> Result<([f32; 3], [f32; 4]), String> {
    if !normal.iter().chain(tangent.iter()).all(|x| x.is_finite())
        || !linear.iter().flatten().all(|x| x.is_finite())
        || ![-1.0, 1.0].contains(&tangent[3])
    {
        return Err("HQ invalid TBN or object transform".into());
    }
    let cof = [
        cross(linear[1], linear[2]),
        cross(linear[2], linear[0]),
        cross(linear[0], linear[1]),
    ];
    let determinant = dot(linear[0], cof[0]);
    if !determinant.is_finite() || determinant == 0.0 {
        return Err("HQ singular object transform".into());
    }
    let source_n = normal.map(f64::from);
    let source_t = [tangent[0] as f64, tangent[1] as f64, tangent[2] as f64];
    let n = normalized(std::array::from_fn(|r| dot(cof[r], source_n) / determinant))?;
    let t0: [f64; 3] = std::array::from_fn(|r| dot(linear[r], source_t));
    let parallel = dot(t0, n);
    let t = normalized(std::array::from_fn(|r| t0[r] - parallel * n[r]))?;
    Ok((
        n.map(|x| x as f32),
        [
            t[0] as f32,
            t[1] as f32,
            t[2] as f32,
            tangent[3] * determinant.signum() as f32,
        ],
    ))
}
pub fn apply_normal_map(
    n: [f32; 3],
    tangent: [f32; 4],
    sample: [f32; 3],
    scale: f32,
    y_sign: f32,
) -> Result<[f32; 3], String> {
    if !n
        .iter()
        .chain(tangent.iter())
        .chain(sample.iter())
        .chain([&scale, &y_sign])
        .all(|x| x.is_finite())
        || ![-1.0, 1.0].contains(&tangent[3])
        || ![-1.0, 1.0].contains(&y_sign)
    {
        return Err("HQ invalid normal-map input".into());
    }
    let n = normalized(n.map(f64::from))?;
    let raw_t = [tangent[0] as f64, tangent[1] as f64, tangent[2] as f64];
    let d = dot(raw_t, n);
    let t = normalized(std::array::from_fn(|c| raw_t[c] - d * n[c]))?;
    let b = cross(n, t).map(|v| v * tangent[3] as f64);
    let local = normalized([
        (sample[0] as f64 * 2.0 - 1.0) * scale as f64,
        (sample[1] as f64 * 2.0 - 1.0) * scale as f64 * y_sign as f64,
        sample[2] as f64 * 2.0 - 1.0,
    ])?;
    Ok(normalized(std::array::from_fn(|c| {
        t[c] * local[0] + b[c] * local[1] + n[c] * local[2]
    }))?
    .map(|v| v as f32))
}

/// Complete resident pages with no second logical-heap allocation. Byte equality
/// resolves hash candidates; all mip records remain even when payloads are shared.
pub struct TextureStore {
    limits: PackingLimits,
    capacity: usize,
    pub segments: Vec<Vec<u8>>,
    pub logical_bytes: usize,
    mips: Vec<[u32; 4]>,
    bindings: Vec<[u32; 8]>,
    candidates: BTreeMap<(u32, u32, Domain, String), Vec<u32>>,
    pub source_bytes_before_dedup: usize,
    pub image_chains: u32,
}
impl TextureStore {
    pub fn new(limits: PackingLimits) -> Result<Self, String> {
        let capacity = limits.segment_capacity()?;
        let mut out = Self {
            limits,
            capacity,
            segments: vec![Vec::new()],
            logical_bytes: 0,
            mips: vec![[0, 1, 1, 0]],
            bindings: vec![[0, 1, 10497, 10497, 9987, 9729, 0, 0]],
            candidates: BTreeMap::new(),
            source_bytes_before_dedup: 4,
            image_chains: 0,
        };
        out.append(&[255; 4])?;
        Ok(out)
    }
    fn append(&mut self, mut bytes: &[u8]) -> Result<u32, String> {
        let offset = u32::try_from(self.logical_bytes / 4)
            .map_err(|_| "HQ texture texel address exceeds u32")?;
        let next = self
            .logical_bytes
            .checked_add(bytes.len())
            .ok_or("HQ texture heap overflow")?;
        if bytes.len() % 4 != 0
            || next / 4 > u32::MAX as usize
            || next > self.capacity.saturating_mul(MAX_SEGMENTS)
        {
            return Err("HQ complete texture heap exceeds resident page capacity".into());
        }
        while !bytes.is_empty() {
            if self.segments.last().unwrap().len() == self.capacity {
                self.segments.push(Vec::new());
            }
            let dst = self.segments.last_mut().unwrap();
            let count = bytes.len().min(self.capacity - dst.len());
            dst.try_reserve(count)
                .map_err(|_| "HQ texture segment allocation failed")?;
            dst.extend_from_slice(&bytes[..count]);
            bytes = &bytes[count..];
        }
        self.logical_bytes = next;
        Ok(offset)
    }
    fn equal_at(&self, texel: u32, mut bytes: &[u8]) -> bool {
        let mut pos = texel as usize * 4;
        while !bytes.is_empty() {
            let (segment, local) = (pos / self.capacity, pos % self.capacity);
            let Some(data) = self.segments.get(segment) else {
                return false;
            };
            let count = bytes.len().min(data.len().saturating_sub(local));
            if count == 0 || data.get(local..local + count) != Some(&bytes[..count]) {
                return false;
            }
            pos += count;
            bytes = &bytes[count..];
        }
        true
    }
    pub fn add_image(&mut self, mips: &[MipLevel], domain: Domain) -> Result<(u32, u32), String> {
        validate_chain(mips)?;
        let first = u32::try_from(self.mips.len()).map_err(|_| "HQ mip count exceeds u32")?;
        for mip in mips {
            self.source_bytes_before_dedup = self
                .source_bytes_before_dedup
                .checked_add(mip.rgba8.len())
                .ok_or("HQ source byte count overflow")?;
            let key = (
                mip.width,
                mip.height,
                domain,
                rurix_pkg::sha256::hex_digest(&mip.rgba8),
            );
            let shared = self.candidates.get(&key).and_then(|items| {
                items
                    .iter()
                    .copied()
                    .find(|&offset| self.equal_at(offset, &mip.rgba8))
            });
            let offset = match shared {
                Some(x) => x,
                None => {
                    let x = self.append(&mip.rgba8)?;
                    self.candidates.entry(key).or_default().push(x);
                    x
                }
            };
            self.mips.push([offset, mip.width, mip.height, 0]);
        }
        self.image_chains = self
            .image_chains
            .checked_add(1)
            .ok_or("HQ image chain count overflow")?;
        Ok((
            first,
            u32::try_from(mips.len()).map_err(|_| "HQ image mip count overflow")?,
        ))
    }
    pub fn add_binding(
        &mut self,
        chain: (u32, u32),
        sampler: Sampler,
        domain: Domain,
    ) -> Result<u32, String> {
        sampler.validate()?;
        if chain.1 == 0
            || chain.0 as usize >= self.mips.len()
            || chain
                .0
                .checked_add(chain.1)
                .map(|v| v as usize > self.mips.len())
                .unwrap_or(true)
        {
            return Err("HQ binding mip chain invalid".into());
        }
        let index = u32::try_from(self.bindings.len()).map_err(|_| "HQ binding count overflow")?;
        if index >= 1 << 24 {
            return Err("HQ binding IDs are not exact f32 integers".into());
        }
        self.bindings.push([
            chain.0,
            chain.1,
            sampler.wrap_s,
            sampler.wrap_t,
            sampler.min_filter,
            sampler.mag_filter,
            domain as u32,
            0,
        ]);
        Ok(index)
    }
    pub fn logical_sha256(&self) -> String {
        let mut h = rurix_pkg::sha256::Sha256::new();
        for segment in &self.segments {
            h.update(segment);
        }
        rurix_pkg::sha256::hex(&h.finalize())
    }
    pub fn metadata(&self) -> Result<Vec<u8>, String> {
        let table = 6usize
            .checked_add(
                self.bindings
                    .len()
                    .checked_mul(8)
                    .ok_or("HQ binding table overflow")?,
            )
            .ok_or("HQ metadata overflow")?;
        let pages = self.logical_bytes.div_ceil(PAGE_BYTES);
        let page_table = table
            .checked_add(
                self.mips
                    .len()
                    .checked_mul(4)
                    .ok_or("HQ mip table overflow")?,
            )
            .ok_or("HQ metadata overflow")?;
        let u32n = |n| u32::try_from(n).map_err(|_| "HQ metadata exceeds u32");
        let mut words = vec![
            u32n(self.bindings.len())?,
            u32n(table)?,
            self.image_chains
                .checked_add(1)
                .ok_or("HQ image count overflow")?,
            HQ_MATERIAL_PROTOCOL,
            u32n(page_table)?,
            u32n(pages)?,
        ];
        words.extend(self.bindings.iter().flatten());
        words.extend(self.mips.iter().flatten());
        for page in 0..pages {
            let byte = page * PAGE_BYTES;
            words.push(u32n(byte / self.capacity)?);
            words.push(u32n((byte % self.capacity) / 4)?);
        }
        let bytes: Vec<u8> = words.into_iter().flat_map(u32::to_le_bytes).collect();
        self.limits.check_buffer(bytes.len(), "texture metadata")?;
        validate_texture_packet(&bytes, &self.segments, self.limits)?;
        Ok(bytes)
    }
    pub fn segment_capacity(&self) -> usize {
        self.capacity
    }
    pub fn binding_count(&self) -> usize {
        self.bindings.len()
    }
}

/// Second, independent fail-closed boundary before resource attachment.
pub fn validate_texture_packet(
    meta: &[u8],
    segments: &[Vec<u8>],
    limits: PackingLimits,
) -> Result<(), String> {
    let capacity = limits.segment_capacity()?;
    limits.check_buffer(meta.len(), "metadata")?;
    if meta.len() < 24 || segments.is_empty() || segments.len() > MAX_SEGMENTS {
        return Err("HQ truncated metadata or invalid segment count".into());
    }
    let words: Vec<u32> = meta
        .chunks_exact(4)
        .map(|x| u32::from_le_bytes(x.try_into().unwrap()))
        .collect();
    if words[3] != HQ_MATERIAL_PROTOCOL || words[0] == 0 {
        return Err("HQ metadata protocol/binding count invalid".into());
    }
    let mut total = 0usize;
    for (i, segment) in segments.iter().enumerate() {
        limits.check_buffer(segment.len(), "texture segment")?;
        if segment.len() > capacity || (i + 1 < segments.len() && segment.len() != capacity) {
            return Err("HQ segment occupancy violates the page address map".into());
        }
        total = total
            .checked_add(segment.len())
            .ok_or("HQ logical heap overflow")?;
    }
    if total / 4 > u32::MAX as usize || segments[0].get(..4) != Some(&[255u8; 4]) {
        return Err("HQ invalid texel address space/dummy".into());
    }
    let bindings = words[0] as usize;
    let table = words[1] as usize;
    let page_table = words[4] as usize;
    let pages = words[5] as usize;
    if 6usize.checked_add(bindings.checked_mul(8).ok_or("HQ metadata overflow")?) != Some(table)
        || table > page_table
        || page_table > words.len()
        || (page_table - table) % 4 != 0
        || page_table.checked_add(pages.checked_mul(2).ok_or("HQ page table overflow")?)
            != Some(words.len())
        || pages != total.div_ceil(PAGE_BYTES)
    {
        return Err("HQ metadata table bounds invalid".into());
    }
    let mip_count = (page_table - table) / 4;
    if words[2] == 0 || words[2] as usize > mip_count {
        return Err("HQ image/mip count invalid".into());
    }
    for b in 0..bindings {
        let r = 6 + b * 8;
        let first = words[r] as usize;
        let count = words[r + 1] as usize;
        if count == 0
            || first >= mip_count
            || first
                .checked_add(count)
                .filter(|&end| end <= mip_count)
                .is_none()
            || words[r + 6] > 2
        {
            return Err(format!("HQ binding {b} mip range/domain invalid"));
        }
        Sampler {
            wrap_s: words[r + 2],
            wrap_t: words[r + 3],
            min_filter: words[r + 4],
            mag_filter: words[r + 5],
        }
        .validate()?;
        let mut previous: Option<(u32, u32)> = None;
        for index in first..first + count {
            let m = table + index * 4;
            let (width, height) = (words[m + 1], words[m + 2]);
            if let Some((pw, ph)) = previous {
                if (pw == 1 && ph == 1) || width != (pw / 2).max(1) || height != (ph / 2).max(1) {
                    return Err("HQ binding mip dimensions invalid".into());
                }
            }
            previous = Some((width, height));
        }
        if previous != Some((1, 1)) {
            return Err("HQ binding lacks complete mip tail".into());
        }
    }
    for i in 0..mip_count {
        let r = table + i * 4;
        let (offset, width, height) = (
            words[r] as usize,
            words[r + 1] as usize,
            words[r + 2] as usize,
        );
        if width == 0
            || height == 0
            || width
                .checked_mul(height)
                .and_then(|n| offset.checked_add(n))
                .filter(|&end| end <= total / 4)
                .is_none()
        {
            return Err("HQ mip references texels outside heap".into());
        }
    }
    for page in 0..pages {
        let logical = page * PAGE_BYTES;
        let r = page_table + page * 2;
        if words[r] as usize != logical / capacity
            || words[r + 1] as usize != (logical % capacity) / 4
        {
            return Err("HQ page mapping is not the declared contiguous resident layout".into());
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "material_tests.rs"]
mod tests;
