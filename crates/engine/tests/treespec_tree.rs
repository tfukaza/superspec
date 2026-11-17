//! Shrubs and trees: expectations come from the definitions' numbers and plain geometry.
use std::cell::Cell;
use std::collections::BTreeMap;
use superspec_engine::treespec::{
    BEND_WEIGHT, CLUSTER, ELEMENT, HEIGHT_RATIO, LEVEL, PIVOT, ROOT, SHADE_NORMAL, THRESHOLD,
    VARIATION,
    hull::CanopyField,
    tree::{
        BACKING_SHRINK, Envelope, GeneratedTree, HullUse, SHADOW_SHRINK, TreeDefinition, generate,
        parse,
    },
};

const SHRUB: &str = include_str!("../src/treespec/fixtures/shrub.tree.json");
const BROADLEAF: &str = include_str!("../src/treespec/fixtures/broadleaf.tree.json");
const CONIFER: &str = include_str!("../src/treespec/fixtures/conifer.tree.json");

fn fixtures() -> Vec<(TreeDefinition, GeneratedTree)> {
    [
        ("shrub", SHRUB),
        ("broadleaf", BROADLEAF),
        ("conifer", CONIFER),
    ]
    .into_iter()
    .map(|(name, text)| {
        let definition = parse(&format!("{name}.tree.json"), text).unwrap();
        let tree = generate(&definition, &|| false).unwrap();
        (definition, tree)
    })
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

#[test]
fn skeletons_taper_attach_and_stay_in_their_envelope() {
    for (definition, tree) in fixtures() {
        let id = &definition.id;
        let stems: Vec<_> = tree.skeleton.iter().filter(|b| b.level == 0).collect();
        assert_eq!(stems.len(), definition.trunk.count, "{id}");
        assert!(
            close(stems[0].length, definition.trunk.height, 1e-9),
            "{id}"
        );
        assert!(close(stems[0].radii[0], definition.trunk.radius, 1e-12));
        let mut levels = BTreeMap::<usize, usize>::new();
        let mut pruned = 0;
        for (index, branch) in tree.skeleton.iter().enumerate() {
            assert_eq!(branch.id, index);
            *levels.entry(branch.level).or_default() += 1;
            assert_eq!(branch.points.len(), branch.radii.len());
            assert!(branch.points.len() >= 2 && branch.length > 0.);
            // Taper: the radius never grows toward the tip, and the tip is the stated share.
            assert!(
                branch.radii.windows(2).all(|pair| pair[1] <= pair[0]),
                "{id} branch {index}"
            );
            let measured: f64 = branch
                .points
                .windows(2)
                .map(|p| norm(sub(p[1], p[0])))
                .sum();
            assert!(close(measured, branch.length, 1e-9));
            pruned += branch.pruned as usize;
            let Some(parent) = branch.parent else {
                assert!(close(
                    *branch.radii.last().unwrap(),
                    definition.trunk.radius * definition.trunk.tip,
                    1e-12
                ));
                continue;
            };
            assert!(parent < index, "parents come first");
            let parent = &tree.skeleton[parent];
            let level = &definition.levels[branch.level - 1];
            assert_eq!(branch.level, parent.level + 1);
            assert!(branch.attach >= level.start - 1e-12 && branch.attach <= level.end + 1e-12);
            // Attachment: the branch starts on its parent's centre line, no thicker than it.
            let (point, _, radius) = parent.at(branch.attach);
            assert!(
                norm(sub(branch.points[0], point)) < 1e-9,
                "{id} branch {index} is attached"
            );
            assert!(close(branch.radii[0], radius * level.radius, 1e-12));
            assert!(branch.radii[0] <= radius);
            assert!(close(
                *branch.radii.last().unwrap(),
                branch.radii[0] * level.tip,
                1e-12
            ));
            // Its start lies on the parent's polyline, checked without `at`.
            let on_parent = parent.points.windows(2).any(|pair| {
                let (edge, to) = (sub(pair[1], pair[0]), sub(point, pair[0]));
                let t = (dot(to, edge) / dot(edge, edge)).clamp(0., 1.);
                norm(sub(to, edge.map(|v| v * t))) < 1e-9
            });
            assert!(on_parent);
            let envelope = definition.envelope.as_ref().unwrap();
            for point in &branch.points {
                assert!(
                    inside(envelope, *point, 1e-6),
                    "{id} branch {index} leaves the envelope at {point:?}"
                );
            }
            assert!(
                branch.length <= parent.length * level.length[0].max(level.length[1]) * 1.15 + 1e-9
            );
        }
        assert_eq!(
            levels.len(),
            definition.levels.len() + 1,
            "{id} grows every level: {levels:?}"
        );
        assert!(pruned > 0, "{id}: the envelope cuts some branches");
        // No level can hold more than count times its parents.
        for (depth, level) in definition.levels.iter().enumerate() {
            assert!(levels[&(depth + 1)] <= levels[&depth] * level.count);
            assert!(
                levels[&(depth + 1)] >= levels[&depth],
                "{id} level {} is sparse: {levels:?}",
                depth + 1
            );
        }
    }
}

/// The envelope test written out again, with a tolerance.
fn inside(envelope: &Envelope, p: [f64; 3], slack: f64) -> bool {
    match envelope {
        Envelope::Ellipsoid { center, radii } => {
            (0..3)
                .map(|a| ((p[a] - center[a]) / radii[a]).powi(2))
                .sum::<f64>()
                <= 1. + slack
        }
        Envelope::Cone { base, top, radius } => {
            p[2] >= base - slack
                && p[2] <= top + slack
                && (p[0] * p[0] + p[1] * p[1]).sqrt()
                    <= radius * (top - p[2]) / (top - base) + slack
        }
    }
}

#[test]
fn bark_tubes_have_metric_coordinates_and_the_counts_their_rings_imply() {
    for (definition, tree) in fixtures() {
        let id = &definition.id;
        for (index, lod) in tree.lods.iter().enumerate() {
            let rings = |points: usize| (points - 1).div_ceil(lod.ring_step) + 1;
            let radial = lod.radial_segments;
            let mut expected = [(0usize, 0usize); 2];
            for branch in &tree.skeleton {
                let part = if branch.level == 0 {
                    0
                } else if branch.level <= lod.max_level {
                    1
                } else {
                    continue;
                };
                let count = rings(branch.points.len());
                // Rings of radial + 1 vertices (a seam) and an end centre; two triangles a
                // quad and a fan over the end.
                expected[part].0 += count * (radial + 1) + 1;
                expected[part].1 += (count - 1) * radial * 2 + radial;
            }
            let trunk = &tree.meshes[lod.trunk];
            trunk.validate().unwrap();
            assert_eq!(trunk.id, format!("trunk/lod{index}"));
            assert_eq!(
                (trunk.positions.len(), trunk.triangles()),
                expected[0],
                "{id} trunk lod {index}"
            );
            match lod.branches {
                Some(mesh) => {
                    let mesh = &tree.meshes[mesh];
                    mesh.validate().unwrap();
                    assert_eq!(
                        (mesh.positions.len(), mesh.triangles()),
                        expected[1],
                        "{id} branches lod {index}"
                    );
                    assert_eq!(mesh.material_slot, "bark");
                }
                None => assert_eq!(expected[1], (0, 0)),
            }
        }
        // Every bark vertex sits one radius from its branch's centre line, and its texture
        // coordinates are meters: along the branch, and around its base girth.
        let lod = &tree.lods[0];
        for mesh in [Some(lod.trunk), lod.branches]
            .into_iter()
            .flatten()
            .map(|i| &tree.meshes[i])
        {
            let mut furthest = BTreeMap::<usize, [f64; 2]>::new();
            for vertex in 0..mesh.positions.len() {
                let branch = &tree.skeleton[mesh.scalars[ELEMENT][vertex] as usize];
                let t = mesh.scalars[HEIGHT_RATIO][vertex];
                let (centre, _, radius) = branch.at(t);
                let offset = norm(sub(mesh.positions[vertex], centre));
                assert!(
                    offset < 1e-9 || close(offset, radius, 1e-9),
                    "{id}: {offset} against {radius}"
                );
                assert!(close(mesh.uvs[vertex][1], t * branch.length, 1e-9));
                assert_eq!(mesh.scalars[LEVEL][vertex], branch.level as f64);
                assert_eq!(mesh.vectors[PIVOT][vertex], branch.points[0]);
                assert!(close(
                    mesh.scalars[BEND_WEIGHT][vertex],
                    branch.bend_at(t),
                    1e-12
                ));
                assert!(close(norm(mesh.normals[vertex]), 1., 1e-9));
                let entry = furthest.entry(branch.id).or_default();
                *entry = [
                    entry[0].max(mesh.uvs[vertex][0]),
                    entry[1].max(mesh.uvs[vertex][1]),
                ];
            }
            for (branch, [around, along]) in furthest {
                let branch = &tree.skeleton[branch];
                assert!(close(around, std::f64::consts::TAU * branch.radii[0], 1e-9));
                assert!(close(along, branch.length, 1e-9));
            }
            // Outward faces: each triangle's normal agrees with its vertices' normals.
            for triangle in mesh.indices.chunks_exact(3) {
                let [a, b, c] = [0, 1, 2].map(|i| mesh.positions[triangle[i] as usize]);
                let (e1, e2) = (sub(b, a), sub(c, a));
                let face = [
                    e1[1] * e2[2] - e1[2] * e2[1],
                    e1[2] * e2[0] - e1[0] * e2[2],
                    e1[0] * e2[1] - e1[1] * e2[0],
                ];
                if norm(face) > 1e-12 {
                    let mean: [f64; 3] = std::array::from_fn(|axis| {
                        triangle
                            .iter()
                            .map(|i| mesh.normals[*i as usize][axis])
                            .sum()
                    });
                    assert!(dot(face, mean) > 0., "{id}: a bark triangle faces inward");
                }
            }
        }
    }
}

#[test]
fn wind_weights_grow_outward_and_join_without_a_step() {
    for (definition, tree) in fixtures() {
        let mut most = 0f64;
        for branch in &tree.skeleton {
            assert!(
                branch.bend[0] >= 0. && branch.bend[1] > branch.bend[0] && branch.bend[1] <= 1.
            );
            most = most.max(branch.bend[1]);
            match branch.parent {
                None => assert_eq!(
                    branch.bend[0], 0.,
                    "{}: the ground does not move",
                    definition.id
                ),
                Some(parent) => {
                    let parent = &tree.skeleton[parent];
                    let t = branch.attach;
                    let expected = parent.bend[0] + (parent.bend[1] - parent.bend[0]) * t * t;
                    assert!(close(branch.bend[0], expected, 1e-12));
                }
            }
        }
        assert_eq!(most, 1.);
    }
}

#[test]
fn leaves_sit_on_branches_and_shade_normals_point_out_of_the_canopy() {
    for (definition, tree) in fixtures() {
        let id = &definition.id;
        let leaves = &tree.leaves;
        assert!(
            leaves.len() > 200,
            "{id} has {} leaf clusters",
            leaves.len()
        );
        let mut outward = 0;
        let hull_centre = CanopyField::new(&tree.canopy.blobs, definition.leaves.size[1])
            .unwrap()
            .origin;
        for index in 0..leaves.len() {
            let transform = leaves.transforms[index];
            let root = leaves.vectors[ROOT][index];
            assert_eq!(transform[3], root);
            let size = norm(transform[1]);
            assert!(
                size >= definition.leaves.size[0] - 1e-9
                    && size <= definition.leaves.size[1] + 1e-9
            );
            for (first, second) in [(0, 1), (1, 2), (0, 2)] {
                assert!(dot(transform[first], transform[second]).abs() < 1e-9);
                assert!(close(norm(transform[first]), size, 1e-9));
            }
            // The cluster hangs from a branch of a leaf-bearing level, within its radius.
            let level = leaves.scalars[LEVEL][index] as usize;
            assert!(level >= definition.leaves.level);
            let pivot = leaves.vectors[PIVOT][index];
            let nearest = tree
                .skeleton
                .iter()
                .filter(|b| b.level == level && b.points[0] == pivot)
                .flat_map(|b| b.points.windows(2))
                .map(|pair| {
                    let (edge, to) = (sub(pair[1], pair[0]), sub(root, pair[0]));
                    let t = (dot(to, edge) / dot(edge, edge)).clamp(0., 1.);
                    norm(sub(to, edge.map(|v| v * t)))
                })
                .fold(f64::INFINITY, f64::min);
            assert!(
                nearest < 1e-9,
                "{id}: a leaf cluster floats {nearest} m from its branch"
            );
            let limb = &tree.skeleton[leaves.scalars[CLUSTER][index] as usize];
            assert!(limb.level <= 1);
            let shade = leaves.vectors[SHADE_NORMAL][index];
            assert!(close(norm(shade), 1., 1e-9));
            let centre: [f64; 3] = std::array::from_fn(|a| root[a] + 0.5 * transform[1][a]);
            // Out of the canopy hull, whose rays leave from the limb balls' weighted centre.
            outward += (dot(shade, sub(centre, hull_centre)) > 0.) as usize;
            for name in [THRESHOLD, VARIATION, BEND_WEIGHT] {
                assert!((0. ..=1.).contains(&leaves.scalars[name][index]), "{name}");
            }
        }
        assert!(
            outward as f64 >= 0.97 * leaves.len() as f64,
            "{id}: {outward} of {} shade normals face out",
            leaves.len()
        );
        // Lit from above, the canopy's top is light and its underside dark.
        let lit = |high: bool| {
            let picked: Vec<f64> = (0..leaves.len())
                .filter(|i| {
                    (leaves.vectors[ROOT][*i][2]
                        > tree.canopy.center[2] + 0.5 * tree.canopy.radii[2])
                        == high
                })
                .filter(|i| {
                    high || leaves.vectors[ROOT][*i][2]
                        < tree.canopy.center[2] - 0.5 * tree.canopy.radii[2]
                })
                .map(|i| leaves.vectors[SHADE_NORMAL][i][2])
                .collect();
            picked.iter().sum::<f64>() / picked.len() as f64
        };
        assert!(
            lit(true) > 0.3 && lit(false) < 0.,
            "{id}: top {} underside {}",
            lit(true),
            lit(false)
        );
        assert!(!tree.canopy.blobs.is_empty());
        for blob in &tree.canopy.blobs {
            assert!(tree.skeleton[blob.branch].level <= 1 && blob.radius.is_finite());
        }
    }
}

/// Crossings of the ray from `p` along +X with the triangles of a mesh (Moller-Trumbore).
fn crossings(mesh: &superspec_engine::treespec::FoliageMesh, p: [f64; 3]) -> usize {
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let direction = [1., 0.0001, 0.0002];
    mesh.indices
        .chunks_exact(3)
        .filter(|t| {
            let [a, b, c] = [0, 1, 2].map(|i| mesh.positions[t[i] as usize]);
            let (e1, e2) = (sub(b, a), sub(c, a));
            let h = cross(direction, e2);
            let det = dot(e1, h);
            if det.abs() < 1e-14 {
                return false;
            }
            let s = sub(p, a);
            let u = dot(s, h) / det;
            let q = cross(s, e1);
            let v = dot(direction, q) / det;
            u >= 0. && v >= 0. && u + v <= 1. && dot(e2, q) / det > 0.
        })
        .count()
}

#[test]
fn canopy_hulls_are_closed_wrap_the_leaves_and_give_their_shade_normals() {
    for (definition, tree) in fixtures() {
        let id = &definition.id;
        let field = CanopyField::new(&tree.canopy.blobs, definition.leaves.size[1]).unwrap();
        let hulls: Vec<_> = tree
            .lods
            .iter()
            .filter_map(|lod| lod.hull_mesh.map(|m| (lod.hull, &tree.meshes[m])))
            .collect();
        let shadow = &tree.meshes[tree.shadow.expect("a plant with leaves has a shadow proxy")];
        assert_eq!(hulls.len(), 3, "{id}");
        for (_, mesh) in hulls.iter().copied().chain([(HullUse::None, shadow)]) {
            mesh.validate().unwrap();
            assert!(!mesh.double_sided);
            // Closed and wound one way: every edge is met once in each direction.
            let mut edges = BTreeMap::<(u32, u32), i32>::new();
            for t in mesh.indices.chunks_exact(3) {
                for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                    *edges.entry((a.min(b), a.max(b))).or_default() += if a < b { 1 } else { -1 };
                }
            }
            assert!(
                edges.values().all(|n| *n == 0),
                "{id} {}: open or misturned",
                mesh.id
            );
            // Normals point away from the hull's centre, as do the faces.
            let outward = mesh
                .normals
                .iter()
                .zip(&mesh.positions)
                .filter(|(n, p)| dot(**n, sub(**p, field.origin)) > 0.)
                .count();
            assert!(
                outward as f64 >= 0.95 * mesh.positions.len() as f64,
                "{id} {}: {outward}",
                mesh.id
            );
            let volume: f64 = mesh
                .indices
                .chunks_exact(3)
                .map(|t| {
                    let [a, b, c] =
                        [0, 1, 2].map(|i| sub(mesh.positions[t[i] as usize], field.origin));
                    dot(
                        a,
                        [
                            b[1] * c[2] - b[2] * c[1],
                            b[2] * c[0] - b[0] * c[2],
                            b[0] * c[1] - b[1] * c[0],
                        ],
                    ) / 6.
                })
                .sum();
            assert!(volume > 0., "{id} {}: volume {volume}", mesh.id);
        }
        let reach = |mesh: &superspec_engine::treespec::FoliageMesh| {
            mesh.positions
                .iter()
                .map(|p| norm(sub(*p, field.origin)))
                .fold(0., f64::max)
        };
        let impostor = hulls
            .iter()
            .find(|(use_, _)| *use_ == HullUse::Impostor)
            .unwrap()
            .1;
        let backing = hulls
            .iter()
            .find(|(use_, _)| *use_ == HullUse::Backing)
            .unwrap()
            .1;
        assert!(
            reach(backing) <= BACKING_SHRINK * reach(impostor) * 1.05,
            "{id}"
        );
        assert!(
            reach(shadow) <= SHADOW_SHRINK * reach(impostor) * 1.05,
            "{id}"
        );
        // The impostor holds most cluster centres (an odd number of crossings is inside).
        let leaves = &tree.leaves;
        let inside = (0..leaves.len())
            .filter(|i| {
                let t = leaves.transforms[*i];
                let centre: [f64; 3] = std::array::from_fn(|a| t[3][a] + 0.5 * t[1][a]);
                crossings(impostor, centre) % 2 == 1
            })
            .count();
        assert!(
            inside as f64 >= 0.85 * leaves.len() as f64,
            "{id}: {inside} of {} clusters inside",
            leaves.len()
        );
        // A cluster at the hull's surface shades as the hull does there.
        for i in 0..leaves.len() {
            let t = leaves.transforms[i];
            let centre: [f64; 3] = std::array::from_fn(|a| t[3][a] + 0.5 * t[1][a]);
            assert_eq!(leaves.vectors[SHADE_NORMAL][i], field.shade(centre));
        }
        // Bend weights are the leaves' (within their range); heights span the hull.
        let bends = &impostor.scalars[BEND_WEIGHT];
        let leaf_bends = &leaves.scalars[BEND_WEIGHT];
        let (low, high) = leaf_bends
            .iter()
            .fold((f64::INFINITY, 0f64), |(l, h), w| (l.min(*w), h.max(*w)));
        assert!(
            bends.iter().all(|w| *w >= low - 1e-9 && *w <= high + 1e-9),
            "{id}"
        );
        let heights = &impostor.scalars[HEIGHT_RATIO];
        assert!(heights.iter().any(|h| *h == 0.) && heights.iter().any(|h| *h == 1.));
    }
}

