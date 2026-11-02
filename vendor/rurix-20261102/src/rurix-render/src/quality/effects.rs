//! RFC-0052 §4.6–4.8: same-frame effects resource recipes and strict adapters.
//! Assisted-by: Codex:gpt-6-astra.
//!
//! These recipes bind new HQ kernels only. G40/G41/G35 legacy entrypoints are
//! never dispatched as screenshots/overlays. GPU correctness needs real runs.

use std::collections::{BTreeMap, BTreeSet};

pub const CLOUD_RAY_FLOATS: usize = 12;
/// HQ-only extension: original G40 pack68 + absolute world origin xyz.
pub const HQ_CLOUD_PARAM_FLOATS: usize = 71;
/// HQ-only extension: compatible water physics slots96 + absolute world origin xyz.
pub const HQ_WATER_PARAM_FLOATS: usize = 99;
pub const AIR_FLOATS: usize = 8;
pub const WATER_SURFACE_FLOATS: usize = 16;
pub const RAIN_PARTICLE_FLOATS: usize = 16;
pub const MAX_SORTED_RAIN_PARTICLES: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectAccess { Read, Write }

/// Binding order is the generated .rx declaration order excluding ThreadCtx/AS.
/// Read/write intent stays distinct from runtime's implicit storage-RW completion.
#[derive(Clone, Copy, Debug)]
pub struct EffectPassRecipe {
    pub kernel: &'static str,
    pub bindings: &'static [(&'static str, EffectAccess)],
    pub reads_current_as: bool,
    pub local_size: [u32; 3],
}

use EffectAccess::{Read as R, Write as W};

