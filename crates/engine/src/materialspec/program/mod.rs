//! A bounded, portable material language. Hosts supply source modules and decoded images.
mod bake;
mod check;
mod compiled;
mod deps;
mod eval;
mod features;
mod glsl;
mod images;
mod lanes;
pub use eval::seed_value;
pub use glsl::{CpuField, Shader};
pub use images::MipCache;
mod memory;
mod numeric;
mod planes;
mod syntax;
mod ts;
mod zones;
pub use deps::{Dependencies, DependencyTable, SampleInputs};
pub use eval::{FieldSelection, OutputSet};
pub use features::{
    COPLANAR_DEGREES, FeatureSet, Follow, MAX_FEATURE_NAMES, MAX_FEATURES, MAX_FOLLOWED,
    MAX_NAMED_SEGMENTS, NamedFeature, NamedFeatures, bend_shaded, followed_features,
    followed_names, orient_relief, receiver_features, receiver_seams,
};
pub use planes::{Binding, FieldPlane, PLANE_MAGIC, Plane};
pub use zones::{
    DEFAULT_DEPTH as ZONE_DEPTH, ZONE_ROW_TEXELS, Zone, ZoneHit, ZoneSet, pack as pack_zones,
    zone_id,
};
pub use ts::{
    EditFailure, EditOp, Edited, Inspection, Patch, Span, apply as apply_edit,
    inspect as inspect_recipe,
};

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use superspec_types::diagnostic::SourceLocation;

pub const COMMON_HELPERS: &str = include_str!("examples/common.ms");
pub const CHIPPED_CONCRETE: &str = include_str!("examples/chipped-concrete.ms");
pub const PAINTED_METAL: &str = include_str!("examples/painted-metal.ms");
/// TypeScript recipes: the same studies as the `.ms` examples, plus a layered group showcase.
pub const COMMON_HELPERS_TS: &str = include_str!("examples/common.material.ts");
pub const CHIPPED_CONCRETE_TS: &str = include_str!("examples/chipped-concrete.material.ts");
pub const PAINTED_METAL_TS: &str = include_str!("examples/painted-metal.material.ts");
pub const TILED_COURTYARD_TS: &str = include_str!("examples/tiled-courtyard.material.ts");

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Sources {
    pub entry: String,
    pub modules: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MaterialRequest {
    pub entry: String,
    #[serde(default)]
    pub parameters: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub seed: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Type {
    Scalar,
    Vec2,
    Vec3,
    Bool,
    String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Source {
    pub module: String,
    pub location: SourceLocation,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProgramDiagnostic {
    pub code: &'static str,
    pub message: String,
    pub source: Source,
}
impl std::fmt::Display for ProgramDiagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}:{} {}",
            self.source.module,
            self.source.location.line,
            self.source.location.column,
            self.message
        )
    }
}
impl std::error::Error for ProgramDiagnostic {}

#[derive(Clone, Debug, Serialize)]
pub struct Parameter {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: Type,
    pub default: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub source: Source,
}
#[derive(Clone, Debug, Serialize)]
pub struct Field {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: Type,
    pub source: Source,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    pub version: u32,
    pub entry: String,
    pub dependencies: Vec<String>,
    pub parameters: Vec<Parameter>,
    pub fields: Vec<Field>,
    pub outputs: Vec<String>,
    pub required_inputs: Vec<String>,
    /// Live renderer operations declared by a TypeScript recipe. The evaluator never runs them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub live: Vec<LiveOperation>,
    /// External inputs the program reads with `external(name, fallback)`, by name. A host may
    /// supply each as a [`Plane`] over a receiver's texels.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub externals: Vec<External>,
    /// Names of the receiver's features the program queries (`feature_distance`,
    /// `feature_tangent`), ascending.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub named_features: Vec<String>,
    /// Names of the receiver's zones the program queries (`zone_coord`, `zone_extent`,
    /// `zone_span`), ascending.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub zones: Vec<String>,
    /// What the recipe is: a material, a decal's appearance or a weathering layer.
    #[serde(skip_serializing_if = "RecipeKind::is_material")]
    pub kind: RecipeKind,
    /// Whether the surface is drawn from both sides.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub double_sided: bool,
    /// The cutoff of a cutout (`alpha_cutout` as the opacity): the material is delivered with
    /// alpha mode MASK, drawn where its opacity reaches this.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpha_cutoff: Option<f64>,
    /// A decal's width and height in meters. Its `uv` spans the rectangle and its `surface`
    /// coordinates are meters from the rectangle's lower left corner.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<[f64; 2]>,
    /// For a decal or a layer, how each channel it outputs combines with the surface beneath,
    /// by output name. A layer's outputs already hold the combined value (it reads the surface
    /// beneath through `external("base.<channel>", ...)`); a decal's hold its own value, and
    /// whoever places it combines them.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub blend: BTreeMap<String, Blend>,
    /// For a layer, the channels of the surface beneath that it reads, by output name.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub reads: Vec<String>,
    /// How the material is drawn: lit by the scene's lights (the default), or `painted`, whose
    /// color reaches the screen without the light loop. A painted material declares live inputs:
    /// it is delivered with its program (see `Shader::painted`), and its baked maps are the
    /// fallback a renderer without the program shows.
    #[serde(skip_serializing_if = "Shading::is_lit")]
    pub shading: Shading,
    /// For a painted material, the field holding the color that reaches the screen (a `vec3`,
    /// linear RGB); without one it is the base color.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paint: Option<String>,
}

