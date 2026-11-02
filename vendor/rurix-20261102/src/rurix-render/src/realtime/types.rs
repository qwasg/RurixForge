//! Explicit host interface for the real-time renderer (RXS-0408/0410).
use std::sync::Arc;

pub type Mat4 = [f32; 16];
pub const IDENTITY: Mat4 = [
    1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
];
pub type TextureId = usize;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub tangent: [f32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlphaMode {
    Opaque,
    Mask,
    Blend,
}

#[derive(Clone, Debug)]
pub struct Material {
    pub base_color: [f32; 4],
    pub emissive: [f32; 3],
    pub metallic: f32,
    pub roughness: f32,
    pub normal_scale: f32,
    pub occlusion_strength: f32,
    pub alpha_cutoff: f32,
    pub alpha_mode: AlphaMode,
    pub unlit: bool,
    pub double_sided: bool,
    /// baseColor / normal / metallicRoughness / occlusion / emissive.
    pub textures: [Option<TextureId>; 5],
}
impl Default for Material {
    fn default() -> Self {
        Self {
            base_color: [1.; 4],
            emissive: [0.; 3],
            metallic: 0.,
            roughness: 0.6,
            normal_scale: 1.,
            occlusion_strength: 1.,
            alpha_cutoff: 0.5,
            alpha_mode: AlphaMode::Opaque,
            unlit: false,
            double_sided: false,
            textures: [None; 5],
        }
    }
}

#[derive(Clone, Debug)]
pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub rgba8: Arc<Vec<u8>>,
    /// glTF sampler constants: 10497 repeat, 33071 clamp, 33648 mirror.
    pub wrap_s: u32,
    pub wrap_t: u32,
}

#[derive(Clone, Debug)]
pub struct Mesh {
    pub id: u64,
    pub vertices: Arc<Vec<Vertex>>,
    pub indices: Arc<Vec<u32>>,
    pub material: usize,
}

