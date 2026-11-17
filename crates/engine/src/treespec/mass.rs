//! The mass model of a plant's foliage: what a painter blocks in before marking leaves. Leaf
//! clusters that crowd together are **clumps**, each its own small metaball surface (the canopy
//! hull's field, `hull.rs`, made from that clump's clusters alone); runs of clusters along a
//! branch outside every clump are **sprigs**, each a tube along its branch as wide as its
//! leaves spread, full near its start and drawn to a point at its end. The model is lit and
//! banded as one surface, so a shoot leaving the mass reads as one stroke with a sunny side
//! and a shaded side rather than as leaves lit one by one, and a plant is many masses, not one
//! shrink-wrapped blob.
//!
//! Deterministic: every collection that orders output is ordered by index or by `BTreeMap`,
//! and only IEEE basic operations and `sqrt` are used, so native and Wasm builds agree.
use super::hull::CanopyField;
use super::random::{TAU, hash, perpendicular, unit_float, unit_or};
use super::tree::{Blob, Branch};
use super::{BEND_WEIGHT, ELEMENT, FoliageMesh, HEIGHT_RATIO, PIVOT, RANDOM, SHADE_NORMAL};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use superspec_geometry::math::{V3, add, cos, cross, dot, length, scale, sin, sub};

/// How a plant's leaf clusters are divided into clumps and sprigs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Mass {
    /// Meters within which a cluster counts its neighbours, and links to another dense one.
    pub radius: f64,
    /// Neighbours within `radius` that make a cluster dense.
    pub dense: usize,
    /// Fewest dense clusters a linked group needs to be a clump; smaller groups go to sprigs.
    pub clump: usize,
    /// Meters: a clump's clusters are gathered into one ball per cell of this size.
    pub cell: f64,
    /// Fewest clusters in a row along a branch, outside every clump, that make a sprig.
    pub sprig: usize,
    /// Structure: clumps never join clusters whose ancestors at this branch level differ, so a
    /// clump stays on one stem (0) or one limb (1: the branches on a stem).
    #[serde(default)]
    pub group: usize,
    /// Structure: a cluster on a branch shorter than this many meters (a side twig) needs only
    /// half of `dense` neighbours to be dense, so twigs fill a clump and long shoots are sprigs.
    #[serde(default)]
    pub twig: f64,
    /// The share of a sprig's clusters, from its end, that stay leaves; the rest of the sprig
    /// and every clump are drawn by their mass. 1 keeps all of a sprig's leaves.
    #[serde(default = "all")]
    pub tips: f64,
    /// Meters across a paint dab on a clump's surface; 0 draws no dabs.
    #[serde(default)]
    pub dab: f64,
    /// How much of a dab's shade normal is the whole plant's hull rather than its own clump's (0 to 1).
    #[serde(default)]
    pub blend: f64,
    /// Small highlight leaves just outside each dab, shown only where the clump is brightest.
    #[serde(default)]
    pub sparkles: usize,
    /// Shells of dabs per clump, the outer at its surface and each further one deeper inside, so a clump has paint
    /// behind and in front of whatever a viewer sees through (1: the surface only).
    #[serde(default = "one_layer")]
    pub layers: usize,
}
fn one_layer() -> usize {
    1
}
fn all() -> f64 {
    1.
}

/// One leaf cluster as the mass model reads it.
#[derive(Clone, Copy, Debug)]
pub struct Site {
    pub branch: usize,
    /// Where on its branch, as a fraction of the branch's length.
    pub t: f64,
    pub centre: V3,
    /// The cluster's length, meters.
    pub size: f64,
}

/// A sprig: a stretch of one branch as a centre line with a radius at every point.
pub struct Sprig {
    pub branch: usize,
    pub points: Vec<V3>,
    pub radii: Vec<f64>,
    pub bends: Vec<f64>,
}

pub struct Model {
    pub clumps: Vec<CanopyField>,
    pub sprigs: Vec<Sprig>,
    /// Per site: `Some(clump)` for a clump's cluster, else `None`.
    pub clump_of: Vec<Option<usize>>,
    /// Per site: `Some(sprig)` for a sprig's cluster, else `None` (a clump's, or a loose one).
    pub sprig_of: Vec<Option<usize>>,
    /// Per site: whether it stays a leaf (loose, or near a sprig's end) rather than being drawn
    /// by the mass.
    pub leaf: Vec<bool>,
}

