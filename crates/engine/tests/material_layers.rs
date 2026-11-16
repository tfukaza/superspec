//! The Kowloon concrete split into an anchored intrinsic base and weathering layers
//! (`kowloon-concrete-anchored` under `kowloon-top-grime`, `kowloon-ground-stain`,
//! `kowloon-moss`, `kowloon-concrete-edges` and `kowloon-feature-grime`): each recipe is what it
//! says it is, the base reads nothing of a receiver but its anchored position, the layers follow
//! named features, and a stack of them bakes on a wall. `bench_layered_concrete` measures the
//! stack against the shipped one-program concrete:
//! `cargo test --release -p superspec-engine --test material_layers bench_layered_concrete -- --ignored --nocapture`.
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Instant};
use superspec_engine::authoring::{
    Anchor, Layer, Mapping, Output, Profile, Project, Receiver, Request, Session,
};
use superspec_engine::materialspec::program::{self, NamedFeature, RecipeKind, Sources};

fn examples() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/materialspec/program/examples")
}

/// Every recipe beside the Kowloon ones, by file name.
fn modules() -> BTreeMap<String, String> {
    std::fs::read_dir(examples())
        .unwrap()
        .filter_map(|file| {
            let path = file.unwrap().path();
            let name = path.file_name()?.to_str()?.to_owned();
            (name.ends_with(".material.ts") || name.ends_with(".ms"))
                .then(|| (name, std::fs::read_to_string(&path).unwrap()))
        })
        .collect()
}

fn asset(name: &str) -> Result<Arc<[u8]>, String> {
    let path = match name.strip_prefix("superspec-material:") {
        Some(catalog) => PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/materials")
            .join(catalog),
        None => examples().join(name),
    };
    std::fs::read(&path)
        .map(Arc::from)
        .map_err(|e| format!("{name}: {e}"))
}

const BASE: &str = "kowloon-concrete-anchored.material.ts";
const SHIPPED: &str = "kowloon-concrete.material.ts";

fn compiled(entry: &str) -> program::Program {
    program::compile(&Sources {
        entry: entry.into(),
        modules: modules(),
    })
    .unwrap_or_else(|e| panic!("{entry}: {e:?}"))
}

#[test]
fn the_split_concrete_is_an_intrinsic_base_and_layers_that_follow_features() {
    let base = compiled(BASE);
    assert_eq!(base.metadata().kind, RecipeKind::Material);
    let table = base.dependency_table();
    for output in ["base_color", "roughness", "metalness", "height"] {
        assert!(
            table.outputs[output].intrinsic(),
            "the base's {output} reads only its anchored position and footprint"
        );
    }
    assert!(
        !base
            .metadata()
            .required_inputs
            .iter()
            .any(|input| input == "bounds")
    );
    for (entry, follows, outputs) in [
        (
            "kowloon-top-grime.material.ts",
            &["top"][..],
            &["base_color"][..],
        ),
        (
            "kowloon-ground-stain.material.ts",
            &["base"],
            &["base_color"],
        ),
        (
            "kowloon-moss.material.ts",
            &["base"],
            &["base_color", "height", "roughness"],
        ),
        (
            "kowloon-concrete-edges.material.ts",
            &[],
            &["base_color", "height", "normal_offset"],
        ),
        (
            "kowloon-feature-grime.material.ts",
            &["follow"],
            &["base_color", "roughness"],
        ),
        (
            "kowloon-joint-rust.material.ts",
            &["joint"],
            &["base_color", "roughness"],
        ),
    ] {
        let layer = compiled(entry);
        let metadata = layer.metadata();
        assert_eq!(metadata.kind, RecipeKind::Layer, "{entry}");
        assert_eq!(metadata.named_features, follows, "{entry}");
        assert_eq!(metadata.outputs, outputs, "{entry}");
        let reads_bounds = metadata
            .required_inputs
            .iter()
            .any(|input| input.contains("bounds"));
        assert!(!reads_bounds, "{entry} reads no receiver bounds");
    }
    // The moss reads the occluder service's planes, and an open wall without them is the moss of
    // the shipped recipe.
    let moss = compiled("kowloon-moss.material.ts");
    let externals: Vec<&str> = moss
        .metadata()
        .externals
        .iter()
        .map(|external| external.name.as_str())
        .collect();
    assert!(externals.contains(&"occlusion") && externals.contains(&"sunExposure"));
}

