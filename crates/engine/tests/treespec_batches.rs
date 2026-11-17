//! Generated foliage as portable instanced batches (`treespec::batches`), and the washroom's
//! foliage written as a project's `batches` for the Studio (ignored; see `write_washroom_batches`).
use std::path::PathBuf;
use superspec_engine::treespec::{
    FoliageMesh, HEIGHT_RATIO, InstanceBatch, RANDOM, SHADE_NORMAL, VARIATION,
    batches::{self, FoliageBatch, Paint},
    grass::{self, DensityMask, GrassSite, GrassTile, GroundMesh},
    tree,
};
use superspec_model::{
    Scene,
    batches::{InstanceBatch as Portable, Placement, place},
    instances::LocalMesh,
};

type V3 = [f64; 3];
const SUN: V3 = [-0.303, -0.505, 0.808];

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn mix(a: V3, b: V3, t: f64) -> V3 {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}
fn light(normal: V3, floor: f64) -> f64 {
    let length = dot(SUN, SUN).sqrt();
    floor + (1. - floor) * (dot(normal, SUN) / length).max(0.)
}
fn hills(low: [f64; 2], high: [f64; 2], cells: usize, relief: f64) -> GroundMesh {
    let mut ground = GroundMesh {
        positions: vec![],
        indices: vec![],
    };
    for row in 0..=cells {
        for column in 0..=cells {
            let x = low[0] + (high[0] - low[0]) * column as f64 / cells as f64;
            let y = low[1] + (high[1] - low[1]) * row as f64 / cells as f64;
            ground.positions.push([
                x,
                y,
                relief * ((x * 1.9).sin() * (y * 1.4 + 0.6).cos() + 0.4 * (x * 0.7 + y).sin()),
            ]);
        }
    }
    let width = cells as u32 + 1;
    for row in 0..cells as u32 {
        for column in 0..cells as u32 {
            let a = row * width + column;
            ground
                .indices
                .extend([a, a + 1, a + width + 1, a, a + width + 1, a + width]);
        }
    }
    ground
}
fn meadow() -> grass::GrassDefinition {
    grass::parse(
        "meadow.grass.json",
        include_str!("../src/treespec/fixtures/meadow.grass.json"),
    )
    .unwrap()
}
fn shrub() -> tree::GeneratedTree {
    tree::generate(
        &tree::parse(
            "shrub.tree.json",
            include_str!("../src/treespec/fixtures/shrub.tree.json"),
        )
        .unwrap(),
        &|| false,
    )
    .unwrap()
}
fn apply(transform: &[V3; 4], p: V3) -> V3 {
    std::array::from_fn(|axis| {
        transform[0][axis] * p[0]
            + transform[1][axis] * p[1]
            + transform[2][axis] * p[2]
            + transform[3][axis]
    })
}

