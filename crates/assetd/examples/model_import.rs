//! Reproducible model smoke: cargo run -p assetd --example model_import -- GLB MANIFEST PROJECT
use assetd::model::{import_model_bundle, load_model, ModelManifest};
use assetd::project::ForgeProject;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err("usage: model_import <glb/gltf> <manifest.json> <project-root>".into());
    }
    let manifest: ModelManifest = serde_json::from_slice(&std::fs::read(PathBuf::from(&args[1]))?)?;
    let project = ForgeProject::load(PathBuf::from(&args[2]))?;
    let published = import_model_bundle(&project, &PathBuf::from(&args[0]), &manifest)?;
    let bundle = load_model(&project, &published.guid)?;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"published":published,"primitives":bundle.primitives.len(),"nodes":bundle.nodes.len(),"materials":bundle.materials.len(),"textures":bundle.textures.len(),"skins":bundle.skins.len(),"clips":bundle.animations.iter().map(|a|&a.name).collect::<Vec<_>>(),"sourceHash":bundle.source_hash})
        )?
    );
    Ok(())
}