/// A paint dab on a clump: its centre, the clump surface's normal there and its wind weight.
pub struct Dab {
    pub centre: V3,
    pub normal: V3,
    pub bend: f64,
    pub clump: usize,
    /// 0 for the clump's surface, deeper shells counting up.
    pub layer: usize,
}

/// Buckets site indices by `floor(p / size)`.
fn grid(sites: &[Site], size: f64) -> HashMap<[i64; 3], Vec<usize>> {
    let mut cells = HashMap::<[i64; 3], Vec<usize>>::new();
    for (index, site) in sites.iter().enumerate() {
        cells
            .entry(site.centre.map(|v| (v / size).floor() as i64))
            .or_default()
            .push(index);
    }
    cells
}

/// Every other site within `radius` of `at`, in a fixed order.
fn near(
    at: usize,
    sites: &[Site],
    cells: &HashMap<[i64; 3], Vec<usize>>,
    radius: f64,
) -> Vec<usize> {
    let cell = sites[at].centre.map(|v| (v / radius).floor() as i64);
    let mut found = vec![];
    for dx in -1..=1 {
        for dy in -1..=1 {
            for dz in -1..=1 {
                if let Some(list) = cells.get(&[cell[0] + dx, cell[1] + dy, cell[2] + dz]) {
                    for &other in list {
                        let d = sub(sites[at].centre, sites[other].centre);
                        if other != at && dot(d, d) < radius * radius {
                            found.push(other);
                        }
                    }
                }
            }
        }
    }
    found.sort_unstable();
    found
}

fn root(parents: &mut [usize], mut i: usize) -> usize {
    while parents[i] != i {
        parents[i] = parents[parents[i]];
        i = parents[i];
    }
    i
}

pub fn build(sites: &[Site], skeleton: &[Branch], mass: &Mass, leaf_size: f64) -> Model {
    let cells = grid(sites, mass.radius);
    let neighbours: Vec<Vec<usize>> = (0..sites.len())
        .map(|i| near(i, sites, &cells, mass.radius))
        .collect();
    // Density and structure: a side twig's clusters need half as many neighbours.
    let dense: Vec<bool> = (0..sites.len())
        .map(|i| {
            let twig = skeleton[sites[i].branch].length < mass.twig;
            neighbours[i].len()
                >= if twig {
                    mass.dense.div_ceil(2)
                } else {
                    mass.dense
                }
        })
        .collect();
    // Each site's ancestor at the grouping level: clumps do not cross it.
    let group: Vec<usize> = sites
        .iter()
        .map(|site| {
            let mut b = site.branch;
            while skeleton[b].level > mass.group {
                b = skeleton[b]
                    .parent
                    .expect("a branch above level 0 has a parent");
            }
            b
        })
        .collect();
    // Dense sites linked to dense neighbours on the same limb, grouped.
    let mut parents: Vec<usize> = (0..sites.len()).collect();
    for i in 0..sites.len() {
        if !dense[i] {
            continue;
        }
        for &j in &neighbours[i] {
            if dense[j] && group[i] == group[j] {
                let (a, b) = (root(&mut parents, i), root(&mut parents, j));
                if a != b {
                    parents[a.max(b)] = a.min(b);
                }
            }
        }
    }
    let mut groups = BTreeMap::<usize, Vec<usize>>::new();
    for i in (0..sites.len()).filter(|i| dense[*i]) {
        let r = root(&mut parents, i);
        groups.entry(r).or_default().push(i);
    }
    let mut clump_of = vec![None; sites.len()];
    let mut clumps = vec![];
    for members in groups.values().filter(|m| m.len() >= mass.clump) {
        // One ball per cell of the clump's clusters.
        let mut balls = BTreeMap::<[i64; 3], Vec<usize>>::new();
        for &i in members {
            balls
                .entry(sites[i].centre.map(|v| (v / mass.cell).floor() as i64))
                .or_default()
                .push(i);
        }
        let blobs: Vec<Blob> = balls
            .values()
            .map(|cell| {
                let n = cell.len() as f64;
                let center = cell
                    .iter()
                    .fold([0.; 3], |s, i| add(s, scale(sites[*i].centre, 1. / n)));
                let (mut radius, mut squares, mut bend) = (0f64, 0., 0.);
                for &i in cell {
                    let d = sub(sites[i].centre, center);
                    radius = radius.max(length(d));
                    squares += dot(d, d);
                    bend += skeleton[sites[i].branch].bend_at(sites[i].t);
                }
                Blob {
                    branch: sites[cell[0]].branch,
                    center,
                    radius,
                    spread: (squares / n).sqrt(),
                    bend: bend / n,
                }
            })
            .collect();
        if let Some(field) = CanopyField::new(&blobs, leaf_size) {
            for &i in members {
                clump_of[i] = Some(clumps.len());
            }
            clumps.push(field);
        }
    }
    // Sprigs: runs along a branch of clusters outside every clump.
    let mut by_branch = BTreeMap::<usize, Vec<usize>>::new();
    for (i, site) in sites.iter().enumerate() {
        by_branch.entry(site.branch).or_default().push(i);
    }
    let mut sprig_of = vec![None; sites.len()];
    // Loose clusters stay leaves; a clump's do not; a sprig's do near its end.
    let mut leaf: Vec<bool> = clump_of.iter().map(|c| c.is_none()).collect();
    let mut sprigs = vec![];
    for (branch, mut list) in by_branch {
        list.sort_by(|a, b| sites[*a].t.total_cmp(&sites[*b].t).then(a.cmp(b)));
        let mut run: Vec<usize> = vec![];
        let mut flush = |run: &mut Vec<usize>, sprigs: &mut Vec<Sprig>| {
            if run.len() >= mass.sprig {
                if let Some(sprig) = sprig(&skeleton[branch], run, sites) {
                    let kept = (mass.tips * run.len() as f64).ceil() as usize;
                    for (k, &i) in run.iter().enumerate() {
                        sprig_of[i] = Some(sprigs.len());
                        leaf[i] = k + kept >= run.len();
                    }
                    sprigs.push(sprig);
                }
            }
            run.clear();
        };
        for i in list {
            if clump_of[i].is_some() {
                flush(&mut run, &mut sprigs);
            } else {
                run.push(i);
            }
        }
        flush(&mut run, &mut sprigs);
    }
    Model {
        clumps,
        sprigs,
        clump_of,
        sprig_of,
        leaf,
    }
}