#[test]
fn a_grass_patch_and_a_shrub_become_valid_batches_that_place_what_was_generated() {
    let ground = hills([0., 0.], [1.2, 1.2], 6, 0.05);
    let patch = grass::generate(
        &meadow(),
        &GrassSite {
            ground,
            mask: DensityMask::Uniform,
        },
        &|| false,
    )
    .unwrap();
    let level = 0;
    let converted = batches::grass(&patch, level, [0.; 3], None).unwrap();
    assert_eq!(converted.len(), patch.lods[level].meshes.len());
    let drawn: usize = patch
        .batches()
        .map(|batch| GrassTile::drawn(batch, patch.lods[level].keep))
        .sum();
    assert_eq!(
        converted
            .iter()
            .map(|b| b.batch.placements.len())
            .sum::<usize>(),
        drawn
    );
    // A placement puts a vertex where the generated transform does (delivered in single precision).
    let variant = 0;
    let source = patch
        .batches()
        .find(|batch| batch.mesh == variant && !batch.is_empty())
        .unwrap();
    let mesh = &patch.meshes[patch.lods[level].meshes[variant]];
    for instance in [0, GrassTile::drawn(source, 1.) - 1] {
        let placed = batches::placement(&source.transforms[instance]).unwrap();
        placed.validate().unwrap();
        for vertex in [0, mesh.positions.len() - 1] {
            let expected = apply(&source.transforms[instance], mesh.positions[vertex]);
            let found = place(&placed.delivered(), mesh.positions[vertex]);
            for axis in 0..3 {
                assert!(
                    (found[axis] - expected[axis]).abs() < 1e-5,
                    "{found:?} {expected:?}"
                );
            }
        }
    }
    let first = &converted[0].batch;
    for name in ["root", "groundNormal"] {
        assert_eq!(first.vectors[name].len(), first.placements.len(), "{name}");
    }
    for name in ["threshold", "variation", "random", "tile"] {
        assert_eq!(first.scalars[name].len(), first.placements.len(), "{name}");
    }
    // Each instance names its tile, and a tile's instances are consecutive.
    let tiles = &first.scalars[batches::TILE];
    assert!(tiles.windows(2).all(|pair| pair[0] <= pair[1]));
    assert!(
        tiles
            .iter()
            .all(|tile| (*tile as usize) < patch.tiles.len())
    );
    // Every level, each batch with the look and its own level, distance and keep.
    let look: superspec_model::batches::Foliage = serde_json::from_value(
        serde_json::json!({"family": "grass", "patch": "ignored", "far": 50, "material": "grass"}),
    )
    .unwrap();
    let levels = batches::grass_levels(&patch, [0.; 3], &look, None).unwrap();
    for (level, lod) in patch.lods.iter().enumerate() {
        let of_level: Vec<_> = levels
            .iter()
            .filter(|b| b.batch.foliage.as_ref().unwrap().level as usize == level)
            .collect();
        assert!(!of_level.is_empty());
        for converted in of_level {
            let foliage = converted.batch.foliage.as_ref().unwrap();
            assert_eq!((foliage.keep, foliage.distance), (lod.keep, lod.distance));
            assert_eq!(foliage.patch, patch.id);
            assert_eq!(foliage.material.as_deref(), Some("grass"));
            assert!(converted.batch.id.ends_with(&format!("/lod{level}")));
        }
    }
    for name in ["heightRatio", "bendWeight", "random", "element"] {
        assert_eq!(
            first.vertex_scalars[name].len(),
            mesh.positions.len(),
            "{name}"
        );
    }
    assert!(first.vertex_vectors.contains_key("shadeNormal"));
    // Blades are two-sided: both faces are delivered.
    assert_eq!(converted[0].mesh.indices.len(), mesh.indices.len() * 2);

    let plant = shrub();
    let offset = [3., -1., 0.5];
    let parts = batches::tree(&plant, 0, offset, None).unwrap();
    let leaves = parts.iter().find(|b| b.batch.id == "shrub/leaves").unwrap();
    assert_eq!(leaves.batch.placements.len(), plant.lods[0].leaf_instances);
    assert!(
        parts
            .iter()
            .any(|b| b.batch.id == "shrub/trunk" && b.batch.placements.len() == 1)
    );
    // A moved plant moves its points (root, pivot) and leaves its directions alone.
    let root = leaves.batch.vectors["root"][0];
    let generated = plant.leaves.vectors["root"][0];
    for axis in 0..3 {
        assert!((f64::from(root[axis]) - generated[axis] - offset[axis]).abs() < 1e-5);
        assert!(
            (leaves.batch.placements[0].translation[axis]
                - plant.leaves.transforms[0][3][axis]
                - offset[axis])
                .abs()
                < 1e-12
        );
    }
    assert_eq!(
        leaves.batch.vectors["shadeNormal"][0],
        plant.leaves.vectors["shadeNormal"][0].map(|v| v as f32)
    );

    // Every batch is admitted by the portable record, and survives a project document.
    let mut scene = Scene::empty(1, "test");
    let all: Vec<FoliageBatch> = converted.into_iter().chain(parts).collect();
    for FoliageBatch { batch, mesh } in &all {
        scene.add_batch(batch, mesh).unwrap();
    }
    superspec_model::batches::check(&scene).unwrap();
    let document = serde_json::json!({ "batches": batches::project_document(&all) });
    let mut again = Scene::empty(1, "test");
    superspec_model::batches::add_project(&mut again, &document).unwrap();
    assert_eq!(again.manifest.batches.len(), scene.manifest.batches.len());
    assert_eq!(
        again.batches.attributes.len(),
        scene.batches.attributes.len()
    );
    assert_eq!(
        again.batches.placements.len(),
        scene.batches.placements.len()
    );

    // A sheared or mirrored transform is refused, not approximated.
    let sheared = [[1., 0., 0.], [0.5, 1., 0.], [0., 0., 1.], [0.; 3]];
    assert!(
        batches::placement(&sheared)
            .unwrap_err()
            .contains("sheared")
    );
    let mirrored = [[1., 0., 0.], [0., 1., 0.], [0., 0., -1.], [0.; 3]];
    assert!(batches::placement(&mirrored).is_err());
}

