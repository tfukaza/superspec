//! Grass patches: a few blade-clump meshes per level of detail and tiled instance batches
//! placed over a ground mesh.
use super::random::{Stream, TAU, hash, noise, perpendicular, unit_or};
use super::{
    BEND_WEIGHT, Bounds, ELEMENT, EMPTY_BOUNDS, FoliageMesh, GROUND_NORMAL, HEIGHT_RATIO,
    InstanceBatch, Limits, MAX_DEFINITION_BYTES, RANDOM, ROOT, SHADE_NORMAL, THRESHOLD,
    TreeDiagnostic, VARIATION, check_cancelled, fail, grow, header, key, range, within,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use superspec_geometry::math::{V3, add, cos, cross, scale, sin, sub, unit};

pub const MAX_GROUND_TRIANGLES: usize = 500_000;
pub const MAX_CANDIDATES: usize = 8_000_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GrassDefinition {
    pub format: String,
    pub version: u32,
    pub kind: String,
    pub id: String,
    #[serde(skip)]
    pub source: String,
    pub seed: u64,
    pub blade: Blade,
    pub clump: Clump,
    /// Clumps per square meter of ground plan where the mask is one.
    pub density: f64,
    #[serde(default)]
    pub clumping: Clumping,
    /// Size range of a placed clump.
    #[serde(default = "one_to_one")]
    pub scale: [f64; 2],
    /// 0 keeps clumps upright; 1 stands them on the ground normal.
    #[serde(default)]
    pub align: f64,
    /// Ground steeper than this, in degrees from level, grows nothing.
    #[serde(default = "default_slope")]
    pub max_slope: f64,
    /// Edge of a square culling tile, meters.
    #[serde(default = "default_tile")]
    pub tile: f64,
    pub lods: Vec<GrassLodDefinition>,
    #[serde(default)]
    pub limits: Limits,
}
fn one_to_one() -> [f64; 2] {
    [1., 1.]
}
fn default_slope() -> f64 {
    60.
}
fn default_tile() -> f64 {
    2.
}

/// One blade: a tapered ribbon that leans and curls. Ranges are `[low, high]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Blade {
    /// Length along the blade, meters.
    pub height: [f64; 2],
    /// Width at the root, meters.
    pub width: [f64; 2],
    /// Angle from vertical at the root, degrees.
    pub lean: [f64; 2],
    /// Further angle the blade turns through by its tip, degrees.
    pub curvature: [f64; 2],
    /// 0 keeps the root width up to the tip's point; 1 narrows steadily from the root.
    pub taper: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Clump {
    /// Distinct clump meshes.
    pub variants: usize,
    /// Blades per clump, `[low, high]`.
    pub blades: [usize; 2],
    /// Radius of the disc the blade roots fall in, meters.
    pub radius: f64,
    /// 1 leans every blade away from the clump's centre; 0 leans them any way.
    pub outward: f64,
}

/// Low-frequency thinning that gathers clumps into drifts.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct Clumping {
    /// Size of a drift, meters.
    pub scale: f64,
    /// 0 is even cover; 1 clears the gaps between drifts.
    pub strength: f64,
    /// Octaves of the drift noise, each half the size and half the weight of the one before: 1 gives round drifts,
    /// more give drifts with ragged, fractal edges and outliers. Left out of keys at 1, so they are as they were.
    #[serde(skip_serializing_if = "one_octave")]
    pub octaves: usize,
    /// Shrinks the drifts to where the noise is strongest (0 as before, up to 0.5): fewer, smaller patches, the rest
    /// bare, as a different grass growing in spots. Left out of keys at 0.
    #[serde(skip_serializing_if = "no_bias")]
    pub bias: f64,
}
fn no_bias(bias: &f64) -> bool {
    *bias == 0.
}
fn one_octave(octaves: &usize) -> bool {
    *octaves == 1
}
impl Default for Clumping {
    fn default() -> Self {
        Self {
            scale: 1.,
            strength: 0.,
            octaves: 1,
            bias: 0.,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GrassLodDefinition {
    /// Segments along a blade. One is a single triangle.
    pub segments: usize,
    /// Fraction of the instances drawn. Survivors are widened by `1 / sqrt(keep)`.
    pub keep: f64,
    /// Suggested distance at which the level starts, meters.
    #[serde(default)]
    pub distance: f64,
    /// Share of each clump's blades the level draws (the first of them), so a far level draws a few strokes where a
    /// near one draws a tuft. 1 keeps every blade.
    #[serde(default = "full", skip_serializing_if = "is_full")]
    pub blades: f64,
    /// A further width factor for the level's blades, beyond the thinning's widening: far levels draw fewer, fatter
    /// blades, quick strokes of a broad brush. 1 keeps the width.
    #[serde(default = "full", skip_serializing_if = "is_full")]
    pub chubby: f64,
}
fn full() -> f64 {
    1.
}
fn is_full(value: &f64) -> bool {
    *value == 1.
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroundMesh {
    pub positions: Vec<V3>,
    pub indices: Vec<u32>,
}

/// Where grass may grow, in plan. Values are densities from 0 to 1.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum DensityMask {
    #[default]
    Uniform,
    /// One inside the closed ring, zero outside.
    Polygon { points: Vec<[f64; 2]> },
    /// `columns` by `rows` samples, row by row from `origin`, `cell` meters apart, read
    /// with bilinear interpolation. Zero outside.
    Field {
        origin: [f64; 2],
        cell: f64,
        columns: usize,
        rows: usize,
        values: Vec<f64>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrassSite {
    pub ground: GroundMesh,
    #[serde(default)]
    pub mask: DensityMask,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrassLod {
    pub segments: usize,
    pub keep: f64,
    pub widen: f64,
    pub distance: f64,
    /// The mesh of each clump variant, as indices into `GrassPatch::meshes`.
    pub meshes: Vec<usize>,
    /// Instances and triangles drawn at this level over the whole patch.
    pub instances: usize,
    pub triangles: usize,
}

/// One culling tile. A batch's `mesh` is a clump variant; its instances are sorted by
/// `threshold`, so a level of detail draws the first `GrassTile::drawn` of them.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrassTile {
    pub index: [i64; 2],
    pub bounds: Bounds,
    pub batches: Vec<InstanceBatch>,
}
impl GrassTile {
    /// How many instances of `batch` a level that keeps the fraction `keep` draws.
    pub fn drawn(batch: &InstanceBatch, keep: f64) -> usize {
        batch.scalars[THRESHOLD].partition_point(|t| *t < keep)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrassStats {
    pub candidates: usize,
    pub instances: usize,
    /// Blades of the most detailed level.
    pub blades: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrassPatch {
    pub id: String,
    pub key: String,
    /// `clump-<variant>/lod<level>` for each mesh, then `placement`.
    pub part_keys: Vec<(String, String)>,
    pub meshes: Vec<FoliageMesh>,
    pub lods: Vec<GrassLod>,
    pub tiles: Vec<GrassTile>,
    pub bounds: Bounds,
    pub stats: GrassStats,
}
impl GrassPatch {
    pub fn batches(&self) -> impl Iterator<Item = &InstanceBatch> {
        self.tiles.iter().flat_map(|tile| &tile.batches)
    }
    pub fn digest(&self) -> String {
        super::digest(&self.meshes, self.batches())
    }
}

pub fn parse(source: &str, text: &str) -> Result<GrassDefinition, TreeDiagnostic> {
    if text.len() > MAX_DEFINITION_BYTES {
        return Err(fail(
            source,
            "/",
            "TREE_LIMIT",
            "Grass definition exceeds 1 MiB.",
        ));
    }
    let mut definition: GrassDefinition = serde_json::from_str(text).map_err(|error| {
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

impl GrassDefinition {
    pub fn check(&self) -> Result<(), TreeDiagnostic> {
        let s = self.source.as_str();
        header(s, &self.format, self.version, &self.kind, "grass", &self.id)?;
        range(s, "/blade/height", self.blade.height, 0.005, 5.)?;
        range(s, "/blade/width", self.blade.width, 0.0005, 0.5)?;
        range(s, "/blade/lean", self.blade.lean, 0., 85.)?;
        range(s, "/blade/curvature", self.blade.curvature, 0., 150.)?;
        within(s, "/blade/taper", self.blade.taper, 0., 1.)?;
        if self.clump.variants == 0 || self.clump.variants > 32 {
            return Err(fail(
                s,
                "/clump/variants",
                "TREE_VALUE",
                "Expected 1 to 32 clump variants.",
            ));
        }
        let [low, high] = self.clump.blades;
        if low == 0 || high < low || high > 64 {
            return Err(fail(
                s,
                "/clump/blades",
                "TREE_VALUE",
                "Expected an ordered range of 1 to 64 blades.",
            ));
        }
        within(s, "/clump/radius", self.clump.radius, 0., 2.)?;
        within(s, "/clump/outward", self.clump.outward, 0., 1.)?;
        within(s, "/density", self.density, 0.01, 10_000.)?;
        within(s, "/clumping/scale", self.clumping.scale, 0.01, 1000.)?;
        within(s, "/clumping/strength", self.clumping.strength, 0., 1.)?;
        within(s, "/clumping/bias", self.clumping.bias, 0., 0.5)?;
        if self.clumping.octaves == 0 || self.clumping.octaves > 6 {
            return Err(fail(
                s,
                "/clumping/octaves",
                "TREE_VALUE",
                "Expected 1 to 6 octaves.",
            ));
        }
        range(s, "/scale", self.scale, 0.05, 20.)?;
        within(s, "/align", self.align, 0., 1.)?;
        within(s, "/maxSlope", self.max_slope, 0., 89.)?;
        within(s, "/tile", self.tile, 0.1, 1000.)?;
        if self.lods.is_empty() || self.lods.len() > 8 {
            return Err(fail(
                s,
                "/lods",
                "TREE_VALUE",
                "Expected 1 to 8 levels of detail.",
            ));
        }
        for (index, lod) in self.lods.iter().enumerate() {
            let pointer = format!("/lods/{index}");
            if lod.segments == 0 || lod.segments > 32 {
                return Err(fail(
                    s,
                    &format!("{pointer}/segments"),
                    "TREE_VALUE",
                    "Expected 1 to 32 segments.",
                ));
            }
            within(s, &format!("{pointer}/keep"), lod.keep, 0.01, 1.)?;
            within(s, &format!("{pointer}/distance"), lod.distance, 0., 1e6)?;
            within(s, &format!("{pointer}/blades"), lod.blades, 0.01, 1.)?;
            within(s, &format!("{pointer}/chubby"), lod.chubby, 0.1, 10.)?;
            if index > 0 {
                let before = self.lods[index - 1];
                if lod.segments > before.segments
                    || lod.keep > before.keep
                    || lod.distance < before.distance
                {
                    return Err(fail(
                        s,
                        &pointer,
                        "TREE_VALUE",
                        "Each level must have no more segments, keep no more instances and start no nearer than the one before.",
                    ));
                }
            }
        }
        self.limits.check(s)
    }

    /// The clump meshes of every level, which do not depend on the site.
    pub fn clump_meshes(&self) -> Result<(Vec<FoliageMesh>, Vec<GrassLod>), TreeDiagnostic> {
        self.check()?;
        let mut meshes = Vec::new();
        let mut lods = Vec::new();
        for (level, lod) in self.lods.iter().enumerate() {
            // Thinned levels widen their survivors to hold the cover.
            let widen = (1. / lod.keep).sqrt().min(3.) * lod.chubby;
            let mut indices = Vec::new();
            for variant in 0..self.clump.variants {
                // The key of a full level is as it was; a level that drops blades keys its share too.
                let geometry_key = if lod.blades == 1. {
                    key(
                        "treespec-grass-clump-v1",
                        &(
                            self.seed,
                            &self.blade,
                            &self.clump,
                            variant,
                            lod.segments,
                            widen,
                        ),
                    )
                } else {
                    key(
                        "treespec-grass-clump-v2",
                        &(
                            self.seed,
                            &self.blade,
                            &self.clump,
                            variant,
                            lod.segments,
                            widen,
                            lod.blades,
                        ),
                    )
                };
                let mut mesh = FoliageMesh::new(
                    format!("clump-{variant}/lod{level}"),
                    geometry_key,
                    "blade",
                    true,
                );
                let blades = self.blades(variant);
                let kept = ((blades.len() as f64 * lod.blades).ceil() as usize).max(1);
                for blade in blades.into_iter().take(kept) {
                    blade.draw(&mut mesh, lod.segments, widen, self.blade.taper);
                }
                indices.push(meshes.len());
                meshes.push(mesh);
            }
            lods.push(GrassLod {
                segments: lod.segments,
                keep: lod.keep,
                widen,
                distance: lod.distance,
                meshes: indices,
                instances: 0,
                triangles: 0,
            });
        }
        let vertices: usize = lods[0]
            .meshes
            .iter()
            .map(|i| meshes[*i].positions.len())
            .sum();
        if vertices > self.limits.max_vertices {
            return Err(fail(
                &self.source,
                "/limits/maxVertices",
                "TREE_BUDGET",
                format!(
                    "The clump meshes need {vertices} vertices, more than maxVertices {}. Use fewer variants, blades or segments.",
                    self.limits.max_vertices
                ),
            ));
        }
        Ok((meshes, lods))
    }

    fn blades(&self, variant: usize) -> Vec<BladeShape> {
        let mut random = Stream::new(self.seed, &[1, variant as i64]);
        let [low, high] = self.clump.blades;
        let count = low + (random.next() * (high - low + 1) as f64) as usize;
        (0..count.min(high))
            .map(|index| {
                let mut random = Stream::new(self.seed, &[2, variant as i64, index as i64]);
                let around = random.next() * TAU;
                let reach = random.next().sqrt() * self.clump.radius;
                let root = [reach * cos(around), reach * sin(around), 0.];
                // A blade at the very centre has no outward direction of its own.
                let turn = around + (1. - self.clump.outward) * random.signed() * TAU / 2.;
                let lean = random.between(self.blade.lean).to_radians();
                let curvature = random
                    .between(self.blade.curvature)
                    .to_radians()
                    .min(170f64.to_radians() - lean);
                BladeShape {
                    index,
                    root,
                    direction: [cos(turn), sin(turn), 0.],
                    length: random.between(self.blade.height),
                    width: random.between(self.blade.width),
                    lean,
                    curvature,
                    random: random.next(),
                    radius: self.clump.radius,
                }
            })
            .collect()
    }
}

struct BladeShape {
    index: usize,
    root: V3,
    /// Horizontal unit direction the blade leans toward.
    direction: V3,
    length: f64,
    width: f64,
    lean: f64,
    curvature: f64,
    random: f64,
    radius: f64,
}
impl BladeShape {
    /// The centre line at `t` from root (0) to tip (1), and the angle from vertical there.
    /// The angle grows linearly, so the line is a circular arc of length `length`.
    fn centre(&self, t: f64) -> (V3, f64) {
        let angle = self.lean + self.curvature * t;
        let (along, up) = if self.curvature.abs() < 1e-9 {
            (t * sin(self.lean), t * cos(self.lean))
        } else {
            (
                (cos(self.lean) - cos(angle)) / self.curvature,
                (sin(angle) - sin(self.lean)) / self.curvature,
            )
        };
        (
            add(
                self.root,
                add(
                    scale(self.direction, along * self.length),
                    [0., 0., up * self.length],
                ),
            ),
            angle,
        )
    }
    fn draw(&self, mesh: &mut FoliageMesh, segments: usize, widen: f64, taper: f64) {
        let first = mesh.positions.len() as u32;
        let across = cross([0., 0., 1.], self.direction);
        // Out of the clump: up, tipped away from the centre.
        let shade = unit_or(
            add([0., 0., 1.], scale(self.root, 0.6 / self.radius.max(1e-6))),
            [0., 0., 1.],
        );
        let vertex = |mesh: &mut FoliageMesh, t: f64, side: f64, half: f64| {
            let (centre, angle) = self.centre(t);
            mesh.positions.push(add(centre, scale(across, side * half)));
            // The upper face of the ribbon.
            mesh.normals.push(add(
                scale(self.direction, -cos(angle)),
                [0., 0., sin(angle)],
            ));
            mesh.uvs.push([0.5 + 0.5 * side, t]);
            mesh.scalar(HEIGHT_RATIO, t);
            mesh.scalar(BEND_WEIGHT, t * t);
            mesh.scalar(RANDOM, self.random);
            mesh.scalar(ELEMENT, self.index as f64);
            mesh.vector(PIVOT_ROOT, self.root);
            mesh.vector(SHADE_NORMAL, shade);
        };
        for row in 0..segments {
            let t = row as f64 / segments as f64;
            let half = 0.5 * self.width * widen * (1. - taper * t);
            vertex(mesh, t, -1., half);
            vertex(mesh, t, 1., half);
        }
        vertex(mesh, 1., 0., 0.);
        for row in 0..segments as u32 - 1 {
            let (left, right) = (first + row * 2, first + row * 2 + 1);
            mesh.indices
                .extend([right, left, left + 2, right, left + 2, right + 2]);
        }
        let last = first + (segments as u32 - 1) * 2;
        mesh.indices.extend([last + 1, last, last + 2]);
    }
}
/// The blade's root in clump space, on every vertex of the blade.
const PIVOT_ROOT: &str = super::PIVOT;

struct Hit {
    height: f64,
    normal: V3,
}

/// Ground triangles bucketed in plan, for finding the surface above a point.
struct Ground<'a> {
    mesh: &'a GroundMesh,
    low: [f64; 2],
    cell: f64,
    columns: usize,
    rows: usize,
    buckets: Vec<Vec<u32>>,
    steepest: f64,
}
impl<'a> Ground<'a> {
    fn new(
        mesh: &'a GroundMesh,
        max_slope: f64,
        source: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, TreeDiagnostic> {
        let triangles = mesh.indices.len() / 3;
        if triangles == 0
            || triangles > MAX_GROUND_TRIANGLES
            || !mesh.indices.len().is_multiple_of(3)
            || mesh
                .indices
                .iter()
                .any(|i| *i as usize >= mesh.positions.len())
            || mesh
                .positions
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || v.abs() > 1e7)
        {
            return Err(fail(
                source,
                "/ground",
                "TREE_GROUND",
                format!(
                    "The ground needs 1 to {MAX_GROUND_TRIANGLES} triangles with valid indices and finite positions."
                ),
            ));
        }
        let mut bounds = EMPTY_BOUNDS;
        mesh.positions.iter().for_each(|p| grow(&mut bounds, *p));
        let extent = (bounds[1][0] - bounds[0][0]).max(bounds[1][1] - bounds[0][1]);
        let side = ((triangles as f64).sqrt().ceil() as usize).clamp(1, 512);
        let cell = (extent / side as f64).max(1e-6);
        let columns = (((bounds[1][0] - bounds[0][0]) / cell) as usize + 1).min(side + 1);
        let rows = (((bounds[1][1] - bounds[0][1]) / cell) as usize + 1).min(side + 1);
        let mut ground = Self {
            mesh,
            low: [bounds[0][0], bounds[0][1]],
            cell,
            columns,
            rows,
            buckets: vec![Vec::new(); columns * rows],
            steepest: cos(max_slope.to_radians()),
        };
        for triangle in 0..triangles {
            if triangle.is_multiple_of(4096) {
                check_cancelled(source, cancelled)?;
            }
            let [a, b, c] = ground.corners(triangle);
            let low = ground.cell_of([a[0].min(b[0]).min(c[0]), a[1].min(b[1]).min(c[1])]);
            let high = ground.cell_of([a[0].max(b[0]).max(c[0]), a[1].max(b[1]).max(c[1])]);
            for row in low[1]..=high[1] {
                for column in low[0]..=high[0] {
                    ground.buckets[row * ground.columns + column].push(triangle as u32);
                }
            }
        }
        Ok(ground)
    }
    fn corners(&self, triangle: usize) -> [V3; 3] {
        std::array::from_fn(|corner| {
            self.mesh.positions[self.mesh.indices[triangle * 3 + corner] as usize]
        })
    }
    fn cell_of(&self, point: [f64; 2]) -> [usize; 2] {
        let at = |value: f64, low: f64, count: usize| {
            (((value - low) / self.cell).max(0.) as usize).min(count - 1)
        };
        [
            at(point[0], self.low[0], self.columns),
            at(point[1], self.low[1], self.rows),
        ]
    }
    /// The highest upward-facing surface over a plan point, unless it is too steep.
    fn above(&self, x: f64, y: f64) -> Option<Hit> {
        let [column, row] = self.cell_of([x, y]);
        let mut best: Option<Hit> = None;
        for triangle in &self.buckets[row * self.columns + column] {
            let [a, b, c] = self.corners(*triangle as usize);
            let turn = |p: V3, q: V3, r: [f64; 2]| {
                (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])
            };
            let area = turn(a, b, [c[0], c[1]]);
            // An upward-facing triangle turns counter-clockwise in plan.
            if area <= 1e-18 {
                continue;
            }
            let (u, v) = (turn(b, c, [x, y]) / area, turn(c, a, [x, y]) / area);
            let w = 1. - u - v;
            if u < -1e-12 || v < -1e-12 || w < -1e-12 {
                continue;
            }
            let height = u * a[2] + v * b[2] + w * c[2];
            if best.as_ref().is_some_and(|hit| hit.height >= height) {
                continue;
            }
            let Ok(normal) = unit(cross(sub(b, a), sub(c, a))) else {
                continue;
            };
            best = Some(Hit { height, normal });
        }
        best.filter(|hit| hit.normal[2] >= self.steepest)
    }
}

impl DensityMask {
    fn check(&self, source: &str) -> Result<(), TreeDiagnostic> {
        let valid = match self {
            Self::Uniform => true,
            Self::Polygon { points } => {
                (3..=4096).contains(&points.len()) && points.iter().flatten().all(|v| v.is_finite())
            }
            Self::Field {
                origin,
                cell,
                columns,
                rows,
                values,
            } => {
                origin.iter().all(|v| v.is_finite())
                    && cell.is_finite()
                    && *cell > 0.
                    && *columns >= 2
                    && *rows >= 2
                    && columns.checked_mul(*rows) == Some(values.len())
                    && values.len() <= 4_194_304
                    && values.iter().all(|v| (0. ..=1.).contains(v))
            }
        };
        if valid {
            Ok(())
        } else {
            Err(fail(
                source,
                "/mask",
                "TREE_MASK",
                "A polygon mask needs 3 to 4096 finite points; a field mask needs at least 2 by 2 values from 0 to 1 and a positive cell.",
            ))
        }
    }
    /// The plan bounds outside which the mask is zero.
    fn extent(&self) -> Option<[[f64; 2]; 2]> {
        match self {
            Self::Uniform => None,
            Self::Polygon { points } => Some(points.iter().fold(
                [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]],
                |[low, high], p| {
                    [
                        [low[0].min(p[0]), low[1].min(p[1])],
                        [high[0].max(p[0]), high[1].max(p[1])],
                    ]
                },
            )),
            Self::Field {
                origin,
                cell,
                columns,
                rows,
                ..
            } => Some([
                *origin,
                [
                    origin[0] + cell * (*columns - 1) as f64,
                    origin[1] + cell * (*rows - 1) as f64,
                ],
            ]),
        }
    }
    pub fn density(&self, x: f64, y: f64) -> f64 {
        match self {
            Self::Uniform => 1.,
            Self::Polygon { points } => {
                // Even-odd crossing count of a ray toward +x.
                let mut within = false;
                for (a, b) in points.iter().zip(points.iter().cycle().skip(1)) {
                    if (a[1] > y) != (b[1] > y)
                        && x < (b[0] - a[0]) * (y - a[1]) / (b[1] - a[1]) + a[0]
                    {
                        within = !within;
                    }
                }
                if within { 1. } else { 0. }
            }
            Self::Field {
                origin,
                cell,
                columns,
                rows,
                values,
            } => {
                let (u, v) = ((x - origin[0]) / cell, (y - origin[1]) / cell);
                if u < 0. || v < 0. || u > (*columns - 1) as f64 || v > (*rows - 1) as f64 {
                    return 0.;
                }
                let (column, row) = ((u as usize).min(columns - 2), (v as usize).min(rows - 2));
                let (fu, fv) = (u - column as f64, v - row as f64);
                let at = |c: usize, r: usize| values[r * columns + c];
                let low = at(column, row) + (at(column + 1, row) - at(column, row)) * fu;
                let high =
                    at(column, row + 1) + (at(column + 1, row + 1) - at(column, row + 1)) * fu;
                low + (high - low) * fv
            }
        }
    }
}

fn site_key(site: &GrassSite) -> String {
    let mut hash = Sha256::new();
    for value in site.ground.positions.iter().flatten() {
        hash.update(value.to_bits().to_le_bytes());
    }
    for index in &site.ground.indices {
        hash.update(index.to_le_bytes());
    }
    hash.update(serde_json::to_vec(&site.mask).expect("mask serializes"));
    format!("{:x}", hash.finalize())
}

/// Places the definition's clumps over the site. Candidates lie on a jittered plan grid
/// fixed in world space, so a changed mask or ground keeps every surviving clump as it was.
pub fn generate(
    definition: &GrassDefinition,
    site: &GrassSite,
    cancelled: &dyn Fn() -> bool,
) -> Result<GrassPatch, TreeDiagnostic> {
    let source = definition.source.as_str();
    check_cancelled(source, cancelled)?;
    let (meshes, mut lods) = definition.clump_meshes()?;
    site.mask.check(source)?;
    let ground = Ground::new(&site.ground, definition.max_slope, source, cancelled)?;

    let mut low = ground.low;
    let mut high = [
        low[0] + ground.cell * ground.columns as f64,
        low[1] + ground.cell * ground.rows as f64,
    ];
    if let Some(extent) = site.mask.extent() {
        for axis in 0..2 {
            low[axis] = low[axis].max(extent[0][axis]);
            high[axis] = high[axis].min(extent[1][axis]);
        }
    }
    let spacing = 1. / definition.density.sqrt();
    let cells = |axis: usize| {
        if high[axis] < low[axis] {
            return 0..0;
        }
        (low[axis] / spacing).floor() as i64..(high[axis] / spacing).floor() as i64 + 1
    };
    let (columns, rows) = (cells(0), cells(1));
    let candidates = (columns.end - columns.start) as usize * (rows.end - rows.start) as usize;
    if candidates > MAX_CANDIDATES {
        return Err(fail(
            source,
            "/density",
            "TREE_BUDGET",
            format!(
                "The site holds {candidates} candidate positions, more than {MAX_CANDIDATES}. Lower the density or split the site."
            ),
        ));
    }

    // The reach of a clump from its root, for tile bounds.
    let reach = definition.scale[1]
        * meshes
            .iter()
            .flat_map(|mesh| &mesh.positions)
            .map(|p| (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt())
            .fold(0., f64::max);
    let variants = definition.clump.variants;
    let mut tiles = BTreeMap::<[i64; 2], Vec<InstanceBatch>>::new();
    let mut instances = 0usize;
    let mut seen = 0usize;
    for row in rows.clone() {
        for column in columns.clone() {
            if seen.is_multiple_of(4096) {
                check_cancelled(source, cancelled)?;
            }
            seen += 1;
            let mut random = Stream::new(definition.seed, &[3, column, row]);
            let x = (column as f64 + random.next()) * spacing;
            let y = (row as f64 + random.next()) * spacing;
            let threshold = random.next();
            // One octave as before; more add finer noise for ragged drifts, normalized to the same range.
            let drift = if definition.clumping.octaves <= 1 {
                noise(
                    hash(definition.seed, &[4]),
                    x / definition.clumping.scale,
                    y / definition.clumping.scale,
                )
            } else {
                let (mut sum, mut weight, mut total, mut size) =
                    (0., 1., 0., definition.clumping.scale);
                for octave in 0..definition.clumping.octaves {
                    sum += weight
                        * noise(
                            hash(definition.seed, &[4, octave as i64]),
                            x / size,
                            y / size,
                        );
                    total += weight;
                    weight *= 0.5;
                    size *= 0.5;
                }
                // Averaging octaves narrows the spread about the middle; stretched well past it, so drifts part into clear patches.
                (0.5 + (sum / total - 0.5) * 5.).clamp(0., 1.)
            };
            let drift = ((drift - 0.3 - definition.clumping.bias) / 0.4).clamp(0., 1.);
            let density =
                site.mask.density(x, y) * (1. - definition.clumping.strength * (1. - drift));
            if threshold >= density {
                continue;
            }
            let Some(hit) = ground.above(x, y) else {
                continue;
            };
            instances += 1;
            if instances > definition.limits.max_instances {
                return Err(fail(
                    source,
                    "/limits/maxInstances",
                    "TREE_BUDGET",
                    format!(
                        "The patch needs more than maxInstances {} clumps ({candidates} candidates at {} per square meter). Lower the density, shrink the mask or raise the limit.",
                        definition.limits.max_instances, definition.density
                    ),
                ));
            }
            let variant = ((random.next() * variants as f64) as usize).min(variants - 1);
            let turn = random.next() * TAU;
            let size = random.between(definition.scale);
            let up = unit_or(
                add(
                    [0., 0., 1. - definition.align],
                    scale(hit.normal, definition.align),
                ),
                [0., 0., 1.],
            );
            let side = perpendicular(up);
            let other = cross(up, side);
            let first = add(scale(side, cos(turn)), scale(other, sin(turn)));
            let second = cross(up, first);
            let root = [x, y, hit.height];
            let tile = [
                (x / definition.tile).floor() as i64,
                (y / definition.tile).floor() as i64,
            ];
            let batches = tiles.entry(tile).or_insert_with(|| {
                (0..variants)
                    .map(|v| {
                        InstanceBatch::new(format!("tile/{}/{}/clump-{v}", tile[0], tile[1]), v)
                    })
                    .collect()
            });
            let batch = &mut batches[variant];
            batch.transforms.push([
                scale(first, size),
                scale(second, size),
                scale(up, size),
                root,
            ]);
            batch.vector(ROOT, root);
            batch.vector(GROUND_NORMAL, hit.normal);
            // The share of the local density at which this clump appears: thinning to a
            // fraction keeps the clumps below it, everywhere in proportion.
            batch.scalar(THRESHOLD, threshold / density);
            batch.scalar(RANDOM, random.next());
            let patch = noise(hash(definition.seed, &[5]), x / 1.7, y / 1.7);
            batch.scalar(VARIATION, 0.7 * patch + 0.3 * random.next());
        }
    }
    check_cancelled(source, cancelled)?;

    let mut bounds = EMPTY_BOUNDS;
    let mut stats = GrassStats {
        candidates,
        instances,
        blades: 0,
    };
    let mut result = Vec::new();
    for (index, mut batches) in tiles {
        let mut tile_bounds = EMPTY_BOUNDS;
        batches.retain(|batch| !batch.is_empty());
        for batch in &mut batches {
            let thresholds = &batch.scalars[THRESHOLD];
            let mut order: Vec<usize> = (0..batch.len()).collect();
            order.sort_by(|a, b| thresholds[*a].total_cmp(&thresholds[*b]).then(a.cmp(b)));
            batch.reorder(&order);
            for root in &batch.vectors[ROOT] {
                grow(&mut tile_bounds, root.map(|v| v - reach));
                grow(&mut tile_bounds, root.map(|v| v + reach));
            }
            for lod in &mut lods {
                let drawn = GrassTile::drawn(batch, lod.keep);
                lod.instances += drawn;
                lod.triangles += drawn * meshes[lod.meshes[batch.mesh]].triangles();
            }
            let blades = meshes[lods[0].meshes[batch.mesh]].scalars[ELEMENT]
                .last()
                .map_or(0, |last| *last as usize + 1);
            stats.blades += blades * GrassTile::drawn(batch, lods[0].keep);
        }
        grow(&mut bounds, tile_bounds[0]);
        grow(&mut bounds, tile_bounds[1]);
        result.push(GrassTile {
            index,
            bounds: tile_bounds,
            batches,
        });
    }
    let mut part_keys: Vec<(String, String)> = meshes
        .iter()
        .map(|mesh| (mesh.id.clone(), mesh.geometry_key.clone()))
        .collect();
    part_keys.push((
        "placement".into(),
        key(
            "treespec-grass-placement-v1",
            &(
                definition.seed,
                definition.density,
                definition.clumping,
                definition.scale,
                definition.align,
                definition.max_slope,
                definition.tile,
                variants,
                site_key(site),
            ),
        ),
    ));
    Ok(GrassPatch {
        id: definition.id.clone(),
        key: key("treespec-grass-v1", &(&part_keys, &definition.lods)),
        part_keys,
        meshes,
        lods,
        tiles: result,
        bounds,
        stats,
    })
}