/// Optical depth divisor: a ray through `OPEN_DENSITY` occluder weights per `radius` of travel loses `1 - 1/e`.
const OPEN_DENSITY: f64 = 20.;

/// How much light gets through the plant's own leaves from each of `points` toward ±x, ±y and ±z: for each axis a
/// cone of rays (the axis and four tilted about 35° off it) marched outward through `occluders` (every leaf
/// cluster's centre, each a `(1 - s)²` kernel of `radius`), their transmittances `exp(-depth)` averaged. In the
/// order +x, +y, +z, -x, -y, -z. A renderer reads any direction from them as an ambient cube, so a clump buried
/// among others reads dark every way, and one shadowed by leaves toward the sun reads dark toward it.
pub fn openness(points: &[V3], occluders: &[V3], radius: f64) -> Vec<[f64; 6]> {
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for p in occluders {
        for a in 0..3 {
            low[a] = low[a].min(p[a]);
            high[a] = high[a].max(p[a]);
        }
    }
    let mut cells = HashMap::<[i64; 3], Vec<usize>>::new();
    for (index, p) in occluders.iter().enumerate() {
        cells
            .entry(p.map(|v| (v / radius).floor() as i64))
            .or_default()
            .push(index);
    }
    let inverse = 1. / (radius * radius);
    let density = |p: V3| -> f64 {
        let cell = p.map(|v| (v / radius).floor() as i64);
        let mut sum = 0.;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(list) = cells.get(&[cell[0] + dx, cell[1] + dy, cell[2] + dz]) {
                        for &i in list {
                            let d = sub(p, occluders[i]);
                            let s = dot(d, d) * inverse;
                            if s < 1. {
                                sum += (1. - s) * (1. - s);
                            }
                        }
                    }
                }
            }
        }
        sum
    };
    let axes: [V3; 6] = [
        [1., 0., 0.],
        [0., 1., 0.],
        [0., 0., 1.],
        [-1., 0., 0.],
        [0., -1., 0.],
        [0., 0., -1.],
    ];
    let step = radius;
    let outside = |p: V3| (0..3).any(|a| p[a] < low[a] - radius || p[a] > high[a] + radius);
    points
        .iter()
        .map(|&point| {
            std::array::from_fn(|k| {
                let axis = axes[k];
                let side = perpendicular(axis);
                let up = cross(axis, side);
                let mut total = 0.;
                for ray in 0..5 {
                    let direction = if ray == 0 {
                        axis
                    } else {
                        let a = TAU * (ray - 1) as f64 / 4.;
                        unit_or(
                            add(
                                axis,
                                scale(add(scale(side, cos(a)), scale(up, sin(a))), 0.7),
                            ),
                            axis,
                        )
                    };
                    let mut depth = 0.;
                    let mut t = radius;
                    loop {
                        let p = add(point, scale(direction, t));
                        if outside(p) || depth > 6. {
                            break;
                        }
                        depth += density(p) * step / (OPEN_DENSITY * radius);
                        t += step;
                    }
                    total += (-depth).exp();
                }
                total / 5.
            })
        })
        .collect()
}

