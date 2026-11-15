//! Checks source ownership and Cargo dependencies without compiling product code.
mod graph;
mod rules;
mod rust;

use std::{collections::BTreeSet, path::PathBuf};

fn run() -> Result<(), String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let packages = graph::workspace(&root)?;
    let mut failures = BTreeSet::new();
    let mut sources = rust::Sources::default();
    let mut scanned = BTreeSet::new();
    for package in &packages {
        for dependency in &package.dependencies {
            if let Some(rule) = rules::crate_edge(&package.name, &dependency.package) {
                failures.insert(format!(
                    "{}: {} -> {}: {rule}",
                    package.manifest.display(),
                    package.name,
                    dependency.package
                ));
            }
        }
        if let Some(library) = &package.library {
            if let Some(library) = graph::product_library(&root, library)? {
                sources.load_crate(&package.name, &library, &package.dependencies)?;
                scanned.insert(package.name.as_str());
            }
        }
    }
    for expected in [
        "superspec-types",
        "superspec-geometry",
        "superspec-catalog",
        "superspec-model",
        "superspec-aml",
        "superspec-profiles",
        "superspec-engine",
        "superspec-export",
        "superspec-wasm",
    ] {
        if !scanned.contains(expected) {
            return Err(format!(
                "Required production library {expected} was not scanned under crates/. Check Cargo metadata and source ownership."
            ));
        }
    }
    failures.extend(sources.violations());
    if failures.is_empty() {
        println!(
            "Architecture boundaries passed ({} Rust modules and all normal workspace dependency edges).",
            sources.len()
        );
        Ok(())
    } else {
        Err(failures.into_iter().collect::<Vec<_>>().join("\n"))
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Architecture check failed:\n{error}");
        std::process::exit(1);
    }
}