#[test]
fn leaf_cluster_meshes_carry_card_coordinates() {
    for (definition, tree) in fixtures() {
        for lod in &tree.lods {
            let mesh = &tree.meshes[lod.leaf];
            mesh.validate().unwrap();
            let cards = mesh.scalars[ELEMENT].last().unwrap() + 1.;
            // The pointed outline: six vertices and four triangles a card.
            assert_eq!(mesh.positions.len() as f64, cards * 6.);
            assert_eq!(mesh.triangles() as f64, cards * 4.);
            assert!(mesh.double_sided && mesh.material_slot == "leaf");
            for vertex in 0..mesh.positions.len() {
                let [u, v] = mesh.uvs[vertex];
                assert!((0. ..=1.).contains(&u) && (0. ..=1.).contains(&v));
                assert_eq!(mesh.scalars[HEIGHT_RATIO][vertex], v);
                // A card is flat and one unit long before widening, from the cluster's origin.
                let reach = norm(mesh.positions[vertex]);
                let half = 0.5 * definition.leaves.aspect * (2. * u - 1.);
                assert!(close(
                    reach,
                    lod.leaf_widen * (v * v + half * half).sqrt(),
                    1e-9
                ));
                assert!(close(norm(mesh.normals[vertex]), 1., 1e-9));
            }
        }
    }
}

