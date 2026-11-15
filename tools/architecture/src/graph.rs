use serde_json::Value;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone, Debug)]
pub struct Dependency {
    pub alias: String,
    pub package: String,
}

pub struct Package {
    pub name: String,
    pub manifest: PathBuf,
    pub library: Option<PathBuf>,
    pub dependencies: Vec<Dependency>,
}

/// Both paths use the filesystem's canonical form, including Windows verbatim prefixes.
pub fn product_library(root: &Path, library: &Path) -> Result<Option<PathBuf>, String> {
    let crates = root
        .join("crates")
        .canonicalize()
        .map_err(|error| format!("Cannot resolve production crate root: {error}"))?;
    let library = library
        .canonicalize()
        .map_err(|error| format!("Cannot resolve library {}: {error}", library.display()))?;
    Ok(library.starts_with(crates).then_some(library))
}

/// Cargo supplies target-specific edges and renamed dependency identities.
pub fn workspace(root: &Path) -> Result<Vec<Package>, String> {
    let output = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .args(["metadata", "--format-version", "1", "--no-deps", "--locked"])
        .current_dir(root)
        .output()
        .map_err(|error| format!("Cannot read Cargo metadata: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    let json: Value = serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())?;
    let packages = json["packages"]
        .as_array()
        .ok_or("Cargo metadata has no packages.")?;
    let members: BTreeSet<_> = json["workspace_members"]
        .as_array()
        .ok_or("Cargo metadata has no workspace members.")?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let names: BTreeSet<_> = packages
        .iter()
        .filter(|p| members.contains(p["id"].as_str().unwrap_or("")))
        .filter_map(|p| p["name"].as_str())
        .collect();
    packages
        .iter()
        .filter(|p| names.contains(p["name"].as_str().unwrap_or("")))
        .map(|p| {
            let name = p["name"]
                .as_str()
                .ok_or("Package name missing.")?
                .to_owned();
            let dependencies = p["dependencies"]
                .as_array()
                .ok_or("Dependencies missing.")?
                .iter()
                .filter(|d| d["kind"].is_null() && names.contains(d["name"].as_str().unwrap_or("")))
                .map(|d| Dependency {
                    package: d["name"].as_str().unwrap().to_owned(),
                    alias: d["rename"]
                        .as_str()
                        .unwrap_or(d["name"].as_str().unwrap())
                        .replace('-', "_"),
                })
                .collect();
            let library = p["targets"]
                .as_array()
                .ok_or("Targets missing.")?
                .iter()
                .find(|t| {
                    t["kind"].as_array().is_some_and(|kinds| {
                        kinds
                            .iter()
                            .any(|kind| matches!(kind.as_str(), Some("lib" | "rlib" | "cdylib")))
                    })
                })
                .and_then(|t| t["src_path"].as_str())
                .map(PathBuf::from);
            Ok(Package {
                name,
                manifest: PathBuf::from(p["manifest_path"].as_str().ok_or("Manifest missing.")?),
                library,
                dependencies,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_ownership_canonicalizes_both_sides_and_excludes_harnesses() {
        let root = std::env::temp_dir().join(format!(
            "superspec-architecture-root-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join("crates/sample/src")).unwrap();
        std::fs::create_dir_all(root.join("tests/acceptance")).unwrap();
        let library = root.join("crates/sample/src/lib.rs");
        std::fs::write(&library, "").unwrap();
        let harness = root.join("tests/acceptance/lib.rs");
        std::fs::write(&harness, "").unwrap();
        let canonical_root = root.canonicalize().unwrap();
        assert_eq!(
            product_library(&canonical_root, &library).unwrap(),
            Some(library.canonicalize().unwrap())
        );
        assert_eq!(
            product_library(&root, &root.join("crates/sample/src/../src/lib.rs")).unwrap(),
            Some(library.canonicalize().unwrap())
        );
        assert!(
            product_library(&canonical_root, &harness)
                .unwrap()
                .is_none()
        );
        assert!(product_library(&canonical_root, &root.join("missing.rs")).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