pub const CLOUD_DIRECTIONAL: EffectPassRecipe = EffectPassRecipe {
    kernel: "hq_cloud_directional", reads_current_as: false, local_size: [8, 8, 1],
    bindings: &[("cloud_params", R), ("weather", R), ("noise_base", R), ("noise_detail", R),
        ("sky_lut", R), ("rays", R), ("air", W)],
};
pub const DIRECTIONAL_RAY_CUTOFF: EffectPassRecipe = EffectPassRecipe {
    kernel: "hq_directional_ray_cutoff", reads_current_as: false, local_size: [8, 8, 1],
    bindings: &[("dims", R), ("rays", R), ("radiance", R), ("bounded_rays", W)],
};
pub const DIRECTIONAL_AIR_RESOLVE: EffectPassRecipe = EffectPassRecipe {
    kernel: "hq_directional_air_resolve", reads_current_as: false, local_size: [8, 8, 1],
    bindings: &[("dims", R), ("radiance", R), ("directional_air", R), ("out_radiance", W)],
};
pub const ENVIRONMENT_FILL: EffectPassRecipe = EffectPassRecipe {
    kernel: "hq_environment_fill", reads_current_as: true, local_size: [8, 8, 1],
    bindings: &[("cloud_params", R), ("sky_lut", R), ("rays", R), ("raw_radiance", R), ("radiance", W)],
};
pub const SKY_BACKGROUND: EffectPassRecipe = EffectPassRecipe {
    kernel: "hq_sky_background", reads_current_as: false, local_size: [8, 8, 1],
    bindings: &[("cloud_params", R), ("sky_lut", R), ("rays", R), ("gbuffer", R), ("opaque_rgb", R), ("sky_opaque_rgb", W)],
};
pub const WATER_BED: EffectPassRecipe = EffectPassRecipe {
    kernel: "hq_water_bed", reads_current_as: true, local_size: [8, 8, 1],
    bindings: &[("bed_params", R), ("bed", W), ("obstacle", W)],
};
pub const WATER_WAVE: EffectPassRecipe = EffectPassRecipe {
    kernel: "hq_water_wave", reads_current_as: false, local_size: [8, 8, 1],
    bindings: &[("wave_params", R), ("h_cur", R), ("h_prev", R), ("obstacle", R), ("h_next", W)],
};
pub const WATER_SURFACE: EffectPassRecipe = EffectPassRecipe {
    kernel: "hq_water_surface_prepare", reads_current_as: false, local_size: [8, 8, 1],
    bindings: &[("water_params", R), ("frame", R), ("wave_h", R), ("wave_prev", R),
        ("bed", R), ("noise2d", R), ("scene_depth", R), ("water_surface", W),
        ("reflection_rays", W), ("refraction_rays", W)],
};
pub const WATER_FOAM_LIGHTING: EffectPassRecipe = EffectPassRecipe {
    kernel: "hq_water_foam_lighting", reads_current_as: true, local_size: [8, 8, 1],
    bindings: &[("water_params", R), ("frame", R), ("effects", R), ("water_surface", R),
        ("bed", R), ("cloud_params", R), ("weather", R), ("noise_base", R),
        ("noise_detail", R), ("sky_lut", R), ("foam_lighting", W)],
};
pub const WATER_CLOSURE: EffectPassRecipe = EffectPassRecipe {
    kernel: "hq_water_closure", reads_current_as: true, local_size: [8, 8, 1],
    bindings: &[("water_params", R), ("frame", R), ("wave_h", R), ("water_surface", R),
        ("reflection_rgb", R), ("refraction_rgb", R), ("refraction_rays", R),
        ("opaque_rgb", R), ("opaque_depth", R), ("water_blur1", R), ("water_blur2", R), ("foam_lighting", R),
        ("water_rgb", W), ("water_feature_facts", W), ("water_reflection_transport", W)],
};
pub const WATER_BLUR: EffectPassRecipe = EffectPassRecipe {
    kernel: "hq_water_blur", reads_current_as: false, local_size: [8, 8, 1],
    bindings: &[("blur_params", R), ("opaque_rgb", R), ("water_blur", W)],
};
pub const CAMERA_AIR_PREPARE: EffectPassRecipe = EffectPassRecipe {
    kernel: "hq_camera_air_prepare", reads_current_as: false, local_size: [8, 8, 1],
    bindings: &[("frame", R), ("effects", R), ("gbuffer", R), ("water_surface", R), ("camera_rays", W)],
};
pub const CLOUD_MOTION: EffectPassRecipe = EffectPassRecipe {
    kernel: "hq_cloud_motion", reads_current_as: false, local_size: [8, 8, 1],
    bindings: &[("frame", R), ("effects", R), ("camera_rays", R), ("air", R), ("cloud_motion", W)],
};
pub const EFFECTS_COMPOSITE: EffectPassRecipe = EffectPassRecipe {
    kernel: "hq_effects_composite", reads_current_as: false, local_size: [8, 8, 1],
    bindings: &[("frame", R), ("effects", R), ("opaque_rgb", R), ("opaque_depth", R),
        ("opaque_mv", R), ("opaque_reactive", R), ("water_surface", R), ("water_rgb", R),
        ("camera_rays", R), ("air", R), ("cloud_motion", R), ("previous_air", R),
        ("surface_rgb", W), ("surface_depth", W),
        ("surface_mv", W), ("surface_reactive", W)],
};
pub const CLOUD_HISTORY_STORE: EffectPassRecipe = EffectPassRecipe {
    kernel: "hq_cloud_history_store", reads_current_as: false, local_size: [8, 8, 1],
    bindings: &[("frame", R), ("camera_rays", R), ("air", R), ("candidate_air", W)],
};
pub const RAIN_SORTED: EffectPassRecipe = EffectPassRecipe {
    kernel: "hq_rain_sorted", reads_current_as: true, local_size: [8, 8, 1],
    bindings: &[("frame", R), ("rain_params", R), ("rain_args", R), ("effects", R), ("rain_particles", R),
        ("rain_ids", R), ("surface_rgb", R), ("surface_depth", R), ("surface_mv", R),
        ("surface_reactive", R), ("cloud_params", R), ("weather", R), ("noise_base", R),
        ("noise_detail", R), ("sky_lut", R), ("previous_rain_coverage", R),
        ("rain_rgb", W), ("rain_mv", W), ("rain_reactive", W), ("candidate_rain_coverage", W)],
};
pub const RAIN_SIM: EffectPassRecipe = EffectPassRecipe {
    kernel: "hq_rain_sim", reads_current_as: false, local_size: [64, 1, 1],
    bindings: &[("sim_params", R), ("rain_args", R), ("rain_state", R), ("rain_updated", W), ("rain_flags", W)],
};
pub const RAIN_COMPACT: EffectPassRecipe = EffectPassRecipe {
    kernel: "hq_rain_compact", reads_current_as: false, local_size: [64, 1, 1],
    bindings: &[("scan_params", R), ("rain_flags", R), ("rain_scan", R), ("rain_updated", R),
        ("rain_ids", R), ("rain_capacity_error", R), ("rain_next", W), ("rain_next_ids", W), ("rain_compact_error", W)],
};
pub const RAIN_EMIT: EffectPassRecipe = EffectPassRecipe {
    kernel: "hq_rain_emit", reads_current_as: false, local_size: [1, 1, 1],
    bindings: &[("emit_params", R), ("rain_offsets", R), ("random_table", R), ("rain_capacity_error", R),
        ("rain_next", W), ("rain_next_ids", W), ("rain_emit_error", W)],
};
pub const RAIN_PACK: EffectPassRecipe = EffectPassRecipe {
    kernel: "hq_rain_pack", reads_current_as: false, local_size: [1, 1, 1],
    bindings: &[("rain_args", R), ("rain_state", R), ("rain_particles", W)],
};
pub const RAIN_SCAN_SUM: EffectPassRecipe = EffectPassRecipe {
    kernel: "hq_rain_scan_seg_sum", reads_current_as: false, local_size: [1, 1, 1],
    bindings: &[("rain_flags", R), ("scan_params", R), ("rain_sums", W)],
};
pub const RAIN_SCAN_SPINE: EffectPassRecipe = EffectPassRecipe {
    kernel: "hq_rain_scan_spine", reads_current_as: false, local_size: [1, 1, 1],
    bindings: &[("rain_sums", R), ("scan_params", R), ("rain_offsets", W)],
};
pub const RAIN_SCAN_APPLY: EffectPassRecipe = EffectPassRecipe {
    kernel: "hq_rain_scan_seg_apply", reads_current_as: false, local_size: [1, 1, 1],
    bindings: &[("rain_flags", R), ("rain_offsets", R), ("scan_params", R), ("rain_scan", W)],
};
pub const RAIN_ARGS: EffectPassRecipe = EffectPassRecipe {
    kernel: "hq_rain_indirect_args", reads_current_as: false, local_size: [1, 1, 1],
    bindings: &[("args_params", R), ("rain_offsets", R), ("rain_args", W), ("rain_capacity_error", W)],
};