impl Model {
    /// Paint dabs spread evenly over every clump's surface, about `spacing` meters apart: the
    /// vertices of the clump's surface mesh at a division chosen from its size.
    pub fn dabs(&self, spacing: f64, layers: usize) -> Vec<Dab> {
        let mut dabs = vec![];
        for layer in 0..layers.max(1) {
            // Each deeper shell is 0.7 of the one outside it.
            let shrink = 0.7f64.powi(layer as i32);
            dabs.extend(
                self.shell(spacing, shrink)
                    .into_iter()
                    .map(|d| Dab { layer, ..d }),
            );
        }
        dabs
    }

    fn shell(&self, spacing: f64, shrink: f64) -> Vec<Dab> {
        let mut dabs = vec![];
        for (index, clump) in self.clumps.iter().enumerate() {
            let probe = clump.mesh(String::new(), String::new(), 2, 1.);
            let reach = probe
                .positions
                .iter()
                .map(|p| length(sub(*p, clump.origin)))
                .sum::<f64>()
                / probe.positions.len() as f64;
            // An octahedron divided k times per edge has about 4k² vertices over 4πR².
            let k = ((reach * shrink * 1.7725 / spacing.max(1e-3)).ceil() as usize).clamp(2, 24);
            let mesh = clump.mesh(String::new(), String::new(), k, shrink);
            for (n, p) in mesh.positions.iter().enumerate() {
                dabs.push(Dab {
                    centre: *p,
                    normal: mesh.normals[n],
                    bend: mesh.scalars[BEND_WEIGHT][n],
                    clump: index,
                    layer: 0,
                });
            }
        }
        dabs
    }
}

/// A sprig along `branch` over the run's stretch: it starts a little before its first cluster
/// (so it leaves the mass it grows from), is as wide as its clusters spread from the branch,
/// widens over its first third and narrows to a point at its end.
fn sprig(branch: &Branch, run: &[usize], sites: &[Site]) -> Option<Sprig> {
    let back = (0.03 / branch.length.max(1e-6)).min(0.2);
    let (t0, t1) = (
        (sites[run[0]].t - back).max(0.),
        sites[run[run.len() - 1]].t,
    );
    let span = (t1 - t0) * branch.length;
    if span < 1e-3 {
        return None;
    }
    let mut spread = 0.;
    for &i in run {
        let (axis, along, _) = branch.at(sites[i].t);
        let d = sub(sites[i].centre, axis);
        let off = sub(d, scale(along, dot(d, along)));
        spread += length(off) + 0.5 * sites[i].size;
    }
    let radius = spread / run.len() as f64;
    let samples = ((span / 0.015).ceil() as usize).clamp(3, 32);
    let (mut points, mut radii, mut bends) = (vec![], vec![], vec![]);
    for k in 0..=samples {
        let u = k as f64 / samples as f64;
        let t = t0 + (t1 - t0) * u;
        let (p, _, _) = branch.at(t);
        let profile = if u < 0.3 {
            0.55 + 0.45 * u / 0.3
        } else {
            let v = (u - 0.3) / 0.7;
            1. - 0.92 * v * v.sqrt()
        };
        points.push(p);
        radii.push(radius * profile);
        bends.push(branch.bend_at(t));
    }
    Some(Sprig {
        branch: branch.id,
        points,
        radii,
        bends,
    })
}