#[test]
fn levels_of_detail_stay_in_their_budgets() {
    for (definition, tree) in fixtures() {
        let id = &definition.id;
        let thresholds = &tree.leaves.scalars[THRESHOLD];
        assert!(thresholds.windows(2).all(|pair| pair[0] <= pair[1]));
        for (index, lod) in tree.lods.iter().enumerate() {
            let source = &definition.lods[index];
            assert_eq!(lod.hull, source.hull);
            assert_eq!(lod.hull_mesh.is_some(), source.hull != HullUse::None);
            if source.hull == HullUse::Impostor {
                // The hull stands in for the leaves.
                assert_eq!(lod.leaf_instances, 0, "{id} level {index}");
            } else {
                assert_eq!(
                    lod.leaf_instances,
                    thresholds.iter().filter(|t| **t < source.leaf_keep).count()
                );
                let share = lod.leaf_instances as f64 / thresholds.len() as f64;
                assert!(
                    (share - source.leaf_keep).abs() < 0.05,
                    "{id} level {index} keeps {share}"
                );
            }
            assert!(close(lod.leaf_widen, (1. / source.leaf_keep).sqrt(), 1e-12));
            let bark: usize = [Some(lod.trunk), lod.branches, lod.hull_mesh]
                .into_iter()
                .flatten()
                .map(|i| tree.meshes[i].triangles())
                .sum();
            assert_eq!(
                lod.triangles,
                bark + lod.leaf_instances * tree.meshes[lod.leaf].triangles()
            );
        }
        let triangles: Vec<usize> = tree.lods.iter().map(|lod| lod.triangles).collect();
        // One plant must import as a single GLB item and thin sharply with distance.
        assert!(triangles[0] <= 250_000, "{id}: {triangles:?}");
        assert!(
            triangles[1] as f64 <= 0.3 * triangles[0] as f64,
            "{id}: {triangles:?}"
        );
        assert!(
            triangles[2] as f64 <= 0.05 * triangles[0] as f64,
            "{id}: {triangles:?}"
        );
        assert!(tree.lods[0].vertices <= definition.limits.max_vertices);
    }
}

