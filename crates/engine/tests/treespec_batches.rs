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
    scene.project_foliage_lods().unwrap();
    let document = serde_json::json!({ "batches": batches::project_document(&all) });
    let mut again = Scene::empty(1, "test");
    superspec_model::batches::add_project(&mut again, &document).unwrap();
    assert_eq!(scene.manifest.geometry_lods, again.manifest.geometry_lods);
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
    scene.project_foliage_lods().unwrap();
    let group = &scene.manifest.geometry_lods[0];
    assert_eq!(
        group
            .levels
            .iter()
            .map(|level| level.distance)
            .collect::<Vec<_>>(),
        plant
            .lods
            .iter()
            .map(|level| level.distance)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        group.levels.iter().flat_map(|level| &level.batches).count(),
        all.iter()
            .filter(|b| family(b) != FoliageFamily::Shadow)
            .count()
    );
    let document = serde_json::json!({ "batches": batches::project_document(&all) });
    let mut again = Scene::empty(1, "test");
    superspec_model::batches::add_project(&mut again, &document).unwrap();
    assert_eq!(again.manifest.batches.len(), all.len());
    assert_eq!(scene.manifest.geometry_lods, again.manifest.geometry_lods);
    assert_eq!(again.manifest.batches[0].foliage, all[0].batch.foliage);

    // The document states every number as the single that is delivered, in the fewest digits that
    // identify it (at most 9), and leaves out all-white vertex colours: what it delivers is what
    // the batches deliver. (A placement is renormalized from the singles it is read as, so it may
    // differ from one made of doubles in its last place.)
    let (read, made) = (&again.batches, &scene.batches);
    assert_eq!(read.positions, made.positions);
    assert_eq!(read.normals, made.normals);
    assert_eq!(read.uvs, made.uvs);
    assert_eq!(read.colors, made.colors);
    assert_eq!(read.indices, made.indices);
    assert_eq!(read.attributes, made.attributes);
    assert_eq!(read.placements.len(), made.placements.len());
    for (a, b) in read.placements.iter().zip(&made.placements) {
        assert!((a - b).abs() <= 1e-6 * b.abs().max(1.), "{a} {b}");
    }
    let listed = document["batches"].as_array().unwrap();
    for (FoliageBatch { batch, mesh }, written) in all.iter().zip(listed) {
        let read = |value: &serde_json::Value| -> Vec<f32> {
            let numbers = value.as_array().unwrap();
            numbers.iter().map(|v| v.as_f64().unwrap() as f32).collect()
        };
        let positions: Vec<f32> = mesh.positions.iter().flatten().map(|v| *v as f32).collect();
        assert_eq!(read(&written["mesh"]["positions"]), positions);
        assert!(written["mesh"].get("colors").is_none(), "{}", batch.id);
        for (name, values) in &batch.vertex_scalars {
            assert_eq!(&read(&written["mesh"]["scalars"][name]), values, "{name}");
        }
        for (name, values) in &batch.vectors {
            assert_eq!(read(&written["vectors"][name]), values.concat(), "{name}");
        }
        for (placement, instance) in batch
            .placements
            .iter()
            .zip(written["instances"].as_array().unwrap())
        {
            assert_eq!(
                read(&instance["rotation"]),
                placement.rotation.map(|v| v as f32)
            );
        }
    }
    let text = serde_json::to_string(&listed).unwrap();
    let longest = text
        .split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .filter(|token| token.contains('.'))
        .map(|token| token.replace('.', "").trim_start_matches('0').len())
        .max()
        .unwrap();
    assert!(longest <= 9, "a number of {longest} digits");
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

/// The lane plants' grade of the painted colours (`write_lane_plants`).
const LANE_PALETTE: superspec_types::batch::FoliagePalette =
    superspec_types::batch::FoliagePalette {
        // Matched to the sunny avenue concept's foliage: its shade, mid and lit values about
        // #2c362b, #424c39 and #7f8358 (a muted olive, warm in the light, deep in the shade),
        // sampled in the avenue view lit from behind the camera.
        hue: -26.,
        saturation: 0.66,
        value: 0.8,
        shade: 0.6,
        shade_hue: -34.,
        contrast: 1.2,
        depth: 2.0,
        // More, finer steps than the milestone's six, so lit and shaded sides step gradually,
        // and a deep end for the crown's occluded inside.
        bands: 14.,
        deep: 0.26,
    };

