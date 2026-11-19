//! Portable values of an instanced batch: one local mesh drawn many times, each time with its own
//! placement and, optionally, a small fixed set of per-instance attributes. The delivered record
//! (ranges into a scene's batch buffers, bounds) is `superspec_model::batches::Batch`.
//!
//! Meters, right-handed Z-up. A placement is translation, rotation and scale (applied scale first,
//! then rotation, then translation), not a matrix: it is what `EXT_mesh_gpu_instancing` stores, so
//! an export writes the authored numbers and never decomposes anything. Shear and mirroring are
//! not representable; a producer that needs them delivers ordinary meshes.

use serde::{Deserialize, Serialize};

/// Batches in one scene.
pub const MAX_BATCHES: usize = 4096;
/// Instances of all batches of one scene together.
pub const MAX_INSTANCES: usize = 1_000_000;
/// Vertices of all batch meshes of one scene together (each mesh is stored once).
pub const MAX_BATCH_VERTICES: usize = 1_500_000;
/// Indices of all batch meshes of one scene together.
pub const MAX_BATCH_INDICES: usize = 9_000_000;
/// Numbers per delivered placement: translation (3), rotation quaternion `x y z w` (4), scale (3).
pub const PLACEMENT_FLOATS: usize = 10;

/// Where one instance of a batch's mesh stands.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    pub translation: [f64; 3],
    /// Unit quaternion `x y z w`.
    #[serde(default = "no_rotation")]
    pub rotation: [f64; 4],
    /// Positive factors along the mesh's own axes.
    #[serde(default = "unit_scale")]
    pub scale: [f64; 3],
}

fn no_rotation() -> [f64; 4] {
    [0., 0., 0., 1.]
}
fn unit_scale() -> [f64; 3] {
    [1.; 3]
}

impl Default for Placement {
    fn default() -> Self {
        Self {
            translation: [0.; 3],
            rotation: no_rotation(),
            scale: unit_scale(),
        }
    }
}

impl Placement {
    /// Finite, within the supported coordinate range, a unit rotation and positive scale.
    pub fn validate(&self) -> Result<(), String> {
        if self
            .translation
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 1e9)
        {
            return Err("Instance translation leaves the supported coordinate range.".into());
        }
        let length = self.rotation.iter().map(|v| v * v).sum::<f64>().sqrt();
        if !length.is_finite() || (length - 1.).abs() > 1e-4 {
            return Err("Instance rotation must be a unit quaternion (x, y, z, w).".into());
        }
        if self
            .scale
            .iter()
            .any(|v| !v.is_finite() || *v < 1e-9 || *v > 1e9)
        {
            return Err("Instance scale must be positive and finite on every axis.".into());
        }
        Ok(())
    }

    /// The delivered form: `PLACEMENT_FLOATS` single-precision numbers, the rotation normalized.
    pub fn delivered(&self) -> [f32; PLACEMENT_FLOATS] {
        let length = self.rotation.iter().map(|v| v * v).sum::<f64>().sqrt();
        let r = self.rotation.map(|v| (v / length) as f32);
        let (t, s) = (
            self.translation.map(|v| v as f32),
            self.scale.map(|v| v as f32),
        );
        [t[0], t[1], t[2], r[0], r[1], r[2], r[3], s[0], s[1], s[2]]
    }
}

/// The rotation columns of a delivered placement, each scaled by its axis: the upper 3x3 of the
/// instance matrix, column-major. Only products and sums, so native and Wasm agree bit for bit.
pub fn basis(placement: &[f32]) -> [[f64; 3]; 3] {
    let [x, y, z, w] = [3, 4, 5, 6].map(|i| f64::from(placement[i]));
    let s = [7, 8, 9].map(|i| f64::from(placement[i]));
    [
        [
            (1. - 2. * (y * y + z * z)) * s[0],
            (2. * (x * y + z * w)) * s[0],
            (2. * (x * z - y * w)) * s[0],
        ],
        [
            (2. * (x * y - z * w)) * s[1],
            (1. - 2. * (x * x + z * z)) * s[1],
            (2. * (y * z + x * w)) * s[1],
        ],
        [
            (2. * (x * z + y * w)) * s[2],
            (2. * (y * z - x * w)) * s[2],
            (1. - 2. * (x * x + y * y)) * s[2],
        ],
    ]
}