/// A wall of the washroom's tall wall's size, 0.25 m thick, its front face on y = 0.
fn cuboid(width: f64) -> Receiver {
    let (min, max) = ([0., 0., 0.], [width, 0.25, 2.6]);
    let corner = |x: usize, y: usize, z: usize| {
        [
            [min[0], max[0]][x],
            [min[1], max[1]][y],
            [min[2], max[2]][z],
        ]
    };
    let faces: [([[usize; 3]; 4], [f64; 3]); 6] = [
        ([[0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]], [0., 0., 1.]),
        ([[0, 1, 0], [1, 1, 0], [1, 0, 0], [0, 0, 0]], [0., 0., -1.]),
        ([[0, 0, 0], [1, 0, 0], [1, 0, 1], [0, 0, 1]], [0., -1., 0.]),
        ([[1, 1, 0], [0, 1, 0], [0, 1, 1], [1, 1, 1]], [0., 1., 0.]),
        ([[0, 1, 0], [0, 0, 0], [0, 0, 1], [0, 1, 1]], [-1., 0., 0.]),
        ([[1, 0, 0], [1, 1, 0], [1, 1, 1], [1, 0, 1]], [1., 0., 0.]),
    ];
    let (mut positions, mut normals, mut uvs, mut indices) = (vec![], vec![], vec![], vec![]);
    for (quad, normal) in faces {
        let base = (positions.len() / 3) as u32;
        for (index, [x, y, z]) in quad.into_iter().enumerate() {
            positions.extend(corner(x, y, z));
            normals.extend(normal);
            uvs.extend([[0., 0.], [1., 0.], [1., 1.], [0., 1.]][index]);
        }
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    Receiver {
        id: "wall".into(),
        object_id: "wall".into(),
        positions,
        normals,
        uvs,
        indices,
        object_to_world: None,
        contacts: Vec::new(),
    }
}

/// The wall's `top` and `base` on its front and back faces, as a panel item names them.
fn features(width: f64) -> Vec<NamedFeature> {
    let line = |name: &str, y: f64, z: f64| NamedFeature {
        name: name.into(),
        points: vec![[0., y, z], [width, y, z]],
        closed: false,
    };
    vec![
        line("base", 0., 0.),
        line("top", 0., 2.6),
        line("base", 0.25, 0.),
        line("top", 0.25, 2.6),
    ]
}

fn layer(entry: &str, parameters: &[(&str, serde_json::Value)], follow: Option<&str>) -> Layer {
    Layer {
        entry: entry.into(),
        parameters: parameters
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect(),
        seed: 7,
        follow: follow
            .map(|feature| [("follow".to_owned(), feature.to_owned())].into())
            .unwrap_or_default(),
    }
}

/// Grime from the top, the ground stain and moss: the three layers the roadmap names.
fn three() -> Vec<Layer> {
    vec![
        layer("kowloon-top-grime.material.ts", &[], None),
        layer("kowloon-ground-stain.material.ts", &[], None),
        layer("kowloon-moss.material.ts", &[], None),
    ]
}

/// The three, worn edges and grime along the base: the shipped concrete's look on an item wall.
fn full() -> Vec<Layer> {
    let mut layers = three();
    layers.push(layer("kowloon-concrete-edges.material.ts", &[], None));
    layers.push(layer(
        "kowloon-feature-grime.material.ts",
        &[
            ("amount", 0.68.into()),
            ("near", 0.into()),
            ("far", 0.3.into()),
            ("color", serde_json::json!([0.035, 0.032, 0.026])),
            ("roughness", 0.95.into()),
        ],
        Some("base"),
    ));
    layers
}

fn wall_request(
    entry: &str,
    layers: Vec<Layer>,
    width: f64,
    resolution: usize,
    profile: Profile,
) -> Request {
    let modules = modules();
    let layered = !layers.is_empty();
    Request {
        kind: "material".into(),
        entry: entry.into(),
        source: modules[entry].clone(),
        modules,
        seed: 7,
        resolution,
        profile,
        receivers: vec![cuboid(width)],
        mapping: if layered {
            Mapping::Anchored
        } else {
            Mapping::World
        },
        anchors: [(
            "wall".to_owned(),
            Anchor {
                origin: [0.; 3],
                u: [1., 0., 0.],
                v: [0., 0., 1.],
            },
        )]
        .into(),
        features: [("wall".to_owned(), features(width))].into(),
        layers,
        ..Request::default()
    }
}

fn build(session: &mut Session, revision: u64, request: &Request) -> (Output, f64) {
    let started = Instant::now();
    let output = session
        .build(request, revision, &asset, &|| false)
        .unwrap_or_else(|e| panic!("{e:?}"));
    (output, started.elapsed().as_secs_f64())
}

/// Mean linear-ish byte value of the color map's texels in a band of rows of its one atlas.
fn color_bytes(output: &Output) -> Vec<u8> {
    let material = output.model.materials.values().next().unwrap();
    let bytes = &output.model.images[material.texture.as_ref().unwrap()].bytes;
    superspec_geometry::image::decode(bytes).unwrap().rgba
}

#[test]
fn the_full_stack_bakes_on_a_wall_and_its_layers_change_what_they_say() {
    let mut session = Session::default();
    let (bare, _) = build(
        &mut session,
        1,
        &wall_request(BASE, Vec::new(), 2.7, 128, Profile::Preview),
    );
    let (layered, _) = build(
        &mut session,
        2,
        &wall_request(BASE, full(), 2.7, 128, Profile::Preview),
    );
    let (bare, layered) = (color_bytes(&bare), color_bytes(&layered));
    assert_eq!(bare.len(), layered.len(), "the same atlas");
    // The layers darken the wall overall (grime, stain, moss and worn edges all do).
    let sum = |bytes: &[u8]| {
        bytes
            .chunks_exact(4)
            .map(|t| t[0] as u64 + t[1] as u64 + t[2] as u64)
            .sum::<u64>()
    };
    assert!(
        sum(&layered) < sum(&bare),
        "{} {}",
        sum(&layered),
        sum(&bare)
    );
    let changed = bare
        .chunks_exact(4)
        .zip(layered.chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count();
    assert!(changed > bare.len() / 4 / 20, "{changed} texels changed");
}

/// The washroom wall item with the split concrete on its concrete slot, anchored: the stack
/// `layers`, at `width` meters.
fn wall_project(width: f64, base: &[(&str, f64)], layers: Vec<Layer>) -> Project {
    use superspec_engine::authoring::{MaterialAssignment, ModelSource};
    let mut modules = modules();
    modules.insert(
        "wall.item.json".into(),
        include_str!("../src/itemspec/definition/fixtures/washroom-wall.item.json").into(),
    );
    modules.insert(
        "plain.ms".into(),
        "material 1;\noutput base_color = vec3(0.2);\noutput roughness = 0.5;\noutput metalness = 1;"
            .into(),
    );
    let concrete = MaterialAssignment {
        id: "concrete".into(),
        entry: BASE.into(),
        parameters: base
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).into()))
            .collect(),
        seed: 7,
        layers,
        ..MaterialAssignment::default()
    };
    let steel = MaterialAssignment {
        id: "steel".into(),
        entry: "plain.ms".into(),
        ..MaterialAssignment::default()
    };
    Project {
        schema_version: 2,
        kind: "material".into(),
        entry: BASE.into(),
        modules,
        resolution: 64,
        texel_density: Some(24.),
        mapping: Mapping::World,
        mappings: [("concrete".to_owned(), Mapping::Anchored)].into(),
        models: vec![ModelSource {
            id: "wall".into(),
            kind: "item".into(),
            entry: Some("wall.item.json".into()),
            parameters: [("width".to_owned(), width.into())].into(),
            slots: [
                ("concrete".to_owned(), "concrete".to_owned()),
                ("frame".to_owned(), "steel".to_owned()),
                ("glass".to_owned(), "steel".to_owned()),
            ]
            .into(),
            transform: Some([[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [0., 0., 0.]]),
            ..ModelSource::default()
        }],
        materials: vec![concrete, steel],
        ..Project::default()
    }
}

