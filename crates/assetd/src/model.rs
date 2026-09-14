//! Lossless standard model assets for Blender/glTF. The legacy RXGB path is independent.
//! A model package is validated in staging and published as one directory transaction.
//! Its JSON bundle embeds decoded textures, so one revision is a coherent render snapshot.

use crate::project::ForgeProject;
use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

mod importer;
mod publication;
pub(crate) mod validation;
pub use publication::{import_model_bundle, load_model, load_model_revision, model_asset_path};
pub use validation::{validate_asset_document, validate_template_document};

fn version_one() -> u32 {
    1
}
fn revision_one() -> u64 {
    1
}
fn prop_kind() -> String {
    "prop".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelManifest {
    #[serde(default = "version_one")]
    pub version: u32,
    pub source_id: String,
    pub name: String,
    #[serde(default = "prop_kind")]
    pub kind: String,
    #[serde(default = "revision_one")]
    pub revision: u64,
    #[serde(default)]
    pub source_blend: Option<String>,
    #[serde(default)]
    pub object_ids: BTreeMap<String, String>,
    #[serde(default)]
    pub idle_clip: Option<String>,
    #[serde(default)]
    pub walk_clip: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelBundle {
    pub version: u32,
    pub guid: String,
    pub revision: u64,
    pub name: String,
    pub source_id: String,
    pub source_hash: String,
    pub kind: String,
    pub roots: Vec<usize>,
    pub primitives: Vec<ModelPrimitive>,
    pub nodes: Vec<ModelNode>,
    pub materials: Vec<ModelMaterial>,
    pub textures: Vec<ModelTexture>,
    pub skins: Vec<ModelSkin>,
    pub animations: Vec<ModelAnimation>,
    #[serde(default)]
    pub idle_clip: String,
    #[serde(default)]
    pub walk_clip: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPrimitive {
    pub id: String,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub tangents: Vec<[f32; 4]>,
    pub uv0: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<[f32; 4]>,
    pub material: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelNode {
    pub id: String,
    pub name: String,
    pub children: Vec<usize>,
    pub primitives: Vec<usize>,
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
    pub matrix: Option<[f32; 16]>,
    pub skin: Option<usize>,
    #[serde(default)]
    pub collision: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelMaterial {
    pub guid: String,
    pub name: String,
    pub base_color: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    pub emissive: [f32; 3],
    pub base_color_texture: Option<usize>,
    pub normal_texture: Option<usize>,
    pub metallic_roughness_texture: Option<usize>,
    pub occlusion_texture: Option<usize>,
    pub emissive_texture: Option<usize>,
    pub normal_scale: f32,
    pub occlusion_strength: f32,
    pub double_sided: bool,
    pub alpha_mode: String,
    pub alpha_cutoff: f32,
    pub unlit: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelTexture {
    pub id: String,
    pub guid: String,
    pub asset_path: String,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// glTF sampler enums, preserved for rendering (10497=REPEAT).
    pub wrap_s: u32,
    pub wrap_t: u32,
    pub mag_filter: Option<u32>,
    pub min_filter: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSkin {
    pub name: String,
    pub joints: Vec<usize>,
    pub inverse_bind_matrices: Vec<[f32; 16]>,
    pub skeleton: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelAnimation {
    pub name: String,
    pub duration: f32,
    pub channels: Vec<ModelAnimationChannel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelAnimationChannel {
    pub node: usize,
    /// translation | rotation | scale
    pub path: String,
    pub times: Vec<f32>,
    /// Vec3 channels use xyz and w=0; cubic uses glTF in/value/out triplets.
    pub values: Vec<[f32; 4]>,
    pub interpolation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishedModel {
    pub guid: String,
    pub revision: u64,
    pub asset_path: String,
    pub prefab_path: String,
    pub prefab_guid: String,
    pub asset_guids: Vec<String>,
    pub changed: bool,
}

pub fn load_bundle(project: &ForgeProject, model_ref: &str) -> Result<ModelBundle> {
    load_model(project, model_ref)
}

/// Pure validation/decoding; does not publish anything. GLB and confined glTF dependencies.
pub fn inspect_model_source(path: &Path, manifest: &ModelManifest) -> Result<ModelBundle> {
    importer::decode(path, manifest)
}

pub fn validate_model_bundle(bundle: &ModelBundle) -> Result<()> {
    importer::validate_bundle(bundle)
}

/// Stable package location derived from a Blender source identity, independent of display name.
pub fn package_path(source_id: &str) -> String {
    format!("Models/{}", stable_guid(source_id, "model"))
}

pub(crate) fn stable_guid(source: &str, child: &str) -> String {
    let hash =
        forge_util::hashutil::sha256_hex(format!("rurix-model-v1\0{source}\0{child}").as_bytes());
    let mut bytes = [0u8; 16];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hash[2 * i..2 * i + 2], 16).unwrap();
    }
    bytes[6] = (bytes[6] & 15) | 0x50;
    bytes[8] = (bytes[8] & 63) | 0x80;
    uuid::Uuid::from_bytes(bytes).to_string()
}

/// Template-local identity is independent of glTF node array order and exactly representable in JS.
pub fn source_entity_id(node_id: &str) -> u64 {
    let hash =
        forge_util::hashutil::sha256_hex(format!("rurix-template-node-v1\0{node_id}").as_bytes());
    u64::from_str_radix(&hash[..12], 16).unwrap() + 2
}

pub(crate) fn model_error(message: impl Into<String>) -> crate::AssetError {
    crate::AssetError::new("MODEL_INVALID", message)
}

pub(crate) fn identity_matrix() -> [f32; 16] {
    [
        1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
    ]
}

pub(crate) fn absolute_model_path(project: &ForgeProject, guid: &str) -> PathBuf {
    project
        .content_root()
        .join("Models")
        .join(guid)
        .join("model.rxmodel")
}
