//! Native delivery for TreeSpec definitions. Grass receives explicit authored ground.
use serde_json::json;
use std::{fs::File, io::Read, path::Path};
use superspec_engine::{
    Scene,
    treespec::{self, batches, grass, tree},
};

pub fn read(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(limit as u64 + 1).read_to_end(&mut bytes))
        .map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    if bytes.len() > limit {
        return Err(format!("{} exceeds its size allowance.", path.display()));
    }
    Ok(bytes)
}

pub fn scene(
    name: &str,
    bytes: &[u8],
    site: Option<&[u8]>,
    revision: u32,
    cancelled: &dyn Fn() -> bool,
) -> Result<Scene, String> {
    if bytes.len() > treespec::MAX_DEFINITION_BYTES {
        return Err("TreeSpec definition exceeds its size allowance.".into());
    }
    let source = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
    // Each generator supplies its patch identity and all authored LOD distances.
    let (id, levels) = if name.ends_with(".grass.json") {
        let def = grass::parse(name, source).map_err(|e| e.to_string())?;
        let site: grass::GrassSite = serde_json::from_slice(
            site.ok_or("Grass generation requires a ground site (--site).")?,
        )
        .map_err(|e| format!("Invalid grass site: {e}"))?;
        let patch = grass::generate(&def, &site, cancelled).map_err(|e| e.to_string())?;
        let look = serde_json::from_value(json!({"family":"grass","patch":patch.id,"far":1000}))
            .map_err(|e| e.to_string())?;
        (
            patch.id.clone(),
            batches::grass_levels(&patch, [0.; 3], &look, None)?,
        )
    } else {
        let def = tree::parse(name, source).map_err(|e| e.to_string())?;
        let plant = tree::generate(&def, cancelled).map_err(|e| e.to_string())?;
        let look = serde_json::from_value(
            json!({"family":"leaves","patch":plant.id,"far":1000,"canopy":{}}),
        )
        .map_err(|e| e.to_string())?;
        (
            plant.id.clone(),
            batches::tree_levels(&plant, [0.; 3], &look)?,
        )
    };
    let mut scene = Scene::empty(revision, "superspec-treespec");
    scene.manifest.name = id.clone();
    scene.manifest.id = id;
    for level in levels {
        if cancelled() {
            return Err("Foliage generation cancelled.".into());
        }
        scene.add_batch(&level.batch, &level.mesh)?;
    }
    scene.project_foliage_lods()?;
    scene.manifest.success = true;
    Ok(scene)
}
pub fn deliver(
    scene: Scene,
    action: &str,
    textures: superspec_export::Textures,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<u8>, String> {
    match action {
        "glb" | "glb-png" => superspec_export::glb(&scene, &superspec_export::Options{textures, cancelled, ..Default::default()}),
        "scene" => Ok(scene.packet()),
        "check" => Ok(json!({"schemaVersion":1,"success":true,"id":scene.manifest.id,"batches":scene.manifest.batches.len(),"diagnostics":scene.manifest.diagnostics}).to_string().into_bytes()),
        _ => Err("Unsupported foliage operation.".into()),
    }
}
