//! Grass patches: expectations are derived here from the definition's own numbers and
//! from plain geometry, not from the generator.
use std::cell::Cell;
use superspec_engine::treespec::{
    BEND_WEIGHT, ELEMENT, FoliageMesh, GROUND_NORMAL, HEIGHT_RATIO, PIVOT, RANDOM, ROOT,
    SHADE_NORMAL, THRESHOLD, VARIATION,
    grass::{
        DensityMask, GrassDefinition, GrassPatch, GrassSite, GrassTile, GroundMesh, generate, parse,
    },
};

const MEADOW: &str = include_str!("../src/treespec/fixtures/meadow.grass.json");

fn meadow() -> GrassDefinition {
    parse("meadow.grass.json", MEADOW).unwrap()
}
/// A rectangle of ground on the plane z = a x + b y + c, as two triangles.
fn plane(low: [f64; 2], high: [f64; 2], [a, b, c]: [f64; 3]) -> GroundMesh {
    let at = |x: f64, y: f64| [x, y, a * x + b * y + c];
    GroundMesh {
        positions: vec![
            at(low[0], low[1]),
            at(high[0], low[1]),
            at(high[0], high[1]),
            at(low[0], high[1]),
        ],
        indices: vec![0, 1, 2, 0, 2, 3],
    }
}
fn flat(size: f64) -> GrassSite {
    GrassSite {
        ground: plane([0., 0.], [size, size], [0., 0., 0.]),
        mask: DensityMask::Uniform,
    }
}
fn even(mut definition: GrassDefinition) -> GrassDefinition {
    definition.clumping.strength = 0.;
    definition
}
fn roots(patch: &GrassPatch) -> Vec<[f64; 3]> {
    patch
        .batches()
        .flat_map(|batch| batch.vectors[ROOT].clone())
        .collect()
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}
fn close(a: f64, b: f64, tolerance: f64) -> bool {
    (a - b).abs() <= tolerance
}
/// Vertex indices of each blade of a clump mesh.
fn blades(mesh: &FoliageMesh) -> Vec<Vec<usize>> {
    let mut result = Vec::<Vec<usize>>::new();
    for (vertex, element) in mesh.scalars[ELEMENT].iter().enumerate() {
        let blade = *element as usize;
        if result.len() <= blade {
            result.resize(blade + 1, vec![]);
        }
        result[blade].push(vertex);
    }
    result
}

#[test]
fn clump_meshes_have_the_counts_their_segments_imply() {
    let definition = meadow();
    let (meshes, lods) = definition.clump_meshes().unwrap();
    assert_eq!(lods.len(), 3);
    assert_eq!(meshes.len(), 3 * definition.clump.variants);
    for lod in &lods {
        assert_eq!(lod.meshes.len(), definition.clump.variants);
        for index in &lod.meshes {
            let mesh = &meshes[*index];
            mesh.validate().unwrap();
            let count = blades(mesh).len();
            assert!((5..=10).contains(&count), "{count} blades");
            // A ribbon of n segments ending in a point: 2n + 1 vertices, 2n - 1 triangles.
            assert_eq!(mesh.positions.len(), count * (2 * lod.segments + 1));
            assert_eq!(mesh.triangles(), count * (2 * lod.segments - 1));
            assert_eq!(mesh.material_slot, "blade");
            assert!(mesh.double_sided);
        }
    }
    // The same blades at every level: only their sampling changes.
    for variant in 0..definition.clump.variants {
        let counts: Vec<usize> = lods
            .iter()
            .map(|lod| blades(&meshes[lod.meshes[variant]]).len())
            .collect();
        assert!(
            counts.windows(2).all(|pair| pair[0] == pair[1]),
            "{counts:?}"
        );
    }
}

