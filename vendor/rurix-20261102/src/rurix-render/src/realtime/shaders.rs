//! Shader compilation is cached once; all GPU work is submitted by rurix-rt.
//! RXS-0409. WGSL is translated to SPIR-V by naga, not executed by wgpu.
use std::sync::OnceLock;

pub(crate) struct Programs {
    pub vs: Vec<u8>,
    pub glass_vs: Vec<u8>,
    pub depth_vs: Vec<u8>,
    pub depth_fs: Vec<u8>,
    pub gbuffer: Vec<u8>,
    pub decal: Vec<u8>,
    pub fog: Vec<u8>,
    pub direct: Vec<u8>,
    pub tile_lights: Vec<u8>,
    pub indirect: Vec<u8>,
    pub signal_temporal: Vec<u8>,
    pub signal_spatial: Vec<u8>,
    pub combine: Vec<u8>,
    pub temporal: Vec<u8>,
    pub glass: Vec<u8>,
    pub bloom: Vec<u8>,
    pub post: Vec<u8>,
    pub pack: Vec<u8>,
}
pub(crate) fn compile(source: &str, label: &str) -> Result<Vec<u8>, String> {
    let module = naga::front::wgsl::parse_str(source)
        .map_err(|e| format!("REALTIME_SHADER {label}: {}", e.emit_to_string(source)))?;
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .map_err(|e| format!("REALTIME_SHADER {label}: {e:?}"))?;
    let mut options = naga::back::spv::Options::default();
    options.lang_version = (1, 4);
    let words = naga::back::spv::write_vec(&module, &info, &options, None)
        .map_err(|e| format!("REALTIME_SHADER {label}: {e}"))?;
    #[cfg(test)]
    let original_words = words.clone();
    let words =
        workgroup_array_layout(words).map_err(|e| format!("REALTIME_SHADER {label}: {e}"))?;
    #[cfg(test)]
    if original_words != words {
        if let Ok(directory) = std::env::var("RURIX_REALTIME_SHADER_AUDIT_DIR") {
            let path = std::path::PathBuf::from(directory);
            std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
            std::fs::write(
                path.join(format!("{label}.unfixed.spv")),
                original_words
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .map_err(|e| e.to_string())?;
            std::fs::write(
                path.join(format!("{label}.validated-layout.spv")),
                words
                    .iter()
                    .flat_map(|w| w.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .map_err(|e| e.to_string())?;
            std::fs::write(path.join(format!("{label}.wgsl")), source)
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(words.into_iter().flat_map(u32::to_le_bytes).collect())
}

/// Naga 25 decorates even workgroup-only arrays with ArrayStride. Vulkan's
/// ordinary Workgroup storage does not admit that explicit interface layout.
/// Remove only those array decorations whose complete type-use graph proves
/// no non-Workgroup variable shares the type. External buffer/UBO/IO layouts
/// are preserved; an ambiguous shared type is rejected, never rewritten.
fn workgroup_array_layout(words: Vec<u32>) -> Result<Vec<u32>, String> {
    use std::collections::{BTreeMap, BTreeSet};
    let mut children = BTreeMap::<u32, Vec<u32>>::new();
    let mut pointers = BTreeMap::<u32, u32>::new();
    let mut variables = Vec::new();
    let mut decorated = BTreeSet::new();
    let mut instructions = Vec::new();
    let mut i = 5;
    while i < words.len() {
        let count = (words[i] >> 16) as usize;
        if count == 0 || i + count > words.len() {
            return Err("malformed generated SPIR-V instruction".into());
        }
        let op = words[i] & 0xffff;
        let w = &words[i..i + count];
        match op {
            23 | 24 | 28 | 29 => {
                children.insert(w[1], vec![w[2]]);
            }
            30 => {
                children.insert(w[1], w[2..].to_vec());
            }
            32 => {
                pointers.insert(w[1], w[3]);
                children.insert(w[1], vec![w[3]]);
            }
            59 => variables.push((w[1], w[3])),
            71 if count >= 4 && w[2] == 6 => {
                decorated.insert(w[1]);
            }
            _ => {}
        }
        instructions.push((i, count));
        i += count;
    }
    let visit = |roots: Vec<u32>| {
        let mut reached = BTreeSet::new();
        let mut stack = roots;
        while let Some(id) = stack.pop() {
            if reached.insert(id) {
                if let Some(c) = children.get(&id) {
                    stack.extend(c);
                }
            }
        }
        reached
    };
    let workgroup = visit(
        variables
            .iter()
            .filter(|(_, class)| *class == 4)
            .filter_map(|(pointer, _)| pointers.get(pointer).copied())
            .collect(),
    );
    let other = visit(
        variables
            .iter()
            .filter(|(_, class)| *class != 4)
            .filter_map(|(pointer, _)| pointers.get(pointer).copied())
            .collect(),
    );
    let affected = decorated
        .intersection(&workgroup)
        .copied()
        .collect::<BTreeSet<_>>();
    if affected.iter().any(|id| other.contains(id)) {
        return Err("workgroup array layout aliases a non-Workgroup interface; no safe layout rewrite exists".into());
    }
    if affected.is_empty() {
        return Ok(words);
    }
    let mut output = words[..5].to_vec();
    for (i, count) in instructions {
        let w = &words[i..i + count];
        if w[0] & 0xffff == 71 && count >= 4 && w[2] == 6 && affected.contains(&w[1]) {
            continue;
        }
        output.extend_from_slice(w);
    }
    Ok(output)
}
pub(crate) fn programs() -> Result<&'static Programs, String> {
    static P: OnceLock<Result<Programs, String>> = OnceLock::new();
    P.get_or_init(||{Ok(Programs{
        vs:compile(&format!("{TYPES}\n{RASTER_BINDINGS}\n{COMMON}\n{}",include_str!("shaders/vertex.wgsl").replace("let inst=instances[ii];let v=vertex_at(vi);", "let reference=draw_references[vi];let ii=reference.y;let inst=instances[ii];let v=vertex_at(reference.x);").replace("@builtin(instance_index) ii:u32","@builtin(instance_index) unused_instance:u32")),"vertex_batched")?,
        glass_vs:compile(&format!("{TYPES}\n{RASTER_BINDINGS}\n{COMMON}\n{}",include_str!("shaders/vertex.wgsl")),"vertex_forward")?,
        depth_vs:compile(&format!("{TYPES}\n{RASTER_BINDINGS}\n{COMMON}\n{}",include_str!("shaders/depth_vertex.wgsl")),"depth_vertex")?,
        depth_fs:compile(&format!("{TYPES}\n{RASTER_BINDINGS}\n{COMMON}\n{}",include_str!("shaders/depth_fragment.wgsl")),"depth_fragment")?,
        gbuffer:compile(&format!("{TYPES}\n{RASTER_BINDINGS}\n{COMMON}\n{}",include_str!("shaders/gbuffer.wgsl")),"gbuffer")?,
        decal:compile(&format!("{TYPES}\n{DECAL_BINDINGS}\n{COMMON}\n{}",include_str!("shaders/decal.wgsl")),"decal")?,
        fog:compile(&format!("{TYPES}\n{FOG_BINDINGS}\n{COMMON}\n{RAYS}\n{}\n{}",include_str!("shaders/fog_density.wgsl"),include_str!("shaders/fog.wgsl")),"fog")?,
        direct:compile(&format!("{TYPES}\n{DIRECT_BINDINGS}\n{COMMON}\n{RAYS}\n{}",include_str!("shaders/direct.wgsl")),"direct")?,
        tile_lights:compile(&format!("{TYPES}\nconst DROP_FIRST_LIGHT:bool={};\n{}",cfg!(test)&&std::env::var("RURIX_REALTIME_TEST_DROP_TILE_LIGHT").as_deref()==Ok("1"),include_str!("shaders/tile_lights.wgsl")),"tile_lights")?,
        indirect:compile(&format!("{TYPES}\n{LIGHT_BINDINGS}\n{COMMON}\n{RAYS}\n{}",include_str!("shaders/indirect.wgsl")),"indirect")?,
        signal_temporal:compile(&format!("{TYPES}\n{}",include_str!("shaders/signal_temporal.wgsl")),"signal_temporal")?,
        signal_spatial:compile(&format!("{TYPES}\n{}",include_str!("shaders/signal_spatial.wgsl")),"signal_spatial")?,
        combine:compile(&format!("{TYPES}\n{}",include_str!("shaders/combine.wgsl")),"combine")?,
        temporal:compile(&format!("{TYPES}\n{}",include_str!("shaders/temporal.wgsl")),"temporal")?,
        glass:compile(&format!("{TYPES}\n{RASTER_BINDINGS}\n{COMMON}\n{}",include_str!("shaders/glass.wgsl")),"glass")?,
        bloom:compile(&format!("{TYPES}\n{}",include_str!("shaders/bloom.wgsl")),"bloom")?,
        post:compile(&format!("{TYPES}\n{}",include_str!("shaders/post.wgsl")),"post")?,
        pack:compile(include_str!("shaders/pack.wgsl"),"pack")?,
    })}).as_ref().map_err(Clone::clone)
}
const TYPES: &str = include_str!("shaders/types.wgsl");
const COMMON: &str = include_str!("shaders/material.wgsl");
const RAYS: &str = include_str!("shaders/rays.wgsl");
#[cfg(test)]
pub(crate) fn vndf_probe_shader(ndf_red: bool) -> Result<Vec<u8>, String> {
    compile(
        &format!(
            "{TYPES}\nconst NDF_RED:bool={ndf_red};\n{}",
            include_str!("shaders/vndf_probe.wgsl")
        ),
        "vndf_gpu_oracle",
    )
}
const RASTER_BINDINGS: &str = r#"
@group(0) @binding(0) var<storage,read> vertices:array<vec4<f32>>;
@group(0) @binding(1) var<storage,read> instances:array<Instance>;
@group(0) @binding(2) var<storage,read> mats:array<Material>;
@group(0) @binding(3) var<storage,read> tex:array<u32>;
@group(0) @binding(4) var<storage,read> lights:array<Light>;
@group(0) @binding(5) var<storage,read> draw_references:array<vec2<u32>>;
@group(0) @binding(6) var<uniform> frame:Frame;
"#;
const LIGHT_BINDINGS: &str = r#"
@group(0) @binding(0) var tlas:acceleration_structure;
@group(0) @binding(1) var<storage,read> vertices:array<vec4<f32>>;
@group(0) @binding(2) var<storage,read> instances:array<Instance>;
@group(0) @binding(3) var<storage,read> mats:array<Material>;
@group(0) @binding(4) var<storage,read> tex:array<u32>;
@group(0) @binding(5) var<storage,read> lights:array<Light>;
@group(0) @binding(6) var gpos:texture_storage_2d<rgba32float,read>;
@group(0) @binding(7) var gnrm:texture_storage_2d<rgba16float,read>;
@group(0) @binding(8) var gbase:texture_storage_2d<rgba16float,read>;
@group(0) @binding(9) var gemit:texture_storage_2d<rgba16float,read>;
@group(0) @binding(10) var output0:texture_storage_2d<rgba16float,write>;
@group(0) @binding(11) var output1:texture_storage_2d<rgba16float,write>;
@group(0) @binding(12) var<uniform> frame:Frame;
"#;
const DIRECT_BINDINGS: &str = r#"
@group(0) @binding(0) var tlas:acceleration_structure;
@group(0) @binding(1) var<storage,read> vertices:array<vec4<f32>>;
@group(0) @binding(2) var<storage,read> instances:array<Instance>;
@group(0) @binding(3) var<storage,read> mats:array<Material>;
@group(0) @binding(4) var<storage,read> tex:array<u32>;
@group(0) @binding(5) var<storage,read> lights:array<Light>;
@group(0) @binding(6) var<storage,read> tile_masks:array<u32>;
@group(0) @binding(7) var gpos:texture_storage_2d<rgba32float,read>;
@group(0) @binding(8) var gnrm:texture_storage_2d<rgba16float,read>;
@group(0) @binding(9) var gbase:texture_storage_2d<rgba16float,read>;
@group(0) @binding(10) var gemit:texture_storage_2d<rgba16float,read>;
@group(0) @binding(11) var output0:texture_storage_2d<rgba16float,write>;
@group(0) @binding(12) var output1:texture_storage_2d<rgba16float,write>;
@group(0) @binding(13) var<uniform> frame:Frame;
"#;
const DECAL_BINDINGS: &str = r#"
@group(0) @binding(0) var<storage,read> vertices:array<vec4<f32>>;
@group(0) @binding(1) var<storage,read> instances:array<Instance>;
@group(0) @binding(2) var<storage,read> mats:array<Material>;
@group(0) @binding(3) var<storage,read> tex:array<u32>;
struct Decal {model:mat4x4<f32>,inverse_model:mat4x4<f32>,size_opacity:vec4<f32>,info:vec4<u32>};
@group(0) @binding(4) var<storage,read> decals:array<Decal>;
@group(0) @binding(5) var gpos:texture_storage_2d<rgba32float,read>;
@group(0) @binding(6) var gnrm:texture_storage_2d<rgba16float,read_write>;
@group(0) @binding(7) var gbase:texture_storage_2d<rgba16float,read_write>;
@group(0) @binding(8) var gemit:texture_storage_2d<rgba16float,read_write>;
@group(0) @binding(9) var<uniform> frame:Frame;
"#;
const FOG_BINDINGS: &str = r#"
@group(0) @binding(0) var tlas:acceleration_structure;
@group(0) @binding(1) var<storage,read> vertices:array<vec4<f32>>;
@group(0) @binding(2) var<storage,read> instances:array<Instance>;
@group(0) @binding(3) var<storage,read> mats:array<Material>;
@group(0) @binding(4) var<storage,read> tex:array<u32>;
@group(0) @binding(5) var<storage,read> lights:array<Light>;
struct FogVolume {inverse_model:mat4x4<f32>,size_density:vec4<f32>,albedo_g:vec4<f32>,emission_shape:vec4<f32>,edge:vec4<f32>};
@group(0) @binding(6) var<storage,read> volumes:array<FogVolume>;
@group(0) @binding(7) var gpos:texture_storage_2d<rgba32float,read>;
@group(0) @binding(8) var volume_output:texture_storage_2d<rgba16float,write>;
@group(0) @binding(9) var<uniform> frame:Frame;
"#;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_gpu_shaders_compile() {
        programs().expect("all native pipeline shaders must compile and validate");
    }
    //@ spec: RXS-0409
    #[test]
    fn workgroup_layout_transform_preserves_interface_and_rejects_alias() {
        let mut words = vec![0x07230203, 0x00010400, 0, 32, 0];
        // Workgroup array3, distinct storage array8. The transform is tested
        // independently of Naga's type arena and must preserve array8's stride.
        words.extend([
            4 << 16 | 28,
            3,
            1,
            2,
            4 << 16 | 71,
            3,
            6,
            16,
            4 << 16 | 32,
            4,
            4,
            3,
            4 << 16 | 59,
            4,
            6,
            4,
        ]);
        words.extend([
            4 << 16 | 28,
            8,
            1,
            2,
            4 << 16 | 71,
            8,
            6,
            16,
            4 << 16 | 32,
            9,
            12,
            8,
            4 << 16 | 59,
            9,
            10,
            12,
        ]);
        let transformed = workgroup_array_layout(words.clone()).unwrap();
        assert!(!transformed
            .windows(4)
            .any(|w| w == [4 << 16 | 71, 3, 6, 16]));
        assert!(transformed
            .windows(4)
            .any(|w| w == [4 << 16 | 71, 8, 6, 16]));
        words.extend([4 << 16 | 32, 11, 12, 3, 4 << 16 | 59, 11, 12, 12]);
        assert!(workgroup_array_layout(words)
            .unwrap_err()
            .contains("aliases a non-Workgroup"));
    }
    // Regression for the independently selected SPIR-V ray-query intersection
    // semantics, not the writer's internal cache representation.
    //@ spec: RXS-0409
    #[test]
    fn candidate_and_committed_selectors_remain_distinct() {
        for committed_first in [false, true] {
            let before = if committed_first {
                "let before=rayQueryGetCommittedIntersection(&q);result[1]=before.kind;"
            } else {
                ""
            };
            let source = format!(
                r#"@group(0) @binding(0) var accel:acceleration_structure;
@group(0) @binding(1) var<storage,read_write> result:array<u32>;
@compute @workgroup_size(1) fn main(){{var q:ray_query;rayQueryInitialize(&q,accel,RayDesc(2u,255u,0.001,10.,vec3<f32>(0.),vec3<f32>(0.,0.,1.)));{before}
while(rayQueryProceed(&q)){{let candidate=rayQueryGetCandidateIntersection(&q);if(candidate.kind==1u){{rayQueryConfirmIntersection(&q);}}}}
let committed=rayQueryGetCommittedIntersection(&q);result[0]=committed.kind;}}"#
            );
            let bytes = compile(&source, "query_selector_regression").unwrap();
            let words = bytes
                .chunks_exact(4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                .collect::<Vec<_>>();
            let mut constants = std::collections::BTreeMap::new();
            let mut selectors = Vec::new();
            let mut i = 5;
            while i < words.len() {
                let count = (words[i] >> 16) as usize;
                let opcode = words[i] & 0xffff;
                assert!(count > 0);
                if opcode == 43 && count == 4 {
                    constants.insert(words[i + 2], words[i + 3]);
                }
                if opcode == 4479 {
                    selectors.push(words[i + 4]);
                }
                i += count;
            }
            let values = selectors
                .into_iter()
                .map(|id| constants[&id])
                .collect::<std::collections::BTreeSet<_>>();
            assert_eq!(
                values,
                std::collections::BTreeSet::from([0, 1]),
                "both literal candidate=0 and committed=1 intersection selectors must exist regardless of first call"
            );
        }
    }
}