/// The washroom pipe item in the shipped rusted steel (which has live operations) under the
/// joint-rust layer, at `amount`.
fn pipe_project(amount: f64) -> Project {
    use superspec_engine::authoring::{MaterialAssignment, ModelSource};
    let mut modules = modules();
    modules.insert(
        "pipe.item.json".into(),
        include_str!("../src/itemspec/definition/fixtures/washroom-pipe.item.json").into(),
    );
    let steel = MaterialAssignment {
        id: "steel".into(),
        entry: "kowloon-rusted-steel.material.ts".into(),
        seed: 7,
        layers: vec![layer(
            "kowloon-joint-rust.material.ts",
            &[("amount", amount.into())],
            None,
        )],
        ..MaterialAssignment::default()
    };
    Project {
        schema_version: 2,
        kind: "material".into(),
        entry: "kowloon-rusted-steel.material.ts".into(),
        modules,
        resolution: 64,
        mapping: Mapping::World,
        models: vec![ModelSource {
            id: "pipe".into(),
            kind: "item".into(),
            entry: Some("pipe.item.json".into()),
            slots: [("steel".to_owned(), "steel".to_owned())].into(),
            transform: Some([[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [0., 0., 0.]]),
            ..ModelSource::default()
        }],
        materials: vec![steel],
        ..Project::default()
    }
}

/// What a GPU host composing a slot's layers itself is given: each mesh's features as each layer
/// queries them (the grime that follows `base` as `follow` measures to the same segments as the
/// ground stain, which reads `base` by that name), and a layer's own CPU fields by
/// `stack.<k>/<field>`, over the receiver's atlas.
#[test]
fn a_gpu_host_gets_each_layers_features_and_fields() {
    let project = wall_project(2.7, &[], full());
    let mut session = Session::default();
    let shaded = session
        .shade_project(&project, 1, &asset, &|| false)
        .unwrap_or_else(|e| panic!("{e:?}"));
    let named = |rows: &serde_json::Value, from: f64| {
        rows.as_array()
            .unwrap()
            .chunks_exact(12)
            .filter(|row| row[3].as_f64().unwrap() >= from)
            .count()
    };
    let mut walls = 0;
    let layouts = session
        .project_layouts(&project, 2, &asset, &|| false)
        .unwrap();
    for (id, mesh) in shaded.metadata["shading"].as_object().unwrap() {
        if mesh["material"] != "concrete" {
            assert!(mesh.get("layerFeatures").is_none(), "{id}");
            continue;
        }
        let layers = mesh["layerFeatures"].as_array().unwrap();
        assert_eq!(layers.len(), full().len(), "{id}");
        // Ground stain (`base`) and the base grime (`follow`, following `base`) see the same segments.
        assert_eq!(named(&layers[1], 5.), named(&layers[4], 5.), "{id}");
        // Worn edges read derived features and no name.
        assert_eq!(named(&layers[3], 5.), 0, "{id}");
        if named(&layers[1], 5.) > 0 {
            walls += 1;
            let fields = session
                .bake_project_fields(
                    &project,
                    id,
                    &["stack.3/shelter".to_owned()],
                    3,
                    &asset,
                    &|| false,
                )
                .unwrap_or_else(|e| panic!("{id}: {e:?}"));
            let layout = layouts
                .receivers
                .iter()
                .find(|layout| layout.id == *id)
                .unwrap();
            assert_eq!(
                (fields.width, fields.height),
                (layout.width, layout.height),
                "{id}"
            );
            assert_eq!(
                fields.fields[0].len(),
                layout.width * layout.height * 4,
                "{id}"
            );
        }
    }
    assert!(walls > 0, "some receiver of the wall has a base");
    // A name of no layer of the slot is an error, not another material's field.
    let mesh = shaded.metadata["shading"]
        .as_object()
        .unwrap()
        .iter()
        .find(|(_, mesh)| mesh["material"] == "concrete")
        .unwrap()
        .0;
    assert!(
        session
            .bake_project_fields(
                &project,
                mesh,
                &["stack.9/shelter".to_owned()],
                4,
                &asset,
                &|| false
            )
            .is_err()
    );
}

/// The packets of a sequence of layered and anchored builds in one session, for the Node
/// parity check against Wasm (`tests/stage2-layers-parity.mjs`): a cold stack, a base edit, a
/// layer edit, a layer added, one removed, the wall widened, and the anchored base alone.
/// `LAYERS_PARITY_OUTPUT=<dir>` writes them.
#[test]
fn native_layered_packets_for_parity() {
    let damp = |value: f64| {
        let mut layers = full();
        layers[1].parameters.insert("dampness".into(), value.into());
        layers
    };
    let steps: Vec<(&str, Project)> = vec![
        ("cold", wall_project(2.7, &[], full())),
        (
            "base-edit",
            wall_project(2.7, &[("darknessA", 0.2)], full()),
        ),
        (
            "layer-edit",
            wall_project(2.7, &[("darknessA", 0.2)], damp(0.8)),
        ),
        ("add-layer", {
            let mut layers = damp(0.8);
            layers.push(layer(
                "kowloon-feature-grime.material.ts",
                &[],
                Some("sill"),
            ));
            wall_project(2.7, &[("darknessA", 0.2)], layers)
        }),
        ("remove-layer", {
            let mut layers = damp(0.8);
            layers.remove(0);
            wall_project(2.7, &[("darknessA", 0.2)], layers)
        }),
        ("widen", wall_project(3.2, &[("darknessA", 0.2)], damp(0.8))),
        (
            "anchored-base",
            wall_project(3.2, &[("darknessA", 0.2)], Vec::new()),
        ),
        (
            "anchored-narrow",
            wall_project(2.7, &[("darknessA", 0.2)], Vec::new()),
        ),
        ("steel-joint-rust", pipe_project(0.9)),
        ("steel-joint-edit", pipe_project(0.5)),
    ];
    let mut session = Session::default();
    let mut written = Vec::new();
    for (revision, (name, project)) in steps.iter().enumerate() {
        let output = session
            .build_project(project, revision as u64 + 1, &asset, &|| false)
            .unwrap_or_else(|errors| panic!("{name}: {errors:?}"));
        let packet = output.scene(revision as u32 + 1).unwrap().packet();
        // A fresh session delivers the same bytes.
        let fresh = Session::default()
            .build_project(project, revision as u64 + 1, &asset, &|| false)
            .unwrap();
        assert_eq!(
            fresh.scene(revision as u32 + 1).unwrap().packet(),
            packet,
            "{name}"
        );
        written.push((name, project, packet));
    }
    if let Ok(directory) = std::env::var("LAYERS_PARITY_OUTPUT") {
        let directory = PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        let mut index = Vec::new();
        for (step, (name, project, packet)) in written.iter().enumerate() {
            std::fs::write(
                directory.join(format!("{step}-{name}.json")),
                serde_json::to_vec(project).unwrap(),
            )
            .unwrap();
            std::fs::write(directory.join(format!("{step}-{name}.bin")), packet).unwrap();
            index.push(format!("{step}-{name}"));
        }
        let assets = [
            "textures/concrete-plain.png",
            "textures/concrete-fine-sand.png",
            "superspec-material:building-plaster-plain.png",
            "superspec-material:rust.png",
        ];
        let mut files = Vec::new();
        for name in assets {
            let file = name.replace(':', "/");
            let path = directory.join(&file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, asset(name).unwrap()).unwrap();
            files.push(serde_json::json!({ "name": name, "file": file }));
        }
        std::fs::write(
            directory.join("index.json"),
            serde_json::to_vec(&serde_json::json!({ "steps": index, "assets": files })).unwrap(),
        )
        .unwrap();
    }
}

/// A layered project built in one session against the same build shared with other sessions
/// (`Session::distribute_bakes`, as the browser's helper workers bake it): the time of the local
/// build, and of each share and of the build that merges them, which must deliver the same bytes:
/// `cargo test --release -p superspec-engine --test material_layers bench_distributed_layers -- --ignored --nocapture`.
#[test]
#[ignore]
fn bench_distributed_layers() {
    let shares = 6;
    let mut projects = vec![
        ("steel under joint rust", pipe_project(0.9)),
        ("split concrete wall", wall_project(2.7, &[], full())),
    ];
    for project in &mut projects {
        project.1.resolution = 512;
        project.1.texel_density = Some(128.);
    }
    // A Studio project file (assets in base64), such as the split washroom of
    // tests/stage2-weathering-browser.mjs.
    let mut supplied: BTreeMap<String, Arc<[u8]>> = BTreeMap::new();
    if let Ok(path) = std::env::var("SPLIT_PROJECT") {
        let mut value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        for (name, text) in value["assets"].as_object().unwrap() {
            let (mut bytes, mut bits, mut count) = (Vec::new(), 0u32, 0);
            for c in text.as_str().unwrap().bytes().filter(|c| *c != b'=') {
                let six = match c {
                    b'A'..=b'Z' => c - b'A',
                    b'a'..=b'z' => c - b'a' + 26,
                    b'0'..=b'9' => c - b'0' + 52,
                    b'+' => 62,
                    _ => 63,
                };
                bits = bits << 6 | u32::from(six);
                count += 6;
                if count >= 8 {
                    count -= 8;
                    bytes.push((bits >> count) as u8);
                }
            }
            supplied.insert(name.clone(), bytes.into());
        }
        value.as_object_mut().unwrap().remove("assets");
        projects = vec![("project file", serde_json::from_value(value).unwrap())];
    }
    let asset = |name: &str| match supplied.get(name) {
        Some(bytes) => Ok(bytes.clone()),
        None => asset(name),
    };
    for (label, project) in projects {
        let started = Instant::now();
        let local = Session::default()
            .build_project(&project, 1, &asset, &|| false)
            .unwrap_or_else(|e| panic!("{e:?}"));
        let local_seconds = started.elapsed().as_secs_f64();
        let mut session = Session::default();
        session.distribute_bakes(true);
        let started = Instant::now();
        assert!(
            session
                .build_project(&project, 1, &asset, &|| false)
                .is_err()
        );
        let first = started.elapsed().as_secs_f64();
        let mut pending = Vec::new();
        while let Some(bake) = session.take_pending_bake() {
            pending.push(bake);
        }
        let mut helpers: Vec<Session> = (0..shares).map(|_| Session::default()).collect();
        let mut times = Vec::new();
        for bake in &pending {
            for (index, helper) in helpers.iter_mut().enumerate() {
                let started = Instant::now();
                let rows = helper
                    .bake_share(bake, index, shares, &asset, &|| false)
                    .unwrap_or_else(|e| panic!("{e:?}"));
                times.push((
                    bake.keys.len(),
                    bake.request.layers.len(),
                    started.elapsed().as_secs_f64(),
                ));
                session.provide_bake_share(&rows).unwrap();
            }
        }
        let started = Instant::now();
        let merged = session
            .build_project(&project, 1, &asset, &|| false)
            .unwrap_or_else(|e| panic!("{e:?}"));
        let last = started.elapsed().as_secs_f64();
        assert_eq!(
            merged.scene(1).unwrap().packet(),
            local.scene(1).unwrap().packet(),
            "{label}"
        );
        println!(
            "{label}: local {local_seconds:.2} s; distributed: report {first:.2} s, shares {:?}, merge {last:.2} s",
            times
                .iter()
                .map(|(keys, layers, seconds)| format!(
                    "{keys} keys/{layers} layers {seconds:.2} s"
                ))
                .collect::<Vec<_>>()
        );
    }
}

fn rss() -> f64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let field = |name: &str| {
        status
            .lines()
            .find(|line| line.starts_with(name))
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|kb| kb.parse::<f64>().ok())
            .unwrap_or(0.)
            / 1024.
    };
    field("VmRSS:")
}