/// See [`Metadata::shading`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Shading {
    #[default]
    Lit,
    Painted,
}
impl Shading {
    fn is_lit(&self) -> bool {
        *self == Self::Lit
    }
}

/// See [`Metadata::kind`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RecipeKind {
    #[default]
    Material,
    /// A bounded image sized in meters, baked without a receiver and placed by its owner.
    Decal,
    /// A contribution over a whole surface, composed over a base material and the layers
    /// beneath it.
    Layer,
}
impl RecipeKind {
    fn is_material(&self) -> bool {
        *self == Self::Material
    }
}

/// How a channel of a decal or layer combines with the value beneath it, by coverage:
/// `mix(beneath, combined, coverage)`, where `combined` is the channel's value (`mix`), the sum
/// (`add`), the product (`multiply`) or the larger or smaller of the two (`max`, `min`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Blend {
    Mix,
    Add,
    Multiply,
    Max,
    Min,
}
impl Blend {
    pub const NAMES: [&'static str; 5] = ["mix", "add", "multiply", "max", "min"];
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "mix" => Self::Mix,
            "add" => Self::Add,
            "multiply" => Self::Multiply,
            "max" => Self::Max,
            "min" => Self::Min,
            _ => return None,
        })
    }
}

/// The prefix of the external inputs a layer reads the surface beneath through:
/// `base.base_color`, `base.roughness`, `base.metalness`, `base.height`, `base.opacity`,
/// `base.normal_offset`.
pub const BASE_PLANE: &str = "base.";
/// The entry of a layered slot's composite program, which the authoring session writes: it reads
/// every channel of its stack and its base's live inputs as planes.
pub const LAYERED_COMPOSITE: &str = "superspec-layered-stack.ms";
/// The channels a layer may read and change. `normal_offset` lets a layer that follows the
/// receiver's creases round them over, as a base material may.
pub const LAYER_CHANNELS: [&str; 6] = [
    "base_color",
    "roughness",
    "metalness",
    "height",
    "opacity",
    "normal_offset",
];

/// What a recipe's entry declares about itself beside its outputs.
#[derive(Clone, Debug, Default)]
pub(crate) struct Traits {
    pub kind: RecipeKind,
    pub double_sided: bool,
    pub size: Option<[f64; 2]>,
    pub blend: BTreeMap<String, Blend>,
    pub shading: Shading,
    pub paint: Option<String>,
}

/// A named per-texel input a host may supply; see [`Metadata::externals`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct External {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: Type,
}