#[test]
fn a_plant_becomes_a_batch_per_part_and_level_with_its_family() {
    use superspec_types::batch::FoliageFamily;
    let plant = shrub();
    let look: superspec_model::batches::Foliage = serde_json::from_value(serde_json::json!({
        "family": "leaves", "patch": "ignored", "far": 60, "material": "canopy", "canopy": {},
    }))
    .unwrap();
    let offset = [3., -1., 0.5];
    let all = batches::tree_levels(&plant, offset, &look).unwrap();
    let family = |batch: &FoliageBatch| batch.batch.foliage.as_ref().unwrap().family;
    for (level, lod) in plant.lods.iter().enumerate() {
        let of_level: Vec<_> = all
            .iter()
            .filter(|b| {
                b.batch.foliage.as_ref().unwrap().level as usize == level
                    && family(b) != FoliageFamily::Shadow
            })
            .collect();
        // Bark: the trunk and branches together, one instance at the plant's place.
        let bark = of_level
            .iter()
            .find(|b| family(b) == FoliageFamily::Bark)
            .unwrap();
        let parts = [Some(lod.trunk), lod.branches];
        let triangles: usize = parts
            .iter()
            .flatten()
            .map(|i| plant.meshes[*i].triangles())
            .sum();
        assert_eq!(bark.mesh.indices.len() / 3, triangles);
        assert_eq!(bark.batch.placements.len(), 1);
        assert_eq!(bark.batch.placements[0].translation, offset);
        assert_eq!(
            bark.batch.vertex_scalars["bendWeight"].len(),
            bark.mesh.positions.len()
        );
        let leaves = of_level.iter().find(|b| family(b) == FoliageFamily::Leaves);
        let hull = of_level.iter().find(|b| family(b) == FoliageFamily::Hull);
        match lod.hull {
            tree::HullUse::Impostor => assert!(leaves.is_none() && hull.is_some(), "level {level}"),
            tree::HullUse::Backing => assert!(leaves.is_some() && hull.is_some(), "level {level}"),
            tree::HullUse::None => assert!(leaves.is_some() && hull.is_none(), "level {level}"),
        }
        if let Some(leaves) = leaves {
            assert_eq!(leaves.batch.placements.len(), lod.leaf_instances);
            assert_eq!(leaves.batch.foliage.as_ref().unwrap().keep, lod.leaf_keep);
            assert_eq!(
                leaves.batch.vectors["shadeNormal"].len(),
                lod.leaf_instances
            );
        }
        for batch in &of_level {
            let foliage = batch.batch.foliage.as_ref().unwrap();
            assert_eq!(
                (foliage.patch.as_str(), foliage.distance),
                ("shrub", lod.distance)
            );
            assert_eq!(foliage.canopy, Some(Default::default()));
        }
    }
    assert_eq!(
        all.iter()
            .filter(|b| family(b) == FoliageFamily::Shadow)
            .count(),
        1
    );
    let mut scene = Scene::empty(1, "test");
    for FoliageBatch { batch, mesh } in &all {
        scene.add_batch(batch, mesh).unwrap();
    }
    superspec_model::batches::check(&scene).unwrap();
    let document = serde_json::json!({ "batches": batches::project_document(&all) });
    let mut again = Scene::empty(1, "test");
    superspec_model::batches::add_project(&mut again, &document).unwrap();
    assert_eq!(again.manifest.batches.len(), all.len());
    assert_eq!(again.manifest.batches[0].foliage, all[0].batch.foliage);
}

