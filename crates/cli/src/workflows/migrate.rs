use super::*;
use sha2::{Digest, Sha256};
use std::cell::RefCell;

const COMPANIONS: [&str; 6] = [
    "src",
    "heightmap",
    "texture",
    "normal-texture",
    "metallic-roughness-texture",
    "height-texture",
];

fn replace_paths(node: &mut superspec_aml::architecture::Node, paths: &BTreeMap<String, String>) {
    for key in COMPANIONS {
        if let Some(next) = node.text(key).and_then(|name| paths.get(name)).cloned() {
            node.attributes
                .insert(key.into(), superspec_aml::architecture::Value::Text(next));
        }
    }
    for child in &mut node.children {
        replace_paths(child, paths);
    }
}

pub(super) fn run(arguments: &Arguments, result: &mut CommandResult) -> Result<(), String> {
    let input = path(&arguments.paths[0])?;
    let output = path(
        arguments
            .values
            .get("--out")
            .ok_or("Migrate requires --out <new-project-directory>.")?,
    )?;
    result.source = Some(input.clone());
    let source = read_source(&input).map_err(|e| e.to_string())?;
    let parent = input.parent().ok_or("Migration source has no parent.")?;
    let companions = RefCell::new(BTreeMap::new());
    let read = |name: &str| -> Result<Vec<u8>, String> {
        if let Some(bytes) = companions.borrow().get(name) {
            return Ok(Vec::clone(bytes));
        }
        let bytes = assets::read(parent, name)?;
        if !name.starts_with("superspec-material:") {
            companions
                .borrow_mut()
                .insert(name.to_owned(), bytes.clone());
        }
        Ok(bytes)
    };
    let aml = superspec_aml::migration::migrate(&source, &cancelled, &|name| {
        let geometry = superspec_engine::itemspec::imports::glb(&read(name)?, &cancelled)?;
        Ok(superspec_aml::migration::ImportedGeometry {
            points: geometry.points,
            triangles: geometry.triangles,
        })
    })?;
    let mut document = superspec_profiles::parse(&aml).map_err(|e| e.to_string())?;
    let mut pending = vec![&document.root];
    while let Some(node) = pending.pop() {
        for key in COMPANIONS {
            if let Some(name) = node.text(key) {
                read(name)?;
            }
        }
        pending.extend(&node.children);
    }
    let mut replacements = BTreeMap::new();
    let mut files = vec![];
    for (name, bytes) in companions.into_inner() {
        let source = Path::new(&name);
        let safe = source
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)));
        let target = if safe {
            name.replace('\\', "/")
        } else {
            let extension = source
                .extension()
                .and_then(|s| s.to_str())
                .filter(|s| s.chars().all(|c| c.is_ascii_alphanumeric()))
                .unwrap_or("bin");
            format!("assets/{:x}.{extension}", Sha256::digest(&bytes))
        };
        replacements.insert(name, target.clone());
        if !files.iter().any(|(name, _)| name == &target) {
            files.push((target, bytes));
        }
    }
    for node in document
        .declarations
        .values_mut()
        .chain(document.elements.iter_mut())
        .chain(document.metadata.iter_mut())
    {
        replace_paths(node, &replacements);
    }
    let aml = superspec_profiles::serialize(&document)?;
    result.artifacts =
        project::initialize_source(&output, &aml, &files).map_err(|e| e.to_string())?;
    result.project = Some(output);
    result.report = Some(
        serde_json::json!({"sourceFormat":"SVGX 2","outputFormat":"AML 1","companions":files.len()}),
    );
    Ok(())
}
