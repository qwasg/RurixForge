//! Graph materials are owned and pruned when neither main nor preview RenderList refers to them.
//! Source acceptance and completed rendering are distinct; errors from Godot's real compiler are
//! captured by Logger, with a unique hash/path and uniform sentinel for each candidate.
use crate::{material::image_texture, rid::Owned};
use engine_host::shader::{Material, Texture};
use godot::{
    classes::{ILogger, ImageTexture, Logger, Os, RenderingServer, ScriptBacktrace},
    prelude::*,
};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, OnceLock, Weak},
};

#[derive(Default)]
struct ErrorLog {
    sequence: u64,
    entries: VecDeque<(u64, String)>,
}
impl ErrorLog {
    fn push(&mut self, message: String) {
        self.sequence += 1;
        self.entries.push_back((self.sequence, message));
        if self.entries.len() > 128 {
            self.entries.pop_front();
        }
    }
    fn since(&self, offset: u64) -> Option<String> {
        (self.sequence > offset).then(|| {
            self.entries
                .iter()
                .filter(|(id, _)| *id > offset)
                .map(|(_, message)| message.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
    }
}
static ERRORS: OnceLock<Mutex<ErrorLog>> = OnceLock::new();
fn errors() -> &'static Mutex<ErrorLog> {
    ERRORS.get_or_init(|| Mutex::new(ErrorLog::default()))
}
#[derive(GodotClass)]
#[class(base=Logger)]
struct ShaderLogger {
    base: Base<Logger>,
}
#[godot_api]
impl ILogger for ShaderLogger {
    fn init(base: Base<Logger>) -> Self {
        Self { base }
    }
    fn log_error(
        &mut self,
        function: GString,
        file: GString,
        line: i32,
        code: GString,
        rationale: GString,
        _editor_notify: bool,
        _error_type: i32,
        _script_backtraces: Array<Gd<ScriptBacktrace>>,
    ) {
        let msg = format!("{file}:{line} {function}: {code} {rationale}");
        let lower = msg.to_lowercase();
        if lower.contains("shader") || lower.contains("pipeline") {
            let mut errors = errors().lock().unwrap();
            errors.push(msg);
        }
    }
    fn log_message(&mut self, _message: GString, _error: bool) {}
}
struct Entry {
    material: Owned,
    _shader: Arc<Owned>,
    _textures: Vec<Gd<ImageTexture>>,
    owner: Weak<Material>,
    hash: String,
    pending: bool,
    error_offset: u64,
}
pub struct GraphMaterials {
    entries: HashMap<String, Entry>,
    programs: HashMap<String, Weak<Owned>>,
    logger: Gd<ShaderLogger>,
}
impl GraphMaterials {
    pub fn new() -> Self {
        let logger = ShaderLogger::new_gd();
        Os::singleton().add_logger(&logger);
        Self {
            entries: HashMap::new(),
            programs: HashMap::new(),
            logger,
        }
    }
    pub fn clear(&mut self) {
        self.entries.clear();
        self.programs.clear();
    }
    pub fn tick(&mut self, rs: &mut Gd<RenderingServer>) {
        self.entries.retain(|_, e| e.owner.strong_count() > 0);
        self.programs.retain(|_, shader| shader.strong_count() > 0);
        for entry in self.entries.values() {
            rs.material_set_param(
                entry.material.rid(),
                "forge_time",
                &entry.owner.upgrade().map_or(0., |g| g.time()).to_variant(),
            );
        }
    }
    pub fn post_draw(&mut self) {
        let errors = errors().lock().unwrap();
        for entry in self.entries.values_mut().filter(|e| e.pending) {
            let result = errors.since(entry.error_offset).map_or(Ok(()), Err);
            engine_host::shader::report_backend(&entry.hash, "godot", result);
            entry.pending = false;
        }
    }
    pub fn material(
        &mut self,
        rs: &mut Gd<RenderingServer>,
        graph: &Arc<Material>,
        source: &Texture,
        canvas: bool,
        tint: [f32; 4],
        uv: [f32; 4],
        flip: [bool; 2],
    ) -> Option<Rid> {
        let key = format!(
            "{}:{canvas}:{}:{}:{:?}:{tint:?}:{uv:?}:{flip:?}",
            graph.key,
            source.width,
            source.height,
            source
                .rgba
                .iter()
                .fold(14695981039346656037u64, |h, b| (h ^ u64::from(*b))
                    .wrapping_mul(1099511628211))
        );
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.owner = Arc::downgrade(graph);
            return Some(entry.material.rid());
        }
        if self.entries.len() >= 512 {
            engine_host::shader::report_backend(
                &graph.program.compiled.hash,
                "godot",
                Err("SHADER_BUDGET: 512 live GPU material instances".into()),
            );
            return None;
        }
        let offset = errors().lock().unwrap().sequence;
        let program_key = format!("{}:{canvas}", graph.program.compiled.program_hash);
        let shader = if let Some(shader) = self.programs.get(&program_key).and_then(Weak::upgrade) {
            shader
        } else {
            let code = if canvas {
                &graph.program.compiled.godot.canvas
            } else {
                &graph.program.compiled.godot.spatial
            };
            let shader = Arc::new(Owned::new(rs.shader_create()));
            rs.shader_set_path_hint(
                shader.rid(),
                &format!(
                    "forge_shader_{}.gdshader",
                    graph.program.compiled.program_hash
                ),
            );
            rs.shader_set_code(shader.rid(), code);
            engine_host::shader::record_program_build("godot");
            let sentinel = format!(
                "forge_compiled_{}",
                &graph.program.compiled.program_hash[..16]
            );
            let found = rs
                .get_shader_parameter_list(shader.rid())
                .iter_shared()
                .any(|p| {
                    p.get("name")
                        .is_some_and(|v| v.stringify().to_string() == sentinel)
                });
            if !found {
                let messages = errors().lock().unwrap();
                let msg = messages.since(offset).unwrap_or_else(|| {
                    "Godot rejected source: compiled parameter sentinel absent".into()
                });
                engine_host::shader::report_backend(
                    &graph.program.compiled.hash,
                    "godot",
                    Err(msg),
                );
                return None;
            }
            self.programs.insert(program_key, Arc::downgrade(&shader));
            shader
        };
        let material = Owned::new(rs.material_create());
        rs.material_set_shader(material.rid(), shader.rid());
        let mut textures = Vec::new();
        for (i, texture) in std::iter::once(source).chain(&graph.textures).enumerate() {
            let Some(image) = image_texture(texture.width, texture.height, &texture.rgba) else {
                engine_host::shader::report_backend(
                    &graph.program.compiled.hash,
                    "godot",
                    Err("Texture upload failed".into()),
                );
                return None;
            };
            rs.material_set_param(
                material.rid(),
                &format!("forge_tex_{i}"),
                &image.get_rid().to_variant(),
            );
            textures.push(image);
        }
        for (i, p) in graph.params.iter().enumerate() {
            rs.material_set_param(
                material.rid(),
                &format!("forge_param_{i}"),
                &Vector4::new(p[0], p[1], p[2], p[3]).to_variant(),
            );
        }
        rs.material_set_param(material.rid(), "forge_time", &graph.time().to_variant());
        rs.material_set_param(
            material.rid(),
            "forge_tint",
            &Vector4::new(tint[0], tint[1], tint[2], tint[3]).to_variant(),
        );
        rs.material_set_param(
            material.rid(),
            "forge_uv_rect",
            &Vector4::new(uv[0], uv[1], uv[2], uv[3]).to_variant(),
        );
        rs.material_set_param(
            material.rid(),
            "forge_flip",
            &Vector2::new(u8::from(flip[0]) as f32, u8::from(flip[1]) as f32).to_variant(),
        );
        let rid = material.rid();
        self.entries.insert(
            key,
            Entry {
                material,
                _shader: shader,
                _textures: textures,
                owner: Arc::downgrade(graph),
                hash: graph.program.compiled.hash.clone(),
                pending: true,
                error_offset: offset,
            },
        );
        Some(rid)
    }
}
impl Drop for GraphMaterials {
    fn drop(&mut self) {
        Os::singleton().remove_logger(&self.logger);
    }
}

pub fn mesh(rs: &mut Gd<RenderingServer>, vertices: &[f32]) -> Owned {
    let mut pos = Vec::new();
    let mut normals = Vec::new();
    let mut uv = Vec::new();
    let mut tangents = Vec::new();
    for tri in vertices.chunks_exact(36) {
        for index in [0, 2, 1] {
            let v = &tri[index * 12..index * 12 + 12];
            pos.push(Vector3::new(v[0], v[1], v[2]));
            normals.push(Vector3::new(v[3], v[4], v[5]));
            uv.push(Vector2::new(v[6], v[7]));
            tangents.extend_from_slice(&v[8..12]);
        }
    }
    let mut arrays = VarArray::new();
    for slot in 0..13 {
        arrays.push(&match slot {
            0 => PackedVector3Array::from(pos.as_slice()).to_variant(),
            1 => PackedVector3Array::from(normals.as_slice()).to_variant(),
            2 => PackedFloat32Array::from(tangents.as_slice()).to_variant(),
            4 => PackedVector2Array::from(uv.as_slice()).to_variant(),
            _ => Variant::nil(),
        });
    }
    let mesh = Owned::new(rs.mesh_create());
    rs.mesh_add_surface_from_arrays(
        mesh.rid(),
        godot::classes::rendering_server::PrimitiveType::TRIANGLES,
        &arrays,
    );
    mesh
}