/// The lawn's height at a point of the plan (see `lawn_ground`).
fn lawn_height(x: f64, y: f64) -> f64 {
    let dx = (SLAB[0][0] - x).max(x - SLAB[1][0]).max(0.);
    let dy = (SLAB[0][1] - y).max(y - SLAB[1][1]).max(0.);
    let away = ((dx * dx + dy * dy).sqrt() / 1.2).min(1.);
    let roll =
        0.6 * (1.1 * x + 0.3).sin() * (0.9 * y - 0.5).cos() + 0.4 * (0.6 * x - 0.8 * y + 1.).sin();
    -0.03 + 0.25 * roll * away * away * (3. - 2. * away)
}

/// A plant definition `factor` times as large: every length, the envelope and the level distances.
fn scaled(mut definition: tree::TreeDefinition, factor: f64) -> tree::TreeDefinition {
    definition.trunk.height *= factor;
    definition.trunk.radius *= factor;
    match &mut definition.envelope {
        Some(tree::Envelope::Ellipsoid { center, radii }) => {
            *center = center.map(|v| v * factor);
            *radii = radii.map(|v| v * factor);
        }
        Some(tree::Envelope::Cone { base, top, radius }) => {
            *base *= factor;
            *top *= factor;
            *radius *= factor;
        }
        None => {}
    }
    definition.leaves.spacing *= factor;
    definition.leaves.size = definition.leaves.size.map(|v| v * factor);
    definition
}

/// Writes the washroom's plants (roadmap stage 7, `washroom-trees.json` in `TREESPEC_OUTPUT`,
/// default `generated/verification/treespec`): the shrub fixture and the broadleaf fixture at
/// 0.6 of its size, standing on the lawn of `write_washroom_lawn`, every level of each as
/// batches with their foliage looks (bark colored by `painted-bark`, leaves and hull by
/// `painted-canopy`). `tests/studio/washroom/build.mjs` adds them when asked for trees (`TREES=1`).
#[test]
#[ignore = "writes the washroom plants for the browser tree test"]
fn write_washroom_trees() {
    let out = std::env::var_os("TREESPEC_OUTPUT").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../generated/verification/treespec"),
        PathBuf::from,
    );
    std::fs::create_dir_all(&out).unwrap();
    let look: superspec_model::batches::Foliage = serde_json::from_value(serde_json::json!({
        "family": "leaves", "patch": "plant", "far": 60, "material": "painted-canopy",
        "wind": { "azimuth": 30, "angle": 2.5, "scale": 6, "speed": 0.8, "bias": 0.7, "flutter": 9 },
        "rejection": { "boundary": 0, "fill": 1, "grazing": 0, "grazingKeep": 0 },
        "canopy": {},
    }))
    .unwrap();
    let parse = |name: &str, text: &str| tree::parse(name, text).unwrap();
    // The washroom is seen from a few meters: levels step at 8 and 16 m (shrub) and 10 and 22 m (tree).
    // SHRUB_TREE names another shrub definition to place, for trying one out on the washroom lawn.
    let mut shrub_definition = match std::env::var("SHRUB_TREE") {
        Ok(path) => parse(&path, &std::fs::read_to_string(&path).unwrap()),
        Err(_) => parse(
            "shrub.tree.json",
            include_str!("../src/treespec/fixtures/shrub.tree.json"),
        ),
    };
    for (lod, distance) in shrub_definition.lods.iter_mut().zip([0., 8., 16.]) {
        lod.distance = distance;
    }
    let mut tree_definition = scaled(
        parse(
            "broadleaf.tree.json",
            include_str!("../src/treespec/fixtures/broadleaf.tree.json"),
        ),
        0.6,
    );
    for (lod, distance) in tree_definition.lods.iter_mut().zip([0., 10., 22.]) {
        lod.distance = distance;
    }
    let mut all = vec![];
    let mut plants = vec![];
    for (definition, at) in [
        (shrub_definition, [3.7, -1.1]),
        (tree_definition, [4.4, -4.1]),
    ] {
        let started = std::time::Instant::now();
        let plant = tree::generate(&definition, &|| false).unwrap();
        let generated = started.elapsed();
        let base = [at[0], at[1], lawn_height(at[0], at[1]) - 0.02];
        let mut levels = batches::tree_levels(&plant, base, &look).unwrap();
        for level in &mut levels {
            let foliage = level.batch.foliage.as_mut().unwrap();
            if foliage.family == superspec_types::batch::FoliageFamily::Bark {
                foliage.material = Some("painted-bark".into());
            }
        }
        println!(
            "{}: {} branches, {} leaf clusters, {} blobs, levels {:?} triangles, generated in {:.1} ms; {} batches",
            plant.id,
            plant.skeleton.len(),
            plant.leaves.len(),
            plant.canopy.blobs.len(),
            plant
                .lods
                .iter()
                .map(|lod| lod.triangles)
                .collect::<Vec<_>>(),
            generated.as_secs_f64() * 1000.,
            levels.len()
        );
        plants.push(serde_json::json!({
            "id": plant.id, "base": base, "branches": plant.skeleton.len(), "leafClusters": plant.leaves.len(),
            "blobs": plant.canopy.blobs.len(), "generatedMs": generated.as_secs_f64() * 1000., "mass": plant.mass,
            "lods": plant.lods.iter().map(|lod| serde_json::json!({
                "distance": lod.distance, "hull": lod.hull, "leafInstances": lod.leaf_instances, "triangles": lod.triangles,
            })).collect::<Vec<_>>(),
        }));
        all.extend(levels);
    }
    let mut scene = Scene::empty(1, "test");
    for FoliageBatch { batch, mesh } in &all {
        scene.add_batch(batch, mesh).unwrap();
    }
    superspec_model::batches::check(&scene).unwrap();
    let document =
        serde_json::json!({ "plants": plants, "batches": batches::project_document(&all) });
    let text = serde_json::to_vec(&document).unwrap();
    std::fs::write(out.join("washroom-trees.json"), &text).unwrap();
    println!(
        "washroom-trees.json: {} batches, {} bytes",
        all.len(),
        text.len()
    );
}