/// A point of the batch's mesh as one delivered placement puts it in the scene.
pub fn place(placement: &[f32], point: [f64; 3]) -> [f64; 3] {
    let m = basis(placement);
    std::array::from_fn(|axis| {
        m[0][axis] * point[0]
            + m[1][axis] * point[1]
            + m[2][axis] * point[2]
            + f64::from(placement[axis])
    })
}

/// What a batch is to collision. Render visibility is the batch's `visible`; the two are
/// independent, as for meshes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Collision {
    /// Render only: nothing collides with the instances (the default).
    #[default]
    None,
    /// Every instance collides as its mesh's triangles.
    Instances,
}

/// How a renderer draws a batch of foliage (roadmap phase 4): one of a small fixed set of vertex
/// and pixel shader families, chosen and tuned by parameters, never by shader code. The batch's
/// own mesh, placements and named attributes are what the families read (TreeSpec's `pivot`,
/// `bendWeight`, `heightRatio` and `random` per vertex; `root`, `groundNormal`, `threshold`,
/// `variation` and `tile` per instance); a batch without them still draws, with neutral values.
///
/// Every level of detail of one field of foliage is its own batch with the same `patch`: a
/// renderer draws, per tile, the level whose `distance` the tile has reached, thins instances by
/// their stable `threshold` toward the next level's `keep`, and draws nothing past `far`.
///
/// `material` names a material of the producing project (not a delivered scene material) whose
/// painted program colors the instances, evaluated once per vertex at the blade's root. Shading is
/// the renderer's business: a renderer without the program shows the batch's own colors.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Foliage {
    pub family: FoliageFamily,
    /// The field of foliage this batch is one level of: 1 to 256 characters.
    pub patch: String,
    /// The level of detail, 0 the most detailed.
    #[serde(default)]
    pub level: u32,
    /// Meters from the camera at which a tile starts drawing this level.
    #[serde(default)]
    pub distance: f64,
    /// The share of the patch's instances this level holds, in (0, 1]: its instances are those
    /// whose `threshold` is below it.
    #[serde(default = "one")]
    pub keep: f64,
    /// Meters from the camera past which nothing of the patch is drawn.
    pub far: f64,
    /// A material of the producing project whose painted program colors the instances.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    #[serde(default)]
    pub profile: FoliageProfile,
    #[serde(default)]
    pub bend: FoliageBend,
    #[serde(default)]
    pub wind: FoliageWind,
    #[serde(default)]
    pub rejection: FoliageRejection,
    #[serde(default)]
    pub tip: FoliageTip,
    /// The parameters of the plant families (`bark`, `leaves`, `hull`, `shadow`); absent for grass,
    /// and read with its defaults where a plant batch has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canopy: Option<FoliageCanopy>,
    /// The plant painted as one mass (TreeSpec's painted trees): a renderer draws it in place of
    /// the plant's `leaves` and `hull` batches, with this batch and at every level that batch
    /// draws. A producer puts it on the plant's `shadow` batch, which draws at every level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mass: Option<FoliageMass>,
}