pub const EFFECT_PASS_RECIPES: &[EffectPassRecipe] = &[CLOUD_DIRECTIONAL, WATER_BED,
    WATER_WAVE, WATER_SURFACE, WATER_CLOSURE, CAMERA_AIR_PREPARE, CLOUD_MOTION,
    EFFECTS_COMPOSITE, RAIN_SORTED, DIRECTIONAL_RAY_CUTOFF, DIRECTIONAL_AIR_RESOLVE,
    RAIN_SIM, RAIN_COMPACT, RAIN_EMIT, RAIN_PACK, RAIN_SCAN_SUM, RAIN_SCAN_SPINE, RAIN_SCAN_APPLY, RAIN_ARGS,
    ENVIRONMENT_FILL, SKY_BACKGROUND, CLOUD_HISTORY_STORE, WATER_BLUR, WATER_FOAM_LIGHTING];

/// Publication state for GPU cloud-history ping-pong. Merely writing the
/// candidate does not make it readable next frame. An uncommitted failed frame
/// must reset; its dirty candidate is cleared to neutral by the owner.
#[derive(Clone, Debug, Default)]
pub struct CloudHistoryState { read:usize, pending:Option<usize> }
impl CloudHistoryState {
    pub fn begin(&mut self, reset:bool)->Result<(usize,usize),String>{
        if self.pending.is_some() && !reset{return Err("uncommitted cloud history requires reset".into());}
        if reset{self.read=0;self.pending=None;}
        let write=1-self.read;self.pending=Some(write);Ok((self.read,write))
    }
    pub fn commit(&mut self)->Result<(),String>{
        self.read=self.pending.take().ok_or("cloud history commit without candidate")?;Ok(())
    }
    pub fn read_index(&self)->usize{self.read}
}