/// Writes the foliage of the washroom picture (the strip of ground with grass and the shrub
/// that `treespec_verification` writes as flattened GLBs) as a project's `batches`:
/// `washroom-foliage.batches.json` in `TREESPEC_OUTPUT` (default
/// `generated/verification/treespec`). `tests/treespec-batches-browser.mjs` opens it.
#[test]
#[ignore = "writes the washroom foliage batches for the browser comparison"]
fn write_washroom_batches() {
    let out = std::env::var_os("TREESPEC_OUTPUT").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../generated/verification/treespec"),
        PathBuf::from,
    );
    std::fs::create_dir_all(&out).unwrap();
    // The site of `treespec_verification::write_fixture_glbs`.
    let ground = hills([2.45, -2.45], [4.65, 0.4], 12, 0.03);
    let shrub_at = [3.6, -1.0];
    let (columns, rows, cell) = (45, 58, 0.05);
    let values = (0..rows)
        .flat_map(|row| (0..columns).map(move |column| (column, row)))
        .map(|(column, row)| {
            let (x, y) = (2.45 + column as f64 * cell, -2.45 + row as f64 * cell);
            let away = ((x - shrub_at[0]).powi(2) + (y - shrub_at[1]).powi(2)).sqrt();
            ((away - 0.2) / 0.25).clamp(0., 1.)
        })
        .collect();
    let patch = grass::generate(
        &meadow(),
        &GrassSite {
            ground: ground.clone(),
            mask: DensityMask::Field {
                origin: [2.45, -2.45],
                cell,
                columns,
                rows,
                values,
            },
        },
        &|| false,
    )
    .unwrap();
    // Stand-in colours, as the flattened GLBs have them: a root-to-tip gradient per vertex and
    // the variation per instance.
    let (tip_low, tip_high) = ([0.3, 0.5, 0.1], [0.5, 0.6, 0.16]);
    let grass_paint = Paint {
        vertex: &|mesh: &FoliageMesh, vertex| {
            let tone = 0.82 + 0.3 * mesh.scalars[RANDOM][vertex];
            mix(
                [0.05, 0.13, 0.03],
                mix(tip_low, tip_high, 0.5),
                mesh.scalars[HEIGHT_RATIO][vertex],
            )
            .map(|v| v * tone)
        },
        instance: &|batch: &InstanceBatch, instance| {
            let wanted = mix(tip_low, tip_high, batch.scalars[VARIATION][instance]);
            let middle = mix(tip_low, tip_high, 0.5);
            std::array::from_fn(|i| (wanted[i] / middle[i]) as f32)
        },
    };
    let palette = [[0.16, 0.42, 0.1], [0.5, 0.66, 0.16]];
    let plant_paint = Paint {
        vertex: &|mesh: &FoliageMesh, vertex| {
            let random = mesh.scalars[RANDOM][vertex];
            if mesh.material_slot == "bark" {
                let tone = (0.75 + 0.35 * random) * light(mesh.normals[vertex], 0.6);
                [0.26, 0.19, 0.13].map(|v| v * tone)
            } else {
                [(0.75 + 0.25 * mesh.scalars[HEIGHT_RATIO][vertex]) * (0.9 + 0.25 * random); 3]
            }
        },
        instance: &|batch: &InstanceBatch, instance| {
            let tone = light(batch.vectors[SHADE_NORMAL][instance], 0.5);
            mix(palette[0], palette[1], batch.scalars[VARIATION][instance])
                .map(|v| (v * tone) as f32)
        },
    };
    let mut all = vec![FoliageBatch {
        batch: Portable {
            id: "foliage/ground".into(),
            object_id: "foliage".into(),
            placements: vec![Placement::default()],
            ..Default::default()
        },
        mesh: LocalMesh {
            normals: vec![[0., 0., 1.]; ground.positions.len()],
            uvs: vec![[0.; 2]; ground.positions.len()],
            colors: vec![[0.12, 0.16, 0.06]; ground.positions.len()],
            positions: ground.positions,
            indices: ground.indices,
            tangents: vec![],
        },
    }];
    all.extend(batches::grass(&patch, 0, [0.; 3], Some(&grass_paint)).unwrap());
    let plant = shrub();
    let mut parts = batches::tree(
        &plant,
        0,
        [shrub_at[0], shrub_at[1], 0.],
        Some(&plant_paint),
    )
    .unwrap();
    // The bark's colours are in its vertices; only leaf clusters are tinted.
    for part in &mut parts {
        if !part.batch.id.ends_with("/leaves") {
            part.batch.tints.clear();
        }
    }
    all.extend(parts);
    let mut scene = Scene::empty(1, "test");
    for FoliageBatch { batch, mesh } in &all {
        scene.add_batch(batch, mesh).unwrap();
        println!(
            "{}: {} instances of {} triangles, {} named attributes",
            batch.id,
            batch.placements.len(),
            mesh.indices.len() / 3,
            batch.scalars.len()
                + batch.vectors.len()
                + batch.vertex_scalars.len()
                + batch.vertex_vectors.len()
        );
    }
    let counts = scene.batches.counts();
    println!(
        "{} batches, {} instances, {} mesh vertices stored once; packet section {} bytes",
        all.len(),
        counts.instances,
        counts.vertices,
        scene.packet().len()
    );
    let text = serde_json::to_vec(&batches::project_document(&all)).unwrap();
    std::fs::write(out.join("washroom-foliage.batches.json"), &text).unwrap();
    println!("washroom-foliage.batches.json: {} bytes", text.len());
}