#[test]
fn blades_rise_from_their_root_with_the_painted_inputs() {
    let definition = meadow();
    let (meshes, lods) = definition.clump_meshes().unwrap();
    for index in &lods[0].meshes {
        let mesh = &meshes[*index];
        let mut randoms = vec![];
        for blade in blades(mesh) {
            let root = mesh.vectors[PIVOT][blade[0]];
            assert_eq!(root[2], 0., "roots lie in the clump's ground plane");
            assert!(norm(root) <= definition.clump.radius + 1e-12);
            // Rows are left and right vertices; the last vertex is the tip.
            let rows: Vec<[f64; 3]> = blade
                .chunks(2)
                .map(|row| {
                    let a = mesh.positions[row[0]];
                    let b = mesh.positions[*row.last().unwrap()];
                    [(a[0] + b[0]) / 2., (a[1] + b[1]) / 2., (a[2] + b[2]) / 2.]
                })
                .collect();
            assert!(
                norm(sub(rows[0], root)) < 1e-12,
                "the first row straddles the root"
            );
            let length: f64 = rows
                .windows(2)
                .map(|pair| norm(sub(pair[1], pair[0])))
                .sum();
            // Chords of an arc: no longer than the blade, and within a percent of it.
            assert!(
                length <= definition.blade.height[1] + 1e-9
                    && length >= definition.blade.height[0] * 0.99,
                "blade length {length}"
            );
            let mut width_before = f64::INFINITY;
            for (row, pair) in blade.chunks(2).enumerate() {
                let t = mesh.scalars[HEIGHT_RATIO][pair[0]];
                assert!(close(t, row as f64 / 12., 1e-12));
                let width = norm(sub(
                    mesh.positions[pair[0]],
                    mesh.positions[*pair.last().unwrap()],
                ));
                assert!(
                    width <= width_before + 1e-12,
                    "blades never widen toward the tip"
                );
                width_before = width;
                if row == 0 {
                    assert!(
                        width >= definition.blade.width[0] - 1e-12
                            && width <= definition.blade.width[1] + 1e-12
                    );
                }
                for vertex in pair {
                    assert_eq!(mesh.uvs[*vertex][1], t);
                    assert!(close(mesh.scalars[BEND_WEIGHT][*vertex], t * t, 1e-12));
                    assert_eq!(mesh.vectors[PIVOT][*vertex], root);
                    let shade = mesh.vectors[SHADE_NORMAL][*vertex];
                    assert!(close(norm(shade), 1., 1e-9) && shade[2] > 0.);
                    assert!(
                        dot(shade, root) >= -1e-12,
                        "shade normals tip away from the centre"
                    );
                    assert!(close(norm(mesh.normals[*vertex]), 1., 1e-9));
                }
            }
            assert_eq!(mesh.scalars[HEIGHT_RATIO][*blade.last().unwrap()], 1.);
            assert_eq!(mesh.uvs[blade[0]][0], 0.);
            assert_eq!(mesh.uvs[blade[1]][0], 1.);
            randoms.push(mesh.scalars[RANDOM][blade[0]]);
        }
        randoms.sort_by(f64::total_cmp);
        randoms.dedup();
        assert!(randoms.len() >= 5, "each blade has its own random value");
    }
}

