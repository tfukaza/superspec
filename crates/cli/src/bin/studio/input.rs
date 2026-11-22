use super::arguments::Arguments;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};
use superspec_engine::authoring::Project;

use superspec_types::limits::{MAX_COMPANION_BYTES as MAX_ASSETS, MAX_FILE_BYTES};
pub struct Input {
    pub project: Project,
    /// The project's optional lighting record, which the engine's project ignores; a GLB export
    /// delivers it.
    pub lighting: Option<superspec_export::Lighting>,
    /// The project's optional instanced batches, which the engine's project ignores too; they are
    /// added to the scene a `scene` or `glb` command delivers.
    pub batches: Vec<superspec_export::ProjectBatch>,
    parent: PathBuf,
    raw_recipe: bool,
    embedded: Option<BTreeMap<String, Arc<[u8]>>>,
}

fn read(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(limit as u64 + 1).read_to_end(&mut bytes))
        .map_err(|error| format!("Cannot read {}: {error}", path.display()))?;
    if bytes.len() > limit {
        return Err(format!("{} exceeds its file allowance.", path.display()));
    }
    Ok(bytes)
}

pub fn source_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.ends_with(".ms") || name.ends_with(".material.ts") || name.ends_with(".item.json")
        })
}

/// Raw-source mode admits bounded siblings, without following links or scanning generated dependency trees.
pub fn modules(root: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut result = BTreeMap::new();
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    let mut entries = 0usize;
    let mut bytes = 0usize;
    while let Some((directory, depth)) = pending.pop() {
        if depth > 32 {
            return Err("Source folder exceeds its directory depth allowance.".into());
        }
        for entry in fs::read_dir(&directory).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            entries += 1;
            if entries > 8192 {
                return Err(
                    "Source folder contains too many entries; use a focused project folder.".into(),
                );
            }
            let kind = entry.file_type().map_err(|error| error.to_string())?;
            if kind.is_symlink() {
                continue;
            }
            let path = entry.path();
            if kind.is_dir() {
                if !["generated", "node_modules", ".git", "target", ".cache"]
                    .iter()
                    .any(|name| entry.file_name() == *name)
                {
                    pending.push((path, depth + 1));
                }
            } else if kind.is_file() && source_name(&path) {
                let source = read(&path, 1024 * 1024)?;
                bytes += source.len();
                if result.len() >= 128 || bytes > 8 * 1024 * 1024 {
                    return Err("Source modules exceed count or byte allowances.".into());
                }
                let name = path
                    .strip_prefix(root)
                    .map_err(|error| error.to_string())?
                    .to_str()
                    .ok_or("Source paths must be UTF-8.")?
                    .replace('\\', "/");
                result.insert(
                    name,
                    String::from_utf8(source).map_err(|_| "Authoring source must be UTF-8.")?,
                );
            }
        }
    }
    Ok(result)
}

fn companions(value: &Value) -> Result<BTreeMap<String, Arc<[u8]>>, String> {
    let values = value
        .as_object()
        .ok_or("A studio bundle must contain an assets object.")?;
    if values.len() > superspec_types::limits::MAX_COMPANIONS {
        return Err("Bundle contains too many companions.".into());
    }
    let mut result = BTreeMap::new();
    let mut total = 0usize;
    for (name, value) in values {
        let encoded = value
            .as_str()
            .ok_or("Bundled companions must be base64 strings.")?;
        if encoded.len() > MAX_FILE_BYTES.div_ceil(3) * 4 {
            return Err(format!("Companion {name} exceeds its byte allowance."));
        }
        let bytes = STANDARD
            .decode(encoded)
            .map_err(|error| format!("Invalid base64 companion {name}: {error}"))?;
        total += bytes.len();
        if bytes.len() > MAX_FILE_BYTES || total > MAX_ASSETS {
            return Err("Bundle companions exceed their byte allowance.".into());
        }
        result.insert(name.clone(), Arc::from(bytes));
    }
    Ok(result)
}