#[test]
fn each_species_has_its_own_silhouette() {
    let trees = fixtures();
    let height = |tree: &GeneratedTree| tree.bounds[1][2];
    let width = |tree: &GeneratedTree| tree.bounds[1][0] - tree.bounds[0][0];
    let (shrub, broadleaf, conifer) = (&trees[0].1, &trees[1].1, &trees[2].1);
    // Mean distance of the leaves from the vertical axis, in a band of heights.
    let girth = |tree: &GeneratedTree, low: f64, high: f64| {
        let picked: Vec<f64> = tree.leaves.vectors[ROOT]
            .iter()
            .filter(|root| root[2] >= low && root[2] < high)
            .map(|root| (root[0] * root[0] + root[1] * root[1]).sqrt())
            .collect();
        assert!(!picked.is_empty(), "no leaves from {low} to {high}");
        picked.iter().sum::<f64>() / picked.len() as f64
    };
    // A shrub: low, wider than tall, leafy to the ground, several stems.
    assert!(
        height(shrub) < 1.6 && width(shrub) > height(shrub),
        "{:?}",
        shrub.bounds
    );
    assert_eq!(shrub.features.len(), 5);
    assert!(shrub.leaves.vectors[ROOT].iter().any(|root| root[2] < 0.35));
    // A broadleaf: a clear trunk under a rounded crown that is widest near its middle.
    assert!(height(broadleaf) > 5.5 && height(broadleaf) < 8.);
    assert!(
        broadleaf.leaves.vectors[ROOT]
            .iter()
            .all(|root| root[2] > 1.6),
        "a clear trunk"
    );
    assert!(girth(broadleaf, 3.5, 5.) > girth(broadleaf, 5.8, 9.));
    assert!(width(broadleaf) > 3.5);
    // A conifer: tall and narrow, tapering steadily to a point.
    assert!(height(conifer) > 8. && width(conifer) < 0.7 * height(conifer));
    let bands = [
        girth(conifer, 1., 3.),
        girth(conifer, 3., 5.),
        girth(conifer, 5., 7.),
        girth(conifer, 7., 9.),
    ];
    assert!(
        bands.windows(2).all(|pair| pair[1] < 0.85 * pair[0]),
        "{bands:?}"
    );
    assert!(bands[3] < 0.35 * bands[0]);
}