#[test]
fn roots_stand_on_a_sloped_ground_mesh() {
    let definition = meadow();
    let (a, b, c) = (0.2, -0.1, 1.5);
    let site = GrassSite {
        ground: plane([-1., -2.], [2., 1.], [a, b, c]),
        mask: DensityMask::Uniform,
    };
    let patch = generate(&definition, &site, &|| false).unwrap();
    let length = (a * a + b * b + 1f64).sqrt();
    let normal = [-a / length, -b / length, 1. / length];
    let mut seen = 0;
    for batch in patch.batches() {
        assert!(batch.mesh < definition.clump.variants);
        for (index, transform) in batch.transforms.iter().enumerate() {
            seen += 1;
            let root = batch.vectors[ROOT][index];
            assert_eq!(transform[3], root);
            assert!((-1. ..=2.).contains(&root[0]) && (-2. ..=1.).contains(&root[1]));
            assert!(
                close(root[2], a * root[0] + b * root[1] + c, 1e-9),
                "root on the plane"
            );
            let ground = batch.vectors[GROUND_NORMAL][index];
            assert!(norm(sub(ground, normal)) < 1e-9);
            // An even rotation and scale: orthogonal axes of one length inside the range.
            let size = norm(transform[2]);
            assert!(size >= definition.scale[0] - 1e-9 && size <= definition.scale[1] + 1e-9);
            for (first, second) in [(0, 1), (1, 2), (0, 2)] {
                assert!(dot(transform[first], transform[second]).abs() < 1e-9);
                assert!(close(norm(transform[first]), size, 1e-9));
            }
            // The clump stands between upright and the ground normal, by `align`.
            let blend: [f64; 3] = std::array::from_fn(|axis| {
                [0., 0., 1.][axis] * (1. - definition.align) + normal[axis] * definition.align
            });
            let expected = blend.map(|v| v / norm(blend));
            assert!(norm(sub(transform[2].map(|v| v / size), expected)) < 1e-9);
            for name in [RANDOM, VARIATION, THRESHOLD] {
                assert!((0. ..=1.).contains(&batch.scalars[name][index]), "{name}");
            }
        }
    }
    assert_eq!(seen, patch.stats.instances);
    assert!(seen > 500, "{seen} clumps on 9 square meters");
}

#[test]
fn even_cover_matches_the_density() {
    let definition = even(meadow());
    let patch = generate(&definition, &flat(4.), &|| false).unwrap();
    let expected = definition.density * 16.;
    let found = patch.stats.instances as f64;
    assert!(
        (found / expected - 1.).abs() < 0.05,
        "{found} clumps, expected about {expected}"
    );
    // Tiles of 2 m over 4 m by 4 m: four tiles, each holding only its own roots.
    assert_eq!(patch.tiles.len(), 4);
    for tile in &patch.tiles {
        for root in tile.batches.iter().flat_map(|batch| &batch.vectors[ROOT]) {
            assert_eq!(
                [(root[0] / 2.).floor() as i64, (root[1] / 2.).floor() as i64],
                tile.index
            );
            for axis in 0..3 {
                assert!(tile.bounds[0][axis] <= root[axis] && root[axis] <= tile.bounds[1][axis]);
            }
        }
    }
    assert!(
        patch.bounds[1][2] >= definition.blade.height[0],
        "bounds include the blades"
    );
}

#[test]
fn clumping_gathers_the_cover_into_drifts() {
    let even_cover = generate(&even(meadow()), &flat(6.), &|| false).unwrap();
    let drifts = generate(&meadow(), &flat(6.), &|| false).unwrap();
    assert!(drifts.stats.instances < even_cover.stats.instances);
    // Count clumps per half-meter cell: drifts leave some cells far emptier than others.
    let spread = |patch: &GrassPatch| {
        let mut cells = vec![0f64; 144];
        for root in roots(patch) {
            cells[((root[1] * 2.) as usize).min(11) * 12 + ((root[0] * 2.) as usize).min(11)] += 1.;
        }
        let mean = cells.iter().sum::<f64>() / 144.;
        (cells.iter().map(|c| (c - mean) * (c - mean)).sum::<f64>() / 144.).sqrt() / mean
    };
    assert!(spread(&drifts) > 2. * spread(&even_cover));
}

/// Winding number: a different test from the generator's crossing count.
fn winding(point: [f64; 2], ring: &[[f64; 2]]) -> i32 {
    let mut turns = 0;
    for (a, b) in ring.iter().zip(ring.iter().cycle().skip(1)) {
        let left = (b[0] - a[0]) * (point[1] - a[1]) - (point[0] - a[0]) * (b[1] - a[1]);
        if a[1] <= point[1] && b[1] > point[1] && left > 0. {
            turns += 1;
        } else if a[1] > point[1] && b[1] <= point[1] && left < 0. {
            turns -= 1;
        }
    }
    turns
}