/// T=1, validity=0, representative distance=0, cutoff=0: reset contributes no
/// previous opacity and contains no invented cloud/background intersection.
pub fn neutral_cloud_history(pixels:usize)->Result<Vec<f32>,String>{
    let count=pixels.checked_mul(4).ok_or("cloud history size overflow")?;
    let mut data=vec![0.0;count];for row in data.chunks_exact_mut(4){row[0]=1.0;}Ok(data)
}

#[derive(Clone, Copy, Debug)]
pub struct EffectResource {
    pub index: u32,
    pub bytes: u64,
}

impl EffectPassRecipe {
    /// Resolve all bindings and reject in-place feedback, descriptor pressure or
    /// range violations before Vulkan. AS availability/generation is a separate
    /// mandatory graph edge supplied by the owner, never inferred from buffers.
    pub fn resolve(&self, resources: &BTreeMap<&str, EffectResource>,
        max_storage_range: u64, max_storage_descriptors: u32,
        current_as_ready: bool) -> Result<Vec<(u32, EffectAccess)>, String> {
        if self.bindings.len() > max_storage_descriptors as usize {
            return Err(format!("{} needs {} storage descriptors (device limit {})",
                self.kernel, self.bindings.len(), max_storage_descriptors));
        }
        if self.reads_current_as && !current_as_ready {
            return Err(format!("{} requires current-generation AS build/write -> query/read", self.kernel));
        }
        let mut out = Vec::with_capacity(self.bindings.len());
        let mut seen = BTreeMap::new();
        for &(name, access) in self.bindings {
            let r = resources.get(name).ok_or_else(|| format!("{} missing resource {name}", self.kernel))?;
            if r.bytes == 0 || r.bytes % 4 != 0 || r.bytes > max_storage_range {
                return Err(format!("{} invalid range for {name}: {}", self.kernel, r.bytes));
            }
            if let Some(old) = seen.insert(r.index, access) {
                if old == W || access == W {
                    return Err(format!("{} aliases writable resource {name}", self.kernel));
                }
            }
            out.push((r.index, access));
        }
        Ok(out)
    }