#[derive(Clone, Debug)]
pub struct Instance {
    pub id: u64,
    pub mesh: usize,
    pub transform: Mat4,
    pub previous_transform: Mat4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightKind {
    Point,
    Spot,
    Rect,
}
#[derive(Clone, Debug)]
pub struct Light {
    pub id: u64,
    pub kind: LightKind,
    pub position: [f32; 3],
    pub direction: [f32; 3],
    pub color: [f32; 3],
    pub intensity: f32,
    pub range: f32,
    pub radius: f32,
    /// Cosines of cone half angles; ignored for Point/Rect.
    pub inner_cos: f32,
    pub outer_cos: f32,
    pub rect_size: [f32; 2],
    pub cast_shadow: bool,
}
impl Default for Light {
    fn default() -> Self {
        Self {
            id: 0,
            kind: LightKind::Point,
            position: [0., 2., 0.],
            direction: [0., -1., 0.],
            color: [1.; 3],
            intensity: 100.,
            range: 10.,
            radius: 0.05,
            inner_cos: 0.9,
            outer_cos: 0.8,
            rect_size: [0.5, 0.5],
            cast_shadow: true,
        }
    }
}
#[derive(Clone, Debug)]
pub struct Environment {
    pub ambient_color: [f32; 3],
    pub ambient_energy: f32,
    pub exposure: f32,
    pub bloom_strength: f32,
    pub bloom_threshold: f32,
    pub fog_color: [f32; 3],
    pub fog_density: f32,
    pub tonemap: Tonemap,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tonemap {
    Linear,
    Aces,
}
impl Default for Environment {
    fn default() -> Self {
        Self {
            ambient_color: [0.45, 0.55, 0.75],
            ambient_energy: 0.015,
            exposure: 1.,
            bloom_strength: 0.12,
            bloom_threshold: 1.,
            fog_color: [0.08, 0.12, 0.18],
            fog_density: 0.004,
            tonemap: Tonemap::Aces,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FogShape {
    Box,
    Ellipsoid,
}
#[derive(Clone, Debug)]
pub struct FogVolume {
    pub id: u64,
    pub transform: Mat4,
    pub size: [f32; 3],
    pub shape: FogShape,
    pub density: f32,
    pub albedo: [f32; 3],
    pub emission: [f32; 3],
    pub anisotropy: f32,
    /// Godot FogMaterial edge_fade exponent (>= 0): 0 keeps a hard boundary.
    pub edge_fade: f32,
}
#[derive(Clone, Debug)]
pub struct Decal {
    pub id: u64,
    pub transform: Mat4,
    pub size: [f32; 3],
    pub material: usize,
    pub opacity: f32,
}

#[derive(Clone, Debug, Default)]
pub struct SceneSnapshot {
    pub revision: u64,
    pub meshes: Vec<Mesh>,
    pub materials: Vec<Material>,
    pub textures: Vec<Texture>,
    pub instances: Vec<Instance>,
    pub lights: Vec<Light>,
    pub environment: Environment,
    pub fog_volumes: Vec<FogVolume>,
    pub decals: Vec<Decal>,
}
#[derive(Clone, Debug)]
pub struct TransformUpdate {
    pub instance_id: u64,
    pub transform: Mat4,
}
#[derive(Clone, Debug, Default)]
pub struct SceneDelta {
    pub revision: u64,
    pub transforms: Vec<TransformUpdate>,
    pub lights: Option<Vec<Light>>,
    pub materials: Option<Vec<Material>>,
    pub environment: Option<Environment>,
    pub fog_volumes: Option<Vec<FogVolume>>,
    pub decals: Option<Vec<Decal>>,
}

#[derive(Clone, Debug)]
pub struct RendererConfig {
    pub max_lights: usize,
    pub max_texture_bytes: u64,
    pub max_geometry_bytes: u64,
    pub gi_enabled: bool,
    pub reflections_enabled: bool,
    pub shadows_enabled: bool,
    /// Explicit overdraw optimization; enable only after a target-scene A/B.
    pub depth_prepass: bool,
    /// Conservative 8x8 world-bounds light lists; false is the exhaustive oracle.
    pub tile_lights: bool,
}
impl Default for RendererConfig {
    fn default() -> Self {
        Self {
            max_lights: 64,
            max_texture_bytes: 768 * 1024 * 1024,
            max_geometry_bytes: 256 * 1024 * 1024,
            gi_enabled: true,
            reflections_enabled: true,
            shadows_enabled: true,
            depth_prepass: false,
            tile_lights: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FrameInput {
    pub view_projection: Mat4,
    pub previous_view_projection: Mat4,
    pub eye: [f32; 3],
    pub output_extent: [u32; 2],
    pub render_scale: f32,
    pub delta_seconds: f32,
    pub reset_history: bool,
    pub readback: bool,
    pub output_slot: Option<usize>,
    /// Explicit headless benchmark intent, never inferred from missing output.
    pub allow_offscreen: bool,
}
impl Default for FrameInput {
    fn default() -> Self {
        Self {
            view_projection: IDENTITY,
            previous_view_projection: IDENTITY,
            eye: [0., 0., 3.],
            output_extent: [1280, 720],
            render_scale: 1.,
            delta_seconds: 1. / 60.,
            reset_history: true,
            readback: true,
            output_slot: None,
            allow_offscreen: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExternalOutput {
    pub handle: usize,
    pub size: u64,
    pub width: u32,
    pub height: u32,
    pub row_pitch: u32,
    pub adapter_luid: [u8; 8],
}
#[derive(Clone, Copy, Debug)]
pub struct ExternalFrame {
    pub slot: usize,
    pub generation: u64,
}
#[derive(Clone, Debug)]
pub struct FrameOutput {
    pub rgba8: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub internal_extent: [u32; 2],
    pub device_name: String,
    /// Sum of measured per-pass intervals, excluding pre-pass AS/update work,
    /// CPU work and native presentation. This is not whole-frame latency/FPS.
    pub gpu_ms: f64,
    pub passes: Vec<(String, f64)>,
    pub triangle_count: usize,
    pub history_reset: bool,
    pub external_output: Option<ExternalFrame>,
    pub validation_errors: u64,
    pub allocated_bytes: u64,
    /// Actual driver heap samples from the owning session; not a continuous peak.
    pub heaps: Vec<rurix_rt::render_exec::HeapBudgetTelemetry>,
    pub cpu_record_ms: f64,
    pub cpu_submit_ms: f64,
    pub cpu_wait_ms: f64,
    #[cfg(test)]
    pub(crate) debug_readbacks: Vec<Vec<u8>>,
}