/// A renderer operation recorded in the material with the fields that feed it.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveOperation {
    pub id: String,
    pub version: u32,
    pub stage: String,
    /// Operation argument name to the program field holding its value.
    pub inputs: BTreeMap<String, String>,
    /// Inputs and capabilities the renderer must supply.
    pub requires: Vec<String>,
    pub source: Source,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Edge {
    pub start: [f64; 3],
    pub end: [f64; 3],
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Inputs {
    #[serde(default)]
    pub parameters: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub seed: u32,
    /// Decoded images by name. They are shared, so inputs that differ only in parameters or seed
    /// hold one copy of the pixels.
    #[serde(default)]
    pub images: BTreeMap<String, std::sync::Arc<Image>>,
    #[serde(default)]
    pub edges: Vec<Edge>,
    /// Receiver creases and open edges in world meters. Hosts that bake receivers derive these
    /// per receiver with [`receiver_features`]; direct callers may supply them here.
    #[serde(default)]
    pub features: Vec<Feature>,
    /// Mip levels of `images`, built on first use by filtered sampling. Clones share the cache,
    /// and a host may give several inputs one cache so equal images are filtered once.
    #[serde(skip)]
    pub mips: images::MipCache,
    /// The values of the render-only inputs, for an evaluation that stands for a drawn pixel: a
    /// CPU reference of the shader, or a check of a program's live outputs. Bakes never bind
    /// them, so a baked value that reads one fails where it is read (`MS_INPUT`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render: Option<RenderBindings>,
}

/// What a renderer supplies where a material is drawn, for evaluating render-only inputs on the
/// CPU. [`RenderBindings::default`] holds the default bindings: the clock at zero, the lighting
/// record's default sun, no instance (origin, white tint, random 0, value zero), no cast shadow,
/// every named attribute at the value its call is typed by, and no scene field.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RenderBindings {
    pub time: f64,
    /// Unit vector from the scene toward the sun.
    pub sun_direction: [f64; 3],
    /// The drawn instance's value (`instance` and `instance_w`).
    pub instance: [f64; 4],
    pub instance_root: [f64; 3],
    pub instance_tint: [f64; 3],
    pub instance_random: f64,
    /// The drawn point through its instance's placement; `None` reads the sample's `world`.
    pub placed_world: Option<[f64; 3]>,
    /// The sun's cast shadow at the drawn point, 1 lit and 0 shadowed, which `shade_band` folds in.
    pub cast_shadow: f64,
    /// Named instance and vertex attributes; a name not bound reads the value its call is typed by.
    pub instance_attributes: BTreeMap<String, Vec<f64>>,
    pub vertex_attributes: BTreeMap<String, Vec<f64>>,
    /// Top-down scene fields by name (see `catalog::SCENE_FIELDS`).
    pub fields: BTreeMap<String, std::sync::Arc<SceneField>>,
}
impl Default for RenderBindings {
    fn default() -> Self {
        Self {
            time: 0.,
            // The lighting record's default sun (`superspec_types::lighting`), normalized.
            sun_direction: superspec_types::lighting::Light::default().direction,
            instance: [0.; 4],
            instance_root: [0.; 3],
            instance_tint: [1.; 3],
            instance_random: 0.,
            placed_world: None,
            cast_shadow: 1.,
            instance_attributes: BTreeMap::new(),
            vertex_attributes: BTreeMap::new(),
            fields: BTreeMap::new(),
        }
    }
}

