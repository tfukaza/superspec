//! Illustrated avenue vegetation. A shadow batch carries the local density sites and twigs
//! that the shared renderer draws, alongside the trunk and branch LODs.
use super::cache::Cache;
use crate::{
    treespec::{
        batches::{self, FoliageBatch},
        tree,
    },
    worldspec::build::ItemOutput,
};
use std::sync::{Arc, Mutex};
use superspec_model::batches::Foliage;
use superspec_types::{architecture::Node, batch::FoliageFamily, diagnostic::Diagnostic};
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
    let all_leaves = &plant.leaves;
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

fn street(station: usize) -> Result<tree::TreeDefinition, String> {
    let mut definition = scaled(
        tree::parse(
            "street.tree.json",
            include_str!("../../../treespec/fixtures/street.tree.json"),
        )
        .map_err(|e| e.to_string())?,
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
    Ok(definition)
}
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
    length: f64,
    width: f64,
    point: &dyn Fn(f64, f64) -> [f64; 2],
    cancelled: &dyn Fn() -> bool,
    look: &superspec_model::batches::Foliage,
    height: &dyn Fn(f64, f64) -> f64,
) -> Result<FoliageBatch, String> {
    use superspec_types::batch::{FoliageFamily, FoliageMass};
    let inset = 0.32;
    let along = [
        2.5_f64.min(length / 4.) + inset,
        length - 3.5_f64.min(length / 4.) - inset,
    ];
    let across = [width / 2. - 0.85 + inset, width / 2. + 0.85 - inset];
    let [bx, by] = point(along[0], across[0]);
    let base = [bx, by, height(bx, by)];
    let mut shrub = tree::parse(
        "shrub.tree.json",
        include_str!("../../../treespec/fixtures/shrub.tree.json"),
    )
    .map_err(|e| e.to_string())?;
    // Each mound contributes leaf sites and twigs to a single painted mass. Keep
    // the near level for TreeSpec's vertex-budget check and omit its unused distance meshes.
    shrub.lods.truncate(1);
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
            let plant = tree::generate(&definition, cancelled).map_err(|e| e.to_string())?;
            let mass = painted_mass(&definition, &plant);
            let [x, y] = point(s, t);
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
    let template = tree::generate(&shrub, cancelled).map_err(|e| e.to_string())?;
    let mut carrier =
        batches::tree_levels_for(&template, base, look, |f| f == FoliageFamily::Shadow)?
            .into_iter()
            .next()
            .ok_or("Missing groundcover shadow proxy")?;
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
        look: "sprays".into(),
    });
    Ok(carrier)
}

fn plant(
    station: usize,
    shrub: bool,
    look: &Foliage,
    cancelled: &dyn Fn() -> bool,
    cache: &Mutex<Cache>,
) -> Result<Arc<[FoliageBatch]>, String> {
    let mut definition = if shrub {
        tree::parse(
            "shrub.tree.json",
            include_str!("../../../treespec/fixtures/shrub.tree.json"),
        )
        .map_err(|e| e.to_string())?
    } else {
        street(station)?
    };
    if shrub {
        for (lod, distance) in definition.lods.iter_mut().zip([0., 8., 16.]) {
            lod.distance = distance;
        }
    }
    crate::treespec::check_cancelled(&definition.source, cancelled).map_err(|e| e.to_string())?;
    // All generation and painterly look inputs are intrinsic. Source locations, row names,
    // translations, rotations and placement scale are applied to a copy when the plant is bound.
    let key = crate::runtime::dependencies::producer_key(
        "kowloon-painted-plant-v1",
        &serde_json::to_vec(&(&definition, look, shrub)).map_err(|e| e.to_string())?,
    );
    if let Some(levels) = cache.lock().expect("plant cache").plant(&key) {
        return Ok(levels);
    }
    let tree = tree::generate(&definition, cancelled).map_err(|e| e.to_string())?;
    let mut mass = painted_mass(&definition, &tree);
    mass.look = "leaves".into();
    mass.palette = LANE_PALETTE;
    if shrub {
        mass.form = 0.8;
        mass.jitter = 0.15;
        mass.leaf_scale = 1.3;
        mass.soft_edges = false;
        mass.card_scale = 0.6;
        mass.shapes = 0.3;
        mass.variation = 0.8;
        mass.sparkles = 0.4;
    }
    let mut levels = batches::tree_levels_for(&tree, [0.; 3], look, |f| {
        matches!(f, FoliageFamily::Bark | FoliageFamily::Shadow)
    })?;
    for level in &mut levels {
        let f = level.batch.foliage.as_mut().unwrap();
        if f.family == FoliageFamily::Shadow {
            f.mass = Some(std::mem::take(&mut mass));
        } else {
            f.material = Some("painted-bark".into());
        }
    }
    let levels: Arc<[FoliageBatch]> = levels.into();
    let bytes = levels.iter().map(plant_bytes).sum();
    cache
        .lock()
        .expect("plant cache")
        .retain_plant(key, levels.clone(), bytes);
    Ok(levels)
}

