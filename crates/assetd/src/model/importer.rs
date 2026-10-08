use super::*;
use base64::Engine;
use gltf::animation::util::ReadOutputs;
use gltf::mesh::Mode;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

const MAX_SOURCE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_VERTICES: usize = 2_000_000;

pub(super) fn decode(path: &Path, manifest: &ModelManifest) -> Result<ModelBundle> {
    if manifest.version != 1
        || manifest.source_id.trim().is_empty()
        || manifest.name.trim().is_empty()
    {
        return Err(model_error(
            "manifest version must be 1 and sourceId/name must be nonempty",
        ));
    }
    if !matches!(manifest.kind.as_str(), "map" | "character" | "prop") {
        return Err(model_error("manifest kind must be map, character or prop"));
    }
    let bytes = read_limited(path)?;
    // gltf utilities may assume validated buffer contents; malformed files must stay tool errors.
    std::panic::catch_unwind(|| decode_bytes(path, &bytes, manifest))
        .map_err(|_| model_error("malformed glTF accessor or buffer data"))?
}

fn read_limited(path: &Path) -> Result<Vec<u8>> {
    if std::fs::metadata(path)?.len() > MAX_SOURCE_BYTES {
        return Err(model_error("model/dependency exceeds 256 MiB limit"));
    }
    Ok(std::fs::read(path)?)
}

fn decode_uri(base: &Path, uri: &str) -> Result<Vec<u8>> {
    if uri.starts_with("data:") {
        let (header, data) = uri
            .split_once(',')
            .ok_or_else(|| model_error("malformed data URI"))?;
        if !header.ends_with(";base64") {
            return Err(model_error("only base64 data URIs are supported"));
        }
        if data.len() as u64 > MAX_SOURCE_BYTES * 4 / 3 + 4 {
            return Err(model_error("data URI too large"));
        }
        return base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|e| model_error(format!("invalid base64: {e}")));
    }
    let mut decoded = Vec::new();
    let mut pos = 0;
    while pos < uri.len() {
        let b = uri.as_bytes()[pos];
        if b == b'%' {
            let hex = uri
                .get(pos + 1..pos + 3)
                .ok_or_else(|| model_error("bad URI escape"))?;
            decoded.push(u8::from_str_radix(hex, 16).map_err(|_| model_error("bad URI escape"))?);
            pos += 3;
        } else {
            decoded.push(b);
            pos += 1;
        }
    }
    let rel =
        String::from_utf8(decoded).map_err(|_| model_error("dependency path must be UTF-8"))?;
    let resolved = forge_util::pathutil::confine_under(&[base], &rel)
        .map_err(|e| model_error(format!("dependency escapes model directory: {e}")))?;
    read_limited(&resolved)
}

fn extras_id(extras: &gltf::json::Extras, fallback: String) -> String {
    extras
        .as_ref()
        .and_then(|v| serde_json::from_str::<Value>(v.get()).ok())
        .and_then(|v| v.get("rurixId").and_then(Value::as_str).map(str::to_string))
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback)
}

fn check_uv(info: &gltf::texture::Info<'_>) -> Result<()> {
    if info.tex_coord() != 0 {
        return Err(model_error(
            "only TEXCOORD_0 is currently renderable; bake other UV sets before export",
        ));
    }
    if let Some(t) = info.texture_transform() {
        if t.offset() != [0., 0.]
            || t.scale() != [1., 1.]
            || t.rotation() != 0.
            || t.tex_coord().unwrap_or(0) != 0
        {
            return Err(model_error(
                "bake KHR_texture_transform into TEXCOORD_0 before export",
            ));
        }
    }
    Ok(())
}