/// The washroom's slab in plan (world XY after the model's quarter turn), which the lawn surrounds.
const SLAB: [[f64; 2]; 2] = [[-0.55, -2.45], [2.45, 0.4]];

/// The lawn outside the washroom (roadmap stage 6): an L of gently rolling ground in front of the
/// room's open sides, 15 cm cells over x -0.55 to 6.35 and y -5.45 to 0.4 with the slab's cells
/// left out. It sits 3 cm below the slab's top at the slab and rolls by up to 25 cm a meter away.
fn lawn_ground() -> GroundMesh {
    let (low, cell, columns, rows) = ([-0.55, -5.45], 0.15, 46usize, 39usize);
    let height = lawn_height;
    let mut ground = GroundMesh {
        positions: vec![],
        indices: vec![],
    };
    for row in 0..=rows {
        for column in 0..=columns {
            let (x, y) = (low[0] + column as f64 * cell, low[1] + row as f64 * cell);
            ground.positions.push([x, y, height(x, y)]);
        }
    }
    let width = columns as u32 + 1;
    for row in 0..rows as u32 {
        for column in 0..columns as u32 {
            let center = [
                low[0] + (column as f64 + 0.5) * cell,
                low[1] + (row as f64 + 0.5) * cell,
            ];
            if center[0] > SLAB[0][0]
                && center[0] < SLAB[1][0]
                && center[1] > SLAB[0][1]
                && center[1] < SLAB[1][1]
            {
                continue;
            }
            let a = row * width + column;
            ground
                .indices
                .extend([a, a + 1, a + width + 1, a, a + width + 1, a + width]);
        }
    }
    ground
}