fn kept(session: &Session) -> String {
    session
        .retained_material()
        .into_iter()
        .filter(|(_, (entries, _))| *entries > 0)
        .map(|(name, (entries, bytes))| {
            format!("{name} {entries}/{:.1} MiB", bytes as f64 / 1048576.)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The layered concrete against the shipped one-program concrete on a washroom-sized wall:
/// cold bakes, a base edit, a layer edit, a layer added and one removed, with CPU time, the
/// process's resident memory after each step and what the session keeps; and each recipe's
/// generated GLSL size.
#[test]
#[ignore]
fn bench_layered_concrete() {
    for (label, program) in [
        ("shipped concrete", compiled(SHIPPED)),
        ("anchored base", compiled(BASE)),
        ("top grime", compiled("kowloon-top-grime.material.ts")),
        ("ground stain", compiled("kowloon-ground-stain.material.ts")),
        ("moss", compiled("kowloon-moss.material.ts")),
        ("edges", compiled("kowloon-concrete-edges.material.ts")),
        (
            "feature grime",
            compiled("kowloon-feature-grime.material.ts"),
        ),
    ] {
        let shader = program.shader().unwrap();
        let plan = program.shader_plan().map(|plan| plan.source.len());
        println!(
            "GLSL {label}: shader {} bytes, plan {}",
            shader.source.len(),
            plan.map_or_else(
                |e| format!("none ({})", e.replace('\n', " ")),
                |n| format!("{n} bytes")
            )
        );
    }
    let sizes: Vec<usize> = std::env::var("BENCH_SIZES")
        .ok()
        .map(|text| text.split(',').filter_map(|n| n.parse().ok()).collect())
        .unwrap_or_else(|| vec![256, 512, 2048]);
    for resolution in sizes {
        for profile in [Profile::Preview, Profile::Production] {
            if profile == Profile::Preview && resolution > 512 {
                continue;
            }
            let tag = format!("{resolution} px {profile:?}");
            // The shipped concrete: cold, and an edit of one parameter.
            let mut session = Session::default();
            let shipped = |parameters: &[(&str, f64)]| {
                let mut request = wall_request(SHIPPED, Vec::new(), 2.7, resolution, profile);
                for (name, value) in parameters {
                    request
                        .parameters
                        .insert((*name).to_owned(), (*value).into());
                }
                request
            };
            let (_, cold) = build(&mut session, 1, &shipped(&[]));
            let after_cold = rss();
            let (_, edit) = build(&mut session, 2, &shipped(&[("darknessA", 0.12)]));
            let (_, ground) = build(
                &mut session,
                3,
                &shipped(&[("darknessA", 0.12), ("dampness", 0.7)]),
            );
            println!(
                "{tag} shipped: cold {cold:.2} s, base-color edit {edit:.2} s, dampness edit {ground:.2} s; RSS {after_cold:.0} MiB"
            );
            drop(session);
            for (stack_label, stack) in [("base + 3 layers", three()), ("base + 5 layers", full())]
            {
                let mut session = Session::default();
                let request = |base: &[(&str, f64)], layers: Vec<Layer>| {
                    let mut request = wall_request(BASE, layers, 2.7, resolution, profile);
                    // On the anchor's lattice, so the base's intrinsic outputs are shared.
                    request.texel_density = Some(resolution as f64 / 8.);
                    for (name, value) in base {
                        request
                            .parameters
                            .insert((*name).to_owned(), (*value).into());
                    }
                    request
                };
                let mut revision = 0;
                let mut step = |session: &mut Session, request: &Request| {
                    revision += 1;
                    build(session, revision, request).1
                };
                let cold = step(&mut session, &request(&[], stack.clone()));
                let after_cold = rss();
                let base_edit = step(
                    &mut session,
                    &request(&[("darknessA", 0.12)], stack.clone()),
                );
                let mut damp = stack.clone();
                damp[2].parameters.insert("dampness".into(), 0.7.into());
                let layer_edit = step(&mut session, &request(&[("darknessA", 0.12)], damp.clone()));
                let mut added = damp.clone();
                added.push(layer(
                    "kowloon-feature-grime.material.ts",
                    &[("far", 0.6.into())],
                    Some("top"),
                ));
                let add = step(&mut session, &request(&[("darknessA", 0.12)], added));
                let mut removed = damp.clone();
                removed.remove(1);
                let remove = step(&mut session, &request(&[("darknessA", 0.12)], removed));
                // A wider wall: the base is read from its intrinsic raster where both cover.
                let mut wide = request(&[("darknessA", 0.12)], damp.clone());
                wide.receivers = vec![cuboid(3.2)];
                wide.features = [("wall".to_owned(), features(3.2))].into();
                let resize = step(&mut session, &wide);
                println!(
                    "{tag} {stack_label}: cold {cold:.2} s, base edit {base_edit:.2} s, layer edit {layer_edit:.2} s, add a layer {add:.2} s, remove a layer {remove:.2} s, widen 2.7 to 3.2 m {resize:.2} s; RSS after cold {after_cold:.0} MiB, now {:.0} MiB; kept {}",
                    rss(),
                    kept(&session)
                );
            }
        }
    }
}
