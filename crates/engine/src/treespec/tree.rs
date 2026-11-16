//! Shrubs and trees: a branch skeleton, bark tubes with metric coordinates and leaf
//! clusters as one instance batch, each with a level-of-detail chain.
use super::random::{Stream, TAU, hash, perpendicular, reject, tilted, unit_float, unit_or};
use super::{
    BEND_WEIGHT, Bounds, CLUSTER, ELEMENT, EMPTY_BOUNDS, Feature, FoliageMesh, HEIGHT_RATIO,
    InstanceBatch, LEVEL, Limits, MAX_DEFINITION_BYTES, PIVOT, RANDOM, ROOT, SHADE_NORMAL,
    THRESHOLD, TreeDiagnostic, VARIATION, check_cancelled, fail, grow, header, key, range, within,
};
use serde::{Deserialize, Serialize};
use superspec_geometry::math::{V3, add, cos, cross, dot, length, lerp, scale, sin, sub};

pub const MAX_BRANCHES: usize = 20_000;
const UP: V3 = [0., 0., 1.];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TreeDefinition {
    pub format: String,
    pub version: u32,
    pub kind: String,
    pub id: String,
    #[serde(skip)]
    pub source: String,
    pub seed: u64,
    pub trunk: Trunk,
    /// `levels[0]` describes the branches on a trunk, `levels[1]` those on them, and so on.
    pub levels: Vec<Level>,
    /// Branches end where they leave it; ones that start outside it do not grow.
    #[serde(default)]
    pub envelope: Option<Envelope>,
    pub leaves: Leaves,
    pub lods: Vec<TreeLodDefinition>,
    #[serde(default)]
    pub limits: Limits,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Trunk {
    /// Stems rising from the base: one for a tree, several for a shrub.
    #[serde(default = "one")]
    pub count: usize,
    /// How far several stems tilt apart, degrees from vertical.
    #[serde(default)]
    pub spread: f64,
    /// Length along the stem, meters.
    pub height: f64,
    /// Radius at the ground, meters.
    pub radius: f64,
    /// Radius at the tip as a fraction of the base radius.
    pub tip: f64,
    pub segments: usize,
    /// Random turn per segment, degrees.
    #[serde(default)]
    pub wobble: f64,
    /// Tilt of a single stem, degrees from vertical.
    #[serde(default)]
    pub lean: f64,
}
fn one() -> usize {
    1
}
fn golden() -> f64 {
    137.5
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Level {
    /// Branches per parent.
    pub count: usize,
    /// The stretch of the parent that bears branches, as fractions of its length.
    pub start: f64,
    pub end: f64,
    /// Angle from the parent's direction, degrees, at the start and end of that stretch.
    pub angle: [f64; 2],
    /// Length as a fraction of the parent's length, at the start and end of that stretch.
    pub length: [f64; 2],
    /// Base radius as a fraction of the parent's radius where the branch leaves it.
    pub radius: f64,
    /// Tip radius as a fraction of the branch's base radius.
    pub tip: f64,
    pub segments: usize,
    /// Total turn toward the ground over the branch, degrees. Negative turns it upward,
    /// as growth toward light does.
    #[serde(default)]
    pub gravity: f64,
    /// Random turn per segment, degrees.
    #[serde(default)]
    pub wobble: f64,
    /// Turn around the parent between one branch and the next, degrees.
    #[serde(default = "golden")]
    pub twist: f64,
    /// 0 spaces branches evenly; 1 moves each anywhere in its share of the parent.
    #[serde(default)]
    pub jitter: f64,
    /// 1 lays the starting direction into the horizontal plane; 0 leaves it.
    #[serde(default)]
    pub flatten: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Envelope {
    Ellipsoid {
        center: V3,
        radii: V3,
    },
    /// Upright about the trunk's base: `radius` wide at height `base`, a point at `top`.
    Cone {
        base: f64,
        top: f64,
        radius: f64,
    },
}
impl Envelope {
    pub fn contains(&self, p: V3) -> bool {
        match self {
            Self::Ellipsoid { center, radii } => {
                (0..3)
                    .map(|a| {
                        let v = (p[a] - center[a]) / radii[a];
                        v * v
                    })
                    .sum::<f64>()
                    <= 1.
            }
            Self::Cone { base, top, radius } => {
                p[2] >= *base && p[2] <= *top && {
                    let allowed = radius * (top - p[2]) / (top - base);
                    p[0] * p[0] + p[1] * p[1] <= allowed * allowed
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LeafShape {
    /// A rectangle for a cutout texture.
    Quad,
    /// A pointed outline that reads as a leaf without a texture.
    Leaf,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Leaves {
    /// Branches of this level and above bear leaves. Zero includes the trunk.
    pub level: usize,
    /// Distance between clusters along a branch, meters.
    pub spacing: f64,
    /// Where on a branch the leaves begin, as a fraction of its length.
    pub start: f64,
    /// Length of a cluster, meters.
    pub size: [f64; 2],
    /// Width of one card over its length.
    pub aspect: f64,
    /// Cards per cluster at the most detailed level.
    pub cards: usize,
    /// Fan of the cards in a cluster, degrees.
    pub spread: f64,
    /// Angle of a cluster from its branch, degrees.
    pub angle: f64,
    /// Pull of the cluster's direction toward the sky, from 0 to 1.
    #[serde(default)]
    pub lift: f64,
    pub shape: LeafShape,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TreeLodDefinition {
    /// Sides of a bark tube.
    pub radial_segments: usize,
    /// Every `ringStep`-th ring of a branch is kept, and always its last.
    pub ring_step: usize,
    /// Branches above this level have no bark mesh.
    pub max_level: usize,
    /// Fraction of the leaf clusters drawn. Survivors are widened by `1 / sqrt(leafKeep)`.
    pub leaf_keep: f64,
    pub leaf_cards: usize,
    #[serde(default)]
    pub distance: f64,
}

/// One limb of the skeleton: a centre line with a radius at every point.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Branch {
    pub id: usize,
    pub parent: Option<usize>,
    /// 0 for a trunk.
    pub level: usize,
    /// Where it leaves its parent, as a fraction of the parent's length.
    pub attach: f64,
    pub points: Vec<V3>,
    pub radii: Vec<f64>,
    /// Distance along the centre line at each point, meters.
    pub stations: Vec<f64>,
    pub length: f64,
    /// Whether the envelope cut it short.
    pub pruned: bool,
    pub random: f64,
    /// Wind weight at the base and the tip, from 0 to 1 over the plant. Between them it
    /// follows `base + (tip - base) * t * t`.
    pub bend: [f64; 2],
}
impl Branch {
    /// Point, direction and radius at the fraction `t` of the length.
    pub fn at(&self, t: f64) -> (V3, V3, f64) {
        let distance = t.clamp(0., 1.) * self.length;
        let segment = self
            .stations
            .partition_point(|s| *s < distance)
            .clamp(1, self.points.len() - 1);
        let (before, after) = (self.stations[segment - 1], self.stations[segment]);
        let local = if after > before {
            (distance - before) / (after - before)
        } else {
            0.
        };
        (
            lerp(self.points[segment - 1], self.points[segment], local),
            unit_or(sub(self.points[segment], self.points[segment - 1]), UP),
            self.radii[segment - 1] + (self.radii[segment] - self.radii[segment - 1]) * local,
        )
    }
    pub fn bend_at(&self, t: f64) -> f64 {
        self.bend[0] + (self.bend[1] - self.bend[0]) * t * t
    }
    fn direction(&self, point: usize) -> V3 {
        let last = self.points.len() - 1;
        unit_or(
            sub(
                self.points[(point + 1).min(last)],
                self.points[point.saturating_sub(1)],
            ),
            UP,
        )
    }
}

/// The leafy volume: its centre and half extents, and one ball per limb of leaves.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Canopy {
    pub center: V3,
    pub radii: V3,
    pub blobs: Vec<Blob>,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Blob {
    /// The limb (a branch of level 1, or a trunk) whose leaves it encloses.
    pub branch: usize,
    pub center: V3,
    pub radius: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeLod {
    pub radial_segments: usize,
    pub ring_step: usize,
    pub max_level: usize,
    pub leaf_keep: f64,
    pub leaf_widen: f64,
    pub distance: f64,
    /// Indices into `GeneratedTree::meshes`.
    pub trunk: usize,
    pub branches: Option<usize>,
    pub leaf: usize,
    /// The first `leaf_instances` of `GeneratedTree::leaves` are drawn.
    pub leaf_instances: usize,
    pub triangles: usize,
    pub vertices: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedTree {
    pub id: String,
    pub key: String,
    /// `skeleton`, each mesh by its ID, then `leaf-placement`.
    pub part_keys: Vec<(String, String)>,
    pub skeleton: Vec<Branch>,
    pub meshes: Vec<FoliageMesh>,
    pub lods: Vec<TreeLod>,
    /// Leaf clusters sorted by `threshold`. A level draws its `leaf` mesh for the first
    /// `leaf_instances` of them.
    pub leaves: InstanceBatch,
    pub features: Vec<Feature>,
    pub canopy: Canopy,
    pub bounds: Bounds,
}
impl GeneratedTree {
    pub fn digest(&self) -> String {
        super::digest(&self.meshes, [&self.leaves])
    }
}

pub fn parse(source: &str, text: &str) -> Result<TreeDefinition, TreeDiagnostic> {
    if text.len() > MAX_DEFINITION_BYTES {
        return Err(fail(
            source,
            "/",
            "TREE_LIMIT",
            "Tree definition exceeds 1 MiB.",
        ));
    }
    let mut definition: TreeDefinition = serde_json::from_str(text).map_err(|error| {
        fail(
            source,
            "/",
            "TREE_JSON",
            format!("{error} (line {}, column {})", error.line(), error.column()),
        )
    })?;
    definition.source = source.into();
    definition.check()?;
    Ok(definition)
}

fn count(
    source: &str,
    pointer: &str,
    value: usize,
    low: usize,
    high: usize,
) -> Result<(), TreeDiagnostic> {
    if (low..=high).contains(&value) {
        Ok(())
    } else {
        Err(fail(
            source,
            pointer,
            "TREE_VALUE",
            format!("Expected a count from {low} to {high}, found {value}."),
        ))
    }
}

impl TreeDefinition {
    pub fn check(&self) -> Result<(), TreeDiagnostic> {
        let s = self.source.as_str();
        header(s, &self.format, self.version, &self.kind, "tree", &self.id)?;
        let trunk = &self.trunk;
        count(s, "/trunk/count", trunk.count, 1, 16)?;
        within(s, "/trunk/spread", trunk.spread, 0., 80.)?;
        within(s, "/trunk/height", trunk.height, 0.05, 100.)?;
        within(s, "/trunk/radius", trunk.radius, 0.002, 5.)?;
        within(s, "/trunk/tip", trunk.tip, 0.01, 1.)?;
        count(s, "/trunk/segments", trunk.segments, 1, 64)?;
        within(s, "/trunk/wobble", trunk.wobble, 0., 30.)?;
        within(s, "/trunk/lean", trunk.lean, 0., 60.)?;
        count(s, "/levels", self.levels.len(), 0, 4)?;
        for (index, level) in self.levels.iter().enumerate() {
            let p = |field: &str| format!("/levels/{index}/{field}");
            count(s, &p("count"), level.count, 1, 256)?;
            within(s, &p("start"), level.start, 0., 1.)?;
            within(s, &p("end"), level.end, level.start, 1.)?;
            for angle in level.angle {
                within(s, &p("angle"), angle, 0., 170.)?;
            }
            for fraction in level.length {
                within(s, &p("length"), fraction, 0.01, 4.)?;
            }
            within(s, &p("radius"), level.radius, 0.05, 1.)?;
            within(s, &p("tip"), level.tip, 0.01, 1.)?;
            count(s, &p("segments"), level.segments, 1, 64)?;
            within(s, &p("gravity"), level.gravity, -170., 170.)?;
            within(s, &p("wobble"), level.wobble, 0., 30.)?;
            within(s, &p("twist"), level.twist, 0., 360.)?;
            within(s, &p("jitter"), level.jitter, 0., 1.)?;
            within(s, &p("flatten"), level.flatten, 0., 1.)?;
        }
        match &self.envelope {
            Some(Envelope::Ellipsoid { center, radii }) => {
                for axis in 0..3 {
                    within(s, "/envelope/center", center[axis], -1000., 1000.)?;
                    within(s, "/envelope/radii", radii[axis], 0.01, 1000.)?;
                }
            }
            Some(Envelope::Cone { base, top, radius }) => {
                within(s, "/envelope/base", *base, -1000., 1000.)?;
                within(s, "/envelope/top", *top, base + 0.01, 1000.)?;
                within(s, "/envelope/radius", *radius, 0.01, 1000.)?;
            }
            None => {}
        }
        let leaves = &self.leaves;
        count(s, "/leaves/level", leaves.level, 0, self.levels.len())?;
        within(s, "/leaves/spacing", leaves.spacing, 0.005, 100.)?;
        within(s, "/leaves/start", leaves.start, 0., 1.)?;
        range(s, "/leaves/size", leaves.size, 0.005, 10.)?;
        within(s, "/leaves/aspect", leaves.aspect, 0.01, 4.)?;
        count(s, "/leaves/cards", leaves.cards, 1, 16)?;
        within(s, "/leaves/spread", leaves.spread, 0., 360.)?;
        within(s, "/leaves/angle", leaves.angle, 0., 170.)?;
        within(s, "/leaves/lift", leaves.lift, 0., 1.)?;
        count(s, "/lods", self.lods.len(), 1, 8)?;
        for (index, lod) in self.lods.iter().enumerate() {
            let p = |field: &str| format!("/lods/{index}/{field}");
            count(s, &p("radialSegments"), lod.radial_segments, 3, 64)?;
            count(s, &p("ringStep"), lod.ring_step, 1, 64)?;
            count(s, &p("maxLevel"), lod.max_level, 0, 4)?;
            within(s, &p("leafKeep"), lod.leaf_keep, 0.01, 1.)?;
            count(s, &p("leafCards"), lod.leaf_cards, 1, 16)?;
            within(s, &p("distance"), lod.distance, 0., 1e6)?;
            if index > 0 {
                let before = self.lods[index - 1];
                if lod.radial_segments > before.radial_segments
                    || lod.ring_step < before.ring_step
                    || lod.max_level > before.max_level
                    || lod.leaf_keep > before.leaf_keep
                    || lod.leaf_cards > before.leaf_cards
                    || lod.distance < before.distance
                {
                    return Err(fail(
                        s,
                        &format!("/lods/{index}"),
                        "TREE_VALUE",
                        "Each level must be no more detailed and start no nearer than the one before.",
                    ));
                }
            }
        }
        self.limits.check(s)
    }

    /// Grows the skeleton: trunks first, then each level in order, so a branch always
    /// follows its parent.
    pub fn skeleton(&self, cancelled: &dyn Fn() -> bool) -> Result<Vec<Branch>, TreeDiagnostic> {
        self.check()?;
        let source = self.source.as_str();
        let mut branches = Vec::<Branch>::new();
        let trunk = &self.trunk;
        for stem in 0..trunk.count {
            let mut random = Stream::new(self.seed, &[10, stem as i64]);
            let (tilt, turn) = if trunk.count == 1 {
                (trunk.lean, random.next() * TAU)
            } else {
                (
                    trunk.spread * (0.55 + 0.45 * random.next()),
                    (stem as f64 + 0.6 * random.signed()) * TAU / trunk.count as f64,
                )
            };
            let direction = tilted(UP, tilt.to_radians(), turn);
            // Several stems leave the ground side by side rather than from one point.
            let start = if trunk.count == 1 {
                [0.; 3]
            } else {
                scale([cos(turn), sin(turn), 0.], trunk.radius * 1.2)
            };
            let length = trunk.height
                * if stem == 0 {
                    1.
                } else {
                    0.75 + 0.25 * random.next()
                };
            let (points, _) = centre_line(
                start,
                direction,
                length,
                trunk.segments,
                // A stem straightens toward the light as it rises.
                if trunk.count == 1 { 0. } else { -0.5 * tilt },
                trunk.wobble,
                None,
                &mut random,
            );
            branches.push(branch(
                branches.len(),
                None,
                0,
                0.,
                points,
                trunk.radius,
                trunk.tip,
                false,
                random.next(),
            ));
        }
        let mut parents = 0..branches.len();
        for (depth, level) in self.levels.iter().enumerate() {
            let first = branches.len();
            for parent in parents.clone() {
                check_cancelled(source, cancelled)?;
                for index in 0..level.count {
                    let mut random =
                        Stream::new(self.seed, &[11, depth as i64, parent as i64, index as i64]);
                    let share = (index as f64 + 0.5 + 0.5 * level.jitter * random.signed())
                        / level.count as f64;
                    let attach = level.start + (level.end - level.start) * share;
                    let (start, along, parent_radius) = branches[parent].at(attach);
                    let angle = level.angle[0] + (level.angle[1] - level.angle[0]) * share;
                    let turn = (index as f64 * level.twist).to_radians()
                        + branches[parent].random * TAU
                        + 0.35 * random.signed();
                    let mut direction =
                        tilted(along, (angle + 6. * random.signed()).to_radians(), turn);
                    direction = unit_or(
                        add(direction, scale(UP, -dot(direction, UP) * level.flatten)),
                        direction,
                    );
                    let intended = branches[parent].length
                        * (level.length[0] + (level.length[1] - level.length[0]) * share)
                        * (0.85 + 0.3 * random.next());
                    if self.envelope.as_ref().is_some_and(|e| !e.contains(start)) {
                        continue;
                    }
                    let (points, pruned) = centre_line(
                        start,
                        direction,
                        intended,
                        level.segments,
                        level.gravity,
                        level.wobble,
                        self.envelope.as_ref(),
                        &mut random,
                    );
                    let grown: f64 = points.windows(2).map(|p| length(sub(p[1], p[0]))).sum();
                    if points.len() < 2 || grown < 0.2 * intended {
                        continue;
                    }
                    if branches.len() >= MAX_BRANCHES {
                        return Err(fail(
                            source,
                            &format!("/levels/{depth}/count"),
                            "TREE_BUDGET",
                            format!(
                                "The skeleton needs more than {MAX_BRANCHES} branches. Lower the branch counts."
                            ),
                        ));
                    }
                    branches.push(branch(
                        branches.len(),
                        Some(parent),
                        depth + 1,
                        attach,
                        points,
                        // Never thicker than the limb it leaves.
                        parent_radius * level.radius,
                        level.tip,
                        pruned,
                        random.next(),
                    ));
                }
            }
            parents = first..branches.len();
        }
        // Wind weight grows from the ground outward and is continuous where limbs join.
        for index in 0..branches.len() {
            let base = match branches[index].parent {
                Some(parent) => branches[parent].bend_at(branches[index].attach),
                None => 0.,
            };
            let own = if branches[index].level == 0 {
                0.25
            } else {
                0.5
            };
            branches[index].bend = [base, base + own];
        }
        let most = branches.iter().map(|b| b.bend[1]).fold(0., f64::max);
        for branch in &mut branches {
            branch.bend = branch.bend.map(|w| w / most);
        }
        Ok(branches)
    }

    fn skeleton_key(&self) -> String {
        key(
            "treespec-skeleton-v1",
            &(self.seed, &self.trunk, &self.levels, &self.envelope),
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn branch(
    id: usize,
    parent: Option<usize>,
    level: usize,
    attach: f64,
    points: Vec<V3>,
    radius: f64,
    tip: f64,
    pruned: bool,
    random: f64,
) -> Branch {
    let mut stations = vec![0.];
    for pair in points.windows(2) {
        stations.push(stations[stations.len() - 1] + length(sub(pair[1], pair[0])));
    }
    let total = stations[stations.len() - 1];
    Branch {
        id,
        parent,
        level,
        attach,
        radii: stations
            .iter()
            .map(|s| radius * (1. - (1. - tip) * s / total))
            .collect(),
        points,
        stations,
        length: total,
        pruned,
        random,
        bend: [0.; 2],
    }
}

/// Steps a centre line from `start`, turning toward the ground by `gravity` degrees in
/// all and by a random `wobble` each step. Stops where it leaves the envelope.
#[allow(clippy::too_many_arguments)]
fn centre_line(
    start: V3,
    mut direction: V3,
    length: f64,
    segments: usize,
    gravity: f64,
    wobble: f64,
    envelope: Option<&Envelope>,
    random: &mut Stream,
) -> (Vec<V3>, bool) {
    let step = length / segments as f64;
    let pull = sin((gravity / segments as f64).to_radians());
    let shake = sin(wobble.to_radians());
    let mut points = vec![start];
    for _ in 0..segments {
        let from = points[points.len() - 1];
        let to = add(from, scale(direction, step));
        if let Some(envelope) = envelope.filter(|e| !e.contains(to)) {
            // Bisect for the last point inside.
            let (mut inside, mut outside) = (0., 1.);
            for _ in 0..20 {
                let middle = 0.5 * (inside + outside);
                if envelope.contains(lerp(from, to, middle)) {
                    inside = middle;
                } else {
                    outside = middle;
                }
            }
            if inside > 1e-6 {
                points.push(lerp(from, to, inside));
            }
            return (points, true);
        }
        points.push(to);
        let nudge = reject(
            [random.signed(), random.signed(), random.signed()],
            direction,
        );
        direction = unit_or(
            add(add(direction, scale(UP, -pull)), scale(nudge, shake)),
            direction,
        );
    }
    (points, false)
}

/// Adds a branch as a tube closed by a flat end. Texture coordinates are meters: around
/// the branch at its base girth, and along it from its base.
fn tube(mesh: &mut FoliageMesh, branch: &Branch, radial: usize, ring_step: usize) {
    let last = branch.points.len() - 1;
    let mut rings: Vec<usize> = (0..last).step_by(ring_step).collect();
    rings.push(last);
    let first = mesh.positions.len() as u32;
    let girth = TAU * branch.radii[0];
    let mut side = perpendicular(branch.direction(0));
    let attributes = |mesh: &mut FoliageMesh, t: f64, shade: V3| {
        mesh.scalar(HEIGHT_RATIO, t);
        mesh.scalar(BEND_WEIGHT, branch.bend_at(t));
        mesh.scalar(LEVEL, branch.level as f64);
        mesh.scalar(ELEMENT, branch.id as f64);
        mesh.scalar(RANDOM, branch.random);
        mesh.vector(PIVOT, branch.points[0]);
        mesh.vector(SHADE_NORMAL, shade);
    };
    for point in &rings {
        let along = branch.direction(*point);
        side = unit_or(reject(side, along), perpendicular(along));
        let other = cross(along, side);
        let t = branch.stations[*point] / branch.length;
        for step in 0..=radial {
            let turn = step as f64 / radial as f64 * TAU;
            let out = add(scale(side, cos(turn)), scale(other, sin(turn)));
            mesh.positions
                .push(add(branch.points[*point], scale(out, branch.radii[*point])));
            mesh.normals.push(out);
            mesh.uvs
                .push([step as f64 / radial as f64 * girth, branch.stations[*point]]);
            attributes(mesh, t, out);
        }
    }
    let width = radial as u32 + 1;
    for ring in 0..rings.len() as u32 - 1 {
        for step in 0..radial as u32 {
            let a = first + ring * width + step;
            let (b, c, d) = (a + 1, a + width + 1, a + width);
            mesh.indices.extend([a, b, c, a, c, d]);
        }
    }
    let along = branch.direction(last);
    let centre = mesh.positions.len() as u32;
    mesh.positions.push(branch.points[last]);
    mesh.normals.push(along);
    mesh.uvs.push([0., branch.length]);
    attributes(mesh, 1., along);
    let ring = first + (rings.len() as u32 - 1) * width;
    for step in 0..radial as u32 {
        mesh.indices.extend([ring + step, ring + step + 1, centre]);
    }
}

/// One leaf cluster in its own space: cards that grow along +Y from the origin, fanned
/// in the XY plane and tipped out of it, with +Z as the face.
fn leaf_mesh(leaves: &Leaves, cards: usize, widen: f64, id: String) -> FoliageMesh {
    let geometry_key = key(
        "treespec-leaf-v1",
        &(leaves.aspect, cards, leaves.spread, leaves.shape, widen),
    );
    let mut mesh = FoliageMesh::new(id, geometry_key, "leaf", true);
    let half = 0.5 * leaves.aspect;
    let outline: &[[f64; 2]] = match leaves.shape {
        LeafShape::Quad => &[[-1., 0.], [1., 0.], [-1., 1.], [1., 1.]],
        LeafShape::Leaf => &[
            [0., 0.],
            [-1., 0.4],
            [1., 0.4],
            [-0.7, 0.75],
            [0.7, 0.75],
            [0., 1.],
        ],
    };
    let faces: &[u32] = match leaves.shape {
        LeafShape::Quad => &[0, 1, 3, 0, 3, 2],
        LeafShape::Leaf => &[0, 2, 1, 1, 2, 4, 1, 4, 3, 3, 4, 5],
    };
    for card in 0..cards {
        let fan = if cards == 1 {
            0.
        } else {
            (card as f64 / (cards - 1) as f64 - 0.5) * leaves.spread.to_radians()
        };
        let roll = if card % 2 == 0 { 28f64 } else { -28f64 }.to_radians();
        let pitch = (((card * 7) % 5) as f64 - 2.) * 14f64.to_radians();
        let place = |p: V3| {
            // Roll about the card's length, tip it out of the plane, then fan it.
            let p = [
                p[0] * cos(roll) + p[2] * sin(roll),
                p[1],
                p[2] * cos(roll) - p[0] * sin(roll),
            ];
            let p = [
                p[0],
                p[1] * cos(pitch) - p[2] * sin(pitch),
                p[2] * cos(pitch) + p[1] * sin(pitch),
            ];
            [
                p[0] * cos(fan) - p[1] * sin(fan),
                p[1] * cos(fan) + p[0] * sin(fan),
                p[2],
            ]
        };
        let first = mesh.positions.len() as u32;
        let random = unit_float(hash(0x1eaf, &[card as i64]));
        for [x, y] in outline {
            mesh.positions
                .push(place([x * half * widen, y * widen, 0.]));
            mesh.normals.push(place([0., 0., 1.]));
            mesh.uvs.push([0.5 + 0.5 * x, *y]);
            mesh.scalar(HEIGHT_RATIO, *y);
            mesh.scalar(BEND_WEIGHT, y * y);
            mesh.scalar(ELEMENT, card as f64);
            mesh.scalar(RANDOM, random);
        }
        mesh.indices.extend(faces.iter().map(|i| first + i));
    }
    mesh
}

pub fn generate(
    definition: &TreeDefinition,
    cancelled: &dyn Fn() -> bool,
) -> Result<GeneratedTree, TreeDiagnostic> {
    let source = definition.source.as_str();
    check_cancelled(source, cancelled)?;
    let skeleton = definition.skeleton(cancelled)?;
    let skeleton_key = definition.skeleton_key();
    let leaves = &definition.leaves;

    // Leaf clusters along every leaf-bearing branch, and one at its tip.
    let mut batch = InstanceBatch::new("leaves".into(), 0);
    let mut limbs = Vec::<usize>::new();
    let mut centres = Vec::<V3>::new();
    for branch in skeleton.iter().filter(|b| b.level >= leaves.level) {
        check_cancelled(source, cancelled)?;
        let mut limb = branch.id;
        while skeleton[limb].level > 1 {
            limb = skeleton[limb]
                .parent
                .expect("a branch above level 0 has a parent");
        }
        let mut distance = leaves.start * branch.length;
        let mut index = 0i64;
        loop {
            let tip = distance >= branch.length;
            let t = (distance / branch.length).min(1.);
            let mut random = Stream::new(definition.seed, &[12, branch.id as i64, index]);
            let (anchor, along, _) = branch.at(t);
            let mut direction = if tip {
                along
            } else {
                tilted(
                    along,
                    leaves.angle.to_radians(),
                    (index as f64 * 137.5).to_radians() + random.next(),
                )
            };
            direction = unit_or(add(direction, scale(UP, leaves.lift)), direction);
            let size = random.between(leaves.size);
            let face = unit_or(reject(UP, direction), perpendicular(direction));
            if batch.len() >= definition.limits.max_instances {
                return Err(fail(
                    source,
                    "/limits/maxInstances",
                    "TREE_BUDGET",
                    format!(
                        "The plant needs more than maxInstances {} leaf clusters. Widen leaves.spacing, raise leaves.level or raise the limit.",
                        definition.limits.max_instances
                    ),
                ));
            }
            batch.transforms.push([
                scale(cross(direction, face), size),
                scale(direction, size),
                scale(face, size),
                anchor,
            ]);
            batch.vector(ROOT, anchor);
            batch.vector(PIVOT, branch.points[0]);
            batch.scalar(THRESHOLD, random.next());
            batch.scalar(RANDOM, random.next());
            batch.scalar(VARIATION, 0.6 * skeleton[limb].random + 0.4 * random.next());
            batch.scalar(BEND_WEIGHT, branch.bend_at(t));
            batch.scalar(CLUSTER, limb as f64);
            batch.scalar(LEVEL, branch.level as f64);
            limbs.push(limb);
            centres.push(add(anchor, scale(direction, 0.5 * size)));
            if tip {
                break;
            }
            distance += leaves.spacing;
            index += 1;
        }
    }

    // Shade normals point out of the canopy, tipped out of each limb's own clump.
    let mut extent = EMPTY_BOUNDS;
    centres.iter().for_each(|c| grow(&mut extent, *c));
    let mut canopy = Canopy {
        center: [0.; 3],
        radii: [1.; 3],
        blobs: vec![],
    };
    if !centres.is_empty() {
        canopy.center = scale(add(extent[0], extent[1]), 0.5);
        canopy.radii = scale(sub(extent[1], extent[0]), 0.5).map(|r| r.max(1e-3));
        let mut sums = std::collections::BTreeMap::<usize, (V3, f64)>::new();
        for (limb, centre) in limbs.iter().zip(&centres) {
            let entry = sums.entry(*limb).or_insert(([0.; 3], 0.));
            entry.0 = add(entry.0, *centre);
            entry.1 += 1.;
        }
        for (limb, (sum, count)) in &sums {
            let center = scale(*sum, 1. / count);
            let radius = limbs
                .iter()
                .zip(&centres)
                .filter(|(l, _)| *l == limb)
                .map(|(_, c)| length(sub(*c, center)))
                .fold(0., f64::max);
            canopy.blobs.push(Blob {
                branch: *limb,
                center,
                radius,
            });
        }
        for (limb, centre) in limbs.iter().zip(&centres) {
            let from = sub(*centre, canopy.center);
            let outward = unit_or(
                std::array::from_fn(|a| from[a] / (canopy.radii[a] * canopy.radii[a])),
                UP,
            );
            let (sum, count) = sums[limb];
            let clump = unit_or(sub(*centre, scale(sum, 1. / count)), outward);
            batch.vector(
                SHADE_NORMAL,
                unit_or(add(scale(outward, 0.7), scale(clump, 0.3)), outward),
            );
        }
    }
    let thresholds = &batch.scalars.get(THRESHOLD).cloned().unwrap_or_default();
    let mut order: Vec<usize> = (0..batch.len()).collect();
    order.sort_by(|a, b| thresholds[*a].total_cmp(&thresholds[*b]).then(a.cmp(b)));
    batch.reorder(&order);

    let mut meshes = Vec::new();
    let mut lods = Vec::new();
    for (index, lod) in definition.lods.iter().enumerate() {
        check_cancelled(source, cancelled)?;
        let bark = |part: &str, low: usize, high: usize| {
            let mut mesh = FoliageMesh::new(
                format!("{part}/lod{index}"),
                key(
                    "treespec-bark-v1",
                    &(
                        &skeleton_key,
                        part,
                        lod.radial_segments,
                        lod.ring_step,
                        high,
                    ),
                ),
                "bark",
                false,
            );
            for branch in skeleton
                .iter()
                .filter(|b| b.level >= low && b.level <= high)
            {
                tube(&mut mesh, branch, lod.radial_segments, lod.ring_step);
            }
            mesh
        };
        let trunk = meshes.len();
        meshes.push(bark("trunk", 0, 0));
        let limb_mesh = bark("branches", 1, lod.max_level);
        let branches = (!limb_mesh.indices.is_empty()).then(|| {
            meshes.push(limb_mesh);
            meshes.len() - 1
        });
        let leaf_widen = (1. / lod.leaf_keep).sqrt().min(3.);
        let leaf = meshes.len();
        meshes.push(leaf_mesh(
            leaves,
            lod.leaf_cards,
            leaf_widen,
            format!("leaves/lod{index}"),
        ));
        let leaf_instances = thresholds_below(&batch, lod.leaf_keep);
        let parts = [Some(trunk), branches];
        let count = |measure: &dyn Fn(&FoliageMesh) -> usize| {
            parts
                .iter()
                .flatten()
                .map(|i| measure(&meshes[*i]))
                .sum::<usize>()
                + leaf_instances * measure(&meshes[leaf])
        };
        lods.push(TreeLod {
            radial_segments: lod.radial_segments,
            ring_step: lod.ring_step,
            max_level: lod.max_level,
            leaf_keep: lod.leaf_keep,
            leaf_widen,
            distance: lod.distance,
            trunk,
            branches,
            leaf,
            leaf_instances,
            triangles: count(&|mesh| mesh.triangles()),
            vertices: count(&|mesh| mesh.positions.len()),
        });
    }
    if lods[0].vertices > definition.limits.max_vertices {
        return Err(fail(
            source,
            "/limits/maxVertices",
            "TREE_BUDGET",
            format!(
                "The most detailed level needs {} vertices ({} branches, {} leaf clusters), more than maxVertices {}.",
                lods[0].vertices,
                skeleton.len(),
                batch.len(),
                definition.limits.max_vertices
            ),
        ));
    }

    let mut bounds = EMPTY_BOUNDS;
    for mesh in &meshes[..lods[0].leaf] {
        mesh.positions.iter().for_each(|p| grow(&mut bounds, *p));
    }
    let reach = meshes[lods[0].leaf]
        .positions
        .iter()
        .map(|p| length(*p))
        .fold(0., f64::max);
    for transform in &batch.transforms {
        let size = length(transform[1]) * reach;
        grow(&mut bounds, transform[3].map(|v| v - size));
        grow(&mut bounds, transform[3].map(|v| v + size));
    }

    // Where each stem meets the ground, for weathering layers that follow `base`.
    let features = skeleton
        .iter()
        .filter(|b| b.level == 0)
        .map(|stem| {
            let along = stem.direction(0);
            let side = perpendicular(along);
            let other = cross(along, side);
            Feature {
                name: "base".into(),
                part: "trunk".into(),
                face: "bark".into(),
                points: (0..16)
                    .map(|step| {
                        let turn = step as f64 / 16. * TAU;
                        add(
                            stem.points[0],
                            scale(
                                add(scale(side, cos(turn)), scale(other, sin(turn))),
                                stem.radii[0],
                            ),
                        )
                    })
                    .collect(),
                closed: true,
                normal: UP,
                width: 0.,
            }
        })
        .collect();

    let mut part_keys = vec![("skeleton".to_owned(), skeleton_key.clone())];
    part_keys.extend(
        meshes
            .iter()
            .map(|mesh| (mesh.id.clone(), mesh.geometry_key.clone())),
    );
    part_keys.push((
        "leaf-placement".into(),
        key(
            "treespec-leaf-placement-v1",
            &(
                &skeleton_key,
                definition.seed,
                leaves.level,
                leaves.spacing,
                leaves.start,
                leaves.size,
                leaves.angle,
                leaves.lift,
            ),
        ),
    ));
    check_cancelled(source, cancelled)?;
    Ok(GeneratedTree {
        id: definition.id.clone(),
        key: key("treespec-tree-v1", &(&part_keys, &definition.lods)),
        part_keys,
        skeleton,
        meshes,
        lods,
        leaves: batch,
        features,
        canopy,
        bounds,
    })
}

fn thresholds_below(batch: &InstanceBatch, keep: f64) -> usize {
    batch
        .scalars
        .get(THRESHOLD)
        .map_or(0, |t| t.partition_point(|t| *t < keep))
}
