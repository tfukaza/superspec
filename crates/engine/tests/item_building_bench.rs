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
        // What the description and the layouts hold, to compare two builds of the engine by.
        let digest = |bytes: &[u8]| {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            bytes.hash(&mut hasher);
            hasher.finish()
        };
        let described = serde_json::to_string(&shaded.metadata).unwrap();
        let (encoded, described) = (described.len(), digest(described.as_bytes()));
        let started = Instant::now();
        let layouts = session
            .project_layouts(project, revision, &asset, &|| false)
            .unwrap_or_else(|errors| panic!("{errors:?}"));
        let layout = started.elapsed().as_secs_f64() * 1e3;
        // A host that wants both prepares the project once: the same description, layouts and scene.
        let started = Instant::now();
        let prepared = session
            .prepare_project(project, revision, &asset, &|| false)
            .unwrap_or_else(|errors| panic!("{errors:?}"));
        let together = session.shade_prepared(&prepared, &|| false).unwrap();
        let laid = session.layouts_prepared(&prepared, &|| false).unwrap();
        let once = started.elapsed().as_secs_f64() * 1e3;
        assert!(
            together == shaded.metadata,
            "{label}: a prepared project is described otherwise"
        );
        assert!(
            laid.records == layouts.records,
            "{label}: a prepared project is laid out otherwise"
        );
        assert!(
            prepared.scene.packet() == shaded.scene(revision as u32).unwrap().packet(),
            "{label}: a prepared project's scene is another"
        );
        println!("{label}: prepared once, described and laid out in {once:.0} ms");
        let texels: usize = layouts.receivers.iter().map(|r| r.width * r.height).sum();
        let records: Vec<u8> = layouts
            .records
            .iter()
            .flat_map(|value| value.to_bits().to_le_bytes())
            .collect();
        println!(
            "{label}: shade_project {shade:.0} ms (metadata {:.1} MiB, {described:016x}), project_layouts {layout:.0} ms; {} receivers, {:.1} M atlas texels, {} layout numbers ({:016x})",
            encoded as f64 / 1048576.,
            layouts.receivers.len(),
            texels as f64 / 1e6,
            layouts.records.len(),
            digest(&records),
        );
        if label == "cold" {
            mesh_cost(&prepared.scene);
        }
        // ITEM_BUILDING_FIELDS=0 leaves the field bakes out (to profile the rest).
        if label != "cold" || std::env::var("ITEM_BUILDING_FIELDS").is_ok_and(|v| v == "0") {
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
        // Each way in a session of its own, so neither finds what the other baked. The project
        // prepared once (what a host does), then, unless ITEM_BUILDING_FIELDS=once, prepared
        // again for every mesh, which must give the same texels.
        let mut shared = Session::default();
        let started = Instant::now();
        let prepared = shared
            .prepare_project_fields(project, 1, &asset, &|| false)
            .unwrap_or_else(|errors| panic!("{errors:?}"));
        let preparing = started.elapsed().as_secs_f64() * 1e3;
        let mut texels = 0;
        let mut content = 0u64;
        let mut together = Vec::new();
        for (mesh, names) in &requests {
            let baked = shared
                .bake_prepared_fields(&prepared, mesh, names, 1, &asset, &|| false)
                .unwrap_or_else(|errors| panic!("{mesh}: {errors:?}"));
            texels += baked.width * baked.height * baked.fields.len();
            together.push(baked);
        }
        let elapsed = started.elapsed().as_secs_f64() * 1e3;
        for baked in &together {
            for field in &baked.fields {
                let bytes: Vec<u8> = field
                    .iter()
                    .flat_map(|v| v.to_bits().to_le_bytes())
                    .collect();
                content = content.rotate_left(7) ^ digest(&bytes);
            }
        }
        println!(
            "fields, the project prepared once: {} meshes, {fields} fields, {:.1} M field texels in {elapsed:.0} ms (preparing {preparing:.0} ms); texels {content:016x}",
            requests.len(),
            texels as f64 / 1e6,
        );
        if std::env::var("ITEM_BUILDING_FIELDS").is_ok_and(|v| v == "once") {
            continue;
        }
        let mut alone = Session::default();
        let started = Instant::now();
        for ((mesh, names), expected) in requests.iter().zip(&together) {
            let baked = alone
                .bake_project_fields(project, mesh, names, 1, &asset, &|| false)
                .unwrap_or_else(|errors| panic!("{mesh}: {errors:?}"));
            assert_eq!(
                (baked.width, baked.height),
                (expected.width, expected.height)
            );
            let same = baked.fields.iter().zip(&expected.fields).all(|(a, b)| {
                a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.to_bits() == b.to_bits())
            });
            assert!(same, "{mesh}: a prepared project bakes other fields");
        }
        println!(
            "fields, the project prepared per mesh: {:.0} ms; every texel equal",
            started.elapsed().as_secs_f64() * 1e3
        );
    }
}

