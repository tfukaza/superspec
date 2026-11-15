use super::*;
use sha2::{Digest, Sha256};
use superspec_aml::architecture::Value;

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
struct WorkDirectory(PathBuf);
impl Drop for WorkDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(super) fn run(arguments: &Arguments, result: &mut CommandResult) -> Result<(), String> {
    let input = path(&arguments.paths[0])?;
    result.source = Some(input.clone());
    let output = path(
        arguments
            .values
            .get("--out")
            .ok_or("Bake requires --out <directory>.")?,
    )?;
    let executable = arguments
        .values
        .get("--blender")
        .cloned()
        .or_else(|| std::env::var_os("BLENDER_PATH"))
        .unwrap_or_else(|| "blender".into());
    let samples = arguments.integer("--samples", 64)?;
    if !(1..=16384).contains(&samples) {
        return Err("Bake samples must be 1..16384.".into());
    }
    let source = read_source(&input).map_err(|e| e.to_string())?;
    let parent = input.parent().unwrap();
    let mut scene = superspec_engine::Session::default()
        .compile_cancellable(
            &source,
            0,
            &|name| assets::read(parent, name),
            true,
            &cancelled,
        )
        .map_err(|e| e.to_string())?;
    superspec_engine::validation::audit(&mut scene, &cancelled)?;
    result.diagnostics = scene.manifest.diagnostics.clone();
    if !scene.manifest.success {
        return Err("Lighting bake requires validated geometry.".into());
    }
    let resolution =
        u32::try_from(arguments.integer("--resolution", 1024)?).map_err(|e| e.to_string())?;
    let scene = superspec_export::prepare_lightmap_sized(&scene, resolution, &cancelled)?;
    let atlas = scene
        .manifest
        .lightmap
        .as_ref()
        .ok_or("Lighting bake requires a visible receiver mesh.")?;
    let parent = output
        .parent()
        .ok_or("Bake output needs a parent directory.")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let working = parent.join(format!(
        ".superspec-lighting-{}-{stamp}",
        std::process::id()
    ));
    std::fs::create_dir(&working).map_err(|e| e.to_string())?;
    let _work = WorkDirectory(working.clone());
    for (name, bytes) in [
        (
            "bake.py",
            include_bytes!("../../assets/lighting/bake.py").as_slice(),
        ),
        (
            "calibrate.py",
            include_bytes!("../../assets/lighting/calibrate.py").as_slice(),
        ),
        (
            "lightmap_io.py",
            include_bytes!("../../assets/lighting/lightmap_io.py").as_slice(),
        ),
    ] {
        std::fs::write(working.join(name), bytes).map_err(|e| e.to_string())?;
    }
    let context = scene
        .manifest
        .architecture
        .as_ref()
        .ok_or("Missing architectural lighting metadata.")?;
    let mut indices = vec![];
    let mut surfaces = vec![];
    let mut parts = vec![];
    let mut atlas_pages = vec![];
    for (i, m) in scene.manifest.meshes.iter().enumerate() {
        let dynamic = context
            .objects
            .iter()
            .find(|o| o.id == m.object_id)
            .and_then(|o| o.attributes.get("dynamic"))
            .is_some_and(|v| matches!(v, Value::Boolean(true)));
        indices.extend(
            scene.indices[m.index_start..m.index_start + m.index_count]
                .iter()
                .map(|j| *j + m.vertex_start as u32),
        );
        surfaces.extend(std::iter::repeat_n(i, m.index_count / 3));
        atlas_pages.extend(std::iter::repeat_n(
            if m.visible && !dynamic { 0 } else { -1 },
            m.index_count / 3,
        ));
        parts.push(serde_json::json!({"id":m.id,"kind":"architecture","layer":m.object_id,"visible":m.visible,"dynamic":dynamic,"triangles":(m.index_start/3..(m.index_start+m.index_count)/3).collect::<Vec<_>>()}));
    }
    let mut resolved = BTreeMap::new();
    let mut material_assets = BTreeMap::new();
    for asset in &scene.manifest.assets {
        let bytes = &scene.asset_bytes[asset.byte_start..asset.byte_start + asset.byte_length];
        let hash = digest(bytes);
        let file = working.join(format!(
            "{hash}.{}",
            if asset.mime == "image/png" {
                "png"
            } else {
                "jpg"
            }
        ));
        if !file.exists() {
            std::fs::write(&file, bytes).map_err(|e| e.to_string())?;
        }
        resolved.insert(asset.id.clone(), file);
        material_assets.insert(asset.id.clone(), hash);
    }
    let sky=context.metadata.iter().find(|n|n.kind=="sky").map(|n|serde_json::json!({"color":n.text("color").unwrap_or("#dce8f5"),"intensity":n.number("intensity",0.5)})).unwrap_or(serde_json::json!({"color":"#dce8f5","intensity":0.5}));
    let lights=context.metadata.iter().filter(|n|n.kind=="light").map(|n|serde_json::json!({"id":n.id,"kind":n.text("kind").unwrap_or("point"),"position":n.vector("position",[0.;3]),"target":n.vector("target",[0.;3]),"color":n.text("color").unwrap_or("#ffffff"),"intensity":n.number("intensity",1.),"angle":n.number("angle",std::f64::consts::FRAC_PI_4)})).collect::<Vec<_>>();
    let state = digest(&scene.packet());
    let uv_hash = digest(&serde_json::to_vec(&scene.lightmap_uvs).map_err(|e| e.to_string())?);
    let payload = serde_json::json!({"id":"scene","geometryHash":state,"lightingStateHash":state,"lightingRevision":digest(format!("{state}/{samples}/17").as_bytes()),"compilerRevision":"superspec-rust-v1","rendererRevision":"cycles",
        "mesh":{"positions":scene.positions,"normals":scene.normals,"uv":scene.uvs,"uv1":scene.lightmap_uvs,"indices":indices,"surfaces":surfaces,"atlasPages":atlas_pages,"chartIds":scene.lightmap_charts},
        "meshes":parts,"surfaces":scene.manifest.meshes.iter().map(|m|serde_json::json!({"material":m.material})).collect::<Vec<_>>(),"materials":scene.manifest.materials,"sky":sky,"lights":lights,"resolvedImages":resolved,"materialAssets":material_assets,
        "atlas":{"pages":[{"id":0,"width":atlas.width,"height":atlas.height}],"uvHash":uv_hash,"padding":2,"safeMip":1}});
    std::fs::write(
        working.join("input.json"),
        serde_json::to_vec(&payload).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    eprintln!("Calibrating the configured Blender/Cycles installation…");
    external::blender_script(
        Path::new(&executable),
        &working,
        "calibrate.py",
        &[
            "--output-dir".into(),
            working.join("calibration").into_os_string(),
        ],
        &cancelled,
    )?;
    eprintln!("Baking lightmap transport…");
    external::blender_script(
        Path::new(&executable),
        &working,
        "bake.py",
        &[
            "--input".into(),
            working.join("input.json").into_os_string(),
            "--output".into(),
            working.join("result").into_os_string(),
            "--calibration".into(),
            working.join("calibration/results.json").into_os_string(),
            "--samples".into(),
            samples.to_string().into(),
        ],
        &cancelled,
    )?;
    let mut files = vec![];
    for entry in std::fs::read_dir(working.join("result")).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_type().map_err(|e| e.to_string())?.is_file() {
            files.push((
                output.join(entry.file_name()),
                std::fs::read(entry.path()).map_err(|e| e.to_string())?,
            ));
        }
    }
    files.push((
        output.join("scene.glb"),
        superspec_export::glb(
            &scene,
            &superspec_export::Options {
                cancelled: &cancelled,
                ..Default::default()
            },
        )?,
    ));
    result.artifacts = publish_files(files)?;
    result.report = Some(
        serde_json::json!({"samples":samples,"width":atlas.width,"height":atlas.height,"stateHash":state}),
    );
    Ok(())
}