fn decode_bytes(path: &Path, bytes: &[u8], manifest: &ModelManifest) -> Result<ModelBundle> {
    let gltf = gltf::Gltf::from_slice(bytes)
        .map_err(|e| model_error(format!("glTF validation failed: {e}")))?;
    for ext in gltf.extensions_required() {
        if !matches!(ext, "KHR_materials_unlit" | "KHR_texture_transform") {
            return Err(model_error(format!(
                "unsupported required extension: {ext}"
            )));
        }
    }
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let mut dependencies = BTreeMap::new();
    let mut buffers = Vec::new();
    for buffer in gltf.buffers() {
        let data = match buffer.source() {
            gltf::buffer::Source::Bin => gltf
                .blob
                .clone()
                .ok_or_else(|| model_error("missing GLB binary chunk"))?,
            gltf::buffer::Source::Uri(uri) => {
                let data = decode_uri(base, uri)?;
                dependencies.insert(uri.to_string(), forge_util::hashutil::sha256_hex(&data));
                data
            }
        };
        if data.len() < buffer.length() {
            return Err(model_error("buffer shorter than declared byteLength"));
        }
        buffers.push(data);
    }
    for view in gltf.views() {
        let end = view
            .offset()
            .checked_add(view.length())
            .ok_or_else(|| model_error("buffer view overflow"))?;
        if end > buffers[view.buffer().index()].len() {
            return Err(model_error("buffer view outside actual buffer bytes"));
        }
    }
    for accessor in gltf.accessors() {
        if let Some(view) = accessor.view() {
            let stride = view.stride().unwrap_or(accessor.size());
            let end = accessor
                .count()
                .saturating_sub(1)
                .checked_mul(stride)
                .and_then(|v| v.checked_add(accessor.offset()))
                .and_then(|v| v.checked_add(accessor.size()))
                .ok_or_else(|| model_error("accessor overflow"))?;
            if accessor.count() > 0 && end > view.length() {
                return Err(model_error("accessor outside buffer view"));
            }
        }
    }
    let get_buffer = |b: gltf::Buffer<'_>| buffers.get(b.index()).map(Vec::as_slice);
    let model_guid = stable_guid(&manifest.source_id, "model");
    let package = package_path(&manifest.source_id);

    let mut image_pixels = Vec::new();
    for image in gltf.images() {
        let encoded = match image.source() {
            gltf::image::Source::View { view, mime_type } => {
                if !matches!(mime_type, "image/png" | "image/jpeg") {
                    return Err(model_error(format!(
                        "unsupported embedded image: {mime_type}"
                    )));
                }
                let buf = &buffers[view.buffer().index()];
                buf[view.offset()..view.offset() + view.length()].to_vec()
            }
            gltf::image::Source::Uri { uri, .. } => {
                let data = decode_uri(base, uri)?;
                dependencies.insert(uri.to_string(), forge_util::hashutil::sha256_hex(&data));
                data
            }
        };
        let mut reader =
            image::ImageReader::new(std::io::Cursor::new(encoded)).with_guessed_format()?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(8192);
        limits.max_image_height = Some(8192);
        limits.max_alloc = Some(256 * 1024 * 1024);
        reader.limits(limits);
        let decoded = reader
            .decode()
            .map_err(|e| model_error(format!("texture decode failed: {e}")))?
            .into_rgba8();
        image_pixels.push(decoded);
    }
    let mut seen_texture_ids = HashSet::new();
    let textures = gltf
        .textures()
        .map(|t| {
            let id = extras_id(
                t.extras(),
                t.name()
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("texture_{}", t.index())),
            );
            if !seen_texture_ids.insert(id.clone()) {
                return Err(model_error(format!("duplicate texture stable ID: {id}")));
            }
            let guid = stable_guid(&manifest.source_id, &format!("texture/{id}"));
            let pixels = &image_pixels[t.source().index()];
            Ok(ModelTexture {
                id,
                asset_path: format!("{package}/Textures/{guid}.png"),
                guid,
                width: pixels.width(),
                height: pixels.height(),
                rgba: pixels.as_raw().clone(),
                wrap_s: t.sampler().wrap_s().as_gl_enum(),
                wrap_t: t.sampler().wrap_t().as_gl_enum(),
                mag_filter: t.sampler().mag_filter().map(|f| f.as_gl_enum()),
                min_filter: t.sampler().min_filter().map(|f| f.as_gl_enum()),
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let mut materials = Vec::new();
    let mut seen_material_ids = HashSet::new();
    for m in gltf.materials() {
        let pbr = m.pbr_metallic_roughness();
        for tex in [
            pbr.base_color_texture(),
            pbr.metallic_roughness_texture(),
            m.emissive_texture(),
        ]
        .into_iter()
        .flatten()
        {
            check_uv(&tex)?;
        }
        if m.normal_texture().is_some_and(|t| t.tex_coord() != 0)
            || m.occlusion_texture().is_some_and(|t| t.tex_coord() != 0)
        {
            return Err(model_error("normal/occlusion textures require TEXCOORD_0"));
        }
        let id = extras_id(
            m.extras(),
            m.name()
                .map(str::to_string)
                .unwrap_or_else(|| format!("material_{}", m.index().unwrap_or(0))),
        );
        if !seen_material_ids.insert(id.clone()) {
            return Err(model_error(format!("duplicate material stable ID: {id}")));
        }
        materials.push(ModelMaterial {
            guid: stable_guid(&manifest.source_id, &format!("material/{id}")),
            name: m.name().unwrap_or(&id).into(),
            base_color: pbr.base_color_factor(),
            metallic: pbr.metallic_factor(),
            roughness: pbr.roughness_factor(),
            emissive: m.emissive_factor(),
            base_color_texture: pbr.base_color_texture().map(|v| v.texture().index()),
            normal_texture: m.normal_texture().map(|v| v.texture().index()),
            metallic_roughness_texture: pbr
                .metallic_roughness_texture()
                .map(|v| v.texture().index()),
            occlusion_texture: m.occlusion_texture().map(|v| v.texture().index()),
            emissive_texture: m.emissive_texture().map(|v| v.texture().index()),
            normal_scale: m.normal_texture().map(|v| v.scale()).unwrap_or(1.),
            occlusion_strength: m.occlusion_texture().map(|v| v.strength()).unwrap_or(1.),
            double_sided: m.double_sided(),
            alpha_mode: match m.alpha_mode() {
                gltf::material::AlphaMode::Opaque => "OPAQUE",
                gltf::material::AlphaMode::Mask => "MASK",
                gltf::material::AlphaMode::Blend => "BLEND",
            }
            .into(),
            alpha_cutoff: m.alpha_cutoff().unwrap_or(0.5),
            unlit: m.unlit(),
        });
    }
    let mut primitives = Vec::new();
    let mut mesh_primitives = BTreeMap::new();
    let mut total_vertices = 0;
    for mesh in gltf.meshes() {
        let mesh_id = extras_id(
            mesh.extras(),
            mesh.name()
                .map(str::to_string)
                .unwrap_or_else(|| format!("mesh_{}", mesh.index())),
        );
        let mut refs = Vec::new();
        for primitive in mesh.primitives() {
            if primitive.mode() != Mode::Triangles {
                return Err(model_error(
                    "export triangulated meshes (TRIANGLES mode required)",
                ));
            }
            if primitive.morph_targets().next().is_some() {
                return Err(model_error(
                    "morph targets require baking before export; skeletal animation is supported",
                ));
            }
            let reader = primitive.reader(get_buffer);
            if reader.read_joints(1).is_some() || reader.read_weights(1).is_some() {
                return Err(model_error(
                    "export at most four joint influences per vertex",
                ));
            }
            let positions: Vec<_> = reader
                .read_positions()
                .ok_or_else(|| model_error("primitive POSITION is missing or unreadable"))?
                .collect();
            total_vertices += positions.len();
            if positions.is_empty() || total_vertices > MAX_VERTICES {
                return Err(model_error("model must contain 1..2000000 vertices"));
            }
            let indices: Vec<_> = reader
                .read_indices()
                .map(|i| i.into_u32().collect())
                .unwrap_or_else(|| (0..positions.len() as u32).collect());
            if indices.is_empty()
                || indices.len() % 3 != 0
                || indices.iter().any(|i| *i as usize >= positions.len())
            {
                return Err(model_error("invalid triangle indices"));
            }
            let normals = reader
                .read_normals()
                .map(|v| v.collect())
                .unwrap_or_else(|| generate_normals(&positions, &indices));
            let uv0: Vec<_> = reader
                .read_tex_coords(0)
                .map(|v| v.into_f32().collect())
                .unwrap_or_default();
            if let Some(m) = primitive.material().index().map(|i| &materials[i]) {
                if uv0.is_empty()
                    && [
                        m.base_color_texture,
                        m.normal_texture,
                        m.metallic_roughness_texture,
                        m.occlusion_texture,
                        m.emissive_texture,
                    ]
                    .iter()
                    .any(Option::is_some)
                {
                    return Err(model_error("textured primitive is missing TEXCOORD_0"));
                }
            }
            let tangents = reader
                .read_tangents()
                .map(|v| v.collect())
                .unwrap_or_else(|| generate_tangents(&positions, &normals, &uv0, &indices));
            let joints: Vec<_> = reader
                .read_joints(0)
                .map(|v| v.into_u16().collect())
                .unwrap_or_default();
            let mut weights: Vec<[f32; 4]> = reader
                .read_weights(0)
                .map(|v| v.into_f32().collect())
                .unwrap_or_default();
            if joints.is_empty() != weights.is_empty() {
                return Err(model_error("JOINTS_0 and WEIGHTS_0 must occur together"));
            }
            for w in &mut weights {
                let sum: f32 = w.iter().sum();
                if !sum.is_finite() || sum <= 0. || w.iter().any(|v| *v < 0.) {
                    return Err(model_error("invalid joint weights"));
                }
                for x in w {
                    *x /= sum;
                }
            }
            for (name, len) in [
                ("NORMAL", normals.len()),
                ("TANGENT", tangents.len()),
                ("TEXCOORD_0", uv0.len()),
                ("JOINTS_0", joints.len()),
                ("WEIGHTS_0", weights.len()),
            ] {
                if len != 0 && len != positions.len() {
                    return Err(model_error(format!("{name} count differs from POSITION")));
                }
            }
            refs.push(primitives.len());
            primitives.push(ModelPrimitive {
                id: format!("{mesh_id}/{}", primitive.index()),
                positions,
                normals,
                tangents,
                uv0,
                indices,
                joints,
                weights,
                material: primitive.material().index(),
            });
        }
        mesh_primitives.insert(mesh.index(), refs);
    }
    if primitives.is_empty() {
        return Err(model_error("model contains no mesh primitives"));
    }
    let mut seen_node_ids = HashSet::new();
    let nodes = gltf
        .nodes()
        .map(|n| {
            let name = n
                .name()
                .map(str::to_string)
                .unwrap_or_else(|| format!("node_{}", n.index()));
            let fallback = manifest
                .object_ids
                .get(&name)
                .cloned()
                .unwrap_or_else(|| name.clone());
            let id = extras_id(n.extras(), fallback);
            if !seen_node_ids.insert(id.clone()) {
                return Err(model_error(format!("duplicate node stable ID: {id}")));
            }
            let transform = n.transform();
            let (translation, rotation, scale) = transform.clone().decomposed();
            let matrix = match transform {
                gltf::scene::Transform::Matrix { matrix } => Some(flat_matrix(matrix)),
                _ => None,
            };
            let collision = n
                .extras()
                .as_ref()
                .and_then(|e| serde_json::from_str::<Value>(e.get()).ok())
                .is_some_and(|e| {
                    e.get("rurixCollision")
                        .or_else(|| e.get("collision"))
                        .and_then(Value::as_bool)
                        == Some(true)
                });
            Ok(ModelNode {
                id,
                name,
                children: n.children().map(|c| c.index()).collect(),
                primitives: n
                    .mesh()
                    .and_then(|m| mesh_primitives.get(&m.index()).cloned())
                    .unwrap_or_default(),
                translation,
                rotation,
                scale,
                matrix,
                skin: n.skin().map(|s| s.index()),
                collision,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    validate_hierarchy(&nodes)?;
    let roots = match gltf.default_scene().or_else(|| gltf.scenes().next()) {
        Some(s) => s.nodes().map(|n| n.index()).collect(),
        None => {
            let children: HashSet<_> = nodes
                .iter()
                .flat_map(|n| n.children.iter().copied())
                .collect();
            (0..nodes.len()).filter(|i| !children.contains(i)).collect()
        }
    };
    let skins = gltf
        .skins()
        .map(|s| {
            let joints: Vec<_> = s.joints().map(|j| j.index()).collect();
            let inverse_bind_matrices: Vec<_> = s
                .reader(get_buffer)
                .read_inverse_bind_matrices()
                .map(|v| v.map(flat_matrix).collect())
                .unwrap_or_else(|| vec![identity_matrix(); joints.len()]);
            if inverse_bind_matrices.len() != joints.len() {
                return Err(model_error(
                    "inverse bind matrix count differs from joint count",
                ));
            }
            Ok(ModelSkin {
                name: s.name().unwrap_or("skin").into(),
                joints,
                inverse_bind_matrices,
                skeleton: s.skeleton().map(|n| n.index()),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    for node in &nodes {
        if let Some(skin) = node.skin.map(|i| &skins[i]) {
            for &pi in &node.primitives {
                let p = &primitives[pi];
                if p.joints.is_empty()
                    || p.joints
                        .iter()
                        .flatten()
                        .any(|j| *j as usize >= skin.joints.len())
                {
                    return Err(model_error(
                        "skinned primitive joint index outside skin palette",
                    ));
                }
            }
        }
    }
    let animations = gltf
        .animations()
        .map(|a| {
            let mut duration = 0f32;
            let mut channels = Vec::new();
            for c in a.channels() {
                let reader = c.reader(get_buffer);
                let times: Vec<_> = reader
                    .read_inputs()
                    .ok_or_else(|| model_error("animation inputs missing"))?
                    .collect();
                if times.is_empty()
                    || times.iter().any(|x| !x.is_finite() || *x < 0.)
                    || times.windows(2).any(|w| w[0] >= w[1])
                {
                    return Err(model_error(
                        "animation times must be finite and strictly increasing",
                    ));
                }
                duration = duration.max(*times.last().unwrap());
                let (path, values): (&str, Vec<[f32; 4]>) = match reader
                    .read_outputs()
                    .ok_or_else(|| model_error("animation outputs missing"))?
                {
                    ReadOutputs::Translations(v) => {
                        ("translation", v.map(|p| [p[0], p[1], p[2], 0.]).collect())
                    }
                    ReadOutputs::Scales(v) => {
                        ("scale", v.map(|p| [p[0], p[1], p[2], 0.]).collect())
                    }
                    ReadOutputs::Rotations(v) => ("rotation", v.into_f32().collect()),
                    ReadOutputs::MorphTargetWeights(_) => {
                        return Err(model_error(
                            "morph weight animation requires baking before export",
                        ))
                    }
                };
                let interpolation = match c.sampler().interpolation() {
                    gltf::animation::Interpolation::Linear => "LINEAR",
                    gltf::animation::Interpolation::Step => "STEP",
                    gltf::animation::Interpolation::CubicSpline => "CUBICSPLINE",
                };
                let required = times.len() * if interpolation == "CUBICSPLINE" { 3 } else { 1 };
                if values.len() != required {
                    return Err(model_error("animation keyframe count mismatch"));
                }
                channels.push(ModelAnimationChannel {
                    node: c.target().node().index(),
                    path: path.into(),
                    times,
                    values,
                    interpolation: interpolation.into(),
                });
            }
            Ok(ModelAnimation {
                name: a
                    .name()
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("animation_{}", a.index())),
                duration,
                channels,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let hash_input = json!({"source":forge_util::hashutil::sha256_hex(bytes),"dependencies":dependencies,"sourceId":manifest.source_id,"name":manifest.name,"kind":manifest.kind,"objectIds":manifest.object_ids,"idleClip":manifest.idle_clip,"walkClip":manifest.walk_clip,"importerVersion":1});
    let idle_clip = if manifest.kind == "character" {
        resolve_clip(&animations, manifest.idle_clip.as_deref().unwrap_or("Idle"))?
    } else {
        String::new()
    };
    let walk_clip = if manifest.kind == "character" {
        resolve_clip(&animations, manifest.walk_clip.as_deref().unwrap_or("Walk"))?
    } else {
        String::new()
    };
    let bundle = ModelBundle {
        version: 1,
        guid: model_guid,
        revision: manifest.revision.max(1),
        name: manifest.name.clone(),
        source_id: manifest.source_id.clone(),
        source_hash: forge_util::hashutil::sha256_hex(&serde_json::to_vec(&hash_input).unwrap()),
        kind: manifest.kind.clone(),
        roots,
        primitives,
        nodes,
        materials,
        textures,
        skins,
        animations,
        idle_clip,
        walk_clip,
    };
    validate_bundle(&bundle)?;
    Ok(bundle)
}

fn resolve_clip(animations: &[ModelAnimation], requested: &str) -> Result<String> {
    let candidates: Vec<_> = animations
        .iter()
        .filter(|a| a.name.eq_ignore_ascii_case(requested))
        .collect();
    if candidates.len() != 1 {
        return Err(crate::AssetError::new("MODEL_CHARACTER_INVALID",format!("character requires one unambiguous {requested:?} clip; export Idle/Walk or set manifest idleClip/walkClip")));
    }
    let clip = candidates[0];
    if clip.channels.is_empty() || !clip.duration.is_finite() || clip.duration <= 0. {
        return Err(crate::AssetError::new(
            "MODEL_CHARACTER_INVALID",
            format!("character clip {:?} has no playable animation", clip.name),
        ));
    }
    Ok(clip.name.clone())
}

fn flat_matrix(matrix: [[f32; 4]; 4]) -> [f32; 16] {
    let mut out = [0.; 16];
    for (i, v) in matrix.into_iter().flatten().enumerate() {
        out[i] = v;
    }
    out
}

fn validate_hierarchy(nodes: &[ModelNode]) -> Result<()> {
    let mut parents = vec![0; nodes.len()];
    let mut states = vec![0u8; nodes.len()];
    fn visit(i: usize, nodes: &[ModelNode], states: &mut [u8]) -> Result<()> {
        if states[i] == 1 {
            return Err(model_error("node hierarchy cycle"));
        }
        if states[i] == 2 {
            return Ok(());
        }
        states[i] = 1;
        for &c in &nodes[i].children {
            visit(c, nodes, states)?;
        }
        states[i] = 2;
        Ok(())
    }
    for n in nodes {
        for &c in &n.children {
            if c >= nodes.len() {
                return Err(model_error("node child index out of range"));
            }
            parents[c] += 1;
            if parents[c] > 1 {
                return Err(model_error("node has multiple parents"));
            }
        }
    }
    // Bound depth to keep recursion safe for adversarial input.
    if nodes.len() > 4096 {
        return Err(model_error("model exceeds 4096 node limit"));
    }
    for i in 0..nodes.len() {
        visit(i, nodes, &mut states)?;
    }
    Ok(())
}

pub(super) fn validate_bundle(b: &ModelBundle) -> Result<()> {
    if b.version != 1 || b.guid.is_empty() || b.primitives.is_empty() {
        return Err(model_error("invalid rxmodel header"));
    }
    validate_hierarchy(&b.nodes)?;
    let mut local_ids = HashSet::new();
    if b.nodes
        .iter()
        .any(|n| !local_ids.insert(source_entity_id(&n.id)))
    {
        return Err(model_error("template node identity collision"));
    }
    if b.roots.iter().any(|i| *i >= b.nodes.len()) {
        return Err(model_error("invalid scene root"));
    }
    for p in &b.primitives {
        if p.positions
            .iter()
            .flatten()
            .chain(p.normals.iter().flatten())
            .chain(p.tangents.iter().flatten())
            .chain(p.uv0.iter().flatten())
            .chain(p.weights.iter().flatten())
            .any(|v| !v.is_finite())
        {
            return Err(model_error("non-finite mesh attribute"));
        }
        if p.positions.is_empty()
            || p.indices.len() % 3 != 0
            || p.indices.iter().any(|i| *i as usize >= p.positions.len())
            || p.material.is_some_and(|i| i >= b.materials.len())
        {
            return Err(model_error("invalid primitive reference"));
        }
        if [
            p.normals.len(),
            p.tangents.len(),
            p.uv0.len(),
            p.joints.len(),
            p.weights.len(),
        ]
        .into_iter()
        .any(|n| n != 0 && n != p.positions.len())
        {
            return Err(model_error("invalid primitive attribute length"));
        }
    }
    for n in &b.nodes {
        if n.primitives.iter().any(|p| *p >= b.primitives.len())
            || n.skin.is_some_and(|s| s >= b.skins.len())
        {
            return Err(model_error("invalid node reference"));
        }
        if n.translation
            .iter()
            .chain(n.rotation.iter())
            .chain(n.scale.iter())
            .chain(n.matrix.iter().flatten())
            .any(|v| !v.is_finite())
        {
            return Err(model_error("non-finite node transform"));
        }
    }
    for m in &b.materials {
        if [
            m.base_color_texture,
            m.normal_texture,
            m.metallic_roughness_texture,
            m.occlusion_texture,
            m.emissive_texture,
        ]
        .into_iter()
        .flatten()
        .any(|t| t >= b.textures.len())
        {
            return Err(model_error("invalid material texture reference"));
        }
        if m.base_color
            .iter()
            .chain(m.emissive.iter())
            .copied()
            .chain([
                m.metallic,
                m.roughness,
                m.normal_scale,
                m.occlusion_strength,
                m.alpha_cutoff,
            ])
            .any(|v| !v.is_finite())
        {
            return Err(model_error("non-finite material parameter"));
        }
    }
    for t in &b.textures {
        if t.width == 0
            || t.height == 0
            || t.width > 8192
            || t.height > 8192
            || t.rgba.len() != t.width as usize * t.height as usize * 4
        {
            return Err(model_error("invalid texture dimensions or bytes"));
        }
    }
    for s in &b.skins {
        if s.joints.len() != s.inverse_bind_matrices.len()
            || s.joints.iter().any(|n| *n >= b.nodes.len())
            || s.skeleton.is_some_and(|n| n >= b.nodes.len())
            || s.inverse_bind_matrices
                .iter()
                .flatten()
                .any(|v| !v.is_finite())
        {
            return Err(model_error("invalid skin"));
        }
    }
    for node in &b.nodes {
        if let Some(si) = node.skin {
            for &pi in &node.primitives {
                let p = &b.primitives[pi];
                if p.joints.is_empty()
                    || p.joints.len() != p.weights.len()
                    || p.joints
                        .iter()
                        .flatten()
                        .any(|j| *j as usize >= b.skins[si].joints.len())
                {
                    return Err(model_error("invalid skin vertex palette"));
                }
            }
        }
    }
    for a in &b.animations {
        for c in &a.channels {
            let factor = if c.interpolation == "CUBICSPLINE" {
                3
            } else {
                1
            };
            if c.node >= b.nodes.len()
                || c.times.is_empty()
                || c.values.len() != c.times.len() * factor
                || c.values.iter().flatten().any(|v| !v.is_finite())
                || c.times.iter().any(|t| !t.is_finite() || *t < 0.)
                || c.times.windows(2).any(|w| w[0] >= w[1])
                || !matches!(c.path.as_str(), "translation" | "rotation" | "scale")
                || !matches!(c.interpolation.as_str(), "LINEAR" | "STEP" | "CUBICSPLINE")
            {
                return Err(model_error("invalid animation channel"));
            }
        }
    }
    if b.kind == "character" {
        let skinned = b.nodes.iter().any(|n| {
            n.skin.is_some_and(|s| !b.skins[s].joints.is_empty())
                && n.primitives
                    .iter()
                    .any(|p| !b.primitives[*p].weights.is_empty())
        });
        if !skinned {
            return Err(crate::AssetError::new(
                "MODEL_CHARACTER_INVALID",
                "character requires a bound skin with weighted mesh vertices",
            ));
        }
        if b.idle_clip.is_empty() || b.walk_clip.is_empty() || b.idle_clip == b.walk_clip {
            return Err(crate::AssetError::new(
                "MODEL_CHARACTER_INVALID",
                "character requires distinct Idle and Walk clip mappings",
            ));
        }
        resolve_clip(&b.animations, &b.idle_clip)?;
        resolve_clip(&b.animations, &b.walk_clip)?;
    }
    Ok(())
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn normalize(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 1e-12 {
        [v[0] / l, v[1] / l, v[2] / l]
    } else {
        [0., 1., 0.]
    }
}
fn generate_normals(p: &[[f32; 3]], idx: &[u32]) -> Vec<[f32; 3]> {
    let mut out = vec![[0.; 3]; p.len()];
    for tri in idx.chunks_exact(3) {
        let n = cross(
            sub(p[tri[1] as usize], p[tri[0] as usize]),
            sub(p[tri[2] as usize], p[tri[0] as usize]),
        );
        for &i in tri {
            for c in 0..3 {
                out[i as usize][c] += n[c];
            }
        }
    }
    out.into_iter().map(normalize).collect()
}
fn generate_tangents(
    p: &[[f32; 3]],
    normals: &[[f32; 3]],
    uv: &[[f32; 2]],
    idx: &[u32],
) -> Vec<[f32; 4]> {
    if uv.len() != p.len() {
        return Vec::new();
    }
    let mut tan = vec![[0.; 3]; p.len()];
    let mut bit = tan.clone();
    for tri in idx.chunks_exact(3) {
        let (a, b, c) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
        let e1 = sub(p[b], p[a]);
        let e2 = sub(p[c], p[a]);
        let d1 = [uv[b][0] - uv[a][0], uv[b][1] - uv[a][1]];
        let d2 = [uv[c][0] - uv[a][0], uv[c][1] - uv[a][1]];
        let det = d1[0] * d2[1] - d1[1] * d2[0];
        if det.abs() < 1e-10 {
            continue;
        }
        for i in [a, b, c] {
            for j in 0..3 {
                tan[i][j] += (e1[j] * d2[1] - e2[j] * d1[1]) / det;
                bit[i][j] += (e2[j] * d1[0] - e1[j] * d2[0]) / det;
            }
        }
    }
    tan.iter()
        .enumerate()
        .map(|(i, t)| {
            let n = normals[i];
            let dot = n[0] * t[0] + n[1] * t[1] + n[2] * t[2];
            let t = normalize([t[0] - n[0] * dot, t[1] - n[1] * dot, t[2] - n[2] * dot]);
            let c = cross(n, t);
            let hand = if c[0] * bit[i][0] + c[1] * bit[i][1] + c[2] * bit[i][2] < 0. {
                -1.
            } else {
                1.
            };
            [t[0], t[1], t[2], hand]
        })
        .collect()
}