/// What the scene's meshes cost a renderer and where it could be less without changing a pixel:
/// vertices against the distinct ones (same position, normal, UV and color, which an index
/// could share), by material; and meshes that are another mesh moved (which could be instances).
fn mesh_cost(scene: &superspec_model::Scene) {
    use std::collections::{BTreeMap, HashMap, HashSet};
    #[derive(Default)]
    struct Cost {
        meshes: usize,
        vertices: usize,
        distinct: usize,
        triangles: usize,
        referenced: usize,
    }
    let mut by_material: BTreeMap<String, Cost> = BTreeMap::new();
    // Meshes by shape: positions from the mesh's low corner (to a tenth of a millimeter),
    // normals, colors and indices.
    let mut shapes: HashMap<Vec<u32>, (usize, usize, String)> = HashMap::new();
    for mesh in scene.manifest.meshes.iter().filter(|mesh| mesh.visible) {
        let (from, count) = (mesh.vertex_start, mesh.vertex_count);
        let vertex = |at: usize| -> [u32; 11] {
            let (p, n, c, uv) = (
                &scene.positions[(from + at) * 3..][..3],
                &scene.normals[(from + at) * 3..][..3],
                &scene.colors[(from + at) * 3..][..3],
                &scene.uvs[(from + at) * 2..][..2],
            );
            std::array::from_fn(|k| {
                match k {
                    0..=2 => p[k],
                    3..=5 => n[k - 3],
                    6..=8 => c[k - 6],
                    _ => uv[k - 9],
                }
                .to_bits()
            })
        };
        let distinct: HashSet<[u32; 11]> = (0..count).map(vertex).collect();
        let indices = &scene.indices[mesh.index_start..mesh.index_start + mesh.index_count];
        let referenced: HashSet<u32> = indices.iter().copied().collect();
        let cost = by_material
            .entry(mesh.material.clone().unwrap_or_else(|| "(none)".into()))
            .or_default();
        cost.meshes += 1;
        cost.vertices += count;
        cost.distinct += distinct.len();
        cost.triangles += indices.len() / 3;
        cost.referenced += referenced.len();
        let low: [f32; 3] = std::array::from_fn(|axis| {
            (0..count)
                .map(|at| scene.positions[(from + at) * 3 + axis])
                .fold(f32::INFINITY, f32::min)
        });
        let mut shape: Vec<u32> = Vec::with_capacity(count * 9 + indices.len());
        for at in 0..count {
            for axis in 0..3 {
                let offset = scene.positions[(from + at) * 3 + axis] - low[axis];
                shape.push((offset * 1e4).round() as u32);
            }
            shape.extend(
                scene.normals[(from + at) * 3..][..3]
                    .iter()
                    .map(|v| v.to_bits()),
            );
            shape.extend(
                scene.colors[(from + at) * 3..][..3]
                    .iter()
                    .map(|v| v.to_bits()),
            );
        }
        shape.extend(indices);
        let entry = shapes.entry(shape).or_insert((0, count, mesh.id.clone()));
        entry.0 += 1;
    }
    let mut total = Cost::default();
    let mut rows: Vec<(&String, &Cost)> = by_material.iter().collect();
    rows.sort_by_key(|(_, cost)| std::cmp::Reverse(cost.vertices));
    println!(
        "meshes: material, meshes, triangles, vertices, vertices an index names, distinct vertices"
    );
    for (_, cost) in &rows {
        total.meshes += cost.meshes;
        total.vertices += cost.vertices;
        total.distinct += cost.distinct;
        total.triangles += cost.triangles;
        total.referenced += cost.referenced;
    }
    for (material, cost) in rows.iter().take(14) {
        println!(
            "  {material}: {} meshes, {} triangles, {} vertices, {} named, {} distinct",
            cost.meshes, cost.triangles, cost.vertices, cost.referenced, cost.distinct
        );
    }
    println!(
        "  all {}: {} meshes, {} triangles, {} vertices, {} named, {} distinct ({:.0}%)",
        by_material.len(),
        total.meshes,
        total.triangles,
        total.vertices,
        total.referenced,
        total.distinct,
        100. * total.distinct as f64 / total.vertices.max(1) as f64
    );
    let mut repeated: Vec<&(usize, usize, String)> =
        shapes.values().filter(|(times, ..)| *times > 1).collect();
    repeated.sort_by_key(|(times, vertices, _)| std::cmp::Reverse((times - 1) * vertices));
    let saved: usize = repeated
        .iter()
        .map(|(times, vertices, _)| (times - 1) * vertices)
        .sum();
    println!(
        "  {} shapes are drawn more than once ({} meshes); as instances they would hold {saved} fewer vertices",
        repeated.len(),
        repeated.iter().map(|(times, ..)| times).sum::<usize>()
    );
    for (times, vertices, id) in repeated.iter().take(8) {
        println!("    {times} x {vertices} vertices, like {id}");
    }
}