impl Model {
    /// Every clump's surface and every sprig's tube in one mesh with the canopy hull's
    /// attributes: normals and `shadeNormal` are the surface's, `element` numbers the parts
    /// (clumps first, then sprigs), `heightRatio` runs from the lowest point to the highest.
    /// `backing`: only the clumps, each `shrink` of its size, to fill the gaps behind a plant's dabs.
    pub fn mesh(
        &self,
        id: String,
        geometry_key: String,
        frequency: usize,
        radial: usize,
        backing: Option<f64>,
    ) -> FoliageMesh {
        let mut mesh = FoliageMesh::new(id, geometry_key, "canopy", false);
        for (index, clump) in self.clumps.iter().enumerate() {
            let part = clump.mesh(
                String::new(),
                String::new(),
                frequency,
                backing.unwrap_or(1.),
            );
            let first = mesh.positions.len() as u32;
            mesh.positions.extend(&part.positions);
            mesh.normals.extend(&part.normals);
            mesh.uvs.extend(&part.uvs);
            for n in 0..part.positions.len() {
                mesh.scalar(BEND_WEIGHT, part.scalars[BEND_WEIGHT][n]);
                mesh.scalar(RANDOM, part.scalars[RANDOM][n]);
                mesh.scalar(ELEMENT, index as f64);
                mesh.vector(SHADE_NORMAL, part.normals[n]);
                mesh.vector(PIVOT, [0.; 3]);
            }
            mesh.indices.extend(part.indices.iter().map(|i| i + first));
        }
        // Coarser clumps take coarser sprigs: every ring at the full frequency, every second at half, every fourth below.
        let stride = (super::tree::IMPOSTOR_FREQUENCY / frequency.max(1)).max(1);
        for (index, sprig) in self.sprigs.iter().enumerate().filter(|_| backing.is_none()) {
            let element = (self.clumps.len() + index) as f64;
            let first = mesh.positions.len() as u32;
            let full = sprig.points.len() - 1;
            // The rings kept, always with the last (the point closes it).
            let mut rings: Vec<usize> = (0..full).step_by(stride).collect();
            rings.push(full);
            let last = rings.len() - 1;
            let width = radial as u32;
            for (n, &k) in rings.iter().enumerate() {
                let along = unit_or(
                    sub(
                        sprig.points[rings[(n + 1).min(last)]],
                        sprig.points[rings[n.saturating_sub(1)]],
                    ),
                    [0., 0., 1.],
                );
                let side = perpendicular(along);
                let up = cross(along, side);
                for step in 0..radial {
                    let angle = TAU * step as f64 / radial as f64;
                    let out = add(scale(side, cos(angle)), scale(up, sin(angle)));
                    mesh.positions
                        .push(add(sprig.points[k], scale(out, sprig.radii[k])));
                    mesh.normals.push(out);
                    mesh.uvs
                        .push([step as f64 / radial as f64, k as f64 / full as f64]);
                    mesh.scalar(BEND_WEIGHT, sprig.bends[k]);
                    mesh.scalar(
                        RANDOM,
                        unit_float(hash(0x5b12, &[index as i64, k as i64, step as i64])),
                    );
                    mesh.scalar(ELEMENT, element);
                    mesh.vector(SHADE_NORMAL, out);
                    mesh.vector(PIVOT, [0.; 3]);
                }
            }
            for k in 0..last as u32 {
                for step in 0..width {
                    let (a, b) = (
                        first + k * width + step,
                        first + k * width + (step + 1) % width,
                    );
                    let (c, d) = (a + width, b + width);
                    mesh.indices.extend([a, b, d, a, d, c]);
                }
            }
            // The point: one vertex past the last ring, closing it.
            let tip_along = unit_or(
                sub(sprig.points[full], sprig.points[rings[last - 1]]),
                [0., 0., 1.],
            );
            let tip = mesh.positions.len() as u32;
            mesh.positions
                .push(add(sprig.points[full], scale(tip_along, sprig.radii[full])));
            mesh.normals.push(tip_along);
            mesh.uvs.push([0.5, 1.]);
            mesh.scalar(BEND_WEIGHT, sprig.bends[full]);
            mesh.scalar(RANDOM, unit_float(hash(0x5b12, &[index as i64, -1])));
            mesh.scalar(ELEMENT, element);
            mesh.vector(SHADE_NORMAL, tip_along);
            mesh.vector(PIVOT, [0.; 3]);
            let ring = first + last as u32 * width;
            for step in 0..width {
                mesh.indices
                    .extend([ring + step, ring + (step + 1) % width, tip]);
            }
        }
        let (low, high) = mesh
            .positions
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), p| {
                (l.min(p[2]), h.max(p[2]))
            });
        let span = (high - low).max(1e-9);
        let heights: Vec<f64> = mesh.positions.iter().map(|p| (p[2] - low) / span).collect();
        mesh.scalars.insert(HEIGHT_RATIO.into(), heights);
        mesh
    }
}
