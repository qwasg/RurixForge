use super::*;
use crate::meta::{MetaDoc, Provenance};
use crate::{meta_path_for, AssetError};
use fs2::FileExt;
use serde_json::{json, Value};
use std::fs::{File, OpenOptions};
use std::io::Write;

/// A real OS lock, released on process exit. Covers reads and directory swaps across MCP/host processes.
struct ProjectLock(File);
impl Drop for ProjectLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}
fn lock(project: &ForgeProject) -> Result<ProjectLock> {
    std::fs::create_dir_all(project.tmp_root())?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(project.tmp_root().join("model-publish.lock"))?;
    file.lock_exclusive()?;
    Ok(ProjectLock(file))
}

fn write_sync(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct Journal {
    guid: String,
    stage: String,
    backup: String,
}

/// Restore a previous committed package after an interrupted directory swap. Journal paths are
/// filenames only and are confined again; damaged journals never authorize arbitrary file moves.
fn recover(project: &ForgeProject) -> Result<()> {
    let root = project.tmp_root().join("model-transactions");
    if !root.is_dir() {
        return Ok(());
    }
    for ent in std::fs::read_dir(&root)? {
        let path = ent?.path();
        if path.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let journal: Journal = serde_json::from_slice(&std::fs::read(&path)?)
            .map_err(|e| model_error(format!("invalid publication journal: {e}")))?;
        if uuid::Uuid::parse_str(&journal.guid).is_err()
            || journal.stage.contains(['/', '\\', ':'])
            || journal.backup.contains(['/', '\\', ':'])
            || journal.stage.contains("..")
            || journal.backup.contains("..")
        {
            return Err(model_error("unsafe publication journal"));
        }
        let target = project.resolve_content_path(&format!("Models/{}", journal.guid))?;
        let backup = root.join(&journal.backup);
        let stage = root.join(&journal.stage);
        if !target.exists() && backup.is_dir() {
            std::fs::rename(&backup, &target)?;
        }
        // Existing target means either old package was never moved, or new package committed.
        if backup.is_dir() {
            std::fs::remove_dir_all(&backup)?;
        }
        if stage.is_dir() {
            std::fs::remove_dir_all(&stage)?;
        }
        std::fs::remove_file(path)?;
    }
    Ok(())
}

fn read_bundle(path: &Path) -> Result<ModelBundle> {
    let data = std::fs::read(path)?;
    let bundle: ModelBundle =
        serde_json::from_slice(&data).map_err(|e| model_error(format!("rxmodel parse: {e}")))?;
    super::importer::validate_bundle(&bundle)?;
    Ok(bundle)
}

/// Resolve GUID or Content-relative .rxmodel. Only actual model assets are accepted.
pub fn model_asset_path(project: &ForgeProject, model_ref: &str) -> Result<PathBuf> {
    if model_ref.ends_with(".rxmodel") {
        let rel = crate::normalize_rel(model_ref.strip_prefix("Content/").unwrap_or(model_ref))?;
        return project.resolve_content_path(&rel);
    }
    if uuid::Uuid::parse_str(model_ref).is_ok() {
        let candidate = absolute_model_path(project, model_ref);
        if candidate.is_file() {
            return Ok(candidate);
        }
        for rel in project.scan_content()? {
            if !rel.ends_with(".rxmodel") {
                continue;
            }
            if MetaDoc::load(&meta_path_for(&project.content_root(), &rel))
                .is_ok_and(|m| m.guid == model_ref)
            {
                return project.resolve_content_path(&rel);
            }
        }
    }
    Err(AssetError::new(
        "MODEL_NOT_FOUND",
        format!("model reference not found: {model_ref}"),
    ))
}

pub fn load_model(project: &ForgeProject, model_ref: &str) -> Result<ModelBundle> {
    let _lock = lock(project)?;
    recover(project)?;
    read_bundle(&model_asset_path(project, model_ref)?)
}

/// Immutable historical snapshots keep locally overridden removed template nodes renderable.
pub fn load_model_revision(
    project: &ForgeProject,
    model_ref: &str,
    revision: u64,
) -> Result<ModelBundle> {
    let _lock = lock(project)?;
    recover(project)?;
    let current = model_asset_path(project, model_ref)
        .ok()
        .and_then(|p| read_bundle(&p).ok());
    if let Some(bundle) = &current {
        if bundle.revision == revision {
            return Ok(bundle.clone());
        }
    }
    let guid = match &current {
        Some(b) => b.guid.clone(),
        None if uuid::Uuid::parse_str(model_ref).is_ok() => model_ref.to_string(),
        _ => {
            return Err(AssetError::new(
                "MODEL_NOT_FOUND",
                format!("model reference not found: {model_ref}"),
            ))
        }
    };
    let path = project
        .cache_root()
        .join("models")
        .join(guid)
        .join(format!("{revision}.rxmodel"));
    if !path.is_file() {
        return Err(AssetError::new(
            "MODEL_REVISION_NOT_FOUND",
            format!("model revision {revision} is unavailable"),
        ));
    }
    read_bundle(&path)
}

fn archive_previous(project: &ForgeProject, path: &Path, bundle: &ModelBundle) -> Result<()> {
    let bytes = std::fs::read(path)?;
    let dir = project.cache_root().join("models").join(&bundle.guid);
    std::fs::create_dir_all(&dir)?;
    let history = dir.join(format!("{}.rxmodel", bundle.revision));
    if history.is_file() {
        if std::fs::read(&history)? != bytes {
            return Err(AssetError::new(
                "MODEL_REVISION_CONFLICT",
                "a historical revision already exists with different bytes",
            ));
        }
        return Ok(());
    }
    let stage = dir.join(format!("{}.tmp", uuid::Uuid::new_v4()));
    write_sync(&stage, &bytes)?;
    std::fs::rename(stage, history)?;
    Ok(())
}

fn package_is_current(target: &Path, bundle: &ModelBundle) -> bool {
    let mut files = vec!["model.rxmodel".to_string(), "template.rxprefab".to_string()];
    files.extend(
        bundle
            .textures
            .iter()
            .map(|t| format!("Textures/{}.png", t.guid)),
    );
    files.extend(
        bundle
            .materials
            .iter()
            .map(|m| format!("Materials/{}.rxmat", m.guid)),
    );
    if files
        .iter()
        .any(|f| !target.join(f).is_file() || !target.join(format!("{f}.meta")).is_file())
    {
        return false;
    }
    fn scan(path: &Path) -> bool {
        let Ok(entries) = std::fs::read_dir(path) else {
            return false;
        };
        for entry in entries {
            let Ok(entry) = entry else { return false };
            let p = entry.path();
            if p.is_dir() {
                if !scan(&p) {
                    return false;
                }
            } else if p.extension().and_then(|e| e.to_str()) == Some("meta") {
                let Ok(meta) = MetaDoc::load(&p) else {
                    return false;
                };
                let Ok(bytes) = std::fs::read(p.with_extension("")) else {
                    return false;
                };
                if meta.build_state.as_deref() != Some("current")
                    || !super::validation::asset_hash_matches(&meta, &bytes)
                {
                    return false;
                }
            }
        }
        true
    }
    scan(target)
}

fn published(bundle: &ModelBundle, changed: bool) -> PublishedModel {
    let package = package_path(&bundle.source_id);
    let prefab_guid = stable_guid(&bundle.source_id, "prefab");
    let mut asset_guids = vec![bundle.guid.clone(), prefab_guid.clone()];
    asset_guids.extend(bundle.materials.iter().map(|m| m.guid.clone()));
    asset_guids.extend(bundle.textures.iter().map(|t| t.guid.clone()));
    PublishedModel {
        guid: bundle.guid.clone(),
        revision: bundle.revision,
        asset_path: format!("{package}/model.rxmodel"),
        prefab_path: format!("{package}/template.rxprefab"),
        prefab_guid,
        asset_guids,
        changed,
    }
}

fn metadata(
    bundle: &ModelBundle,
    guid: &str,
    kind: &str,
    importer: &str,
    old: Option<MetaDoc>,
) -> MetaDoc {
    let mut m = old.unwrap_or(MetaDoc {
        guid: guid.into(),
        atype: kind.into(),
        importer: importer.into(),
        import_settings: Default::default(),
        provenance: None,
        build_state: None,
        semantic: None,
    });
    m.guid = guid.into();
    m.atype = kind.into();
    m.importer = importer.into();
    m.build_state = Some("current".into());
    m.provenance = Some(Provenance {
        origin: "blender".into(),
        detail: Some(
            json!({"sourceId":bundle.source_id,"modelGuid":bundle.guid,"revision":bundle.revision,"sourceHash":bundle.source_hash}),
        ),
    });
    m
}

fn write_meta(
    stage: &Path,
    old_target: &Path,
    relative: &str,
    bundle: &ModelBundle,
    guid: &str,
    kind: &str,
    importer: &str,
) -> Result<()> {
    let old = MetaDoc::load(&old_target.join(format!("{relative}.meta"))).ok();
    let mut m = metadata(bundle, guid, kind, importer, old);
    if let Some(Value::Object(detail)) = m.provenance.as_mut().and_then(|p| p.detail.as_mut()) {
        detail.insert(
            "assetHash".into(),
            json!(forge_util::hashutil::sha256_file(&stage.join(relative))?),
        );
    }
    let text = serde_yaml::to_string(&m).map_err(|e| model_error(e.to_string()))?;
    write_sync(&stage.join(format!("{relative}.meta")), text.as_bytes())
}

fn template(bundle: &ModelBundle) -> Value {
    let mut components = Vec::new();
    if bundle.kind == "map" {
        components.extend([
            json!({"type":"Collider","enabled":true,"props":{"shape":"mesh","model":bundle.guid}}),
            json!({"type":"RigidBody","enabled":true,"props":{"kind":"static","mass":1.0}}),
            json!({"type":"Category","enabled":true,"props":{"category":"map"}}),
        ]);
    } else if bundle.kind == "character" {
        let idle = &bundle.idle_clip;
        let walk = &bundle.walk_clip;
        components.extend([
            json!({"type":"Animator","enabled":true,"props":{"clip":idle,"idleClip":idle,"walkClip":walk,"time":0.0,"playing":!idle.is_empty(),"loop":true,"speed":1.0}}),
            json!({"type":"CharacterController","enabled":true,"props":{"speed":3.0,"radius":0.35,"height":1.8}}),
            json!({"type":"Category","enabled":true,"props":{"category":"role"}}),
        ]);
    }
    let mut parents = vec![None; bundle.nodes.len()];
    for (i, n) in bundle.nodes.iter().enumerate() {
        for &child in &n.children {
            parents[child] = Some(i);
        }
    }
    let source_nodes:Vec<_>=bundle.nodes.iter().enumerate().map(|(i,n)|json!({"id":n.id,"name":n.name,"parentId":parents[i].map(|p|&bundle.nodes[p].id),"modelNode":i,"transform":{"translation":n.translation,"rotation":n.rotation,"scale":n.scale},"matrix":n.matrix,"primitives":n.primitives,"skin":n.skin,"collision":n.collision})).collect();
    let source_roots: Vec<_> = bundle.roots.iter().map(|i| &bundle.nodes[*i].id).collect();
    let category = if bundle.kind == "character" {
        "role"
    } else {
        "map"
    };
    if bundle.kind == "prop" {
        components.push(json!({"type":"Category","enabled":true,"props":{"category":category}}));
    }
    let mut entities = vec![
        json!({"id":1,"name":bundle.name,"transform":{"translation":[0.,0.,0.],"rotation":[0.,0.,0.,1.],"scale":[1.,1.,1.]},"components":components}),
    ];
    for (i, n) in bundle.nodes.iter().enumerate() {
        let parent = parents[i]
            .map(|p| source_entity_id(&bundle.nodes[p].id))
            .unwrap_or(1);
        let mut components = vec![
            json!({"type":"Parent","enabled":true,"props":{"entity":parent}}),
            json!({"type":"ModelNode","enabled":true,"props":{"model":bundle.guid,"nodeId":n.id}}),
            json!({"type":"Category","enabled":true,"props":{"category":category}}),
        ];
        if !n.primitives.is_empty() {
            components.push(json!({"type":"ModelRenderer","enabled":true,"props":{"model":bundle.guid,"nodeId":n.id}}));
        }
        entities.push(json!({"id":source_entity_id(&n.id),"name":n.name,"transform":{"translation":n.translation,"rotation":n.rotation,"scale":n.scale},"components":components}));
    }
    let next_id = bundle
        .nodes
        .iter()
        .map(|n| source_entity_id(&n.id))
        .max()
        .unwrap_or(1)
        + 1;
    json!({"version":1,"name":bundle.name,"kind":bundle.kind,"model":bundle.guid,"sourceId":bundle.source_id,"sourceNodes":source_nodes,"sourceRoots":source_roots,"revision":bundle.revision,"next_id":next_id,"entities":entities})
}

fn stage_bundle(
    stage: &Path,
    target: &Path,
    bundle: &ModelBundle,
    manifest: &ModelManifest,
) -> Result<()> {
    write_sync(
        &stage.join("model.rxmodel"),
        &serde_json::to_vec(bundle).map_err(|e| model_error(e.to_string()))?,
    )?;
    write_meta(
        stage,
        target,
        "model.rxmodel",
        bundle,
        &bundle.guid,
        "model",
        "blender-model-v1",
    )?;
    // Preserve full source/export recipe in the model sidecar without inventing another asset type.
    let mp = stage.join("model.rxmodel.meta");
    let mut meta = MetaDoc::load(&mp)?;
    if let Some(p) = &mut meta.provenance {
        if let Some(Value::Object(detail)) = &mut p.detail {
            detail.insert("manifest".into(), serde_json::to_value(manifest).unwrap());
        }
    }
    write_sync(
        &mp,
        serde_yaml::to_string(&meta)
            .map_err(|e| model_error(e.to_string()))?
            .as_bytes(),
    )?;
    let prefab_guid = stable_guid(&bundle.source_id, "prefab");
    write_sync(
        &stage.join("template.rxprefab"),
        &serde_json::to_vec_pretty(&template(bundle)).unwrap(),
    )?;
    write_meta(
        stage,
        target,
        "template.rxprefab",
        bundle,
        &prefab_guid,
        "prefab",
        "model-template-v1",
    )?;
    for t in &bundle.textures {
        let rel = format!("Textures/{}.png", t.guid);
        let mut png = std::io::Cursor::new(Vec::new());
        let rgba = image::RgbaImage::from_raw(t.width, t.height, t.rgba.clone())
            .ok_or_else(|| model_error("invalid texture"))?;
        image::DynamicImage::ImageRgba8(rgba)
            .write_to(&mut png, image::ImageFormat::Png)
            .map_err(|e| model_error(e.to_string()))?;
        write_sync(&stage.join(&rel), &png.into_inner())?;
        write_meta(stage, target, &rel, bundle, &t.guid, "texture", "png")?;
    }
    for m in &bundle.materials {
        let mut texture_refs = serde_json::Map::new();
        for (slot, index) in [
            ("albedo", m.base_color_texture),
            ("normal", m.normal_texture),
            ("metallicRoughness", m.metallic_roughness_texture),
            ("occlusion", m.occlusion_texture),
            ("emissive", m.emissive_texture),
        ] {
            if let Some(index) = index {
                texture_refs.insert(slot.into(), json!(bundle.textures[index].guid));
            }
        }
        let doc = json!({"version":1,"shader":if m.unlit {"unlit"}else{"pbr-default"},"params":{"baseColor":m.base_color,"metallic":m.metallic,"roughness":m.roughness,"emissive":m.emissive,"normalScale":m.normal_scale,"occlusionStrength":m.occlusion_strength,"doubleSided":m.double_sided,"alphaMode":m.alpha_mode,"alphaCutoff":m.alpha_cutoff},"textures":texture_refs});
        let rel = format!("Materials/{}.rxmat", m.guid);
        write_sync(&stage.join(&rel), &serde_json::to_vec_pretty(&doc).unwrap())?;
        write_meta(stage, target, &rel, bundle, &m.guid, "material", "material")?;
    }
    Ok(())
}

/// Validate and publish a complete source revision. Duplicate content is idempotent; a changed
/// export requires a newer manifest revision, preventing late jobs from replacing newer content.
/// Failure before commit leaves every prior model, texture, material, sidecar and template intact.
pub fn import_model_bundle(
    project: &ForgeProject,
    glb_path: &Path,
    manifest: &ModelManifest,
) -> Result<PublishedModel> {
    let bundle = super::importer::decode(glb_path, manifest)?;
    publish_bundle(project, bundle, manifest, false)
}

fn publish_bundle(
    project: &ForgeProject,
    bundle: ModelBundle,
    manifest: &ModelManifest,
    fail_after_backup: bool,
) -> Result<PublishedModel> {
    project.ensure_dirs()?;
    let _lock = lock(project)?;
    recover(project)?;
    let package = package_path(&manifest.source_id);
    let target = project.resolve_content_path(&package)?;
    let old_path = target.join("model.rxmodel");
    if old_path.is_file() {
        // A newer validated export may repair an older bundle that lacks newly required
        // character metadata. Still verify identity before replacing this source-owned folder.
        let old: ModelBundle = serde_json::from_slice(&std::fs::read(&old_path)?)
            .map_err(|e| model_error(format!("previous rxmodel parse: {e}")))?;
        if old.guid != bundle.guid || old.source_id != bundle.source_id {
            return Err(AssetError::new(
                "MODEL_SOURCE_CONFLICT",
                "target package belongs to another source identity",
            ));
        }
        if old.source_hash == bundle.source_hash && package_is_current(&target, &old) {
            super::importer::validate_bundle(&old)?;
            return Ok(published(&old, false));
        }
        if bundle.revision <= old.revision {
            return Err(AssetError::new(
                "MODEL_REVISION_CONFLICT",
                format!(
                    "revision {} is not newer than committed revision {}",
                    bundle.revision, old.revision
                ),
            ));
        }
        archive_previous(project, &old_path, &old)?;
    }
    let tx_root = project.tmp_root().join("model-transactions");
    std::fs::create_dir_all(&tx_root)?;
    std::fs::create_dir_all(project.content_root().join("Models"))?;
    let nonce = uuid::Uuid::new_v4().to_string();
    let journal = Journal {
        guid: bundle.guid.clone(),
        stage: format!("{nonce}.stage"),
        backup: format!("{nonce}.backup"),
    };
    let stage = tx_root.join(&journal.stage);
    let backup = tx_root.join(&journal.backup);
    if let Err(e) = stage_bundle(&stage, &target, &bundle, manifest) {
        let _ = std::fs::remove_dir_all(&stage);
        return Err(e);
    }
    // Validate the actual on-disk staged bytes before the commit point.
    if let Err(e) = read_bundle(&stage.join("model.rxmodel")) {
        let _ = std::fs::remove_dir_all(&stage);
        return Err(e);
    }
    let journal_path = tx_root.join(format!("{nonce}.json"));
    write_sync(&journal_path, &serde_json::to_vec(&journal).unwrap())?;
    let had_previous = target.exists();
    if had_previous {
        if let Err(e) = std::fs::rename(&target, &backup) {
            let _ = std::fs::remove_dir_all(&stage);
            let _ = std::fs::remove_file(&journal_path);
            return Err(e.into());
        }
    }
    let commit = if fail_after_backup {
        Err(std::io::Error::other("injected commit failure"))
    } else {
        std::fs::rename(&stage, &target)
    };
    if let Err(e) = commit {
        if had_previous {
            std::fs::rename(&backup, &target).map_err(|restore| {
                AssetError::new(
                    "MODEL_RECOVERY_REQUIRED",
                    format!("commit failed: {e}; restore failed: {restore}; journal retained"),
                )
            })?;
        }
        let _ = std::fs::remove_dir_all(&stage);
        let _ = std::fs::remove_file(&journal_path);
        return Err(AssetError::new(
            "MODEL_PUBLISH_FAILED",
            format!("publication rolled back: {e}"),
        ));
    }
    // Commit succeeded. Cleanup failure is recoverable on the next operation, never a failed publish.
    if !backup.exists() || std::fs::remove_dir_all(&backup).is_ok() {
        let _ = std::fs::remove_file(&journal_path);
    }
    Ok(published(&bundle, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        fn scan(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for e in std::fs::read_dir(dir).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    scan(root, &p, out);
                } else {
                    out.insert(
                        p.strip_prefix(root).unwrap().to_owned(),
                        std::fs::read(p).unwrap(),
                    );
                }
            }
        }
        let mut out = BTreeMap::new();
        scan(root, root, &mut out);
        out
    }
    #[test]
    fn directory_commit_failure_restores_every_old_byte() {
        let root =
            std::env::temp_dir().join(format!("assetd-model-rollback-{}", uuid::Uuid::new_v4()));
        let project = ForgeProject::with_defaults(root.clone());
        let manifest = ModelManifest {
            version: 1,
            source_id: "rollback".into(),
            name: "Rollback".into(),
            kind: "prop".into(),
            revision: 1,
            source_blend: None,
            object_ids: Default::default(),
            idle_clip: None,
            walk_clip: None,
        };
        let bundle = ModelBundle {
            version: 1,
            guid: stable_guid("rollback", "model"),
            revision: 1,
            name: "Rollback".into(),
            source_id: "rollback".into(),
            source_hash: "first".into(),
            kind: "prop".into(),
            roots: vec![],
            primitives: vec![ModelPrimitive {
                id: "p".into(),
                positions: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
                normals: vec![],
                tangents: vec![],
                uv0: vec![],
                indices: vec![0, 1, 2],
                joints: vec![],
                weights: vec![],
                material: None,
            }],
            nodes: vec![],
            materials: vec![],
            textures: vec![ModelTexture {
                id: "t".into(),
                guid: stable_guid("rollback", "texture/t"),
                asset_path: format!(
                    "{}/Textures/{}.png",
                    package_path("rollback"),
                    stable_guid("rollback", "texture/t")
                ),
                width: 1,
                height: 1,
                rgba: vec![1, 2, 3, 255],
                wrap_s: 10497,
                wrap_t: 10497,
                mag_filter: None,
                min_filter: None,
            }],
            skins: vec![],
            animations: vec![],
            idle_clip: String::new(),
            walk_clip: String::new(),
        };
        let first = publish_bundle(&project, bundle.clone(), &manifest, false).unwrap();
        let path = project.content_root().join(&first.asset_path);
        let bytes = std::fs::read(&path).unwrap();
        let old_snapshot = snapshot(path.parent().unwrap());
        let mut changed = bundle;
        changed.revision = 2;
        changed.source_hash = "second".into();
        changed.primitives[0].positions[1][0] = 9.;
        changed.textures[0].rgba = vec![4, 5, 6, 255];
        let error = publish_bundle(
            &project,
            changed,
            &ModelManifest {
                revision: 2,
                ..manifest
            },
            true,
        )
        .unwrap_err();
        assert_eq!(error.code, "MODEL_PUBLISH_FAILED");
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(snapshot(path.parent().unwrap()), old_snapshot);
        assert_eq!(load_model(&project, &first.guid).unwrap().revision, 1);
        // Simulate process death immediately after old -> backup; next read must restore it.
        let tx_root = project.tmp_root().join("model-transactions");
        let stage = tx_root.join("crash.stage");
        let backup = tx_root.join("crash.backup");
        std::fs::create_dir_all(&stage).unwrap();
        write_sync(
            &tx_root.join("crash.json"),
            &serde_json::to_vec(&Journal {
                guid: first.guid.clone(),
                stage: "crash.stage".into(),
                backup: "crash.backup".into(),
            })
            .unwrap(),
        )
        .unwrap();
        std::fs::rename(path.parent().unwrap(), &backup).unwrap();
        assert_eq!(load_model(&project, &first.guid).unwrap().revision, 1);
        assert_eq!(snapshot(path.parent().unwrap()), old_snapshot);
        assert!(!backup.exists());
        assert!(!stage.exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
