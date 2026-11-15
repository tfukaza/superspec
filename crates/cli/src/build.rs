//! Staged publication preserves previous artifacts and unrelated user files on failure.
use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};
use superspec_engine::Scene;
const MANAGED: &str = ".superspec-artifacts.json";

struct Staging {
    directory: PathBuf,
    files: Vec<PathBuf>,
    preserve: bool,
}
impl Staging {
    fn write(&mut self, name: &str, bytes: &[u8]) -> Result<(), String> {
        let path = self.directory.join(name);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| e.to_string())?;
        self.files.push(path);
        file.write_all(bytes).map_err(|e| e.to_string())
    }
}
impl Drop for Staging {
    fn drop(&mut self) {
        if self.preserve {
            return;
        }
        for file in self.files.iter().rev() {
            let _ = fs::remove_file(file);
        }
        let _ = fs::remove_dir_all(&self.directory);
    }
}
struct Lock(PathBuf);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn check(cancelled: &dyn Fn() -> bool) -> Result<(), String> {
    if cancelled() {
        Err("Build cancelled.".into())
    } else {
        Ok(())
    }
}

/// Only artifacts recorded by a prior successful build may be replaced.
pub fn publish(
    scene: &Scene,
    output: &Path,
    textures: superspec_export::Textures,
    blender: Option<&Path>,
    quake: Option<&crate::external::QuakeOptions>,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<Vec<PathBuf>, String> {
    let parent = output.parent().ok_or("Output needs a parent directory.")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let directory = parent.join(format!(".superspec-build-{}-{stamp}", std::process::id()));
    fs::create_dir(&directory).map_err(|e| e.to_string())?;
    let mut staging = Staging {
        directory,
        files: vec![],
        preserve: false,
    };
    let options = superspec_export::Options {
        cancelled,
        textures,
        ..Default::default()
    };
    check(cancelled)?;
    eprintln!("Writing scene packet…");
    staging.write("scene.lsp", &scene.packet())?;
    eprintln!("Exporting portable models…");
    let glb = superspec_export::glb(scene, &options)?;
    let (gltf, bin) = superspec_export::gltf_files(&glb)?;
    staging.write("scene.glb", &glb)?;
    if let Some(quake) = quake {
        if fs::metadata(&quake.palette)
            .map_err(|e| e.to_string())?
            .len()
            != 768
        {
            return Err("Quake palette must contain exactly 768 RGB bytes.".into());
        }
        let palette =
            fs::read(&quake.palette).map_err(|e| format!("Cannot read Quake palette: {e}"))?;
        let files = superspec_export::quake::encode(scene, &palette, &options)?;
        staging.write("scene.map", &files.map)?;
        staging.write("scene.wad", &files.wad)?;
        crate::external::quake(quake, &staging.directory, cancelled)?;
        let mut report = serde_json::to_value(&files.report).map_err(|e| e.to_string())?;
        let mut warnings = std::collections::BTreeMap::new();
        for tool in ["qbsp", "vis", "light"] {
            let log = fs::read_to_string(staging.directory.join(format!("scene.{tool}.log")))
                .map_err(|e| e.to_string())?;
            let lines: Vec<_> = log
                .lines()
                .filter(|line| {
                    line.to_ascii_uppercase().contains("WARNING")
                        || line.to_ascii_uppercase().contains("ERROR")
                })
                .collect();
            warnings.insert(
                tool,
                lines.into_iter().map(str::to_owned).collect::<Vec<_>>(),
            );
        }
        report["compilerWarnings"] = serde_json::to_value(warnings).map_err(|e| e.to_string())?;
        staging.write(
            "scene.bsp.json",
            &serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
        )?;
        for name in [
            "scene.bsp",
            "scene.lit",
            "scene.qbsp.log",
            "scene.vis.log",
            "scene.light.log",
        ] {
            let path = staging.directory.join(name);
            if path.is_file() {
                staging.files.push(path);
            }
        }
    }
    if let Some(blender) = blender {
        staging.write("fbx-convert.py", include_bytes!("../assets/convert_fbx.py"))?;
        staging.files.push(staging.directory.join("scene.fbx"));
        staging.files.push(staging.directory.join("fbx.log"));
        crate::external::fbx(blender, &staging.directory, cancelled)?;
        let script = staging.directory.join("fbx-convert.py");
        fs::remove_file(&script).map_err(|e| e.to_string())?;
        staging.files.retain(|p| p != &script);
    }
    staging.write("scene.gltf", &gltf)?;
    staging.write("scene.bin", &bin)?;
    staging.write(
        "collision.glb",
        &superspec_export::glb(
            scene,
            &superspec_export::Options {
                purpose: superspec_export::Purpose::Collision,
                cancelled,
                ..Default::default()
            },
        )?,
    )?;
    check(cancelled)?;
    staging.write("scene.json", &superspec_export::runtime_json(scene)?)?;
    staging.write("scene.assets", &scene.asset_bytes)?;
    let metadata = scene.manifest.architecture.as_ref().map(|c| &c.metadata);
    let markers=metadata.into_iter().flatten().filter(|n|n.kind=="marker").map(|n|{
        let [x,y,z]=n.vector("position",[0.;3]);
        serde_json::json!({"id":n.id,"kind":n.text("kind"),"team":n.text("team"),"label":n.text("label"),"region":n.text("element"),"position":[x,z,-y]})
    }).collect::<Vec<_>>();
    let player = scene
        .manifest
        .validation
        .as_ref()
        .map(|v| v.agent)
        .unwrap_or_default();
    staging.write("game.json",&serde_json::to_vec_pretty(&serde_json::json!({"id":scene.manifest.id,"name":scene.manifest.name,"coordinateSystem":"glTF Y-up metres","player":player,"markers":markers,"visual":"scene.glb","collision":"collision.glb"})).map_err(|e|e.to_string())?)?;
    staging.write(
        "manifest.json",
        &serde_json::to_vec_pretty(&scene.manifest).map_err(|e| e.to_string())?,
    )?;
    let names: Vec<String> = staging
        .files
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    staging.write(
        MANAGED,
        &serde_json::to_vec(&names).map_err(|e| e.to_string())?,
    )?;
    check(cancelled)?;
    if output
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink() || !m.is_dir())
    {
        return Err("Output must be an ordinary directory.".into());
    }
    fs::create_dir_all(output).map_err(|e| e.to_string())?;
    let lock_path = output.join(".superspec-build.lock");
    let _lock_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)
        .map_err(|e| format!("Cannot claim output directory: {e}"))?;
    let _lock = Lock(lock_path);
    let old: Vec<String> = match fs::read(output.join(MANAGED)) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|e| format!("Invalid managed artifact list: {e}"))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let previous = fs::read(output.join("manifest.json"))
                .ok()
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
            if previous.is_some_and(|m| {
                m["compiler"] == scene.manifest.compiler
                    && m["id"] == scene.manifest.id
                    && m["schemaVersion"] == 3
            }) {
                ["scene.lsp", "scene.glb", "collision.glb", "manifest.json"]
                    .into_iter()
                    .filter(|name| output.join(name).is_file())
                    .map(str::to_owned)
                    .collect()
            } else {
                vec![]
            }
        }
        Err(e) => return Err(e.to_string()),
    };
    if old.iter().any(|name| {
        Path::new(name).file_name().and_then(|s| s.to_str()) != Some(name.as_str())
            || name == "."
            || name == ".."
    }) {
        return Err("Invalid managed artifact name.".into());
    }
    for name in &names {
        if output.join(name).symlink_metadata().is_ok() && !old.contains(name) {
            return Err(format!("Output contains an unmanaged file named {name}."));
        }
    }
    let mut replace: BTreeSet<String> = old.into_iter().chain(names.iter().cloned()).collect();
    replace.insert(MANAGED.into());
    let mut backups = vec![];
    let mut written = vec![];
    let commit = (|| -> Result<(), String> {
        for (i, name) in replace.into_iter().enumerate() {
            check(cancelled)?;
            let path = output.join(&name);
            if path.symlink_metadata().is_ok() {
                let backup = staging.directory.join(format!("backup-{i}"));
                fs::rename(&path, &backup).map_err(|e| e.to_string())?;
                staging.files.push(backup.clone());
                backups.push((path, backup));
            }
        }
        for name in names
            .iter()
            .map(String::as_str)
            .chain(std::iter::once(MANAGED))
        {
            check(cancelled)?;
            let target = output.join(name);
            fs::rename(staging.directory.join(name), &target).map_err(|e| e.to_string())?;
            written.push(target);
        }
        Ok(())
    })();
    if let Err(error) = commit {
        for path in written.iter().rev() {
            let _ = fs::remove_file(path);
        }
        let mut recovery = vec![];
        for (path, backup) in backups.iter().rev() {
            if let Err(e) = fs::rename(backup, path) {
                recovery.push(e.to_string());
                staging.files.retain(|p| p != backup);
            }
        }
        if !recovery.is_empty() {
            staging.preserve = true;
            return Err(format!(
                "{error}; recovery files retained at {}: {}",
                staging.directory.display(),
                recovery.join(", ")
            ));
        }
        return Err(error);
    }
    Ok(names.into_iter().map(|name| output.join(name)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_during_replacement_restores_previous_artifacts() {
        let scene = superspec_engine::compile(r#"<architecture version="1" id="publish"><box id="box" width="1" depth="1" height="1"/></architecture>"#, 0).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("output");
        publish(
            &scene,
            &output,
            superspec_export::Textures::Png,
            None,
            None,
            &|| false,
        )
        .unwrap();
        fs::write(output.join("user-note.txt"), "keep").unwrap();
        let before = fs::read(output.join("scene.glb")).unwrap();
        let result = publish(
            &scene,
            &output,
            superspec_export::Textures::Png,
            None,
            None,
            &|| !output.join("scene.lsp").exists(),
        );
        assert!(result.unwrap_err().contains("cancelled"));
        assert_eq!(fs::read(output.join("scene.glb")).unwrap(), before);
        assert_eq!(
            fs::read_to_string(output.join("user-note.txt")).unwrap(),
            "keep"
        );
        publish(
            &scene,
            &output,
            superspec_export::Textures::Png,
            None,
            None,
            &|| false,
        )
        .unwrap();
    }
}