/// Writes the washroom's lawn (`washroom-lawn.json` in `TREESPEC_OUTPUT`, default
/// `generated/verification/treespec`): the ground mesh and the meadow grown on it as instanced
/// batches, every level of detail, each with the foliage look the renderer draws it with
/// (painted, colored by the project's `painted-grass` material). `tests/studio/washroom/build.mjs`
/// adds both to the washroom when asked for grass (`GRASS=1`).
#[test]
#[ignore = "writes the washroom lawn for the browser grass test"]
fn write_washroom_lawn() {
    let out = std::env::var_os("TREESPEC_OUTPUT").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../generated/verification/treespec"),
        PathBuf::from,
    );
    std::fs::create_dir_all(&out).unwrap();
    let ground = lawn_ground();
    let mut definition = meadow();
    // The washroom is seen from a few meters: its levels step at 7 and 14 m and stop at 22 m.
    for (lod, distance) in definition.lods.iter_mut().zip([0., 7., 14.]) {
        lod.distance = distance;
    }
    let started = std::time::Instant::now();
    let patch = grass::generate(
        &definition,
        &GrassSite {
            ground: ground.clone(),
            mask: DensityMask::Uniform,
        },
        &|| false,
    )
    .unwrap();
    let generated = started.elapsed();
    let look: superspec_model::batches::Foliage = serde_json::from_value(serde_json::json!({
        "family": "grass", "patch": "lawn", "far": 22, "material": "painted-grass",
    }))
    .unwrap();
    let mut levels = batches::grass_levels(&patch, [0.; 3], &look, None).unwrap();
    for level in &mut levels {
        level.batch.id = level.batch.id.replacen(&patch.id, "lawn", 1);
        level.batch.object_id = "lawn".into();
        level.batch.foliage.as_mut().unwrap().patch = "lawn".into();
    }
    let mut scene = Scene::empty(1, "test");
    for FoliageBatch { batch, mesh } in &levels {
        scene.add_batch(batch, mesh).unwrap();
    }
    let counts = scene.batches.counts();
    // Every level draws a prefix of each tile's instances, and the tile index is consecutive.
    for level in &levels {
        let tiles = &level.batch.scalars[batches::TILE];
        assert!(
            tiles.windows(2).all(|pair| pair[0] <= pair[1]),
            "{}",
            level.batch.id
        );
    }
    println!(
        "lawn: {} candidates, {} clumps, {} blades at level 0, {} tiles; {} batches, {} instances over every level, {} mesh vertices; generated in {:.1} ms",
        patch.stats.candidates,
        patch.stats.instances,
        patch.stats.blades,
        patch.tiles.len(),
        levels.len(),
        counts.instances,
        counts.vertices,
        generated.as_secs_f64() * 1000.
    );
    let document = serde_json::json!({
        "ground": {
            "positions": ground.positions.iter().flatten().map(|v| *v as f32).collect::<Vec<_>>(),
            "indices": ground.indices,
        },
        "stats": patch.stats,
        "tiles": patch.tiles.len(),
        "batches": batches::project_document(&levels),
    });
    let text = serde_json::to_vec(&document).unwrap();
    std::fs::write(out.join("washroom-lawn.json"), &text).unwrap();
    println!("washroom-lawn.json: {} bytes", text.len());
}