/// Retained local bark and shadow levels have a single placement, named vertex attributes
/// and, on the carrier, the canopy sites and twigs. Charge buffer capacities, including the
/// per-twig vectors, plus the batch, strings, map nodes and the single transform.
fn plant_bytes(level: &FoliageBatch) -> usize {
    let mesh = &level.mesh;
    let batch = &level.batch;
    let mass = batch.foliage.as_ref().and_then(|f| f.mass.as_ref());
    8192 + 24 * (mesh.positions.capacity() + mesh.normals.capacity() + mesh.colors.capacity())
        + 16 * mesh.uvs.capacity()
        + 4 * mesh.indices.capacity()
        + batch
            .vertex_scalars
            .values()
            .map(|v| v.capacity() * 4)
            .sum::<usize>()
        + batch
            .vertex_vectors
            .values()
            .map(|v| v.capacity() * 12)
            .sum::<usize>()
        + mass.map_or(0, |m| {
            m.sites.capacity() * 32
                + m.twigs.capacity() * size_of::<Vec<[f64; 3]>>()
                + m.twigs.iter().map(|t| t.capacity() * 24).sum::<usize>()
        })
}

pub(super) fn append(
    items: &mut ItemOutput<'_>,
    node: &Node,
    cancelled: &dyn Fn() -> bool,
    cache: &Mutex<Cache>,
) -> Result<(), Diagnostic> {
    let look:Foliage=serde_json::from_str(r#"{"family":"leaves","patch":"plant","far":60,"material":"painted-canopy","wind":{"azimuth":30,"angle":2.5,"scale":6,"speed":0.8,"bias":0.7,"flutter":9},"rejection":{"boundary":0,"fill":1,"grazing":0,"grazingKeep":0},"canopy":{}}"#).expect("street foliage settings");
    let origin = node.vector("position", [0.; 3]);
    let step = node.vector("step", [0.; 3]);
    let count = node.number("count", 1.) as usize;
    let angle = node.number("heading", 0.).to_radians();
    for index in 0..count {
        let at = std::array::from_fn(|k| origin[k] + index as f64 * step[k]);
        let owner = superspec_profiles::city::placement_id(&node.id, count, index + 1);
        let levels = if node.kind == "avenue" {
            let (sin, cos) = angle.sin_cos();
            let point = |s: f64, t: f64| [at[0] + cos * s + sin * t, at[1] + sin * s - cos * t];
            let height = |x: f64, y: f64| {
                at[2] + 0.35 - 0.14
                    + 0.052
                    + 0.03
                        * ((1.3 * x + 0.4).sin() * (0.8 * y).cos()
                            + 0.5 * (0.5 * x - 0.9 * y).sin())
            };
            groundcover(
                node.number("length", 32.),
                node.number("width", 8.2),
                &point,
                cancelled,
                &look,
                &height,
            )
            .map(|b| Arc::from([b]))
        } else {
            plant(index, node.kind == "shrub", &look, cancelled, cache)
        }
        .map_err(|e| node.fail("AML_CITY", e))?;
        for level in levels.iter() {
            let mut batch = level.batch.clone();
            batch.id = format!("{owner}/{}", batch.id);
            batch.object_id = owner.clone();
            batch.source = node.source.clone();
            batch.foliage.as_mut().unwrap().patch = owner.clone();
            if node.kind != "avenue" {
                for p in &mut batch.placements {
                    p.translation = at;
                    p.rotation = [0., 0., (angle / 2.).sin(), (angle / 2.).cos()];
                    for axis in &mut p.scale {
                        *axis *= node.number("scale", 1.);
                    }
                }
            }
            items
                .add_batch(&batch, &level.mesh)
                .map_err(|e| node.fail("AML_CITY", e))?;
        }
    }
    items
        .project_foliage()
        .map_err(|e| node.fail("AML_CITY", e))?;
    Ok(())
}
