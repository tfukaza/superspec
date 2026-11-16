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
    for name in ["threshold", "variation", "random"] {
        assert_eq!(first.scalars[name].len(), first.placements.len(), "{name}");
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