    #[cfg(feature = "vulkan")]
    pub fn compute_pass<'a>(&self, spirv: &'a [u8], extent: [u32; 2],
        resolved: &[(u32, EffectAccess)], as_index: u32)
        -> Result<(rurix_rt::render_exec::Pass<'a>, Vec<(u32, rurix_rt::render_exec::TargetState)>), String> {
        use rurix_rt::render_exec::{Bindings, ComputePass, DispatchSpec, Pass, TargetState};
        if extent.contains(&0) || resolved.len() != self.bindings.len()
            || resolved.iter().zip(self.bindings).any(|((_, a), (_, b))| a != b) {
            return Err(format!("{} invalid extent or resolved binding layout", self.kernel));
        }
        if spirv.len() < 20 || spirv.len() % 4 != 0 || spirv[..4] != [3, 2, 35, 7] {
            return Err(format!("{} missing valid SPIR-V header", self.kernel));
        }
        let plan = resolved.iter().map(|&(id, access)| (id, match access {
            R => TargetState::ShaderRead, W => TargetState::StorageWrite,
        })).collect();
        let pass = Pass::Compute(ComputePass { name: self.kernel, spirv, entry: Some("main"),
            dispatch: DispatchSpec::Direct([extent[0].div_ceil(self.local_size[0]), extent[1].div_ceil(self.local_size[1]), 1]),
            bindings: Bindings { storage_buffers: resolved.iter().map(|r| r.0).collect(),
                accel_structs: if self.reads_current_as { vec![as_index] } else { vec![] },
                ..Bindings::default() } });
        Ok((pass, plan))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct HqWaterDomain {
    pub dim: u32,
    pub center_x: f32,
    pub center_z: f32,
    pub extent_m: f32,
    pub water_level_m: f32,
    pub query_top_m: f32,
    pub query_bottom_m: f32,
    pub ray_epsilon_m: f32,
    pub scene_generation: u32,
    pub wave_speed_m_s: f32,
    pub dt_s: f32,
}

impl HqWaterDomain {
    pub fn validate(&self) -> Result<(), String> {
        if self.dim < 8 || !self.dim.is_power_of_two() || self.dim > 4096 {
            return Err("HQ wave grid must be power-of-two in [8,4096]".into());
        }
        let floats = [self.center_x, self.center_z, self.extent_m, self.water_level_m,
            self.query_top_m, self.query_bottom_m, self.ray_epsilon_m, self.wave_speed_m_s, self.dt_s];
        if !floats.iter().all(|v| v.is_finite()) || self.extent_m <= 0.0
            || self.ray_epsilon_m <= 0.0 || self.dt_s <= 0.0 || self.wave_speed_m_s < 0.0
            || self.query_top_m <= self.water_level_m || self.query_bottom_m >= self.water_level_m
            || self.scene_generation >= (1 << 24) {
            return Err("HQ water domain has invalid physical bounds/generation".into());
        }
        let c_dt_dx = self.wave_speed_m_s * self.dt_s / (self.extent_m / self.dim as f32);
        if !c_dt_dx.is_finite() || c_dt_dx >= std::f32::consts::FRAC_1_SQRT_2 {
            return Err("HQ physical wave CFL c*dt/dx must be < 1/sqrt(2)".into());
        }
        Ok(())
    }

    pub fn bed_params(&self) -> Result<[f32; 16], String> {
        self.validate()?;
        let mut p = [0.0; 16];
        p[..9].copy_from_slice(&[self.dim as f32, self.center_x, self.center_z, self.extent_m,
            self.water_level_m, self.query_top_m, self.query_bottom_m, self.ray_epsilon_m,
            self.scene_generation as f32]);
        Ok(p)
    }

    /// The frozen G41 formula uses unit grid distance. Convert physical speed to
    /// cells/second explicitly, preserving its difference equation and CFL law.
    pub fn wave_params(&self, damping: f32, sponge_frac: f32,
        drops: &[crate::world::water_surface::WaveDrop]) -> Result<Vec<f32>, String> {
        self.validate()?;
        if !damping.is_finite() || !(0.0..=1.0).contains(&damping)
            || !sponge_frac.is_finite() || !(0.0..=0.5).contains(&sponge_frac)
            || drops.len() > crate::world::water_surface::WAVE_MAX_DROPS {
            return Err("HQ wave damping/sponge/drop count invalid (no truncation)".into());
        }
        if drops.iter().any(|d| ![d.u,d.v,d.intensity,d.radius].iter().all(|v|v.is_finite())
            || !(0.0..=1.0).contains(&d.u) || !(0.0..=1.0).contains(&d.v) || d.radius <= 0.0) {
            return Err("HQ wave drop is nonfinite/out of bounds".into());
        }
        let wp = crate::world::water_surface::WaveParams {
            speed: self.wave_speed_m_s / (self.extent_m / self.dim as f32),
            dt: self.dt_s, damping, sponge_frac,
        };
        Ok(crate::world::water_surface::pack_wave_params(self.dim as usize, &wp, drops))
    }
}

/// Identity proof for rain packing: use the actual previous GPU position for the
/// same (pid,epoch), never reconstruct from velocity after gravity/respawn.
pub fn validate_rain_records(particles: &[f32], ids: &[u32]) -> Result<(), String> {
    if particles.len() % RAIN_PARTICLE_FLOATS != 0 {
        return Err("HQ rain AoS stride mismatch".into());
    }
    let n = particles.len() / RAIN_PARTICLE_FLOATS;
    if n > MAX_SORTED_RAIN_PARTICLES || ids.len() != n * 2 {
        return Err("HQ rain count/identity buffer mismatch".into());
    }
    let mut unique = BTreeSet::new();
    for (record, id) in particles.chunks_exact(16).zip(ids.chunks_exact(2)) {
        if !record.iter().all(|f| f.is_finite()) || record[3] <= 0.0
            || !(0.0..=1.0).contains(&record[7]) || record[8..11].iter().any(|c| *c < 0.0)
            || ![0.0,1.0].contains(&record[12]) || !unique.insert((id[0],id[1])) {
            return Err("HQ rain invalid radius/alpha/radiance/identity/motion validity".into());
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
pub struct HqWaterFeatures {
    pub water: bool,
    pub caustics: bool,
    pub refraction: bool,
    pub foam: bool,
    pub volume: bool,
    pub reflection: bool,
    /// Single-ray continuation is the explicit initial supported variant.
    pub dispersion: bool,
    /// Explicit two-level opaque scene-linear box chain, physical hit gated.
    pub refraction_blur: bool,
}

/// Packs G41-compatible material/camera slots directly from a real world domain;
/// it never constructs a fake LagoonScene. Slots for analytical floor/rocks stay
/// unused zero. All sunlight and ambient values must come from the one HQ light
/// contract; the caller does not also add another copy of its specular term.
#[allow(clippy::too_many_arguments)]
pub fn pack_water_surface_params(domain: &HqWaterDomain,
    material: &crate::world::water_surface::WaterParams,
    camera: &crate::world::water_surface::WaterCamera,
    sun_direction: [f32;3], sun_radiance: [f32;3], ambient_top: [f32;3], ambient_bottom: [f32;3],
    extent: [u32;2], frame_index: u32, time_s: f32, noise_dim: u32,
    sky_lut_extent: [u32;2], features: HqWaterFeatures, world_origin:[f32;3]) -> Result<[f32;HQ_WATER_PARAM_FLOATS],String> {
    domain.validate()?;
    if features.dispersion || material.dispersion != 0.0 {
        return Err("HQ single-ray water continuation cannot claim wavelength dispersion".into());
    }
    if !features.refraction_blur && material.scatter_blur_density != 0.0 {
        return Err("inactive HQ refraction blur requires zero scatter_blur_density".into());
    }
    let mut p=[0.0;HQ_WATER_PARAM_FLOATS];
    p[96..99].copy_from_slice(&world_origin);
    p[0]=extent[0] as f32; p[1]=extent[1] as f32;
    p[2..5].copy_from_slice(&camera.origin);
    p[5..8].copy_from_slice(&camera.forward);
    p[8..11].copy_from_slice(&camera.right);
    p[11..14].copy_from_slice(&camera.up);
    p[14]=camera.tan_half_fov; p[15]=camera.aspect;
    p[16..19].copy_from_slice(&sun_direction);
    p[19..22].copy_from_slice(&sun_radiance);
    p[22..25].copy_from_slice(&ambient_top);
    p[25..28].copy_from_slice(&ambient_bottom);
    p[28]=domain.water_level_m; p[29]=domain.center_x; p[30]=domain.center_z;
    p[42]=domain.extent_m; p[43]=domain.dim as f32; p[44]=material.wave_amplitude_m;
    p[45..48].copy_from_slice(&material.absorption);
    p[48..51].copy_from_slice(&material.scattering);
    p[51]=material.roughness; p[52]=material.normal_detail; p[53]=material.refraction_strength;
    p[54]=material.max_refraction_cross_m; p[55]=material.max_cross_m;
    p[56]=material.refract_steps as f32; p[57]=material.refract_thickness_m;
    p[58]=material.phase_g; p[59]=0.0; p[60]=if features.refraction_blur{material.scatter_blur_density}else{0.0};
    p[61]=material.foam_threshold; p[62]=material.foam_intensity; p[63]=material.shore_foam_depth_m;
    p[64]=material.caustic_intensity; p[65]=material.caustic_convergence;
    p[66]=material.reflection_strength;
    p[67]=crate::world::water_surface::AIR_TO_WATER_ETA;
    p[68]=crate::world::water_surface::WATER_F0;
    p[69]=crate::world::water_surface::EXP_FACTOR;
    p[70]=crate::world::water_surface::REFRACTION_REFERENCE_DISTANCE;
    p[71]=frame_index as f32; p[72]=noise_dim as f32;
    p[73]=sky_lut_extent[0] as f32; p[74]=sky_lut_extent[1] as f32;
    p[76]=material.max_cross_m; p[78]=6.0; p[79]=1.0; p[81]=time_s;
    let bit=|v:bool| if v {1.0} else {0.0};
    p[82]=bit(features.caustics);p[83]=bit(features.refraction);p[84]=0.0;
    p[85]=bit(features.foam);p[86]=bit(features.volume);p[87]=bit(features.reflection);
    p[89]=bit(features.water);
    p[90]=bit(features.refraction_blur);
    if !p.iter().all(|v|v.is_finite()) || extent.contains(&0) || extent.iter().any(|v|*v>=(1<<24))
        || sky_lut_extent.contains(&0) || noise_dim<2 || !noise_dim.is_power_of_two()
        || frame_index>=(1<<24) || camera.origin[1]<=domain.water_level_m+material.wave_amplitude_m
        || p[14]<=0.0 || p[15]<=0.0 || p[44]<0.0 || p[45..51].iter().any(|v|*v<0.0)
        || !(0.008..=1.0).contains(&p[51]) || p[54]<=0.0 || p[55]<=0.0
        || p[57]<=0.0 || !(-0.99..=0.99).contains(&p[58]) || p[60]<0.0
        || !(0.0..=1.0).contains(&p[62]) || p[63]<=0.0 || p[64]<0.0 || p[66]!=1.0 {
        return Err("HQ water surface parameters violate finite/physical/single-source bounds".into());
    }
    for direction in [&camera.forward,&camera.right,&camera.up,&sun_direction] {
        let len2: f32=direction.iter().map(|x|x*x).sum();
        if (len2-1.0).abs()>1e-5 {return Err("HQ camera/light directions must be unit length".into());}
    }
    for (a,b) in [(&camera.forward,&camera.right),(&camera.forward,&camera.up),(&camera.right,&camera.up)] {
        let dot: f32=a.iter().zip(b).map(|(a,b)|a*b).sum();
        if dot.abs()>1e-5 {return Err("HQ camera basis must be orthogonal".into());}
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_history_candidate_does_not_publish_until_success_and_failure_needs_reset() {
        let mut history=CloudHistoryState::default();
        assert_eq!(history.begin(true).unwrap(),(0,1));
        assert_eq!(history.read_index(),0,"candidate device write is not a commit");
        assert!(history.begin(false).is_err(),"failed uncommitted frame cannot continue");
        assert_eq!(history.begin(true).unwrap(),(0,1));
        history.commit().unwrap();assert_eq!(history.read_index(),1);
        assert_eq!(history.begin(false).unwrap(),(1,0));
        history.commit().unwrap();assert_eq!(history.read_index(),0);
        assert!(history.commit().is_err(),"no double publication");
        assert_eq!(neutral_cloud_history(2).unwrap(),vec![1.0,0.0,0.0,0.0,1.0,0.0,0.0,0.0]);
    }

    #[test]
    fn inactive_water_features_are_rejected_or_zero_and_blur_flag_is_real() {
        let domain=HqWaterDomain{dim:16,center_x:0.0,center_z:0.0,extent_m:16.0,
            water_level_m:0.0,query_top_m:3.0,query_bottom_m:-4.0,ray_epsilon_m:0.001,
            scene_generation:0,wave_speed_m_s:1.0,dt_s:0.016};
        let camera=crate::world::water_surface::WaterCamera{origin:[0.0,2.0,0.0],
            forward:[0.0,0.0,-1.0],right:[1.0,0.0,0.0],up:[0.0,1.0,0.0],tan_half_fov:0.5,aspect:1.0};
        let pack=|m:&crate::world::water_surface::WaterParams,f:HqWaterFeatures|
            pack_water_surface_params(&domain,m,&camera,[0.0,1.0,0.0],[1.0;3],[0.1;3],[0.1;3],
                [8,8],0,0.0,8,[8,4],f,[0.0;3]);
        let mut material=crate::world::water_surface::WaterParams::default();
        let mut features=HqWaterFeatures{water:true,caustics:true,refraction:true,foam:true,
            volume:true,reflection:true,dispersion:false,refraction_blur:true};
        assert!(pack(&material,features).is_err(),"nonzero inactive dispersion may not be silently copied");
        material.dispersion=0.0;
        let p=pack(&material,features).unwrap();assert_eq!(p[59],0.0);assert_eq!(p[84],0.0);assert_eq!(p[90],1.0);
        features.dispersion=true;assert!(pack(&material,features).is_err());features.dispersion=false;
        features.refraction_blur=false;assert!(pack(&material,features).is_err());
        material.scatter_blur_density=0.0;
        let p=pack(&material,features).unwrap();assert_eq!(p[60],0.0);assert_eq!(p[90],0.0);
    }

    #[test]
    fn physical_wave_cfl_scales_with_world_cell_size_and_rejects_truncation() {
        let domain = HqWaterDomain { dim: 16, center_x:0.0,center_z:0.0,extent_m:16.0,
            water_level_m:0.0,query_top_m:2.0,query_bottom_m:-4.0,ray_epsilon_m:0.001,
            scene_generation:1,wave_speed_m_s:1.0,dt_s:0.1 };
        let a = domain.wave_params(0.004,0.1,&[]).unwrap();
        let mut finer = domain; finer.extent_m = 8.0;
        let b = finer.wave_params(0.004,0.1,&[]).unwrap();
        assert_eq!(b[1],a[1]*4.0);
        finer.extent_m = 1.0;
        assert!(finer.validate().is_err());
        assert!(domain.wave_params(f32::NAN,0.1,&[]).is_err());
    }

    #[test]
    fn recipes_reject_missing_as_alias_and_capability_pressure() {
        let mut resources = BTreeMap::new();
        for (i,&(name,_)) in WATER_BED.bindings.iter().enumerate() {
            resources.insert(name,EffectResource{index:i as u32,bytes:64});
        }
        assert!(WATER_BED.resolve(&resources,1024,16,false).is_err());
        assert!(WATER_BED.resolve(&resources,1024,2,true).is_err());
        assert!(WATER_BED.resolve(&resources,32,16,true).is_err());
        assert_eq!(WATER_BED.resolve(&resources,1024,16,true).unwrap().len(),3);
        resources.insert("obstacle",EffectResource{index:0,bytes:64});
        assert!(WATER_BED.resolve(&resources,1024,16,true).is_err());
    }

    #[test]
    fn rain_identity_and_opacity_are_not_silently_repaired() {
        let mut p = vec![0.0;32];
        for r in p.chunks_exact_mut(16) { r[3]=0.02; r[7]=0.5; r[12]=1.0; }
        assert!(validate_rain_records(&p,&[1,0,1,1]).is_ok());
        assert!(validate_rain_records(&p,&[1,0,1,0]).is_err());
        p[7]=1.01;
        assert!(validate_rain_records(&p,&[1,0,1,1]).is_err());
    }
}