/// A plant's foliage painted as one mass (roadmap milestone `painted-trees-milestone`): a leaf
/// density field made from its leaf clusters, cut at `level` into a painted core with leafy fuzz
/// on its outline, and painted billboards (round cards and sprays rooted on the twigs) through
/// the thinner foliage around it, all lit by the plant's broad form. Points are in the space of
/// the batch's mesh, placed by each of its placements. The look's remaining settings are the
/// milestone's and belong to the renderer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct FoliageMass {
    /// Every leaf cluster as `[x, y, z, size]`: its middle and its length, meters.
    pub sites: Vec<[f64; 4]>,
    /// Every leafy twig as points from its base toward its tip: the sprays grow along them.
    pub twigs: Vec<Vec<[f64; 3]>>,
    /// Meters of each cluster's kernel in the density field, 0.05 to 5.
    pub radius: f64,
    /// Meters of the field's cells, 0.02 to 2.
    pub cell: f64,
    /// The painted core's level, a share of the field's dense value, above 0 and at most 2.
    pub level: f64,
    /// Share of the card candidates kept at the core's level, 0 to 1.
    pub card_keep: f64,
    /// How the painted colours are graded for the scene the plant stands in (the default keeps
    /// the milestone's own).
    pub palette: FoliagePalette,
}
impl Default for FoliageMass {
    fn default() -> Self {
        Self {
            sites: vec![],
            twigs: vec![],
            radius: 0.5,
            cell: 0.2,
            level: 0.6,
            card_keep: 0.9,
            palette: FoliagePalette::default(),
        }
    }
}

/// A grade of a painted mass's colours, applied to every painted colour in this order: `hue`
/// turns it about the grey axis (degrees, -180 to 180), `saturation` scales its distance from
/// its grey (0 to 2), `value` scales it (0 to 2). `shade` (0 to 2) scales the darker colours
/// alone, fading out toward the light ones, and `shadeHue` (degrees) turns them alone, so the
/// shade side can go deeper and toward another hue than the light. Before the grade, `contrast`
/// (0 to 3) scales how far the light is stretched across the painted bands, and `depth` (0 to 3)
/// how much the plant's shade side and its inside darken, both as multiples of the renderer's own
/// (the milestone's).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct FoliagePalette {
    pub hue: f64,
    pub saturation: f64,
    pub value: f64,
    pub shade: f64,
    pub shade_hue: f64,
    pub contrast: f64,
    pub depth: f64,
}
impl Default for FoliagePalette {
    fn default() -> Self {
        Self {
            hue: 0.,
            saturation: 1.,
            value: 1.,
            shade: 1.,
            shade_hue: 0.,
            contrast: 1.,
            depth: 1.,
        }
    }
}
/// Leaf clusters of one painted mass.
pub const MAX_MASS_SITES: usize = 100_000;
/// Twigs of one painted mass, and points of one twig.
pub const MAX_MASS_TWIGS: usize = 20_000;
pub const MAX_TWIG_POINTS: usize = 64;
/// Cells of a painted mass's density field.
pub const MAX_MASS_CELLS: f64 = 8e6;

fn one() -> f64 {
    1.
}

/// The shader family (roadmap phases 4 and 5). Every level of one plant is a `patch` of its own:
///
/// - `grass`: ribbons that lean and sway about their roots;
/// - `bark`: a plant's trunk and branches, swaying about the plant's base by their `bendWeight`;
/// - `leaves`: leaf clusters drawn as camera-facing paint dabs with a cutout edge, swaying with
///   their branch and fluttering by the card's own `bendWeight`;
/// - `hull`: the metaball canopy hull, a shell behind thinned leaves or the far level in their
///   place, its silhouette broken into dabs;
/// - `shadow`: the canopy's shadow proxy, drawn only into the sun's shadow map.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FoliageFamily {
    Grass,
    Bark,
    Leaves,
    Hull,
    Shadow,
}