/// A top-down field over a rectangle of the world's XY plane: `width` by `height` texels of
/// `components` values each, rows from the rectangle's lower (minimum Y) edge, texel `(i, j)`
/// standing for the point `min + (i + 0.5, j + 0.5) * (max - min) / (width, height)`. Sampling is
/// bilinear between texel centers, with the edge texels continuing outside the rectangle, which
/// is what a renderer's float texture with clamped `texelFetch` reads (see `glsl.rs`).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneField {
    pub width: usize,
    pub height: usize,
    pub components: usize,
    pub min: [f64; 2],
    pub max: [f64; 2],
    pub values: Vec<f32>,
}
impl SceneField {
    /// Whether the field's sizes agree and every value is finite.
    pub fn valid(&self) -> bool {
        self.width > 0
            && self.height > 0
            && (1..=4).contains(&self.components)
            && self.width.saturating_mul(self.height) <= 1 << 24
            && self.values.len() == self.width * self.height * self.components
            && self.min.iter().chain(&self.max).all(|v| v.is_finite())
            && self.max[0] > self.min[0]
            && self.max[1] > self.min[1]
            && self.values.iter().all(|v| v.is_finite())
    }
    /// The field at a world XY position: the first three components (zero past `components`).
    pub fn sample(&self, at: [f64; 2]) -> [f64; 3] {
        let size = [self.width, self.height];
        let mut cell = [0usize; 2];
        let mut next = [0usize; 2];
        let mut t = [0.; 2];
        for axis in 0..2 {
            let span = self.max[axis] - self.min[axis];
            let x = (at[axis] - self.min[axis]) / span * size[axis] as f64 - 0.5;
            let last = (size[axis] - 1) as f64;
            let x = x.clamp(0., last);
            let base = x.floor();
            cell[axis] = base as usize;
            next[axis] = (base + 1.).min(last) as usize;
            t[axis] = x - base;
        }
        let at = |i: usize, j: usize, c: usize| {
            if c < self.components {
                f64::from(self.values[(j * self.width + i) * self.components + c])
            } else {
                0.
            }
        };
        std::array::from_fn(|c| {
            let low = at(cell[0], cell[1], c) * (1. - t[0]) + at(next[0], cell[1], c) * t[0];
            let high = at(cell[0], next[1], c) * (1. - t[0]) + at(next[0], next[1], c) * t[0];
            low * (1. - t[1]) + high * t[1]
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FeatureKind {
    /// Two faces meeting outward, like the corner of a box.
    Convex,
    /// Two faces meeting inward, like a floor meeting a wall.
    Concave,
    /// An edge with only one face, like the rim of an open panel.
    Boundary,
    /// An edge between faces whose UVs differ across it, whether or not the faces bend.
    Seam,
    /// An edge where the receiver meets another surface, supplied by a host that knows the scene.
    Contact,
    /// A segment of a feature its geometry's owner named (see [`NamedFeature`]); selected only
    /// by that name.
    Named,
}

/// A finite receiver edge in world meters.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Feature {
    pub start: [f64; 3],
    pub end: [f64; 3],
    pub kind: FeatureKind,
    /// Angle between the adjoining face normals in degrees; 180 for boundary edges.
    pub angle: f64,
    /// World-space unit normal halfway between the adjoining faces: the normal at the middle of
    /// the edge once it is rounded over. Absent for boundaries, which have one face.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normal: Option<[f64; 3]>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Sample {
    pub uv: [f64; 2],
    pub surface: [f64; 2],
    pub object: [f64; 3],
    pub world: [f64; 3],
    pub normal: [f64; 3],
    /// Sample diameter in meters; coordinate arithmetic propagates a filtering footprint estimate.
    pub footprint: f64,
    /// Dimensionless UV sample diameter. None leaves UV fields unfiltered.
    pub uv_footprint: Option<f64>,
    pub gravity: [f64; 3],
    /// World-space bounding box of the receiver being sampled, in meters. Hosts that bake a
    /// receiver supply its bounds; a swatch bake supplies its rectangle's.
    pub bounds_min: [f64; 3],
    pub bounds_max: [f64; 3],
}
impl Default for Sample {
    fn default() -> Self {
        Self {
            uv: [0.; 2],
            surface: [0.; 2],
            object: [0.; 3],
            world: [0.; 3],
            normal: [0., 0., 1.],
            footprint: 0.,
            uv_footprint: None,
            gravity: [0., 0., -1.],
            bounds_min: [0.; 3],
            bounds_max: [0.; 3],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum FieldValue {
    Scalar(f64),
    Vec2([f64; 2]),
    Vec3([f64; 3]),
    Bool(bool),
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outputs {
    /// Linear-light RGB; texture baking encodes this as sRGB.
    pub base_color: [f64; 3],
    pub roughness: f64,
    pub metalness: f64,
    /// Signed displacement in meters; baking derives a tangent-space +Y normal map.
    pub height: f64,
    pub emissive: [f64; 3],
    pub opacity: f64,
    /// World-space unit shading normal the relief is applied around; None keeps the receiver's own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receiver_normal: Option<[f64; 3]>,
    /// World-space vector added to the shaded normal after the height relief; None adds nothing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normal_offset: Option<[f64; 3]>,
    pub fields: BTreeMap<String, FieldValue>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Domain {
    pub origin: [f64; 3],
    /// Object-space edges of the bake rectangle, in meters.
    pub u: [f64; 3],
    pub v: [f64; 3],
    /// Column-major affine object-to-world transform.
    pub world_from_object: [f64; 16],
}
impl Default for Domain {
    fn default() -> Self {
        Self {
            origin: [0.; 3],
            u: [1., 0., 0.],
            v: [0., 1., 0.],
            world_from_object: [
                1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
            ],
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BakeRequest {
    pub width: usize,
    pub height: usize,
    #[serde(default)]
    pub domain: Domain,
    #[serde(default)]
    pub inspection: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Baked {
    pub width: usize,
    pub height: usize,
    pub base_color: Vec<u8>,
    pub normal: Vec<u8>,
    /// Linear RGBA8: R=occlusion(1), G=roughness, B=metalness, A=1.
    pub properties: Vec<u8>,
    pub emissive: Vec<u8>,
    /// Multiply decoded emissive RGB by this factor to restore HDR emission.
    pub emissive_strength: f64,
    pub height_meters: Vec<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inspection: Option<Vec<u8>>,
}

#[derive(Clone)]
pub struct Program {
    sources: Sources,
    definitions: BTreeMap<String, syntax::Definition>,
    outputs: BTreeMap<String, syntax::Expr>,
    metadata: Metadata,
    /// The same definitions with names and calls resolved, built once after checking.
    compiled: std::sync::Arc<compiled::Compiled>,
}
/// A finite batch of samples over one immutable, validated set of material inputs.
pub struct Evaluator<'a> {
    program: &'a Program,
    inputs: &'a Inputs,
    features: &'a [Feature],
    /// A grid over `features`, so feature queries visit only the features near a sample.
    index: Grid<'a>,
    /// Segments of the name with the most named features, for the bound on a sample's queries.
    largest_named: usize,
    parameters: Vec<eval::Value<'static>>,
    remaining: usize,
    scratch: eval::Scratch<'a>,
}

/// An evaluator's feature grid: its own, or one shared with every evaluator of the same
/// features ([`FeatureSet`]).
enum Grid<'a> {
    Own(features::Index),
    Shared(&'a features::Index),
}
impl std::ops::Deref for Grid<'_> {
    type Target = features::Index;
    fn deref(&self) -> &features::Index {
        match self {
            Grid::Own(index) => index,
            Grid::Shared(index) => index,
        }
    }
}

/// Operations a row's allowance grants for each of its samples, and the least and most a row
/// is granted (see [`Evaluator::replenish_row`]).
const ROW_OPERATIONS_PER_SAMPLE: usize = 2_200_000;
const ROW_OPERATIONS: [usize; 2] = [64_000_000, 4_000_000_000];
impl<'a> Evaluator<'a> {
    pub fn evaluate(&mut self, sample: &Sample) -> Result<Outputs, ProgramDiagnostic> {
        self.wanted(sample, eval::Want::All)
    }
    /// Every output channel, but only the one inspected field (or none), for bakes. Channel values
    /// are identical to [`Self::evaluate`]'s; unrequested fields are simply not computed.
    pub fn evaluate_channels(
        &mut self,
        sample: &Sample,
        inspect: Option<&str>,
    ) -> Result<Outputs, ProgramDiagnostic> {
        match inspect {
            Some(name) => self.wanted(sample, eval::Want::Channels(&[name])),
            None => self.wanted(sample, eval::Want::Channels(&[])),
        }
    }
    /// Every output channel plus the named fields, such as live renderer arguments.
    pub fn evaluate_channels_with(
        &mut self,
        sample: &Sample,
        fields: &[&str],
    ) -> Result<Outputs, ProgramDiagnostic> {
        self.wanted(sample, eval::Want::Channels(fields))
    }
    /// Every output channel plus fields chosen once with [Program::select_fields], written to
    /// `values` in the order they were named. The returned outputs' own field map stays empty, so
    /// a bake can read per-texel fields without allocating or comparing names.
    pub fn evaluate_selected(
        &mut self,
        sample: &Sample,
        selection: &FieldSelection,
        values: &mut [FieldValue],
    ) -> Result<Outputs, ProgramDiagnostic> {
        self.wanted(sample, eval::Want::Selected(selection, values))
    }
    /// Only `outputs`, plus fields chosen once with [Program::select_fields] written to `values`.
    /// Outputs outside the set keep their defaults in the result; the others are identical to
    /// [`Self::evaluate_selected`]'s, so a bake of one map evaluates just what that map reads.
    pub fn evaluate_outputs(
        &mut self,
        sample: &Sample,
        outputs: OutputSet,
        selection: &FieldSelection,
        values: &mut [FieldValue],
    ) -> Result<Outputs, ProgramDiagnostic> {
        self.wanted_outputs(sample, eval::Want::Selected(selection, values), outputs)
    }
    /// Like [`Self::evaluate_outputs`] for every sample of a row: the result of each sample is
    /// exactly what evaluating it alone returns, and `values` receives the selected fields
    /// sample by sample (`selection.len()` each). Programs whose cost bounds allow it evaluate
    /// the samples together, which amortizes the walk over the program; any sample that would
    /// fail, or a program or allowance that cannot be bounded, evaluates one sample at a time, so
    /// the first failing sample reports the error it always reported.
    pub fn evaluate_row(
        &mut self,
        samples: &[Sample],
        outputs: OutputSet,
        selection: &FieldSelection,
        values: &mut Vec<FieldValue>,
    ) -> Result<Vec<Outputs>, ProgramDiagnostic> {
        self.evaluate_row_with_planes(samples, &[], outputs, selection, values, &[], &mut [])
    }
    /// [`Self::evaluate_outputs`] of a sample that stands for `texel` of `planes`: `external`
    /// reads its plane there, and a field whose plane holds the texel is read instead of
    /// computed (see [`Plane`]). Steps are charged exactly as [`Self::evaluate_row_with_planes`]
    /// charges the same sample, so both fail or finish alike.
    pub fn evaluate_outputs_at(
        &mut self,
        sample: &Sample,
        texel: usize,
        planes: &[&Plane],
        outputs: OutputSet,
        selection: &FieldSelection,
        values: &mut [FieldValue],
    ) -> Result<Outputs, ProgramDiagnostic> {
        let kept;
        let planes = match self.honored(planes) {
            Some(externals) => {
                kept = externals;
                &kept[..]
            }
            None => planes,
        };
        self.outputs_at(
            sample,
            texel,
            planes,
            eval::Want::Selected(selection, values),
            outputs,
        )
    }
    /// [`Self::evaluate_row`] with planes (see [`Plane`]): `external` reads the plane bound to
    /// its name, a field whose `injected` plane holds a sample's texel is read instead of
    /// computed, and the fields of `capture` planes that are computed are recorded. `texels`
    /// gives each sample's index into the planes; planes need one for every sample. Field planes
    /// only change what is computed: every result equals the one without them, for planes of the
    /// same samples and inputs. They serve only programs whose cost is bounded whatever their
    /// queries visit ([`Self::rows_evaluate_together`]), because a kept plane spares a sample its
    /// field's steps and a limit must not depend on what was kept. External planes are inputs
    /// and are always read.
    #[allow(clippy::too_many_arguments)]
    pub fn evaluate_row_with_planes(
        &mut self,
        samples: &[Sample],
        texels: &[usize],
        outputs: OutputSet,
        selection: &FieldSelection,
        values: &mut Vec<FieldValue>,
        injected: &[&Plane],
        capture: &mut [FieldPlane],
    ) -> Result<Vec<Outputs>, ProgramDiagnostic> {
        if !(injected.is_empty() && capture.is_empty()) && texels.len() != samples.len() {
            return Err(self.program.diagnostic(
                "MS_INPUT",
                "Planes need a texel for every sample.",
                &syntax::Mark::root(&self.program.metadata.entry),
            ));
        }
        let kept;
        let injected = match self.honored(injected) {
            Some(externals) => {
                kept = externals;
                &kept[..]
            }
            None => injected,
        };
        let slots = selection.len();
        if let Some(done) = self.together(
            samples, texels, outputs, selection, values, injected, capture,
        ) {
            return Ok(done);
        }
        let mut results = Vec::with_capacity(samples.len());
        for (i, sample) in samples.iter().enumerate() {
            results.push(self.outputs_at(
                sample,
                texels.get(i).copied().unwrap_or(usize::MAX),
                injected,
                eval::Want::Selected(selection, &mut values[i * slots..(i + 1) * slots]),
                outputs,
            )?);
        }
        Ok(results)
    }
    /// Evaluates `outputs` and the selected fields for every sample of a row and leaves the
    /// selected fields' values, exact and with their filtering estimates, in the `fill` planes
    /// bound to them, at each sample's texel. Unlike the planes [`Self::evaluate_row_with_planes`]
    /// keeps when it can, these are complete: a row that cannot be evaluated together fills them
    /// one sample at a time. This is how one program's fields become another's external inputs
    /// ([`Plane::into_external`]). `externals` are read as there; kept field planes are not.
    #[allow(clippy::too_many_arguments)]
    pub fn fill_row(
        &mut self,
        samples: &[Sample],
        texels: &[usize],
        outputs: OutputSet,
        selection: &FieldSelection,
        values: &mut Vec<FieldValue>,
        externals: &[&Plane],
        fill: &mut [Plane],
    ) -> Result<Vec<Outputs>, ProgramDiagnostic> {
        if texels.len() != samples.len() || externals.iter().any(|plane| plane.field().is_some()) {
            return Err(self.program.diagnostic(
                "MS_INPUT",
                "Filling planes needs a texel for every sample and reads external planes only.",
                &syntax::Mark::root(&self.program.metadata.entry),
            ));
        }
        // The row evaluator fills planes only where it finishes; a declined row leaves them as
        // they were, apart from its count of samples without a value.
        if let Some(done) =
            self.together(samples, texels, outputs, selection, values, externals, fill)
        {
            return Ok(done);
        }
        let slots = selection.len();
        let mut results = Vec::with_capacity(samples.len());
        for (i, sample) in samples.iter().enumerate() {
            results.push(self.filling_at(
                sample,
                texels[i],
                externals,
                eval::Want::Selected(selection, &mut values[i * slots..(i + 1) * slots]),
                outputs,
                fill,
            )?);
        }
        Ok(results)
    }
    /// The planes of `planes` an evaluation reads when it may not read them all: without the
    /// field planes, for a program they do not serve (see [`Self::evaluate_row_with_planes`]).
    fn honored<'p>(&self, planes: &[&'p Plane]) -> Option<Vec<&'p Plane>> {
        let kept = planes.iter().any(|plane| plane.field().is_some());
        (kept && !self.rows_evaluate_together()).then(|| {
            planes
                .iter()
                .copied()
                .filter(|plane| plane.field().is_none())
                .collect()
        })
    }
    /// The part of [`Self::evaluate_row`] that evaluates the samples together: `None` where it
    /// declines (see there), with `values` sized for the samples either way.
    #[doc(hidden)]
    pub fn evaluate_row_together(
        &mut self,
        samples: &[Sample],
        outputs: OutputSet,
        selection: &FieldSelection,
        values: &mut Vec<FieldValue>,
    ) -> Option<Vec<Outputs>> {
        self.together(samples, &[], outputs, selection, values, &[], &mut [])
    }
    #[allow(clippy::too_many_arguments)]
    fn together(
        &mut self,
        samples: &[Sample],
        texels: &[usize],
        outputs: OutputSet,
        selection: &FieldSelection,
        values: &mut Vec<FieldValue>,
        injected: &[&Plane],
        capture: &mut [FieldPlane],
    ) -> Option<Vec<Outputs>> {
        values.clear();
        values.resize(samples.len() * selection.len(), FieldValue::Scalar(0.));
        lanes::evaluate(
            self.program,
            &self.program.compiled,
            self.inputs,
            self.features,
            &self.index,
            samples,
            &self.parameters,
            &mut self.remaining,
            &mut self.scratch,
            outputs,
            &selection.0,
            values,
            texels,
            injected,
            capture,
        )
    }
    /// Only the height output, for derivative samples; identical to [`Self::evaluate`]'s height.
    pub fn evaluate_height(&mut self, sample: &Sample) -> Result<f64, ProgramDiagnostic> {
        Ok(self.wanted(sample, eval::Want::Height)?.height)
    }
    /// Whether [Self::evaluate_row] can evaluate this program's samples together: its cost
    /// bounds stay under the per-sample limits for these inputs and features.
    pub fn rows_evaluate_together(&self) -> bool {
        lanes::bounded(
            &self.program.compiled,
            self.inputs.edges.len(),
            self.features.len().max(self.largest_named),
        )
    }
    /// The operation allowance left for this batch.
    pub fn allowance(&self) -> usize {
        self.remaining
    }
    /// Grants a fresh operation allowance (at most 64,000,000), for hosts that bound work per
    /// row of a bake; each sample keeps its own step and depth budget.
    pub fn replenish(&mut self, operations: usize) {
        self.remaining = operations.min(64_000_000);
    }
    /// Grants the allowance of a row of `samples` samples: 2,200,000 operations for each, at
    /// least the 64,000,000 every row has always had and at most 4,000,000,000 (the same on
    /// every target, 32-bit ones included). A sample's own step budget bounds what it can
    /// charge: its steps, and eight operations for each feature its queries visit, of which the
    /// step budget admits about 262,000. So no sample that is within its own budget exhausts a
    /// row of up to 1,800 samples; a fixed allowance failed long rows of programs with many
    /// queries by their length alone.
    pub fn replenish_row(&mut self, samples: usize) {
        self.remaining = samples
            .saturating_mul(ROW_OPERATIONS_PER_SAMPLE)
            .clamp(ROW_OPERATIONS[0], ROW_OPERATIONS[1]);
    }
    fn wanted(
        &mut self,
        sample: &Sample,
        want: eval::Want<'_>,
    ) -> Result<Outputs, ProgramDiagnostic> {
        self.wanted_outputs(sample, want, OutputSet::ALL)
    }
    fn wanted_outputs(
        &mut self,
        sample: &Sample,
        want: eval::Want<'_>,
        outputs: OutputSet,
    ) -> Result<Outputs, ProgramDiagnostic> {
        self.outputs_at(sample, usize::MAX, &[], want, outputs)
    }
    fn outputs_at(
        &mut self,
        sample: &Sample,
        texel: usize,
        planes: &[&Plane],
        want: eval::Want<'_>,
        outputs: OutputSet,
    ) -> Result<Outputs, ProgramDiagnostic> {
        self.filling_at(sample, texel, planes, want, outputs, &mut [])
    }
    #[allow(clippy::too_many_arguments)]
    fn filling_at(
        &mut self,
        sample: &Sample,
        texel: usize,
        planes: &[&Plane],
        want: eval::Want<'_>,
        outputs: OutputSet,
        capture: &mut [Plane],
    ) -> Result<Outputs, ProgramDiagnostic> {
        eval::evaluate_wanted(
            self.program,
            &self.program.compiled,
            self.inputs,
            self.features,
            &self.index,
            sample,
            &self.parameters,
            &mut self.remaining,
            &mut self.scratch,
            want,
            outputs,
            planes,
            texel,
            capture,
        )
    }
}
pub fn compile(sources: &Sources) -> Result<Program, Vec<ProgramDiagnostic>> {
    check::compile(sources)
}
impl Program {
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }
    /// This program with the live operations `live`, whose inputs name its own fields: how a
    /// layered slot's composite carries its base's live operations, reading the base's fields
    /// through planes. Fields a live input names that the program lacks are refused.
    pub(crate) fn with_live(&self, live: Vec<LiveOperation>) -> Result<Self, String> {
        for operation in &live {
            for field in operation.inputs.values() {
                if !self
                    .metadata
                    .fields
                    .iter()
                    .any(|known| known.name == *field)
                {
                    return Err(format!(
                        "Live input field '{field}' is not a field of the program."
                    ));
                }
            }
        }
        let mut program = self.clone();
        program.metadata.live = live;
        Ok(program)
    }
    pub fn source_modules(&self) -> impl Iterator<Item = (&str, &str)> {
        std::iter::once(self.metadata.entry.as_str())
            .chain(self.metadata.dependencies.iter().map(String::as_str))
            .map(|name| (name, self.sources.modules[name].as_str()))
    }
    pub fn evaluate(&self, inputs: &Inputs, sample: &Sample) -> Result<Outputs, ProgramDiagnostic> {
        eval::evaluate(self, inputs, sample)
    }
    /// The program as a GLSL preview: see [`Shader`].
    pub fn shader(&self) -> Result<Shader, String> {
        glsl::shader(self)
    }
    /// The program split between GPU and CPU: see [`glsl::plan`]. Programs with no CPU-only
    /// operation return the same shader as [`Self::shader`], with no CPU fields.
    pub fn shader_plan(&self) -> Result<Shader, String> {
        glsl::plan(self)
    }
    /// Steps of each field's own body, by field index: what computing the field costs a sample.
    pub fn field_steps(&self) -> &[usize] {
        &self.compiled.field_steps
    }
    /// Resolves field names once for [Evaluator::evaluate_selected].
    pub fn select_fields(&self, names: &[&str]) -> Result<FieldSelection, String> {
        FieldSelection::new(self, names)
    }
    pub fn evaluator<'a>(&'a self, inputs: &'a Inputs) -> Result<Evaluator<'a>, ProgramDiagnostic> {
        self.evaluator_with_budget(inputs, 64_000_000)
    }
    pub fn evaluator_with_budget<'a>(
        &'a self,
        inputs: &'a Inputs,
        operations: usize,
    ) -> Result<Evaluator<'a>, ProgramDiagnostic> {
        if operations == 0 || operations > 64_000_000 {
            return Err(self.diagnostic(
                "MS_LIMIT",
                "A sample batch requires an operation allowance from 1 to 64000000.",
                &syntax::Mark::root(&self.metadata.entry),
            ));
        }
        let parameters = eval::validate_inputs(self, inputs)?;
        Ok(Evaluator {
            program: self,
            inputs,
            features: &inputs.features,
            index: Grid::Own(features::Index::new(&inputs.features)),
            largest_named: 0,
            parameters,
            remaining: operations,
            scratch: eval::Scratch::default(),
        })
    }
    /// An evaluator over a receiver's features and named features, both with grids built once
    /// and shared: a bake's threads, and every bake of a layout, read the same ones.
    pub fn evaluator_with_feature_sets<'a>(
        &'a self,
        inputs: &'a Inputs,
        features: &'a FeatureSet,
        named: &'a NamedFeatures,
    ) -> Result<Evaluator<'a>, ProgramDiagnostic> {
        let mut evaluator = self.evaluator(inputs)?;
        eval::validate_features(self, features.features())?;
        evaluator.features = features.features();
        evaluator.index = Grid::Shared(features.index());
        evaluator.largest_named = named.largest();
        evaluator.scratch.named = Some(named);
        Ok(evaluator)
    }
    /// An evaluator whose receiver features replace `inputs.features`, so a host can bake each
    /// receiver against its own geometry while sharing parameters and decoded images.
    pub fn evaluator_with_features<'a>(
        &'a self,
        inputs: &'a Inputs,
        features: &'a [Feature],
    ) -> Result<Evaluator<'a>, ProgramDiagnostic> {
        let mut evaluator = self.evaluator(inputs)?;
        eval::validate_features(self, features)?;
        evaluator.features = features;
        evaluator.index = Grid::Own(features::Index::new(features));
        Ok(evaluator)
    }
    pub fn bake(&self, request: &BakeRequest, inputs: &Inputs) -> Result<Baked, ProgramDiagnostic> {
        bake::bake(self, request, inputs)
    }
    pub fn bake_cancellable(
        &self,
        request: &BakeRequest,
        inputs: &Inputs,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Baked, ProgramDiagnostic> {
        bake::bake_cancellable(self, request, inputs, cancelled)
    }
    fn diagnostic(
        &self,
        code: &'static str,
        message: impl Into<String>,
        mark: &syntax::Mark,
    ) -> ProgramDiagnostic {
        diagnostic(&self.sources, code, message, mark)
    }
}

fn diagnostic(
    sources: &Sources,
    code: &'static str,
    message: impl Into<String>,
    mark: &syntax::Mark,
) -> ProgramDiagnostic {
    let text = sources
        .modules
        .get(&mark.module)
        .map(String::as_str)
        .unwrap_or("");
    let start = mark.start.min(text.len());
    let end = mark.end.min(text.len());
    let before = &text[..start];
    ProgramDiagnostic {
        code,
        message: message.into(),
        source: Source {
            module: mark.module.clone(),
            location: SourceLocation {
                line: before.bytes().filter(|c| *c == b'\n').count() + 1,
                column: before
                    .rsplit('\n')
                    .next()
                    .unwrap_or("")
                    .encode_utf16()
                    .count()
                    + 1,
                offset: before.encode_utf16().count(),
                end: Some(text[..end].encode_utf16().count()),
            },
        },
    }
}