#[test]
fn a_polygon_mask_bounds_the_grass() {
    let definition = even(meadow());
    // An L of area 16 - 4 = 12 square meters inside a 5 m square of ground.
    let ring = vec![
        [0.5, 0.5],
        [4.5, 0.5],
        [4.5, 2.5],
        [2.5, 2.5],
        [2.5, 4.5],
        [0.5, 4.5],
    ];
    let site = GrassSite {
        ground: plane([0., 0.], [5., 5.], [0., 0., 0.]),
        mask: DensityMask::Polygon {
            points: ring.clone(),
        },
    };
    let patch = generate(&definition, &site, &|| false).unwrap();
    let found = roots(&patch);
    assert!(
        found
            .iter()
            .all(|root| winding([root[0], root[1]], &ring) != 0)
    );
    let expected = definition.density * 12.;
    assert!(
        (found.len() as f64 / expected - 1.).abs() < 0.06,
        "{} of about {expected}",
        found.len()
    );

    // Every clump of the masked patch is the same clump in the unmasked one.
    let open = generate(
        &definition,
        &GrassSite {
            mask: DensityMask::Uniform,
            ..site.clone()
        },
        &|| false,
    )
    .unwrap();
    let all: std::collections::BTreeMap<[u64; 3], (usize, [[f64; 3]; 4], f64)> = open
        .batches()
        .flat_map(|batch| {
            (0..batch.len()).map(move |i| {
                (
                    batch.vectors[ROOT][i].map(f64::to_bits),
                    (batch.mesh, batch.transforms[i], batch.scalars[RANDOM][i]),
                )
            })
        })
        .collect();
    for batch in patch.batches() {
        for i in 0..batch.len() {
            let same = all[&batch.vectors[ROOT][i].map(f64::to_bits)];
            assert_eq!(
                same,
                (batch.mesh, batch.transforms[i], batch.scalars[RANDOM][i])
            );
        }
    }
    // The mask is part of the placement key only.
    let keys = |patch: &GrassPatch| {
        patch
            .part_keys
            .iter()
            .cloned()
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let (masked, unmasked) = (keys(&patch), keys(&open));
    for (part, key) in &masked {
        assert_eq!(key == &unmasked[part], part != "placement", "{part}");
    }
    assert_ne!(patch.key, open.key);
}

#[test]
fn a_density_field_thins_the_grass_in_proportion() {
    let definition = even(meadow());
    // Three by three samples two meters apart: density rises from 0 at x = 0 to 1 at x = 4,
    // over 4 m by 4 m, on ground that reaches a meter further every way.
    let ramp = GrassSite {
        ground: plane([-1., -1.], [5., 5.], [0., 0., 0.]),
        mask: DensityMask::Field {
            origin: [0., 0.],
            cell: 2.,
            columns: 3,
            rows: 3,
            values: (0..3).flat_map(|_| [0., 0.5, 1.]).collect(),
        },
    };
    let patch = generate(&definition, &ramp, &|| false).unwrap();
    let found = roots(&patch);
    assert!(
        found
            .iter()
            .all(|root| (0. ..=4.).contains(&root[0]) && (0. ..=4.).contains(&root[1]))
    );
    // Mean density over the field is one half, of 16 square meters.
    let expected = definition.density * 16. * 0.5;
    assert!(
        (found.len() as f64 / expected - 1.).abs() < 0.08,
        "{} of about {expected}",
        found.len()
    );
    // The integral of x/4 over [0,2] against [2,4] is 1 to 3.
    let left = found.iter().filter(|root| root[0] < 2.).count() as f64;
    let ratio = (found.len() as f64 - left) / left;
    assert!((2.5..3.6).contains(&ratio), "right to left ratio {ratio}");
    let near_zero = found.iter().filter(|root| root[0] < 0.2).count() as f64;
    assert!(near_zero < 0.01 * found.len() as f64);
}

#[test]
fn steep_and_downward_ground_grows_nothing_and_the_top_surface_wins() {
    let definition = even(meadow());
    // A level strip, a 70 degree bank beside it, and a ledge one meter over part of the strip.
    let rise = 70f64.to_radians().tan();
    let mut ground = plane([0., 0.], [2., 2.], [0., 0., 0.]);
    let bank = plane([2., 0.], [3., 2.], [rise, 0., -2. * rise]);
    let ledge = plane([0., 0.], [1., 2.], [0., 0., 1.]);
    for part in [bank, ledge] {
        let base = ground.positions.len() as u32;
        ground.positions.extend(part.positions);
        ground.indices.extend(part.indices.iter().map(|i| i + base));
    }
    // The underside of the ledge: the same quad wound the other way.
    let base = ground.positions.len() as u32;
    ground
        .positions
        .extend(plane([0., 0.], [1., 2.], [0., 0., 0.9]).positions);
    ground.indices.extend([0, 2, 1, 0, 3, 2].map(|i| i + base));
    let patch = generate(
        &definition,
        &GrassSite {
            ground,
            mask: DensityMask::Uniform,
        },
        &|| false,
    )
    .unwrap();
    let found = roots(&patch);
    assert!(!found.is_empty());
    for root in &found {
        assert!(root[0] <= 2., "nothing on the bank: {root:?}");
        assert_eq!(root[2], if root[0] < 1. { 1. } else { 0. }, "{root:?}");
    }
}

#[test]
fn levels_of_detail_thin_by_a_stable_threshold_and_stay_in_budget() {
    let definition = even(meadow());
    let patch = generate(&definition, &flat(6.), &|| false).unwrap();
    let total = patch.stats.instances;
    let mut expected = [0usize; 3];
    let mut triangles = [0usize; 3];
    for batch in patch.batches() {
        let thresholds = &batch.scalars[THRESHOLD];
        assert!(
            thresholds.windows(2).all(|pair| pair[0] <= pair[1]),
            "sorted for prefix draws"
        );
        for (level, lod) in definition.lods.iter().enumerate() {
            let drawn = thresholds.iter().filter(|t| **t < lod.keep).count();
            assert_eq!(GrassTile::drawn(batch, lod.keep), drawn);
            expected[level] += drawn;
            let mesh = &patch.meshes[patch.lods[level].meshes[batch.mesh]];
            triangles[level] += drawn * blades(mesh).len() * (2 * lod.segments - 1);
        }
    }
    for (level, lod) in patch.lods.iter().enumerate() {
        assert_eq!(lod.instances, expected[level]);
        assert_eq!(lod.triangles, triangles[level]);
        let share = lod.instances as f64 / total as f64;
        assert!(
            (share - definition.lods[level].keep).abs() < 0.03,
            "level {level} keeps {share}"
        );
        // Survivors widen so the cover holds: width times kept count is constant.
        assert!(close(lod.widen, (1. / lod.keep).sqrt(), 1e-12));
    }
    // 12, 3 and 1 segments at 100%, 50% and 20%: 23, 5 and 1 triangles a blade.
    let base = patch.lods[0].triangles as f64;
    assert!(patch.lods[1].triangles as f64 <= base * 0.5 * 5. / 23. * 1.1);
    assert!(patch.lods[2].triangles as f64 <= base * 0.2 / 23. * 1.15);
    let width = |level: usize| {
        let mesh = &patch.meshes[patch.lods[level].meshes[0]];
        norm(sub(mesh.positions[0], mesh.positions[1]))
    };
    assert!(close(width(1) / width(0), 2f64.sqrt(), 1e-9));
    assert!(close(width(2) / width(0), 5f64.sqrt(), 1e-9));
    assert_eq!(
        patch.stats.blades,
        patch
            .batches()
            .map(|batch| batch.len() * blades(&patch.meshes[batch.mesh]).len())
            .sum::<usize>()
    );
}

#[test]
fn the_same_seed_gives_the_same_bytes() {
    let definition = meadow();
    let first = generate(&definition, &flat(3.), &|| false).unwrap();
    let again = generate(&definition, &flat(3.), &|| false).unwrap();
    assert_eq!(first, again);
    assert_eq!(first.digest(), again.digest());
    // Checked against a wasm32 build of the same code; see the module README.
    assert_eq!(first.digest(), GOLDEN, "native output changed");
    let mut other = definition.clone();
    other.seed = 8;
    let changed = generate(&other, &flat(3.), &|| false).unwrap();
    assert_ne!(first.digest(), changed.digest());
    assert!(
        first
            .part_keys
            .iter()
            .zip(&changed.part_keys)
            .all(|(a, b)| a.0 == b.0 && a.1 != b.1)
    );
    // A change to level 1 leaves the keys of the level 0 and 2 meshes alone.
    let mut coarser = definition.clone();
    coarser.lods[1].segments = 2;
    let (meshes, _) = coarser.clump_meshes().unwrap();
    for (mesh, original) in meshes.iter().zip(&first.meshes) {
        assert_eq!(
            mesh.geometry_key == original.geometry_key,
            !mesh.id.ends_with("lod1"),
            "{}",
            mesh.id
        );
    }
}
const GOLDEN: &str = "601529b7f03f68bdab62f9c6d60e7f97e21e80257203881f102d251c7f8d5d12";

#[test]
fn budgets_cancellation_and_bad_input_are_reported() {
    let mut definition = meadow();
    definition.limits.max_instances = 50;
    let error = generate(&definition, &flat(3.), &|| false).unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.pointer.as_str()),
        ("TREE_BUDGET", "/limits/maxInstances")
    );
    assert!(error.message.contains("maxInstances 50"), "{error}");
    assert_eq!(error.source, "meadow.grass.json");

    let mut definition = meadow();
    definition.limits.max_vertices = 100;
    let error = definition.clump_meshes().unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.pointer.as_str()),
        ("TREE_BUDGET", "/limits/maxVertices")
    );

    let mut definition = meadow();
    definition.density = 10_000.;
    let error = generate(&definition, &flat(40.), &|| false).unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.pointer.as_str()),
        ("TREE_BUDGET", "/density")
    );

    let definition = meadow();
    assert_eq!(
        generate(&definition, &flat(3.), &|| true).unwrap_err().code,
        "TREE_CANCELLED"
    );
    // Cancelled part-way through a large patch: placement polls as it goes.
    let polls = Cell::new(0);
    let error = generate(&definition, &flat(30.), &|| {
        polls.set(polls.get() + 1);
        polls.get() > 10
    })
    .unwrap_err();
    assert_eq!(error.code, "TREE_CANCELLED");
    assert!(
        polls.get() < 40,
        "stopped soon after the request: {} polls",
        polls.get()
    );

    let empty = GrassSite {
        ground: GroundMesh {
            positions: vec![],
            indices: vec![],
        },
        mask: DensityMask::Uniform,
    };
    assert_eq!(
        generate(&definition, &empty, &|| false).unwrap_err().code,
        "TREE_GROUND"
    );
    let mut site = flat(3.);
    site.mask = DensityMask::Polygon {
        points: vec![[0., 0.], [1., 1.]],
    };
    assert_eq!(
        generate(&definition, &site, &|| false).unwrap_err().code,
        "TREE_MASK"
    );

    let error = parse(
        "bad.grass.json",
        &MEADOW.replace("\"density\": 140", "\"density\": -1"),
    )
    .unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.pointer.as_str()),
        ("TREE_VALUE", "/density")
    );
    let error = parse("bad.grass.json", &MEADOW.replace("\"tile\"", "\"tiles\"")).unwrap_err();
    assert_eq!(error.code, "TREE_JSON");
    let error = parse(
        "bad.grass.json",
        &MEADOW.replace("\"kind\": \"grass\"", "\"kind\": \"tree\""),
    )
    .unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.pointer.as_str()),
        ("TREE_FORMAT", "/kind")
    );
    let error = parse(
        "bad.grass.json",
        &MEADOW.replace(
            "\"segments\": 3, \"keep\": 0.5",
            "\"segments\": 3, \"keep\": 1.5",
        ),
    )
    .unwrap_err();
    assert_eq!(error.pointer, "/lods/1/keep");
}