fn validate_bundle(value: &Value) -> Result<(), String> {
    // Version 2 adds scene `models` and `materials`, which Rust validates when it builds them.
    if !(value["schemaVersion"] == 1 || value["schemaVersion"] == 2)
        || !matches!(value["kind"].as_str(), Some("material" | "item" | "world"))
        || (value["kind"] == "world" && !value["world"].is_object())
        || !value["entry"].is_string()
        || !value["modules"].is_object()
        || !value["parameters"].is_object()
        || value["seed"]
            .as_u64()
            .is_none_or(|seed| seed > u32::MAX as u64)
        || value["resolution"].as_u64().is_none_or(|size| {
            let max = if value["profile"] == "production" {
                2048
            } else {
                512
            };
            !(16..=max).contains(&size)
        })
        || !(value["profile"].is_null()
            || matches!(value["profile"].as_str(), Some("preview" | "production")))
        || !value["inspection"].is_string()
        || !matches!(value["mapping"].as_str(), Some("uv" | "object" | "world"))
        || !matches!(
            value["base"]["kind"].as_str(),
            Some("swatch" | "aml" | "glb")
        )
        || !matches!(value["target"]["scope"].as_str(), Some("mesh" | "binding"))
        || !value["target"]["enabled"].is_boolean()
        || value["domain"].as_array().is_none_or(|domain| {
            domain.len() != 2
                || domain
                    .iter()
                    .any(|v| v.as_f64().is_none_or(|v| !v.is_finite() || v <= 0.))
        })
    {
        return Err("This is not a supported version 1 or 2 studio project.".into());
    }
    Ok(())
}