/// The parameters of a plant's families: how leaves face the camera and are cut out, which are
/// highlight leaves, and what the lit profile adds to the light loop. Wind reads `Foliage::wind`:
/// `angle` is the plant's sway at the tips (by `bendWeight`) and `flutter` each leaf card's own.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct FoliageCanopy {
    /// 0 to 1: how far leaf clusters turn from their grown orientation to face the camera.
    pub facing: f64,
    /// Size of a dab as a multiple of its cluster, 0.25 to 4.
    pub dab: f64,
    /// Cutout threshold of a dab's coverage, 0 to 1 (as MaterialSpec's `alphaCutout`).
    pub cutout: f64,
    /// Share of leaf clusters drawn as highlight leaves where the band is lit, 0 to 1.
    pub highlights: f64,
    /// Linear RGB of highlight leaves, 0 to 1.
    pub highlight: [f64; 3],
    /// Lit profile: light through the leaves from behind, 0 to 1.
    pub translucency: f64,
    /// Lit profile: share of leaf clusters with a glossy glint, 0 to 1.
    pub glint: f64,
    /// Lit profile: two-sided diffuse, how far light wraps past the terminator, 0 to 1.
    pub wrap: f64,
}
impl Default for FoliageCanopy {
    fn default() -> Self {
        Self {
            facing: 0.8,
            dab: 1.25,
            cutout: 0.4,
            highlights: 0.18,
            highlight: [0.62, 0.74, 0.3],
            translucency: 0.45,
            glint: 0.2,
            wrap: 0.5,
        }
    }
}

/// The look over the same geometry and instances: `painted` (unlit, the program's paint, blades
/// kept near shade boundaries) or `lit` (physically lit, every blade drawn).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FoliageProfile {
    #[default]
    Painted,
    Lit,
}

/// A static lean of each blade about its root, toward a stable random heading.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct FoliageBend {
    /// Degrees, 0 to 80.
    pub angle: f64,
    /// 0 to 1: each blade's angle varies by up to this share.
    pub randomness: f64,
}
impl Default for FoliageBend {
    fn default() -> Self {
        Self {
            angle: 0.,
            randomness: 0.4,
        }
    }
}

/// Wind driven by the renderer's clock: a rigid lean of each blade about its root by a gust field
/// that travels over the world's XY plane, plus a small flutter per blade.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct FoliageWind {
    /// Degrees counterclockwise from +X that the wind blows toward.
    pub azimuth: f64,
    /// The strongest lean, degrees, 0 to 60. Zero turns wind off.
    pub angle: f64,
    /// Meters across one gust, above 0.
    pub scale: f64,
    /// Meters per second the gusts travel, 0 or more.
    pub speed: f64,
    /// 0 to 1: how much the lean follows `azimuth` rather than the gust's own direction.
    pub bias: f64,
    /// Degrees of each blade's own flutter, 0 to 20.
    pub flutter: f64,
}
impl Default for FoliageWind {
    fn default() -> Self {
        Self {
            azimuth: 30.,
            angle: 14.,
            scale: 2.5,
            speed: 1.2,
            bias: 0.6,
            flutter: 2.,
        }
    }
}

/// Which blades the vertex stage draws in the painted profile: those near the boundary of the
/// ground's shade (a distance field the renderer remakes when the sun or the ground changes),
/// those where the view grazes the ground, and a share everywhere. Rejection is per blade over the
/// stable candidate set, so nothing is scattered again when the sun moves.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct FoliageRejection {
    /// Meters from the shade boundary within which blades are kept, densest at the boundary.
    /// Zero keeps every blade.
    pub boundary: f64,
    /// Share of blades kept everywhere, 0 to 1.
    pub fill: f64,
    /// Degrees either side of edge-on (the view along the ground) within which blades are kept.
    pub grazing: f64,
    /// Share of blades kept where the view grazes the ground, 0 to 1.
    pub grazing_keep: f64,
}
impl Default for FoliageRejection {
    fn default() -> Self {
        Self {
            boundary: 0.22,
            fill: 0.,
            grazing: 15.,
            grazing_keep: 0.5,
        }
    }
}

