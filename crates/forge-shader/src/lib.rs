//! A bounded, backend-neutral material DAG. It has no executable scripts or file IO.
//! Generated source is a derived artifact; stable graph/node/parameter IDs belong to the document.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

mod emit;
pub use emit::{compile_spirv, diagnostic_for_source};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Domain {
    Sprite2d,
    Pbr3d,
    Unlit3d,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ValueType {
    Float,
    Vec2,
    Vec3,
    Vec4,
    Color,
    Texture2d,
}
impl ValueType {
    pub fn lanes(self) -> usize {
        match self {
            Self::Float => 1,
            Self::Vec2 => 2,
            Self::Vec3 => 3,
            Self::Vec4 | Self::Color => 4,
            Self::Texture2d => 0,
        }
    }
    fn numeric(self) -> Self {
        if self == Self::Color {
            Self::Vec4
        } else {
            self
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Parameter {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub ty: ValueType,
    pub default: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ValueSource {
    Node {
        node: String,
        pin: String,
    },
    Constant {
        #[serde(rename = "const")]
        value: Value,
    },
    Parameter {
        param: String,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default)]
    pub pos: [f64; 2],
    #[serde(default)]
    pub inputs: BTreeMap<String, ValueSource>,
    #[serde(default)]
    pub options: BTreeMap<String, Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphDoc {
    pub version: u32,
    pub id: String,
    pub name: String,
    pub domain: Domain,
    #[serde(default)]
    pub parameters: Vec<Parameter>,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub outputs: BTreeMap<String, ValueSource>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub code: String,
    pub message: String,
    pub severity: String,
    pub backend: String,
    pub stage: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
}
impl Diagnostic {
    pub fn error(code: &str, message: impl Into<String>, node: Option<&str>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            severity: "error".into(),
            backend: "graph".into(),
            stage: "validate".into(),
            node_id: node.map(str::to_string),
            pin: None,
            line: None,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSpan {
    pub node_id: String,
    pub line: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sources {
    pub sprite: String,
    pub model: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GodotSources {
    pub canvas: String,
    pub spatial: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompiledGraph {
    pub hash: String,
    /// Executable source / parameter ABI identity, independent of document layout and defaults.
    pub program_hash: String,
    pub domain: Domain,
    pub parameters: Vec<Parameter>,
    /// Slot 0 is the renderer's source texture; graph texture parameters start at 1.
    pub texture_slots: Vec<String>,
    pub wgsl: Sources,
    pub godot: GodotSources,
    pub source_map: BTreeMap<String, Vec<SourceSpan>>,
    pub animated: bool,
}

pub fn content_hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn graph_hash(graph: &GraphDoc) -> String {
    content_hash(&serde_json::to_vec(graph).unwrap_or_default())
}
pub fn program_hash(graph: &GraphDoc) -> String {
    let mut executable = graph.clone();
    executable.id.clear();
    executable.name.clear();
    for node in &mut executable.nodes {
        node.pos = [0.; 2];
    }
    for parameter in &mut executable.parameters {
        parameter.name.clear();
        parameter.default = Value::Null;
    }
    // Keep node IDs, edges, literal values, domain, and ordered parameter types/IDs:
    // those determine generated source, source maps, or its storage ABI.
    graph_hash(&executable)
}
fn identifier(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 96
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
}

pub fn validate_parameter(ty: ValueType, value: &Value) -> bool {
    if ty == ValueType::Texture2d {
        return value.as_str().is_some();
    }
    let valid = |v: &Value| {
        v.as_f64()
            .is_some_and(|f| f.is_finite() && f.abs() <= f32::MAX as f64)
    };
    if ty == ValueType::Float {
        valid(value)
    } else {
        value
            .as_array()
            .is_some_and(|a| a.len() == ty.lanes() && a.iter().all(valid))
    }
}
pub fn compile(graph: &GraphDoc) -> Result<CompiledGraph, Vec<Diagnostic>> {
    let mut errors = Vec::new();
    if graph.version != 1 {
        errors.push(Diagnostic::error(
            "SHADER_VERSION",
            "Shader Graph version must be 1",
            None,
        ));
    }
    if !identifier(&graph.id) {
        errors.push(Diagnostic::error("SHADER_ID", "Invalid graph ID", None));
    }
    if graph.nodes.len() > 256 || graph.parameters.len() > 64 {
        errors.push(Diagnostic::error(
            "SHADER_BUDGET",
            "Maximum 256 nodes and 64 parameters",
            None,
        ));
    }
    let mut ids = BTreeSet::new();
    for p in &graph.parameters {
        if !identifier(&p.id) || !ids.insert(&p.id) || !validate_parameter(p.ty, &p.default) {
            errors.push(Diagnostic::error(
                "SHADER_PARAMETER",
                format!("Invalid or duplicate parameter {}", p.id),
                None,
            ));
        }
    }
    if graph
        .parameters
        .iter()
        .filter(|p| p.ty == ValueType::Texture2d)
        .count()
        > 15
    {
        errors.push(Diagnostic::error(
            "SHADER_BUDGET",
            "Maximum 15 texture parameters plus the source texture",
            None,
        ));
    }
    ids.clear();
    for n in &graph.nodes {
        if !identifier(&n.id) || !ids.insert(&n.id) || n.pos.iter().any(|v| !v.is_finite()) {
            errors.push(Diagnostic::error(
                "SHADER_NODE_ID",
                "Invalid or duplicate node ID",
                Some(&n.id),
            ));
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    emit::compile(graph).map_err(|e| vec![e])
}

pub fn catalog() -> Value {
    serde_json::json!({"version":1,"domains":["sprite2d","pbr3d","unlit3d"],"types":["float","vec2","vec3","vec4","color","texture2d"],
        "nodes":["constant","color","parameter","uv","uvTransform","time","texture","add","subtract","multiply","divide","mix","clamp","sin","cos","abs","fract","oneMinus","normalize","dot","split","combine","normalMap"],
        "limits":{"nodes":256,"parameters":64,"textures":15},"outputs":{"sprite2d":["color","alpha"],"pbr3d":["baseColor","alpha","metallic","roughness","emission","normal"],"unlit3d":["baseColor","alpha","emission"]}})
}