#[test]
fn the_base_feature_rings_each_stem_at_the_ground() {
    for (definition, tree) in fixtures() {
        assert_eq!(tree.features.len(), definition.trunk.count);
        for (feature, stem) in tree.features.iter().zip(&tree.skeleton) {
            assert_eq!(
                (
                    feature.name.as_str(),
                    feature.part.as_str(),
                    feature.face.as_str()
                ),
                ("base", "trunk", "bark")
            );
            assert!(feature.closed && feature.points.len() == 16);
            assert_eq!(feature.normal, [0., 0., 1.]);
            for point in &feature.points {
                assert!(close(
                    norm(sub(*point, stem.points[0])),
                    definition.trunk.radius,
                    1e-9
                ));
                assert!(point[2].abs() <= definition.trunk.radius, "at the ground");
            }
        }
        assert!(tree.bounds[0][2] >= -definition.trunk.radius - 1e-9);
    }
}

#[test]
fn keys_follow_what_each_part_reads_and_output_is_deterministic() {
    let golden = [GOLDEN_SHRUB, GOLDEN_BROADLEAF, GOLDEN_CONIFER];
    for ((definition, tree), golden) in fixtures().into_iter().zip(golden) {
        let again = generate(&definition, &|| false).unwrap();
        assert_eq!(tree, again);
        // Checked against a wasm32 build of the same code; see the module README.
        assert_eq!(tree.digest(), golden, "{} changed", definition.id);
        let keys =
            |tree: &GeneratedTree| tree.part_keys.iter().cloned().collect::<BTreeMap<_, _>>();
        let original = keys(&tree);
        assert_eq!(
            original.len(),
            tree.part_keys.len(),
            "part names are unique"
        );
        let changed = |edit: &dyn Fn(&mut TreeDefinition)| {
            let mut other = definition.clone();
            edit(&mut other);
            let other = keys(&generate(&other, &|| false).unwrap());
            original
                .iter()
                .filter(|(part, key)| other[*part] != **key)
                .map(|(part, _)| part.clone())
                .collect::<Vec<_>>()
        };
        // Leaf size moves leaves and the canopy hull made around them only.
        assert_eq!(
            changed(&|d| d.leaves.size[1] += 0.01),
            [
                "hull/lod0",
                "hull/lod1",
                "hull/lod2",
                "leaf-placement",
                "shadow"
            ]
        );
        // The second level's bark detail rebuilds that level's bark only.
        let rebuilt = changed(&|d| d.lods[1].radial_segments -= 1);
        assert!(
            rebuilt
                .iter()
                .all(|part| part.ends_with("lod1") && !part.starts_with("leaves"))
                && !rebuilt.is_empty(),
            "{rebuilt:?}"
        );
        // The seed regrows the skeleton, and with it everything but the leaf cluster meshes.
        let regrown = changed(&|d| d.seed += 1);
        let kept: Vec<_> = original
            .keys()
            .filter(|part| !regrown.contains(part))
            .collect();
        assert!(
            kept.iter().all(|part| part.starts_with("leaves/")) && kept.len() == 3,
            "{kept:?}"
        );
    }
}
const GOLDEN_SHRUB: &str = "9d4c616da1aed7b83b2accc7478a994ac1f0b90ed6b07feb6161129322117c82";
const GOLDEN_BROADLEAF: &str = "798a75a222af624b9d47e06b11eb66e372e0708345b50ed46e8a7e818ed281ef";
const GOLDEN_CONIFER: &str = "965e4c7e0a9794d0bc66fc8bf67cb89415d8d2d966bf6cbe204b97538112ca4b";