/// The per-pixel part of the painted profile: the paint lifts toward the tip and, on the lit side
/// of the band, fades toward a highlight color.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct FoliageTip {
    /// Multiplier of the paint at the tip, 0.5 to 2.
    pub lift: f64,
    /// Linear RGB, 0 to 1.
    pub highlight: [f64; 3],
    /// How far the tip fades toward `highlight`, 0 to 1.
    pub amount: f64,
    /// Share of the blade's height where the fade starts, 0 to 1.
    pub start: f64,
    /// Highlight grass (the painterly study's highlight population): four linear RGB colors, 0 to 1. With it, each
    /// blade takes one of them, at random, as its own tip color in place of `highlight`, and the fade runs from `start`
    /// to 0.35 above it at full `amount`, on the lit side only; below it the blade is the paint of the ground at its
    /// root, so it melts into the ground.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub palette: Option<[[f64; 3]; 4]>,
}
impl Default for FoliageTip {
    fn default() -> Self {
        Self {
            lift: 1.08,
            highlight: [0.62, 0.72, 0.28],
            amount: 0.35,
            start: 0.55,
            palette: None,
        }
    }
}

impl Foliage {
    /// Every number finite and within its stated range.
    pub fn validate(&self) -> Result<(), String> {
        let within = |value: f64, low: f64, high: f64, name: &str| {
            if value.is_finite() && value >= low && value <= high {
                Ok(())
            } else {
                Err(format!("Foliage {name} must be from {low} to {high}."))
            }
        };
        if self.patch.is_empty() || self.patch.len() > 256 {
            return Err("Foliage patch identity must be 1 to 256 characters.".into());
        }
        if self
            .material
            .as_ref()
            .is_some_and(|id| id.is_empty() || id.len() > 64)
        {
            return Err("Foliage material identity must be 1 to 64 characters.".into());
        }
        within(f64::from(self.level), 0., 15., "level")?;
        within(self.distance, 0., 1e6, "distance")?;
        within(self.far, 0., 1e6, "far")?;
        if self.far <= self.distance {
            return Err("Foliage far must be beyond the level's distance.".into());
        }
        if !(self.keep.is_finite() && self.keep > 0. && self.keep <= 1.) {
            return Err("Foliage keep must be above 0 and at most 1.".into());
        }
        within(self.bend.angle, 0., 80., "bend angle")?;
        within(self.bend.randomness, 0., 1., "bend randomness")?;
        within(self.wind.azimuth, -360., 360., "wind azimuth")?;
        within(self.wind.angle, 0., 60., "wind angle")?;
        within(self.wind.scale, 1e-3, 1e4, "wind scale")?;
        within(self.wind.speed, 0., 100., "wind speed")?;
        within(self.wind.bias, 0., 1., "wind bias")?;
        within(self.wind.flutter, 0., 20., "wind flutter")?;
        within(self.rejection.boundary, 0., 100., "rejection boundary")?;
        within(self.rejection.fill, 0., 1., "rejection fill")?;
        within(self.rejection.grazing, 0., 89., "rejection grazing")?;
        within(self.rejection.grazing_keep, 0., 1., "rejection grazingKeep")?;
        within(self.tip.lift, 0.5, 2., "tip lift")?;
        for channel in self.tip.highlight {
            within(channel, 0., 1., "tip highlight")?;
        }
        within(self.tip.amount, 0., 1., "tip amount")?;
        within(self.tip.start, 0., 1., "tip start")?;
        for color in self.tip.palette.iter().flatten() {
            for channel in color {
                within(*channel, 0., 1., "tip palette")?;
            }
        }
        if let Some(canopy) = &self.canopy {
            if self.family == FoliageFamily::Grass {
                return Err(
                    "Foliage canopy parameters belong to plant families, not grass.".into(),
                );
            }
            within(canopy.facing, 0., 1., "canopy facing")?;
            within(canopy.dab, 0.25, 4., "canopy dab")?;
            within(canopy.cutout, 0., 1., "canopy cutout")?;
            within(canopy.highlights, 0., 1., "canopy highlights")?;
            for channel in canopy.highlight {
                within(channel, 0., 1., "canopy highlight")?;
            }
            within(canopy.translucency, 0., 1., "canopy translucency")?;
            within(canopy.glint, 0., 1., "canopy glint")?;
            within(canopy.wrap, 0., 1., "canopy wrap")?;
        }
        if let Some(mass) = &self.mass {
            if self.family == FoliageFamily::Grass {
                return Err("A painted mass belongs to a plant family, not grass.".into());
            }
            within(mass.radius, 0.05, 5., "mass radius")?;
            within(mass.cell, 0.02, 2., "mass cell")?;
            within(mass.level, 1e-3, 2., "mass level")?;
            within(mass.card_keep, 0., 1., "mass cardKeep")?;
            within(mass.palette.hue, -180., 180., "mass palette hue")?;
            within(mass.palette.saturation, 0., 2., "mass palette saturation")?;
            within(mass.palette.value, 0., 2., "mass palette value")?;
            within(mass.palette.shade, 0., 2., "mass palette shade")?;
            within(mass.palette.shade_hue, -180., 180., "mass palette shadeHue")?;
            within(mass.palette.contrast, 0., 3., "mass palette contrast")?;
            within(mass.palette.depth, 0., 3., "mass palette depth")?;
            if mass.sites.is_empty() || mass.sites.len() > MAX_MASS_SITES {
                return Err(format!(
                    "A painted mass needs 1 to {MAX_MASS_SITES} leaf cluster sites."
                ));
            }
            if mass.twigs.len() > MAX_MASS_TWIGS
                || mass.twigs.iter().any(|twig| twig.len() > MAX_TWIG_POINTS)
            {
                return Err(format!(
                    "A painted mass holds at most {MAX_MASS_TWIGS} twigs of at most {MAX_TWIG_POINTS} points."
                ));
            }
            let (mut low, mut high) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
            for [x, y, z, size] in &mass.sites {
                for (axis, value) in [x, y, z].into_iter().enumerate() {
                    within(*value, -1e6, 1e6, "mass site")?;
                    low[axis] = low[axis].min(*value);
                    high[axis] = high[axis].max(*value);
                }
                within(*size, 0., 1e3, "mass site size")?;
            }
            for point in mass.twigs.iter().flatten() {
                for value in point {
                    within(*value, -1e6, 1e6, "mass twig point")?;
                }
            }
            // The widest field a renderer makes: the sites' bounds with a margin, at twice the radius.
            let cells: f64 = (0..3)
                .map(|axis| (high[axis] - low[axis] + 6. * mass.radius) / mass.cell + 3.)
                .product();
            if cells > MAX_MASS_CELLS {
                return Err(
                    "A painted mass's density field would be too large: widen its cell.".into(),
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foliage_refuses_what_is_out_of_range() {
        // Parsing (defaults, unknown fields) is checked where documents are read: superspec-model.
        let parsed = Foliage {
            family: FoliageFamily::Grass,
            patch: "lawn".into(),
            level: 0,
            distance: 0.,
            keep: 1.,
            far: 40.,
            material: None,
            profile: FoliageProfile::default(),
            bend: FoliageBend::default(),
            wind: FoliageWind::default(),
            rejection: FoliageRejection::default(),
            tip: FoliageTip::default(),
            canopy: None,
            mass: None,
        };
        parsed.validate().unwrap();
        let plant = Foliage {
            family: FoliageFamily::Leaves,
            canopy: Some(FoliageCanopy::default()),
            ..parsed.clone()
        };
        plant.validate().unwrap();
        let mass = FoliageMass {
            sites: vec![[0., 0., 2., 0.15], [0.3, 0., 2.2, 0.15]],
            twigs: vec![vec![[0., 0., 1.8], [0.1, 0., 2.]]],
            ..FoliageMass::default()
        };
        Foliage {
            family: FoliageFamily::Shadow,
            mass: Some(mass.clone()),
            ..plant.clone()
        }
        .validate()
        .unwrap();
        for broken in [
            // A mass without sites, with a site off in the void, or with too fine a field.
            Foliage {
                mass: Some(FoliageMass::default()),
                ..plant.clone()
            },
            Foliage {
                mass: Some(FoliageMass {
                    sites: vec![[0., f64::NAN, 0., 0.1]],
                    ..mass.clone()
                }),
                ..plant.clone()
            },
            Foliage {
                mass: Some(FoliageMass {
                    sites: vec![[0., 0., 0., 0.1], [900., 900., 900., 0.1]],
                    ..mass.clone()
                }),
                ..plant.clone()
            },
            Foliage {
                mass: Some(FoliageMass {
                    palette: FoliagePalette {
                        saturation: 3.,
                        ..FoliagePalette::default()
                    },
                    ..mass.clone()
                }),
                ..plant.clone()
            },
            // Grass has no mass.
            Foliage {
                mass: Some(mass.clone()),
                ..parsed.clone()
            },
        ] {
            assert!(broken.validate().is_err(), "{broken:?}");
        }
        for broken in [
            Foliage {
                canopy: Some(FoliageCanopy {
                    dab: 0.,
                    ..FoliageCanopy::default()
                }),
                ..plant.clone()
            },
            Foliage {
                canopy: Some(FoliageCanopy {
                    highlight: [0., 2., 0.],
                    ..FoliageCanopy::default()
                }),
                ..plant.clone()
            },
            // Grass has no canopy.
            Foliage {
                canopy: Some(FoliageCanopy::default()),
                ..parsed.clone()
            },
        ] {
            assert!(broken.validate().is_err(), "{broken:?}");
        }
        for broken in [
            Foliage {
                far: 0.,
                ..parsed.clone()
            },
            Foliage {
                keep: 0.,
                ..parsed.clone()
            },
            Foliage {
                patch: String::new(),
                ..parsed.clone()
            },
            Foliage {
                wind: FoliageWind {
                    angle: 90.,
                    ..FoliageWind::default()
                },
                ..parsed.clone()
            },
            Foliage {
                tip: FoliageTip {
                    highlight: [0., f64::NAN, 0.],
                    ..FoliageTip::default()
                },
                ..parsed.clone()
            },
        ] {
            assert!(broken.validate().is_err(), "{broken:?}");
        }
    }

    #[test]
    fn placements_validate_and_place_points() {
        assert!(Placement::default().validate().is_ok());
        let half = std::f64::consts::FRAC_1_SQRT_2;
        // A quarter turn about +Z, doubled along the mesh's X, moved to (10, 0, 1).
        let placement = Placement {
            translation: [10., 0., 1.],
            rotation: [0., 0., half, half],
            scale: [2., 1., 1.],
        };
        placement.validate().unwrap();
        let point = place(&placement.delivered(), [1., 0., 0.]);
        for (found, expected) in point.iter().zip([10., 2., 1.]) {
            assert!((found - expected).abs() < 1e-6, "{point:?}");
        }
        for invalid in [
            Placement {
                rotation: [0., 0., 0., 2.],
                ..Default::default()
            },
            Placement {
                scale: [1., -1., 1.],
                ..Default::default()
            },
            Placement {
                scale: [1., 0., 1.],
                ..Default::default()
            },
            Placement {
                translation: [f64::NAN, 0., 0.],
                ..Default::default()
            },
        ] {
            assert!(invalid.validate().is_err(), "{invalid:?}");
        }
        // The delivered rotation is unit length whatever rounding the authored one carried.
        let delivered = Placement {
            rotation: [0., 0., 0.70712, 0.70712],
            ..Default::default()
        }
        .delivered();
        let length = delivered[3..7].iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((length - 1.).abs() < 1e-6);
    }

    #[test]
    fn collision_defaults_to_none() {
        assert_eq!(Collision::default(), Collision::None);
    }
}
