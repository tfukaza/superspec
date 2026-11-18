//! `inspect <source.item.json>`: what an item definition generates for given control values,
//! without building a scene: part keys, derived values, attachments, features, pattern
//! anchors, decals and drag handles, in the item's own coordinates (meters, Z up).
use crate::arguments::Arguments;
use serde_json::{Value, json};
use std::{process::ExitCode, sync::Arc};
use superspec_engine::itemspec::definition::{self, ItemDiagnostic, Values};

fn failure(diagnostic: Value) -> ExitCode {
    eprintln!(
        "{}",
        json!({"schemaVersion": 1, "success": false, "diagnostics": [diagnostic]})
    );
    ExitCode::from(1)
}

fn message(source: &str, text: String) -> Value {
    json!({"code": "AUTHORING_INPUT", "source": source, "message": text})
}

pub fn inspect(arguments: &Arguments) -> ExitCode {
    let name = arguments.source.to_string_lossy().into_owned();
    let text = match std::fs::read_to_string(&arguments.source) {
        Ok(text) => text,
        Err(error) => return failure(message(&name, format!("Cannot read {name}: {error}"))),
    };
    let mut values = Values::new();
    if let Some(path) = &arguments.parameters {
        let parsed = std::fs::read_to_string(path)
            .map_err(|error| error.to_string())
            .and_then(|text| serde_json::from_str::<Values>(&text).map_err(|e| e.to_string()));
        match parsed {
            Ok(parsed) => values = parsed,
            Err(error) => {
                return failure(message(
                    &name,
                    format!("Cannot read parameter values {}: {error}", path.display()),
                ));
            }
        }
    }
    // Companion assets (an imported GLB) are read beside the definition.
    let directory = arguments
        .source
        .parent()
        .map(|parent| parent.to_path_buf())
        .unwrap_or_default();
    let assets = |asset: &str| -> Result<Arc<[u8]>, String> {
        std::fs::read(directory.join(asset))
            .map(Arc::from)
            .map_err(|error| format!("{asset}: {error}"))
    };
    let located = |mut error: ItemDiagnostic| {
        if error.source == name {
            definition::locate_diagnostic(&text, &mut error);
        }
        failure(serde_json::to_value(error).expect("finite diagnostic"))
    };
    let parsed = match definition::parse(&name, &text) {
        Ok(parsed) => parsed,
        Err(error) => return located(error),
    };
    // Sub-item entries are read the same way, and native parts use the shipped fixtures.
    let mut environment = definition::Environment::new(&assets)
        .with_natives(superspec_engine::worldspec::shipped::item_generators());
    let prepared = match definition::prepare_in(&parsed, &values, &mut environment, &|| false) {
        Ok(prepared) => prepared,
        Err(error) => return located(error),
    };
    let item = match prepared.build(&|| false) {
        Ok(item) => item,
        Err(error) => return located(error),
    };
    let geometry = &item.geometry;
    let mesh = |index: &usize| {
        let mesh = &geometry.meshes[*index];
        json!({"id": mesh.id, "slot": mesh.material_slot, "vertices": mesh.positions.len(), "triangles": mesh.indices.len() / 3})
    };
    let parts: Vec<Value> = if geometry.parts.is_empty() {
        vec![json!({"id": "item", "key": item.geometry_key,
            "meshes": geometry.nodes.iter().flat_map(|node| node.meshes.iter().map(mesh)).collect::<Vec<_>>()})]
    } else {
        geometry
            .parts
            .iter()
            .map(|part| json!({"id": part.id, "key": part.geometry_key, "meshes": part.meshes.iter().map(mesh).collect::<Vec<_>>()}))
            .collect()
    };
    println!(
        "{}",
        json!({
            "schemaVersion": 1, "success": true, "item": item.id, "geometryKey": item.geometry_key,
            "parameters": prepared.parameters, "derived": prepared.derived, "bounds": geometry.bounds,
            "parts": parts, "attachments": item.attachments, "features": geometry.features,
            "anchors": geometry.anchors, "decals": item.decals.iter().map(|decal| json!({
                "id": decal.id, "part": decal.part, "face": decal.face, "image": decal.image, "layer": decal.layer,
                "size": decal.size, "offset": decal.offset, "corners": decal.corners})).collect::<Vec<_>>(),
            "handles": item.handles,
        })
    );
    ExitCode::SUCCESS
}