impl Input {
    pub fn open(arguments: &Arguments) -> Result<Self, String> {
        let path = arguments
            .source
            .canonicalize()
            .map_err(|error| format!("Cannot open {}: {error}", arguments.source.display()))?;
        let parent = path
            .parent()
            .ok_or("Input has no containing folder.")?
            .to_path_buf();
        let raw = source_name(&path);
        let raw_recipe = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".material.ts"));
        let mut value;
        let embedded;
        if raw {
            let entry = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or("Source filename must be UTF-8.")?;
            value = json!({"schemaVersion":1,"kind":if entry.ends_with(".item.json") {"item"} else {"material"},
                "entry":entry,"modules":modules(&parent)?,"parameters":{},"seed":0,"resolution":128,
                "inspection":"final","mapping":"object","domain":[1,1],"base":{"kind":"swatch"},
                "target":{"scope":"mesh","enabled":false}});
            embedded = None;
        } else {
            let bytes = read(&path, superspec_types::limits::MAX_PROJECT_FILE_BYTES)?;
            let bundle = Self::decode_bundle(&bytes, arguments.profile)?;
            value = bundle.0;
            embedded = Some(bundle.1);
        }
        if let Some(profile) = arguments.profile {
            value["profile"] = json!(profile);
        }
        if let Some(resolution) = arguments.resolution {
            value["resolution"] = json!(resolution);
        }
        if let Some(seed) = arguments.seed {
            value["seed"] = json!(seed);
        }
        if let Some(path) = &arguments.parameters {
            let parameters: Value = serde_json::from_slice(&read(path, 1024 * 1024)?)
                .map_err(|error| format!("Invalid parameter JSON: {error}"))?;
            let supplied = parameters
                .as_object()
                .ok_or("Parameter overrides must be a JSON object.")?;
            let target = value["parameters"]
                .as_object_mut()
                .ok_or("Project parameters must be an object.")?;
            target.extend(supplied.clone());
        }
        Self::from_parts(value, parent, raw_recipe, embedded)
    }

    fn decode_bundle(
        bytes: &[u8],
        profile: Option<superspec_engine::authoring::Profile>,
    ) -> Result<(Value, BTreeMap<String, Arc<[u8]>>), String> {
        let (mut value, archived) = if super::archive::is_archive(bytes) {
            let (value, assets) = super::archive::read(bytes)?;
            (value, Some(assets))
        } else {
            (
                serde_json::from_slice(bytes)
                    .map_err(|error| format!("Invalid studio project JSON: {error}"))?,
                None,
            )
        };
        if let Some(profile) = profile {
            value["profile"] = json!(profile);
        }
        validate_bundle(&value)?;
        let assets = match archived {
            Some(assets) => assets,
            None => companions(&value["assets"])?,
        };
        value
            .as_object_mut()
            .ok_or("Studio project must be an object.")?
            .remove("assets");
        Ok((value, assets))
    }

    pub fn uploaded(bytes: &[u8]) -> Result<Self, String> {
        let (value, assets) = Self::decode_bundle(bytes, None)?;
        Self::from_parts(value, PathBuf::new(), false, Some(assets))
    }

    pub fn with_companions((value, assets): super::archive::Contents) -> Result<Self, String> {
        validate_bundle(&value)?;
        Self::from_parts(value, PathBuf::new(), false, Some(assets))
    }

    fn from_parts(
        value: Value,
        parent: PathBuf,
        raw_recipe: bool,
        embedded: Option<BTreeMap<String, Arc<[u8]>>>,
    ) -> Result<Self, String> {
        let lighting = superspec_export::project_lighting(&value).map_err(|findings| {
            let findings: Vec<String> = findings.iter().map(ToString::to_string).collect();
            format!("Invalid lighting record: {}.", findings.join("; "))
        })?;
        let batches = superspec_export::project_batches(&value)?;
        let project = serde_json::from_value(value)
            .map_err(|error| format!("Invalid studio project: {error}"))?;
        Ok(Self {
            project,
            lighting,
            batches,
            parent,
            raw_recipe,
            embedded,
        })
    }

    /// The exact source snapshot admitted for the native build, including its canonical root.
    pub fn recipe_sources(&self) -> Result<(&Path, &BTreeMap<String, String>), String> {
        if !self.raw_recipe {
            return Err("Recipe tooling reads .material.ts files.".into());
        }
        Ok((&self.parent, &self.project.modules))
    }

    /// Bundled authored paths never fall back to the host filesystem. Catalog IDs use their explicit engine namespace.
    pub fn asset(&self, name: &str) -> Result<Arc<[u8]>, String> {
        if name.starts_with("superspec-material:") {
            return crate::assets::read_confined(&self.parent, name).map(Arc::from);
        }
        if let Some(embedded) = &self.embedded {
            return embedded
                .get(name)
                .cloned()
                .ok_or_else(|| format!("Missing bundled companion {name}."));
        }
        crate::assets::read_confined(&self.parent, name).map(Arc::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn checker_sources_retain_the_native_admission_snapshot_after_disk_edits() {
        let directory = tempfile::tempdir().unwrap();
        let entry = directory.path().join("main.material.ts");
        let helper = directory.path().join("helper.material.ts");
        fs::write(&entry, "original entry").unwrap();
        fs::write(&helper, "original helper").unwrap();
        let arguments = Arguments::parse([OsString::from("check"), entry.clone().into_os_string()])
            .unwrap()
            .unwrap();
        let admitted = Input::open(&arguments).unwrap();
        fs::write(&entry, "later entry").unwrap();
        fs::remove_file(&helper).unwrap();
        fs::write(directory.path().join("later.material.ts"), "new module").unwrap();
        let (parent, modules) = admitted.recipe_sources().unwrap();
        assert_eq!(parent, directory.path().canonicalize().unwrap());
        assert_eq!(
            modules,
            &BTreeMap::from([
                ("main.material.ts".into(), "original entry".into()),
                ("helper.material.ts".into(), "original helper".into())
            ])
        );
        let refreshed = Input::open(&arguments).unwrap();
        assert_eq!(
            refreshed.recipe_sources().unwrap().1["main.material.ts"],
            "later entry"
        );
        assert!(
            refreshed
                .recipe_sources()
                .unwrap()
                .1
                .contains_key("later.material.ts")
        );
        let ms = directory.path().join("main.ms");
        fs::write(&ms, "material 1;").unwrap();
        let arguments = Arguments::parse([OsString::from("check"), ms.into_os_string()])
            .unwrap()
            .unwrap();
        assert!(Input::open(&arguments).unwrap().recipe_sources().is_err());
    }
}