/// A plant's painted mass (roadmap milestone `painted-trees-milestone`), in the plant's own space
/// (its base at the origin), with the milestone's settings:
/// - every leaf cluster, clumped or not (the mass model only sorts them, so the plant without it
///   has them all), as `[x, y, z, size]`: the cluster's middle and length, for the density field;
/// - every leaf-bearing branch's leafy stretch (from `leaves.start` to its tip) as 9 points: the
///   sprays grow along them.
fn painted_mass(
    definition: &tree::TreeDefinition,
    plant: &tree::GeneratedTree,
) -> superspec_types::batch::FoliageMass {
    let all_leaves = tree::generate(
        &tree::TreeDefinition {
            mass: None,
            ..definition.clone()
        },
        &|| false,
    )
    .unwrap()
    .leaves;
    let sites = all_leaves
        .transforms
        .iter()
        .map(|[_, along, _, anchor]| {
            let size = along.iter().map(|v| v * v).sum::<f64>().sqrt();
            [
                anchor[0] + 0.5 * along[0],
                anchor[1] + 0.5 * along[1],
                anchor[2] + 0.5 * along[2],
                size,
            ]
        })
        .collect();
    let twigs = plant
        .skeleton
        .iter()
        .filter(|b| b.level >= definition.leaves.level && b.length > 0.)
        .map(|b| {
            (0..9)
                .map(|k| {
                    let t =
                        definition.leaves.start + (1. - definition.leaves.start) * k as f64 / 8.;
                    b.at(t).0
                })
                .collect()
        })
        .collect();
    superspec_types::batch::FoliageMass {
        sites,
        twigs,
        ..Default::default()
    }
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
    // TREE_TREE names another tree definition to place in the broadleaf's stead, at its own size and level distances.
    let tree_definition = match std::env::var("TREE_TREE") {
        Ok(path) => parse(&path, &std::fs::read_to_string(&path).unwrap()),
        Err(_) => {
            let mut definition = scaled(
                parse(
                    "broadleaf.tree.json",
                    include_str!("../src/treespec/fixtures/broadleaf.tree.json"),
                ),
                0.6,
            );
            for (lod, distance) in definition.lods.iter_mut().zip([0., 10., 22.]) {
                lod.distance = distance;
            }
            definition
        }
    };
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
        // The painted trees' sites and twigs (`painted_mass`), in the scene, flat `[x, y, z, size, ...]` and
        // `[x, y, z, ...]` per twig.
        let mass = painted_mass(&definition, &plant);
        let sites: Vec<f64> = mass
            .sites
            .iter()
            .flat_map(|[x, y, z, size]| [x + base[0], y + base[1], z + base[2], *size])
            .map(|v| (v * 1000.).round() / 1000.)
            .collect();
        let twigs: Vec<Vec<f64>> = mass
            .twigs
            .iter()
            .map(|twig| {
                twig.iter()
                    .flat_map(|p| [p[0] + base[0], p[1] + base[1], p[2] + base[2]])
                    .map(|v| (v * 1000.).round() / 1000.)
                    .collect()
            })
            .collect();
        plants.push(serde_json::json!({
            "twigs": twigs,
            "id": plant.id, "base": base, "branches": plant.skeleton.len(), "leafClusters": plant.leaves.len(),
            "blobs": plant.canopy.blobs.len(), "generatedMs": generated.as_secs_f64() * 1000., "mass": plant.mass,
            "sites": sites,
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

/// The scene's pedestrian avenue along the Kowloon building's right side
/// (`tests/studio/washroom-items/avenue.json`, which `build.mjs` places as
/// `kowloon/pedestrian-avenue`): the one source of where its median, trees and shrubs stand.
fn avenue() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../../tests/studio/washroom-items/avenue.json"
    ))
    .unwrap()
}

/// The scene point `along` and `across` (meters) from the avenue's origin.
fn avenue_point(avenue: &serde_json::Value, along: f64, across: f64) -> [f64; 2] {
    let v = |key: &str, k: usize| avenue[key][k].as_f64().unwrap();
    [0, 1].map(|k| v("origin", k) + along * v("along", k) + across * v("across", k))
}

/// The ground of the avenue's median planting: cells of about 15 cm over the planter's soil
/// inside its kerbs, a few centimeters above it and rolling by a few, below the kerbs' top.
fn lane_ground() -> GroundMesh {
    let avenue = avenue();
    let m = |key: &str| avenue["median"][key].as_f64().unwrap();
    let inset = m("kerb") + 0.005;
    let along = [m("start") + inset, m("end") - inset];
    let across = [
        m("offset") - m("width") / 2. + inset,
        m("offset") + m("width") / 2. - inset,
    ];
    let soil = avenue["origin"][2].as_f64().unwrap() + m("rise") - m("soil");
    let rows = ((along[1] - along[0]) / 0.15).round().max(1.) as usize;
    let columns = ((across[1] - across[0]) / 0.15).round().max(1.) as usize;
    let height = |x: f64, y: f64| {
        soil + 0.052
            + 0.03 * ((1.3 * x + 0.4).sin() * (0.8 * y).cos() + 0.5 * (0.5 * x - 0.9 * y).sin())
    };
    let mut ground = GroundMesh {
        positions: vec![],
        indices: vec![],
    };
    for row in 0..=rows {
        for column in 0..=columns {
            let [x, y] = avenue_point(
                &avenue,
                along[0] + (along[1] - along[0]) * row as f64 / rows as f64,
                across[0] + (across[1] - across[0]) * column as f64 / columns as f64,
            );
            ground.positions.push([x, y, height(x, y)]);
        }
    }
    // Columns run across and rows along: the triangles face up when across turns
    // counterclockwise of along.
    let v = |key: &str, k: usize| avenue[key][k].as_f64().unwrap();
    let turned = v("along", 0) * v("across", 1) - v("along", 1) * v("across", 0) > 0.;
    let width = columns as u32 + 1;
    for row in 0..rows as u32 {
        for column in 0..columns as u32 {
            let a = row * width + column;
            ground.indices.extend(if turned {
                [a, a + width + 1, a + 1, a, a + width, a + width + 1]
            } else {
                [a, a + 1, a + width + 1, a, a + width + 1, a + width]
            });
        }
    }
    ground
}

/// The lane groundcover's grade: the trees' (`LANE_PALETTE`), darker and with deeper shade, as
/// the concept's planting: dark mounds with sunlit tops.
const GROUNDCOVER_PALETTE: superspec_types::batch::FoliagePalette =
    superspec_types::batch::FoliagePalette {
        value: 0.6,
        shade: 0.5,
        ..LANE_PALETTE
    };

/// The median's planting: dense, low mounds of groundcover between its planter kerbs, painted
/// as one mass made of the leaf sites and twigs of many small shrubs (each its own seed and
/// size, on a staggered row about 0.6 m apart). It is carried by an invisible box over the
/// planter, a `shadow` batch (drawn at every level, casting nothing for a mass), so the planting
/// is culled and drawn as one.
fn groundcover(
    avenue: &serde_json::Value,
    look: &superspec_model::batches::Foliage,
    height: &dyn Fn(f64, f64) -> f64,
) -> FoliageBatch {
    use superspec_types::batch::{FoliageFamily, FoliageMass};
    let m = |key: &str| avenue["median"][key].as_f64().unwrap();
    let inset = m("kerb") + 0.12;
    let along = [m("start") + inset, m("end") - inset];
    let across = [
        m("offset") - m("width") / 2. + inset,
        m("offset") + m("width") / 2. - inset,
    ];
    let [bx, by] = avenue_point(avenue, along[0], across[0]);
    let base = [bx, by, height(bx, by)];
    let shrub = tree::parse(
        "shrub.tree.json",
        include_str!("../src/treespec/fixtures/shrub.tree.json"),
    )
    .unwrap();
    let spacing = 0.6;
    let rows = ((across[1] - across[0]) / spacing).floor() as usize + 1;
    let (mut sites, mut twigs) = (vec![], vec![]);
    let mut k = 0u64;
    let mut station = along[0];
    while station <= along[1] {
        for row in 0..rows {
            k += 1;
            // A stable jitter in [-0.5, 0.5) per mound and purpose.
            let jitter = |n: u64| {
                ((k.wrapping_mul(2_654_435_761) ^ n.wrapping_mul(40_503)) % 1000) as f64 / 1000.
                    - 0.5
            };
            let s = station + 0.5 * spacing * (row % 2) as f64 + 0.3 * spacing * jitter(1);
            if s > along[1] {
                continue;
            }
            let t = if rows == 1 {
                0.5 * (across[0] + across[1])
            } else {
                across[0] + (across[1] - across[0]) * row as f64 / (rows - 1) as f64
            } + 0.2 * spacing * jitter(2);
            let mut definition = scaled(shrub.clone(), 0.3 + 0.1 * (jitter(3) + 0.5));
            definition.seed = shrub.seed + k;
            let plant = tree::generate(&definition, &|| false).unwrap();
            let mass = painted_mass(&definition, &plant);
            let [x, y] = avenue_point(avenue, s, t);
            let o = [x - base[0], y - base[1], height(x, y) - 0.04 - base[2]];
            sites.extend(
                mass.sites
                    .iter()
                    .map(|[a, b, c, size]| [a + o[0], b + o[1], c + o[2], *size]),
            );
            twigs.extend(mass.twigs.into_iter().map(|twig| {
                twig.into_iter()
                    .map(|p| [p[0] + o[0], p[1] + o[1], p[2] + o[2]])
                    .collect::<Vec<_>>()
            }));
        }
        station += spacing;
    }
    // The carrier: the shrub's shadow proxy batch with its mesh replaced by the box around the
    // planting's sites.
    let template = tree::generate(&shrub, &|| false).unwrap();
    let mut carrier = batches::tree_levels(&template, base, look)
        .unwrap()
        .into_iter()
        .find(|level| level.batch.foliage.as_ref().unwrap().family == FoliageFamily::Shadow)
        .expect("the shrub has a shadow proxy");
    let (mut low, mut high) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for site in &sites {
        for axis in 0..3 {
            low[axis] = low[axis].min(site[axis] - 0.2);
            high[axis] = high[axis].max(site[axis] + 0.2);
        }
    }
    let corner = |i: usize| {
        [
            if i & 1 == 0 { low[0] } else { high[0] },
            if i & 2 == 0 { low[1] } else { high[1] },
            if i & 4 == 0 { low[2] } else { high[2] },
        ]
    };
    carrier.mesh = superspec_model::instances::LocalMesh {
        positions: (0..8).map(corner).collect(),
        normals: vec![[0., 0., 1.]; 8],
        uvs: vec![[0., 0.]; 8],
        colors: vec![[1.; 3]; 8],
        indices: vec![
            0, 2, 1, 1, 2, 3, 4, 5, 6, 5, 7, 6, 0, 1, 4, 1, 5, 4, 2, 6, 3, 3, 6, 7, 0, 4, 2, 2, 4,
            6, 1, 3, 5, 3, 7, 5,
        ],
        tangents: vec![],
    };
    carrier.batch.vertex_scalars.clear();
    carrier.batch.vertex_vectors.clear();
    carrier.batch.id = "lane-groundcover/shadow".into();
    carrier.batch.object_id = "lane-groundcover".into();
    let foliage = carrier.batch.foliage.as_mut().unwrap();
    foliage.patch = "lane-groundcover".into();
    // Millimeters are plenty, and keep the document small.
    let mm = |v: f64| (v * 1000.).round() / 1000.;
    foliage.mass = Some(FoliageMass {
        sites: sites.into_iter().map(|s| s.map(mm)).collect(),
        twigs: twigs
            .into_iter()
            .map(|twig| twig.into_iter().map(|p| p.map(mm)).collect())
            .collect(),
        radius: 0.36,
        cell: 0.12,
        // Full: a continuous, hedge-like mass of rounded low shrubs.
        level: 0.42,
        card_keep: 0.9,
        form: 0.9,
        jitter: 0.1,
        leaf_scale: 1.,
        soft_edges: false,
        spray_scale: 1.,
        card_scale: 0.6,
        shapes: 0.3,
        variation: 0.8,
        sparkles: 0.4,
        palette: GROUNDCOVER_PALETTE,
        look: String::new(),
    });
    carrier
}

/// Writes the lane's plants (`lane-plants.json` in `TREESPEC_OUTPUT`, default
/// `generated/verification/treespec`): the median's soil (`lane_ground`) and its planting, the
/// avenue's birches and shrubs painted as the milestone's trees (each a `FoliageMass`) standing
/// in a row on the median along the Kowloon building's right side, and the dense groundcover
/// around them (`groundcover`).
/// `tests/studio/washroom-items/build.mjs` adds them with the building (`LANE=1`).
#[test]
#[ignore = "writes the lane plants for the item building picture"]
fn write_lane_plants() {
    let out = std::env::var_os("TREESPEC_OUTPUT").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../generated/verification/treespec"),
        PathBuf::from,
    );
    std::fs::create_dir_all(&out).unwrap();
    // The median's soil (the planting's ground): no grass on it, the groundcover below covers it.
    let ground = lane_ground();
    let mut all: Vec<FoliageBatch> = vec![];
    let look: superspec_model::batches::Foliage = serde_json::from_value(serde_json::json!({
        "family": "leaves", "patch": "plant", "far": 60, "material": "painted-canopy",
        "wind": { "azimuth": 30, "angle": 2.5, "scale": 6, "speed": 0.8, "bias": 0.7, "flutter": 9 },
        "rejection": { "boundary": 0, "fill": 1, "grazing": 0, "grazingKeep": 0 },
        "canopy": {},
    }))
    .unwrap();
    // The painted trees of the milestone (`painted-mass/milestone-birch.png`): the birch, scaled to a
    // avenue tree reaching the building's third storey (about 9 m) with an upright street-tree
    // habit for a narrow avenue (trunk line 4.1 m from either side): shorter, steeper limbs and
    // leaves kept within 1.7 m of the trunk (the painted crown, with its cards, about 2.4 m),
    // from 3.2 m up, so awnings, lamps and walkways are clear.
    // Full, as the milestone's (core cut near 0.85, over half the cards kept): gaps come from
    // between its clusters, not from thinning; each cluster lit as its own rounded mass (lit
    // top, dark beneath), with little per-card scatter, and leaves painted larger, as the
    // trees are seen from across a street.
    let birch = |factor: f64| {
        let mut definition = scaled(
            tree::parse(
                "birch.tree.json",
                include_str!("../src/treespec/fixtures/birch.tree.json"),
            )
            .unwrap(),
            factor,
        );
        if let Some(tree::Envelope::Ellipsoid { center, radii }) = &mut definition.envelope {
            let top = center[2] + radii[2];
            let foot = 3.2_f64.min(top - 3.);
            *center = [0., 0., 0.5 * (top + foot)];
            *radii = [radii[0].min(1.7), radii[1].min(1.7), 0.5 * (top - foot)];
        }
        if let Some(limbs) = definition.levels.first_mut() {
            limbs.length = [0.24, 0.12];
            limbs.angle = [44., 26.];
            limbs.start = limbs.start.max(0.38);
            // Bold limbs: nothing drawn thinner than about 3 cm, as an illustration avoids hairlines.
            limbs.tip = 0.5;
        }
        definition.trunk.radius *= 1.25;
        if let Some(twigs) = definition.levels.get_mut(1) {
            twigs.radius = 0.6;
            twigs.tip = 0.6;
        }
        let look = superspec_types::batch::FoliageMass {
            level: 0.88,
            card_keep: 0.6,
            form: 0.9,
            jitter: 0.1,
            leaf_scale: 1.5,
            soft_edges: false,
            // Longer sprays of pointed leaves at the clusters' rims, so they read as leaves.
            spray_scale: 1.25,
            // Smaller cards and more of them, of varied size and shape (a third wide and flat),
            // with tiny bright leaves on the lit clumps.
            card_scale: 0.7,
            shapes: 0.35,
            variation: 0.8,
            sparkles: 0.5,
            ..Default::default()
        };
        (definition, look)
    };
    // The shrub painted the same way, at the milestone's default core level.
    let mut shrub = tree::parse(
        "shrub.tree.json",
        include_str!("../src/treespec/fixtures/shrub.tree.json"),
    )
    .unwrap();
    for (lod, distance) in shrub.lods.iter_mut().zip([0., 8., 16.]) {
        lod.distance = distance;
    }
    let shrub = (
        shrub,
        superspec_types::batch::FoliageMass {
            form: 0.8,
            jitter: 0.15,
            leaf_scale: 1.3,
            soft_edges: false,
            card_scale: 0.6,
            shapes: 0.3,
            variation: 0.8,
            sparkles: 0.4,
            ..Default::default()
        },
    );
    let height = |x: f64, y: f64| {
        ground
            .positions
            .iter()
            .min_by(|a, b| {
                ((a[0] - x).powi(2) + (a[1] - y).powi(2))
                    .total_cmp(&((b[0] - x).powi(2) + (b[1] - y).powi(2)))
            })
            .filter(|p| (p[0] - x).abs() < 0.2 && (p[1] - y).abs() < 0.2)
            .map_or(0., |p| p[2])
    };
    // Trees and shrubs, each painted as one mass: its bark and shadow proxy are batches, and
    // its foliage is the mass on the shadow proxy, which a renderer draws in place of leaves and
    // hull, so those are left out (a Studio project holds at most 128 MiB).
    // They stand in a row down the avenue's median (`avenue.json`): the trees at their stations,
    // alternately the larger and the smaller birch, and a shrub between each two.
    let avenue = avenue();
    let stations = |key: &str| -> Vec<f64> {
        let v = |field: &str| avenue[key][field].as_f64().unwrap();
        (0..v("count") as usize)
            .map(|i| v("first") + i as f64 * v("spacing"))
            .collect()
    };
    let middle = avenue["median"]["offset"].as_f64().unwrap();
    // The lane's plants are drawn in the illustrated `leaves` look (`FoliageMass::look`; the
    // groundcover in `sprays`, a tenth of the triangles for a bed seen from above): every leaf a
    // shape of its own on the twigs. The trees are the street tree (`street.tree.json`: a clear
    // bole forking into rising limbs under a full crown), each station its own seed and size, its
    // twigs kept within 2.9 m of the trunk and above 3.2 m (the avenue is narrow).
    // LANE_LOOK=painted draws them all as painted masses instead, the trees as the birches above
    // with their painted settings; any other LANE_LOOK names the look of every plant. LANE_TREE
    // (street or birch) picks the tree whatever the look.
    let street = |station: usize| {
        let mut definition = scaled(
            tree::parse(
                "street.tree.json",
                include_str!("../src/treespec/fixtures/street.tree.json"),
            )
            .unwrap(),
            [1.0, 0.92, 1.05][station % 3],
        );
        definition.seed += 7 * station as u64;
        if let Some(tree::Envelope::Ellipsoid { center, radii }) = &mut definition.envelope {
            let top = center[2] + radii[2];
            let foot = 3.2_f64.min(top - 3.);
            *center = [0., 0., 0.5 * (top + foot)];
            *radii = [radii[0].min(2.9), radii[1].min(2.9), 0.5 * (top - foot)];
        }
        // Steeper, shorter limbs for the narrower crown, so they fork up through it.
        if let Some(limbs) = definition.levels.first_mut() {
            limbs.length = [0.52, 0.36];
            limbs.angle = [52., 22.];
        }
        (definition, superspec_types::batch::FoliageMass::default())
    };
    let asked = std::env::var("LANE_LOOK").unwrap_or_else(|_| "leaves".into());
    let plant_look = if asked == "painted" {
        String::new()
    } else {
        asked
    };
    let street_tree =
        std::env::var("LANE_TREE").map_or(!plant_look.is_empty(), |tree| tree == "street");
    let mut row = vec![];
    for (i, along) in stations("trees").into_iter().enumerate() {
        let (definition, mut mass) = if street_tree {
            street(i)
        } else {
            birch([1.08, 1.0][i % 2])
        };
        mass.look = plant_look.clone();
        row.push(((definition, mass), avenue_point(&avenue, along, middle)));
    }
    for along in stations("shrubs") {
        let (definition, mut mass) = shrub.clone();
        mass.look = plant_look.clone();
        row.push(((definition, mass), avenue_point(&avenue, along, middle)));
    }
    for (index, ((definition, mass), at)) in row.into_iter().enumerate() {
        use superspec_types::batch::FoliageFamily;
        let plant = tree::generate(&definition, &|| false).unwrap();
        let base = [at[0], at[1], height(at[0], at[1]) - 0.02];
        // Beside the weathered building the greens are graded to the avenue concept's
        // (`LANE_PALETTE`): a muted olive, darker, with deep shade and warm, restrained lights.
        let painted = superspec_types::batch::FoliageMass {
            level: mass.level,
            card_keep: mass.card_keep,
            form: mass.form,
            leaf_scale: mass.leaf_scale,
            soft_edges: mass.soft_edges,
            spray_scale: mass.spray_scale,
            card_scale: mass.card_scale,
            shapes: mass.shapes,
            variation: mass.variation,
            sparkles: mass.sparkles,
            jitter: mass.jitter,
            look: mass.look.clone(),
            palette: LANE_PALETTE,
            ..painted_mass(&definition, &plant)
        };
        let mut levels = batches::tree_levels(&plant, base, &look).unwrap();
        levels.retain(|level| {
            !matches!(
                level.batch.foliage.as_ref().unwrap().family,
                FoliageFamily::Leaves | FoliageFamily::Hull
            )
        });
        for level in &mut levels {
            // Each plant is an object and a patch of its own, so two of one definition share
            // neither IDs nor the patch's sway origin.
            level.batch.id = format!("lane-plant-{index}/{}", level.batch.id);
            level.batch.object_id = format!("lane-plant-{index}");
            let foliage = level.batch.foliage.as_mut().unwrap();
            foliage.patch = format!("lane-plant-{index}");
            match foliage.family {
                FoliageFamily::Bark => foliage.material = Some("painted-bark".into()),
                FoliageFamily::Shadow => foliage.mass = Some(painted.clone()),
                _ => {}
            }
        }
        assert!(
            levels
                .iter()
                .any(|level| level.batch.foliage.as_ref().unwrap().mass.is_some()),
            "every lane plant has a shadow proxy to carry its painted mass"
        );
        all.extend(levels);
    }
    let mut bed = groundcover(&avenue, &look, &height);
    if !plant_look.is_empty() {
        let mass = bed.batch.foliage.as_mut().unwrap().mass.as_mut().unwrap();
        mass.look = if plant_look == "leaves" {
            "sprays".into()
        } else {
            plant_look.clone()
        };
    }
    all.push(bed);
    let mut scene = Scene::empty(1, "test");
    for FoliageBatch { batch, mesh } in &all {
        scene.add_batch(batch, mesh).unwrap();
    }
    superspec_model::batches::check(&scene).unwrap();
    let document = serde_json::json!({
        "ground": {
            "positions": ground.positions.iter().flatten().map(|v| *v as f32).collect::<Vec<_>>(),
            "indices": ground.indices,
        },
        "batches": batches::project_document(&all),
    });
    let text = serde_json::to_vec(&document).unwrap();
    std::fs::write(out.join("lane-plants.json"), &text).unwrap();
    println!(
        "lane-plants.json: {} batches, {} groundcover leaf sites, {} bytes",
        all.len(),
        all.iter()
            .filter_map(|level| level.batch.foliage.as_ref()?.mass.as_ref())
            .filter(|mass| mass.cell < 0.2)
            .map(|mass| mass.sites.len())
            .sum::<usize>(),
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
/// A square test lawn for shading and shadows (`LAWN_SITE=hill`): 7 m a side, a hill gentle to the north and steep to
/// the south, and a bank about 0.7 m high, near vertical, along its east edge, so some grass faces away from the sun
/// and some stands on slopes too steep for it.
fn hill_ground() -> GroundMesh {
    let (low, cell, cells) = ([-0.6, -6.4], 0.1, 70usize);
    let height = |x: f64, y: f64| {
        let (dx, dy) = (x - 2.6, y + 2.9);
        // Wide to the north, narrow (steep) to the south.
        let sy = if dy < 0. { 0.6 } else { 1.5 };
        let hill = 1.1 * (-(dx * dx / (2. * 1.4 * 1.4) + dy * dy / (2. * sy * sy))).exp();
        let bank = 0.7 / (1. + (-(x - 5.2) / 0.06).exp());
        -0.03 + hill + bank
    };
    let mut ground = GroundMesh {
        positions: vec![],
        indices: vec![],
    };
    for row in 0..=cells {
        for column in 0..=cells {
            let (x, y) = (low[0] + column as f64 * cell, low[1] + row as f64 * cell);
            ground.positions.push([x, y, height(x, y)]);
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
    // LAWN_SITE=hill writes the square test lawn with a hill and a steep bank instead (hill_ground).
    let ground = if std::env::var("LAWN_SITE").is_ok_and(|v| v == "hill") {
        hill_ground()
    } else {
        lawn_ground()
    };
    let mut definition = lawn_definition();
    // LAWN_LEVEL=k writes level k alone, from 0 m, to picture it up close.
    if let Some(level) = std::env::var("LAWN_LEVEL")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
    {
        let mut only = definition.lods[level];
        only.distance = 0.;
        definition.lods = vec![only];
    }
    write_lawn(&out, ground, definition);
}

/// The washroom lawn's grass: the meadow fixture made fuller, calmer and even.
fn lawn_definition() -> grass::GrassDefinition {
    let mut definition = meadow();
    // The washroom is seen from a few meters: its levels step at 7 and 14 m. Two far levels follow for a large
    // field, a painter's quick strokes: at 22 m half of each clump's blades, 2.2 times as wide, and at 30 m a third,
    // 3.5 times as wide, each keeping fewer clumps; the grass stops at 40 m.
    // Fuller blades than the meadow fixture's: half as wide again, and narrowing less toward the tip.
    definition.blade.width = definition.blade.width.map(|w| w * 1.5);
    definition.blade.taper = 0.42;
    // And fewer of them: wider blades hold the cover with about a quarter fewer clumps.
    definition.density = 105.;
    // A calm meadow: blades from 4 to 13 cm, each clump scaled from 0.6 to 1.4.
    definition.blade.height = [0.04, 0.13];
    // Even cover: the meadow fixture's drifts clear about 40 % of the clumps between them; here only a little.
    definition.clumping.strength = 0.15;
    definition.scale = [0.6, 1.4];
    // The cover holds with distance: the middle levels keep more of the clumps than the fixture's (0.75 and 0.45, from
    // 0.5 and 0.2) and start farther (12 and 20 m), their survivors widened to fill the gaps.
    definition.lods[1].keep = 0.75;
    definition.lods[2].keep = 0.45;
    let mut far = definition.lods[2];
    far.keep = 0.2;
    far.blades = 0.5;
    far.chubby = 2.2;
    let mut farther = far;
    farther.keep = 0.1;
    farther.blades = 0.34;
    farther.chubby = 3.5;
    definition.lods.extend([far, farther]);
    for (lod, distance) in definition.lods.iter_mut().zip([0., 12., 20., 28., 36.]) {
        lod.distance = distance;
    }
    definition
}

/// Grows `definition` on `ground` and writes the lawn document `washroom-lawn.json` in `out`.
fn write_lawn(out: &std::path::Path, ground: GroundMesh, definition: grass::GrassDefinition) {
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
    // Grass everywhere, as games draw it: every blade kept. LAWN_BOUNDARY=1 writes the stage-6 look instead, whose
    // painted profile keeps blades only near shade boundaries (the default rejection).
    let rejection = if std::env::var("LAWN_BOUNDARY").is_ok_and(|v| v == "1") {
        serde_json::json!({})
    } else {
        serde_json::json!({ "rejection": { "boundary": 0, "fill": 1, "grazing": 0, "grazingKeep": 0 } })
    };
    let mut fields = serde_json::json!({
        "family": "grass", "patch": "lawn", "far": 40, "material": "painted-grass",
    });
    fields
        .as_object_mut()
        .unwrap()
        .extend(rejection.as_object().unwrap().clone());
    let look: superspec_model::batches::Foliage = serde_json::from_value(fields).unwrap();
    let mut levels = batches::grass_levels(&patch, [0.; 3], &look, None).unwrap();
    for level in &mut levels {
        level.batch.id = level.batch.id.replacen(&patch.id, "lawn", 1);
        level.batch.object_id = "lawn".into();
        level.batch.foliage.as_mut().unwrap().patch = "lawn".into();
    }
    // Accent grass (the web painterly study's highlight grass): a patch of its own, drawn 2 cm above the ground, its
    // blades tipped by a palette, as quick strokes laid over the lawn; the renderer can blur it alone
    // (`?grassblur=<px>&grassblurpatch=lawn-accent`). Its near three levels only. Off for now while the base grass is tuned: LAWN_ACCENT=1 adds it.
    if std::env::var("LAWN_ACCENT").is_ok_and(|v| v == "1") && std::env::var("LAWN_LEVEL").is_err()
    {
        // The web study's highlight grass: a few blades a clump, near the contour grass's width but much taller and of
        // very uneven height, gathered in drifts with fractal edges (strong 2-octave clumping: clean blobs, a little ragged); about a third as many blades as the lawn.
        let mut accent = definition.clone();
        accent.seed = definition.seed + 101;
        accent.density = 300.;
        accent.clump.blades = [1, 3];
        accent.clump.radius = 0.03;
        accent.blade.height = [0.1, 0.3];
        accent.blade.width = accent.blade.width.map(|w| w * 2.6);
        accent.clumping.scale = 3.;
        accent.clumping.strength = 1.;
        accent.clumping.octaves = 2;
        accent.clumping.bias = 0.4;
        accent.lods.truncate(3);
        let accent_patch = grass::generate(
            &accent,
            &GrassSite {
                ground: ground.clone(),
                mask: DensityMask::Uniform,
            },
            &|| false,
        )
        .unwrap();
        // Painted as the lawn's grass (its roots the ground beneath them); each blade's tip fades to one of the study's
        // four tip colors from 35 % of its height to 70 %, on the lit side: the sun-yellow highlight, and three greens near the lawn's own (a blue-green, a green, a yellow-green), so the off-hue strokes follow the grass around them.
        let mut accent_look = look.clone();
        accent_look.far = 22.;
        accent_look.tip.lift = 1.;
        accent_look.tip.amount = 1.;
        accent_look.tip.start = 0.35;
        accent_look.tip.palette = Some([
            [0.13, 0.40, 0.22],
            [0.807, 1.0, 0.0467],
            [0.16, 0.50, 0.08],
            [0.30, 0.56, 0.08],
        ]);
        let mut accent_levels =
            batches::grass_levels(&accent_patch, [0., 0., 0.02], &accent_look, None).unwrap();
        for level in &mut accent_levels {
            level.batch.id = level.batch.id.replacen(&accent_patch.id, "lawn-accent", 1);
            level.batch.object_id = "lawn-accent".into();
            level.batch.foliage.as_mut().unwrap().patch = "lawn-accent".into();
        }
        println!(
            "lawn accent: {} clumps, {} blades at level 0",
            accent_patch.stats.instances, accent_patch.stats.blades
        );
        levels.extend(accent_levels);
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