#[test]
fn budgets_cancellation_and_bad_input_are_reported() {
    let definition = parse("broadleaf.tree.json", BROADLEAF).unwrap();
    let mut small = definition.clone();
    small.limits.max_instances = 100;
    let error = generate(&small, &|| false).unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.pointer.as_str()),
        ("TREE_BUDGET", "/limits/maxInstances")
    );
    assert!(error.message.contains("maxInstances 100"));
    let mut small = definition.clone();
    small.limits.max_vertices = 5000;
    let error = generate(&small, &|| false).unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.pointer.as_str()),
        ("TREE_BUDGET", "/limits/maxVertices")
    );
    assert!(error.message.contains("maxVertices 5000") && error.source == "broadleaf.tree.json");
    let mut dense = definition.clone();
    for level in &mut dense.levels {
        level.count = 200;
    }
    dense.envelope = None;
    let error = generate(&dense, &|| false).unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.pointer.as_str()),
        ("TREE_BUDGET", "/levels/1/count")
    );

    assert_eq!(
        generate(&definition, &|| true).unwrap_err().code,
        "TREE_CANCELLED"
    );
    let polls = Cell::new(0);
    let error = generate(&definition, &|| {
        polls.set(polls.get() + 1);
        polls.get() > 20
    })
    .unwrap_err();
    assert_eq!(error.code, "TREE_CANCELLED");
    assert_eq!(
        polls.get(),
        21,
        "stopped at the first poll after the request"
    );

    let bad =
        |from: &str, to: &str| parse("bad.tree.json", &BROADLEAF.replace(from, to)).unwrap_err();
    let error = bad("\"radius\": 0.17", "\"radius\": 0");
    assert_eq!(
        (error.code.as_str(), error.pointer.as_str()),
        ("TREE_VALUE", "/trunk/radius")
    );
    assert_eq!(
        bad("\"leafKeep\": 0.45", "\"leafKeep\": 2").pointer,
        "/lods/1/leafKeep"
    );
    assert_eq!(
        bad("\"shape\": \"leaf\"", "\"shape\": \"star\"").code,
        "TREE_JSON"
    );
    assert_eq!(
        bad("\"kind\": \"tree\"", "\"kind\": \"grass\"").pointer,
        "/kind"
    );
    assert_eq!(
        bad("\"radialSegments\": 6", "\"radialSegments\": 12").pointer,
        "/lods/1"
    );
}
