//! Times what the engine does when a host opens the procedural building's scene project
//! (`tests/item-building-perf.mjs` measures the same project in the Studio): describing it for
//! a shading renderer and laying out its receivers' atlases, cold, again unchanged, and after
//! one control of the building changed.
//!
//! Write the project first (`node tools/perf/item-building-project.mjs <path>`), then run
//! `ITEM_BUILDING_PROJECT=<path> cargo test --release -p superspec-engine --test
//! item_building_bench -- --ignored --nocapture`.

use std::{collections::BTreeMap, sync::Arc, time::Instant};
use superspec_engine::authoring::{Project, Session};

fn base64(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len() / 4 * 3);
    let (mut bits, mut held) = (0u32, 0u32);
    for symbol in text.bytes() {
        let value = match symbol {
            b'A'..=b'Z' => symbol - b'A',
            b'a'..=b'z' => symbol - b'a' + 26,
            b'0'..=b'9' => symbol - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => continue,
        };
        held = held << 6 | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push((held >> bits) as u8);
        }
    }
    bytes
}

#[test]
#[ignore = "a timing, not a check: needs ITEM_BUILDING_PROJECT"]
fn bench_building_project() {
    let path = std::env::var("ITEM_BUILDING_PROJECT").expect("ITEM_BUILDING_PROJECT");
    let started = Instant::now();
    let text = std::fs::read_to_string(&path).unwrap();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    let assets: BTreeMap<String, Arc<[u8]>> = value["assets"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, bytes)| (name.clone(), Arc::from(base64(bytes.as_str().unwrap()))))
        .collect();
    let project: Project = serde_json::from_value(value).unwrap();
    println!(
        "read {:.1} MiB, parsed in {:.0} ms",
        text.len() as f64 / 1048576.,
        started.elapsed().as_secs_f64() * 1e3
    );
    let asset = |name: &str| {
        assets
            .get(name)
            .cloned()
            .ok_or_else(|| format!("missing asset {name}"))
    };
    let mut session = Session::default();
    let mut edited = project.clone();
    let building = edited
        .models
        .iter_mut()
        .find(|model| model.id == "building")
        .expect("the building");
    let seed = building.parameters.get("seed").and_then(|v| v.as_f64());
    building
        .parameters
        .insert("seed".into(), serde_json::json!(seed.unwrap_or(0.) + 1.));
    let mut revision = 0;
    for (label, project) in [
        ("cold", &project),
        ("unchanged", &project),
        ("edited seed", &edited),
        ("back", &project),
    ] {
        revision += 1;
        let started = Instant::now();
        let shaded = session
            .shade_project(project, revision, &asset, &|| false)
            .unwrap_or_else(|errors| panic!("{errors:?}"));
        let shade = started.elapsed().as_secs_f64() * 1e3;
        let encoded = serde_json::to_string(&shaded.metadata).unwrap().len();
        let started = Instant::now();
        let layouts = session
            .project_layouts(project, revision, &asset, &|| false)
            .unwrap_or_else(|errors| panic!("{errors:?}"));
        let layout = started.elapsed().as_secs_f64() * 1e3;
        let texels: usize = layouts.receivers.iter().map(|r| r.width * r.height).sum();
        println!(
            "{label}: shade_project {shade:.0} ms (metadata {:.1} MiB), project_layouts {layout:.0} ms; {} receivers, {:.1} M atlas texels, {} layout numbers",
            encoded as f64 / 1048576.,
            layouts.receivers.len(),
            texels as f64 / 1e6,
            layouts.records.len(),
        );
        if label != "cold" {
            continue;
        }
        // The CPU fields a GPU host asks for, mesh by mesh, as the Studio's scene bake does: a
        // hybrid program's own and each layer's (all but the planes beneath it).
        let cpu_fields = |described: &serde_json::Value| -> Vec<String> {
            let shader = described
                .get("plan")
                .map_or(&described["shader"], |plan| &plan["shader"]);
            shader["cpuFields"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|field| field["name"].as_str())
                .filter(|name| !name.starts_with("below."))
                .map(str::to_owned)
                .collect()
        };
        let mut requests: Vec<(String, Vec<String>)> = Vec::new();
        for (mesh, described) in shaded.metadata["shading"].as_object().into_iter().flatten() {
            let material = &shaded.metadata["materials"][described["material"].as_str().unwrap()];
            let mut names = if material.get("plan").is_some() {
                cpu_fields(material)
            } else {
                Vec::new()
            };
            for layer in material["layers"].as_array().into_iter().flatten() {
                let id = layer["id"].as_str().unwrap();
                names.extend(
                    cpu_fields(layer)
                        .into_iter()
                        .map(|name| format!("{id}/{name}")),
                );
            }
            if !names.is_empty() {
                requests.push((mesh.clone(), names));
            }
        }
        let fields: usize = requests.iter().map(|(_, names)| names.len()).sum();
        let started = Instant::now();
        let mut texels = 0;
        for (mesh, names) in &requests {
            let baked = session
                .bake_project_fields(project, mesh, names, revision, &asset, &|| false)
                .unwrap_or_else(|errors| panic!("{mesh}: {errors:?}"));
            texels += baked.width * baked.height * baked.fields.len();
        }
        let one_by_one = started.elapsed().as_secs_f64() * 1e3;
        println!(
            "fields, a call per mesh: {} meshes, {fields} fields, {:.1} M field texels in {one_by_one:.0} ms",
            requests.len(),
            texels as f64 / 1e6
        );
    }
}
