use super::{
    ItemDiagnostic,
    expression::{Expression, ExpressionError},
    fail,
};
use crate::materialspec::program::{Follow, MAX_FOLLOWED, MaterialRequest};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use superspec_geometry::Matrix;

pub type Values = BTreeMap<String, Value>;
/// Evaluated derived values by name.
pub type Derived = BTreeMap<String, f64>;
pub type Transform = [[f64; 3]; 4];
/// The texel densities a slot may declare, in texels per meter (those a bake request accepts).
const MIN_DENSITY: f64 = 1.;
const MAX_DENSITY: f64 = 100_000.;
const MAX_PARAMETERS: usize = 160;
const MAX_MATERIAL_SLOTS: usize = 128;
const MAX_ATTACHMENTS: usize = 256;
const MAX_LAYERS_PER_SLOT: usize = 16;
const MAX_ITEM_LOD_ALTERNATIVES: usize = 7;
pub fn identity() -> Transform {
    Matrix::default().0
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemDefinition {
    pub format: String,
    pub version: u32,
    pub id: String,
    #[serde(default)]
    pub parameters: BTreeMap<String, Parameter>,
    /// Version 2: name to expression text. See `expression.rs` for the grammar.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub derived: BTreeMap<String, String>,
    /// Version 1: the item's single generator. Version 2 leaves it absent and lists `parts`.
    #[serde(default, skip_serializing_if = "Generator::is_parts")]
    pub generator: Generator,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<PartDefinition>,
    #[serde(default)]
    pub slots: BTreeMap<String, MaterialRequest>,
    #[serde(default = "identity")]
    pub transform: Transform,
    #[serde(default)]
    pub attachments: Vec<AttachmentDefinition>,
    #[serde(default)]
    pub collision: Collision,
    /// Version 2: images placed on a part's face. See `decal.rs`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decals: Vec<DecalDefinition>,
    /// Version 2: dimensions an editor may drag, each linked to one numeric geometry control.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub handles: Vec<HandleDefinition>,
    /// Version 2: named features the author places (where grime or wear should go), beside
    /// those the generators report.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<FeatureDefinition>,
    /// Weathering layers over each slot's material, lowest first: a MaterialSpec layer
    /// recipe, its parameters and the features it follows.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub layers: BTreeMap<String, Vec<LayerRequest>>,
    /// Texels per meter a slot asks for where its host states none: a sign's lettering or a
    /// notice needs more than a wall, stacked goods less. It is a default; a scene's own
    /// density for the slot, its material or a mesh comes first.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub densities: BTreeMap<String, f64>,
    /// Authored render alternatives. The ordinary definition remains level zero;
    /// each later level overrides only declared geometry controls.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lods: Vec<ItemLodDefinition>,
    #[serde(skip)]
    pub source: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemLodDefinition {
    /// Camera distance in meters at which this alternative becomes eligible.
    pub distance: f64,
    pub parameters: Values,
}

/// An authored feature: a polyline in item definition coordinates given to one part's
/// receivers. It may repeat along one axis and be conditional, as parts are.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureDefinition {
    pub name: String,
    pub part: String,
    #[serde(default = "front_face")]
    pub face: String,
    pub points: Vec<[Scalar; 3]>,
    #[serde(default)]
    pub closed: bool,
    #[serde(default = "toward_front")]
    pub normal: [f64; 3],
    #[serde(default)]
    pub width: Scalar,
    pub repeat: Option<RepeatAxis>,
    pub when: Option<Scalar>,
    /// Whose receivers are given the feature (see [`Reach`]); its own part's by default.
    #[serde(default, skip_serializing_if = "Reach::is_part")]
    pub reach: Reach,
    /// With a wider reach: how near a receiver of another part or item must come to the
    /// feature to be given it, in meters (3 by default, at most 50).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub within: Option<Scalar>,
    /// Makes the polyline the edge of a weathering zone (see [`ZoneDefinition`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zone: Option<ZoneDefinition>,
}

/// A weathering zone below an edge: what hangs from it (runoff, rust, soot) and how far. Its
/// coordinates are `u`, meters along the edge from `u0` at its first point, and `v`, meters
/// below the edge along `down`. A material reads them with `zoneCoord`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ZoneDefinition {
    /// The greatest drop below the edge, in meters.
    pub length: Scalar,
    /// Downhill along the surface, in the definition's coordinates.
    #[serde(default = "downward")]
    pub down: [f64; 3],
    /// How far off the edge's surface the zone reaches, in meters (0.03 by default).
    pub depth: Option<Scalar>,
    /// `u` at the edge's first point.
    #[serde(default)]
    pub u0: Scalar,
    /// A value given to the zone's materials (`zoneExtent`), 1 by default.
    pub scale: Option<Scalar>,
    /// Varies the zone's identity (`zoneCoord`'s id) beyond where it stands.
    #[serde(default)]
    pub seed: Scalar,
}
fn downward() -> [f64; 3] {
    [0., 0., -1.]
}

/// A part's rule for zones below the features its generator reports (`top`, `sill`, ...): the
/// item knows where each edge is and what stands below it, so its zones start at the edge and
/// stop where the next edge below stops the water. `u` is `u0` plus the edge's start along the
/// part's X axis, then meters along the edge, so walls placed with the `u0` of a footprint run
/// on round a building. On a panel, `until: "openings"` ends a column at the head of an opening
/// below the edge, and `clip: "openings"` leaves the opening's own rectangle out.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ZoneRuleDefinition {
    /// The zone's name, as a layer follows it; the feature's own name when absent.
    pub name: Option<String>,
    /// The generated feature the zone hangs below.
    pub below: String,
    /// Only the feature on this face (a panel's `front` or `back`); every face when absent.
    pub face: Option<String>,
    pub length: Scalar,
    pub depth: Option<Scalar>,
    #[serde(default)]
    pub u0: Scalar,
    pub scale: Option<Scalar>,
    #[serde(default)]
    pub seed: Scalar,
    #[serde(default = "zone_stop")]
    pub until: ZoneStop,
    #[serde(default)]
    pub clip: ZoneStop,
}
/// What ends or cuts a zone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ZoneStop {
    #[default]
    None,
    Openings,
}
fn zone_stop() -> ZoneStop {
    ZoneStop::Openings
}

/// Whose receivers a feature is given to. A generated feature reaches its part. An authored
/// one may reach further, to the receivers that come within its `within` distance: those of
/// every part of the placed item it ends up in (`item`: through any depth of sub-items, a
/// building's other sides and its pavement), or those of every item of the scene (`scene`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Reach {
    #[default]
    Part,
    Item,
    Scene,
}
impl Reach {
    pub fn is_part(&self) -> bool {
        *self == Self::Part
    }
}
/// How near a receiver must come to a feature of wider reach when the feature does not say.
pub const REACH_WITHIN: f64 = 3.;
/// The furthest a feature of wider reach may be given.
pub const MAX_REACH_WITHIN: f64 = 50.;

fn front_face() -> String {
    "front".into()
}
fn toward_front() -> [f64; 3] {
    [0., -1., 0.]
}

/// A weathering layer on a slot (`authoring::Layer` once placed): `follow` maps the layer's
/// own feature names to the receiver's. Parameter values may bind item controls with
/// `{"parameter": name}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerRequest {
    pub entry: String,
    #[serde(default)]
    pub parameters: BTreeMap<String, Value>,
    #[serde(default)]
    pub seed: u32,
    /// Each of the layer's names to the receiver feature it follows, or to a list of them: the
    /// name then measures to the nearest of all their polylines.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub follow: BTreeMap<String, Follow>,
}

/// The part and face a decal sits on. `face` is `front` (default) or `back` on a panel,
/// `front` on a box (its y = 0 side) and unused on a route.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecalTarget {
    pub part: String,
    pub face: Option<String>,
}

/// An image placed on a part. On a panel or box face `u` and `v` place its near edges by the
/// shared rule; on a route `along` places its centre by arc length and `around` turns it
/// about the route, in degrees from the section's first axis. `size` is width by height in
/// meters: across and up a face; around and along a route.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecalDefinition {
    pub id: String,
    pub image: String,
    pub size: [Scalar; 2],
    pub on: DecalTarget,
    pub u: Option<Rule>,
    pub v: Option<Rule>,
    pub along: Option<Rule>,
    pub around: Option<Scalar>,
    /// Stacking order: each layer stands 0.6 mm further from the surface.
    #[serde(default)]
    pub layer: u32,
}

fn unit_scale() -> f64 {
    1.
}
/// The link from a dimension to a control: dragging the handle `d` meters along `axis`
/// changes `parameter` by `d * scale`. `at` and `axis` are in item definition coordinates;
/// `at` may read controls and derived values, so the handle follows the dimension.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandleDefinition {
    pub id: String,
    pub parameter: String,
    pub at: [Scalar; 3],
    pub axis: [f64; 3],
    #[serde(default = "unit_scale")]
    pub scale: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    #[default]
    Geometry,
    Finish,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ParameterType {
    Number,
    Integer,
    Boolean,
    Color,
    Choice,
    /// A plan polygon: 3 to 64 `[x, y]` points in meters, counterclockwise seen from above.
    Points,
    /// Up to 64 booleans, such as one per edge of a footprint.
    Flags,
    /// Nested lists, at most four deep and 4096 entries in all, of numbers or (with `choices`)
    /// texts among the choices: an explicit plan, such as what each bay of each storey holds.
    /// Expressions read it with `cell(grid, i, j, ...)` and `count(grid, i, ...)`.
    Grid,
    /// Free text up to 256 bytes, such as a sign's lettering.
    Text,
    /// Up to 64 texts of up to 256 bytes: `{"element": texts, "at": expression}` hands one to a part.
    Texts,
}

/// Whether `value` is a grid of at most `depth` nested lists whose leaves are numbers, or texts
/// among `choices` when there are any; counts its leaves into `count`.
fn grid(value: &Value, choices: &[String], depth: usize, count: &mut usize) -> bool {
    match value {
        Value::Array(items) => {
            depth > 0
                && items.len() <= 64
                && items
                    .iter()
                    .all(|item| grid(item, choices, depth - 1, count))
        }
        Value::Number(number) => {
            *count += 1;
            choices.is_empty() && number.as_f64().is_some_and(f64::is_finite) && *count <= 4096
        }
        Value::String(text) => {
            *count += 1;
            choices.iter().any(|choice| choice == text) && *count <= 4096
        }
        _ => false,
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parameter {
    #[serde(rename = "type")]
    pub kind: ParameterType,
    #[serde(default)]
    pub role: Role,
    pub default: Value,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub unit: Option<String>,
    #[serde(default)]
    pub choices: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterReference {
    pub parameter: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DerivedReference {
    pub derived: String,
}

/// A scalar field: a number, a control or derived value by name, or an inline expression
/// (the derived-value grammar) that may also read the copy indices of a repeated part.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Scalar {
    Number(f64),
    Parameter(ParameterReference),
    Derived(DerivedReference),
    Expression(String),
}
impl Default for Scalar {
    fn default() -> Self {
        Self::Number(0.)
    }
}
impl Scalar {
    /// The absent value of an optional field: a literal zero.
    pub(super) fn is_zero(&self) -> bool {
        matches!(self, Self::Number(value) if *value == 0.)
    }
    fn is_one(&self) -> bool {
        matches!(self, Self::Number(value) if *value == 1.)
    }
}
fn segments() -> Scalar {
    Scalar::Number(16.)
}
fn one() -> Scalar {
    Scalar::Number(1.)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Switch {
    Boolean(bool),
    Parameter(ParameterReference),
}
impl Default for Switch {
    fn default() -> Self {
        Self::Boolean(true)
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Generator {
    #[serde(rename_all = "camelCase")]
    Pipe {
        length: Scalar,
        radius: Scalar,
        #[serde(default)]
        wall_thickness: Scalar,
        #[serde(default = "segments")]
        segments: Scalar,
        #[serde(default)]
        capped: Switch,
    },
    #[serde(rename_all = "camelCase")]
    Duct {
        length: Scalar,
        width: Scalar,
        height: Scalar,
        #[serde(default)]
        wall_thickness: Scalar,
        #[serde(default)]
        capped: Switch,
    },
    Glb {
        asset: String,
        scene: Option<usize>,
    },
    /// Version 2 part generator. The box spans from the part origin to `size`. `bevel` rounds
    /// its twelve edges with that radius in `bevelSegments` (1 or 2) chords; 0 leaves them
    /// sharp. With `bevelAxis` only the four edges along that axis are rounded, so boxes laid
    /// end to end (a coping, a kerb) keep one section.
    #[serde(rename_all = "camelCase")]
    Box {
        size: [Scalar; 3],
        #[serde(default, skip_serializing_if = "Scalar::is_zero")]
        bevel: Scalar,
        #[serde(default = "one", skip_serializing_if = "Scalar::is_one")]
        bevel_segments: Scalar,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bevel_axis: Option<Axis>,
        /// For an X-axis bar, the end setbacks per meter of Y depth.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        miters: Option<[Scalar; 2]>,
    },
    /// Version 2 part generator.
    Panel(PanelDefinition),
    /// Version 2 part generator.
    Route(RouteDefinition),
    /// Version 2 part generator: a footprint swept upward, with its street corners cut and
    /// an optional inset. Face groups `side`, `top`, `bottom`. `bevel` rounds the rims of its
    /// top and bottom as a box's.
    #[serde(rename_all = "camelCase")]
    Prism {
        footprint: FootprintReference,
        height: Scalar,
        #[serde(default)]
        inset: Scalar,
        #[serde(default, skip_serializing_if = "Scalar::is_zero")]
        bevel: Scalar,
        #[serde(default = "one", skip_serializing_if = "Scalar::is_one")]
        bevel_segments: Scalar,
    },
    /// Version 2 part generator: a section swept round a plan arc (a wall, band or moulding
    /// that turns a round corner as one smooth surface).
    Arc(ArcDefinition),
    /// Version 2 part generator: another item definition, placed as a sub-item.
    Item(ItemReference),
    /// Version 2 part generator: a generator supplied by the host (for example a theme's
    /// fixtures), named and given parameters.
    Native(NativeReference),
    /// Version 2 part generator: one shared shape of a host generator, drawn from its content
    /// as [`super::NativeGenerators::prototype_meshes`] gives it, so a library of prototypes
    /// is built from what an item's placements name.
    Prototype(PrototypeReference),
    /// The value of an absent root `generator`: the item is defined by `parts`.
    #[default]
    Parts,
}
/// An axis of a part's own frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Axis {
    X,
    Y,
    Z,
}
impl Generator {
    pub fn is_parts(&self) -> bool {
        matches!(self, Self::Parts)
    }
}

/// One sub-item or alternative sub-items: `entry` is an item definition path, or a list of
/// them from which `pick` (floored, then wrapped into range) chooses. `{name}` in an entry
/// is replaced by the integer value of that copy index or derived value.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Entry {
    One(String),
    Choice(Vec<String>),
}

/// A value handed to a sub-item or native generator: `{"parameter": name}` passes a control
/// of the parent unchanged, `{"derived": name}` and `{"expr": text}` give numbers,
/// `{"choose": [values], "by": text}` picks one value by an expression, and any other JSON
/// value is passed literally.
pub type Arguments = BTreeMap<String, Value>;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemReference {
    pub entry: Entry,
    pub pick: Option<Scalar>,
    #[serde(default)]
    pub parameters: Arguments,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeReference {
    pub name: String,
    #[serde(default)]
    pub parameters: Arguments,
}

/// A prototype by what its key names (`Prototype::key`): the host generator, the content it
/// is drawn from, passed literally, and whether it is the reflected shape.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrototypeReference {
    pub name: String,
    #[serde(default)]
    pub content: Values,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mirrored: bool,
}

/// One axis of a repeated part: `count` copies (0 to 256), indexed from zero by `name`.
/// Inside the part, `name` and `<name>_count` read as numbers. With `footprint` the axis runs
/// over the footprint's edges instead, and each copy also reads the edge's geometry (see
/// `footprint.rs`).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepeatAxis {
    pub name: String,
    #[serde(default)]
    pub count: Scalar,
    pub footprint: Option<FootprintReference>,
    #[serde(
        default,
        rename = "footprintMode",
        skip_serializing_if = "FootprintMode::is_edges"
    )]
    pub footprint_mode: FootprintMode,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FootprintMode {
    #[default]
    Edges,
    Rectangles,
    LargestRectangle,
}

impl FootprintMode {
    fn is_edges(&self) -> bool {
        *self == Self::Edges
    }
}

/// A footprint read from controls: `points` names a `points` control, `streets` an optional
/// `flags` control (one per edge, edge i running from point i to point i + 1), and `cut` the
/// length a corner between two street edges is cut back along each, at 45 degrees on a square
/// corner. Only convex corners are cut. With `round` of 2 to 16 the cut is that many facets
/// along the arc that touches both edges where the cut starts and ends, and `facet` says
/// which of them a copy's cut corner values describe.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FootprintReference {
    pub points: String,
    pub streets: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holes: Option<String>,
    #[serde(default)]
    pub cut: Scalar,
    #[serde(default, skip_serializing_if = "Scalar::is_zero")]
    pub round: Scalar,
    #[serde(default, skip_serializing_if = "Scalar::is_zero")]
    pub facet: Scalar,
}

/// One generator of a version 2 item, with its own meshes, features and geometry key.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartDefinition {
    pub id: String,
    pub generator: Generator,
    /// Face group to item slot. Empty means every face group uses a slot of its own name.
    /// For a sub-item or native part, a group is a slot of the generated meshes.
    #[serde(default)]
    pub slots: BTreeMap<String, String>,
    pub placement: Option<PlacementDefinition>,
    /// Copies of the part over up to three nested axes; copy IDs are `<id>-<i>[-<j>[-<k>]]`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repeat: Vec<RepeatAxis>,
    /// The part (or each copy) is emitted only when this is positive.
    pub when: Option<Scalar>,
    /// Weathering zones below the features the generator reports.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub zones: Vec<ZoneRuleDefinition>,
}

/// Where a part sits inside its item: a position and extrinsic XYZ rotations in degrees.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacementDefinition {
    #[serde(default)]
    pub position: [Scalar; 3],
    #[serde(default)]
    pub rotation: [Scalar; 3],
    /// Instead of `rotation`: the direction in plan that the part's X axis runs along, so the
    /// part's front (-Y) faces to its right. No trigonometry, so copies along a footprint's
    /// edges stand exactly on them.
    pub along: Option<[Scalar; 2]>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Edge {
    #[default]
    Start,
    End,
    Center,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OffsetRule {
    pub from: Edge,
    #[serde(default)]
    pub offset: Scalar,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FractionRule {
    pub fraction: Scalar,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EveryRule {
    pub every: Scalar,
    #[serde(default)]
    pub inset: Scalar,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PointRule {
    pub point: usize,
}
/// The one placement rule shared by openings and fittings (and, later, decals).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Rule {
    Offset(OffsetRule),
    Fraction(FractionRule),
    Every(EveryRule),
    Point(PointRule),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PanelDefinition {
    pub width: Scalar,
    pub height: Scalar,
    pub thickness: Scalar,
    #[serde(default)]
    pub openings: Vec<OpeningDefinition>,
    /// `[u, v]` of the pattern anchor on the front face; the bottom-left corner when absent.
    pub pattern_anchor: Option<[Scalar; 2]>,
    /// Rounds the rim of the front face (where it meets the four edge faces) and the front
    /// rim of every opening with this radius in `bevelSegments` (1 or 2) chords.
    #[serde(default, skip_serializing_if = "Scalar::is_zero")]
    pub bevel: Scalar,
    #[serde(default = "one", skip_serializing_if = "Scalar::is_one")]
    pub bevel_segments: Scalar,
    /// Rounds the panel's upright end edges where it meets a neighbour at a corner, with this
    /// radius: each of the two panels draws its half of the round as one chord of its `front`.
    #[serde(default, skip_serializing_if = "Scalar::is_zero")]
    pub arris: Scalar,
    /// The tangent of half the plan turn at the panel's start and end (1 at a square corner);
    /// 0 leaves that end sharp.
    #[serde(default, skip_serializing_if = "Scalar::is_zero")]
    pub arris_start: Scalar,
    #[serde(default, skip_serializing_if = "Scalar::is_zero")]
    pub arris_end: Scalar,
    /// Start and end setbacks per meter of depth, measured from the front corners.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub miters: Option<[Scalar; 2]>,
}

/// A section swept round a plan arc. The part's origin is where the arc starts; it leaves
/// along +X, turns to the left (toward +Y) by `turn` degrees about an upright axis `radius`
/// away, in `segments` chords, and its outside faces -Y at the start, as a panel's front does.
/// `section` lists `[out, up]` points: meters outside the arc of `radius`, and height. A closed
/// section runs counterclockwise in (out, up); an open one keeps its visible side on its right
/// (an outer face is listed from its foot to its head).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArcDefinition {
    pub radius: Scalar,
    pub turn: Scalar,
    #[serde(default = "arc_segments")]
    pub segments: Scalar,
    pub section: Vec<[Scalar; 2]>,
    #[serde(default = "yes")]
    pub closed: bool,
    /// Rounds every corner of the section, set back this far along both of its edges, in
    /// `bevelSegments` (1 or 2) chords.
    #[serde(default, skip_serializing_if = "Scalar::is_zero")]
    pub bevel: Scalar,
    #[serde(default = "one", skip_serializing_if = "Scalar::is_one")]
    pub bevel_segments: Scalar,
    /// Section edges longer than this are split evenly (0: never), so a tall face is rows of
    /// faces no taller.
    #[serde(default, skip_serializing_if = "Scalar::is_zero")]
    pub step: Scalar,
    /// `[u, v]` of the pattern anchor: along the arc at `radius` and up.
    pub pattern_anchor: Option<[Scalar; 2]>,
    /// Named features that follow the arc at `[out, up]`, beside `base` and `top`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rings: Vec<RingDefinition>,
    /// Bands of an upright outer face set back into the section, where another part is laid.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recesses: Vec<ArcRecessDefinition>,
}
/// A recess of an arc's section: the part of the upright outer face (an edge of constant `out`
/// listed upward) between the heights `from` and `to`, set back `depth` into the section.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArcRecessDefinition {
    pub from: Scalar,
    pub to: Scalar,
    pub depth: Scalar,
    /// Copies along one axis; the index reads as a number.
    pub repeat: Option<RepeatAxis>,
    /// The recess (or each copy) is cut only when this is positive.
    pub when: Option<Scalar>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RingDefinition {
    pub name: String,
    pub at: [Scalar; 2],
}
fn arc_segments() -> Scalar {
    Scalar::Number(8.)
}
fn yes() -> bool {
    true
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OpeningShape {
    #[default]
    Rectangle,
    Arch,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OpeningMode {
    #[default]
    Through,
    Recess,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpeningDefinition {
    pub id: String,
    #[serde(default)]
    pub shape: OpeningShape,
    #[serde(default)]
    pub mode: OpeningMode,
    pub depth: Option<Scalar>,
    pub width: Scalar,
    pub height: Scalar,
    pub u: Rule,
    pub v: Rule,
    pub frame: Option<FrameDefinition>,
    pub glazing: Option<GlazingDefinition>,
    pub sill: Option<SillDefinition>,
    /// Rounds the two upright front arrises of a rectangular opening (its jambs: the edges of
    /// the piers beside it) with this radius, in two chords of the panel's `front`.
    #[serde(default, skip_serializing_if = "Scalar::is_zero")]
    pub jambs: Scalar,
    /// A recess cut for another part laid in it (tiles, a plinth): the wall goes on behind
    /// that part as if it were whole, so the recess reports no `opening-edge` or `sill`, stops
    /// no zone, and may come within 2 mm of the panel's edge and of other openings.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub seat: bool,
    /// Copies of the opening along one axis, named `<id>-<i>`; the index reads as a number.
    pub repeat: Option<RepeatAxis>,
    /// The opening (or each copy) is cut only when this is positive.
    pub when: Option<Scalar>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameDefinition {
    pub border: Scalar,
    pub depth: Scalar,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlazingDefinition {
    pub thickness: Scalar,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SillDefinition {
    pub depth: Scalar,
    #[serde(default)]
    pub overhang: Scalar,
    pub thickness: Option<Scalar>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum SectionDefinition {
    Round { radius: Scalar },
    Rect { width: Scalar, height: Scalar },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachmentReference {
    pub attachment: String,
}
fn bend_segments() -> Scalar {
    Scalar::Number(8.)
}
/// What a route's body and end caps are drawn as in place of its `section`. The route's
/// path, fittings, features, attachments and decals still follow `section`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrawnSectionDefinition {
    pub section: SectionDefinition,
    /// The drawn section's centre from the centreline: across the section, then along `up`.
    #[serde(default)]
    pub offset: [Scalar; 2],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RouteDefinition {
    pub section: SectionDefinition,
    /// A different section to draw the body with; absent draws `section`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drawn: Option<DrawnSectionDefinition>,
    #[serde(default)]
    pub wall_thickness: Scalar,
    pub points: RoutePoints,
    /// Grid of offsets from each straight leg's centerline to its mounting face.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports: Option<ParameterReference>,
    #[serde(default)]
    pub bend_radius: Scalar,
    /// Sides around a round section.
    #[serde(default = "segments")]
    pub segments: Scalar,
    /// Steps along each bend.
    #[serde(default = "bend_segments")]
    pub bend_segments: Scalar,
    /// Direction of a rectangular section's height; projected perpendicular to the first run.
    pub up: Option<[f64; 3]>,
    #[serde(default)]
    pub capped: Switch,
    #[serde(default)]
    pub fittings: Vec<FittingDefinition>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub omit_unfittable_repeats: bool,
    /// Starts the route at another part's attachment; `points` are then relative to it.
    pub from: Option<AttachmentReference>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RoutePoints {
    Inline(Vec<[Scalar; 3]>),
    Parameter(ParameterReference),
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Handle {
    #[default]
    Wheel,
    Lever,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum FittingDefinition {
    Coupling {
        id: String,
        at: Rule,
        length: Scalar,
        clearance: Scalar,
    },
    /// `radius` is the outer radius on a round section; `overhang` (how far the flange
    /// stands proud of the section) works on both. Give exactly one.
    Flange {
        id: String,
        at: Rule,
        radius: Option<Scalar>,
        overhang: Option<Scalar>,
        thickness: Scalar,
        #[serde(default)]
        bolts: Scalar,
    },
    Valve {
        id: String,
        at: Rule,
        length: Scalar,
        #[serde(default)]
        handle: Handle,
    },
    Bracket {
        id: String,
        at: Rule,
        reach: Scalar,
        direction: [f64; 3],
        width: Option<Scalar>,
        thickness: Option<Scalar>,
    },
    Tee {
        id: String,
        at: Rule,
        direction: [f64; 3],
        length: Scalar,
    },
}
impl FittingDefinition {
    pub fn id(&self) -> &str {
        match self {
            Self::Coupling { id, .. }
            | Self::Flange { id, .. }
            | Self::Valve { id, .. }
            | Self::Bracket { id, .. }
            | Self::Tee { id, .. } => id,
        }
    }
    pub fn at(&self) -> &Rule {
        match self {
            Self::Coupling { at, .. }
            | Self::Flange { at, .. }
            | Self::Valve { at, .. }
            | Self::Bracket { at, .. }
            | Self::Tee { at, .. } => at,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachmentDefinition {
    pub name: String,
    pub position: [Scalar; 3],
    pub direction: [f64; 3],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CollisionIntent {
    #[default]
    None,
    Bounds,
    Mesh,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum CollisionShape {
    Box { center: [f64; 3], size: [f64; 3] },
    Sphere { center: [f64; 3], radius: f64 },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Collision {
    Intent(CollisionIntent),
    Shape(CollisionShape),
}
impl Default for Collision {
    fn default() -> Self {
        Self::Intent(CollisionIntent::None)
    }
}

pub(super) fn bounded_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-./".contains(&c))
}

impl Parameter {
    /// A control's value as an expression reads it: a number, a boolean as 1 or 0, and a
    /// choice as its index among the control's choices.
    fn number(&self, value: &Value) -> Option<f64> {
        value
            .as_f64()
            .or_else(|| value.as_bool().map(f64::from))
            .or_else(|| {
                (self.kind == ParameterType::Choice)
                    .then(|| value.as_str())
                    .flatten()
                    .and_then(|text| self.choices.iter().position(|choice| choice == text))
                    .map(|index| index as f64)
            })
    }
    fn accepts(&self, value: &Value) -> bool {
        match self.kind {
            ParameterType::Number | ParameterType::Integer => {
                value.as_f64().is_some_and(|number| {
                    number.is_finite()
                        && (self.kind != ParameterType::Integer || number.fract() == 0.)
                        && self.min.is_none_or(|min| number >= min)
                        && self.max.is_none_or(|max| number <= max)
                })
            }
            ParameterType::Boolean => value.is_boolean(),
            ParameterType::Color => {
                value.as_str().is_some_and(|color| {
                    color.len() == 7
                        && color.starts_with('#')
                        && color[1..].bytes().all(|c| c.is_ascii_hexdigit())
                }) || value.as_array().is_some_and(|rgb| {
                    rgb.len() == 3
                        && rgb.iter().all(|v| {
                            v.as_f64()
                                .is_some_and(|v| v.is_finite() && (0. ..=1.).contains(&v))
                        })
                })
            }
            ParameterType::Choice => value
                .as_str()
                .is_some_and(|v| self.choices.iter().any(|choice| choice == v)),
            ParameterType::Points => value.as_array().is_some_and(|points| {
                (3..=64).contains(&points.len())
                    && points.iter().all(|point| {
                        point.as_array().is_some_and(|xy| {
                            xy.len() == 2
                                && xy.iter().all(|v| {
                                    v.as_f64().is_some_and(|v| v.is_finite() && v.abs() <= 1e5)
                                })
                        })
                    })
            }),
            ParameterType::Flags => value
                .as_array()
                .is_some_and(|flags| flags.len() <= 64 && flags.iter().all(Value::is_boolean)),
            ParameterType::Grid => value.is_array() && grid(value, &self.choices, 4, &mut 0),
            ParameterType::Text => value.as_str().is_some_and(|text| text.len() <= 256),
            ParameterType::Texts => value.as_array().is_some_and(|texts| {
                texts.len() <= 64
                    && texts
                        .iter()
                        .all(|t| t.as_str().is_some_and(|t| t.len() <= 256))
            }),
        }
    }
}

impl ItemDefinition {
    pub fn resolve_parameters(&self, overrides: &Values) -> Result<Values, ItemDiagnostic> {
        if self.format != "itemspec" || !(1..=2).contains(&self.version) {
            return Err(fail(
                &self.source,
                "/version",
                "ITEM_VERSION",
                "Expected itemspec version 1 or 2.",
            ));
        }
        if self.version == 1 {
            if !matches!(
                self.generator,
                Generator::Pipe { .. } | Generator::Duct { .. } | Generator::Glb { .. }
            ) {
                return Err(fail(
                    &self.source,
                    "/generator",
                    "ITEM_PART",
                    "A version 1 item has one pipe, duct or glb generator.",
                ));
            }
            if !self.parts.is_empty() || !self.derived.is_empty() {
                return Err(fail(
                    &self.source,
                    if self.parts.is_empty() {
                        "/derived"
                    } else {
                        "/parts"
                    },
                    "ITEM_VERSION",
                    "Parts and derived values need itemspec version 2.",
                ));
            }
        } else {
            if !self.generator.is_parts() {
                return Err(fail(
                    &self.source,
                    "/generator",
                    "ITEM_PART",
                    "A version 2 item lists its generators under parts.",
                ));
            }
            if self.parts.is_empty() || self.parts.len() > 64 {
                return Err(fail(
                    &self.source,
                    "/parts",
                    "ITEM_LIMIT",
                    "A version 2 item has from 1 to 64 parts.",
                ));
            }
            if self.derived.len() > 64 {
                return Err(fail(
                    &self.source,
                    "/derived",
                    "ITEM_LIMIT",
                    "An item has at most 64 derived values.",
                ));
            }
        }
        if !bounded_name(&self.id) {
            return Err(fail(
                &self.source,
                "/id",
                "ITEM_ID",
                "Item ID must be a bounded nonempty identifier.",
            ));
        }
        if self.parameters.len() > MAX_PARAMETERS
            || self.slots.len() > MAX_MATERIAL_SLOTS
            || self.attachments.len() > MAX_ATTACHMENTS
        {
            return Err(fail(
                &self.source,
                "/",
                "ITEM_LIMIT",
                "Item controls, slots or attachments exceed their limits.",
            ));
        }
        for name in overrides.keys() {
            if !self.parameters.contains_key(name) {
                return Err(fail(
                    &self.source,
                    &format!("/parameters/{}", super::pointer_key(name)),
                    "ITEM_PARAMETER",
                    "Unknown parameter override.",
                ));
            }
        }
        let mut values = Values::new();
        for (name, parameter) in &self.parameters {
            let pointer = format!("/parameters/{}", super::pointer_key(name));
            if !bounded_name(name)
                || parameter.unit.as_ref().is_some_and(|unit| unit.len() > 32)
                || parameter.choices.len() > 64
                || parameter.choices.iter().any(|v| v.len() > 128)
                || parameter.min.is_some_and(|v| !v.is_finite())
                || parameter.max.is_some_and(|v| !v.is_finite())
                || parameter.min.zip(parameter.max).is_some_and(|(a, b)| a > b)
                || !parameter.accepts(&parameter.default)
            {
                return Err(fail(
                    &self.source,
                    &pointer,
                    "ITEM_PARAMETER",
                    "Invalid parameter declaration or default.",
                ));
            }
            let value = overrides.get(name).unwrap_or(&parameter.default);
            if !parameter.accepts(value) {
                return Err(fail(
                    &self.source,
                    &pointer,
                    "ITEM_PARAMETER",
                    "Parameter value violates its type, range or choices.",
                ));
            }
            values.insert(name.clone(), value.clone());
        }
        Matrix(self.transform)
            .checked()
            .and_then(Matrix::inverse)
            .map_err(|e| fail(&self.source, "/transform", "ITEM_TRANSFORM", e))?;
        if self.transform.iter().flatten().any(|v| v.abs() > 1e7) {
            return Err(fail(
                &self.source,
                "/transform",
                "ITEM_TRANSFORM",
                "Transform exceeds item coordinate limits.",
            ));
        }
        if self.lods.len() > MAX_ITEM_LOD_ALTERNATIVES {
            return Err(fail(
                &self.source,
                "/lods",
                "ITEM_LOD",
                "An item has at most seven render alternatives.",
            ));
        }
        let mut previous_distance = 0.;
        for (index, level) in self.lods.iter().enumerate() {
            let pointer = format!("/lods/{index}");
            if !level.distance.is_finite()
                || level.distance <= previous_distance
                || level.distance > 1e6
            {
                return Err(fail(
                    &self.source,
                    &format!("{pointer}/distance"),
                    "ITEM_LOD",
                    "LOD distances must be finite, positive and strictly increasing, at most 1000000 meters.",
                ));
            }
            previous_distance = level.distance;
            if level.parameters.is_empty() || level.parameters.len() > 64 {
                return Err(fail(
                    &self.source,
                    &format!("{pointer}/parameters"),
                    "ITEM_LOD",
                    "A render alternative overrides from one to 64 geometry controls.",
                ));
            }
            for (name, value) in &level.parameters {
                if !self.parameters.get(name).is_some_and(|parameter| {
                    parameter.role == Role::Geometry && parameter.accepts(value)
                }) {
                    return Err(fail(
                        &self.source,
                        &format!("{pointer}/parameters/{}", super::pointer_key(name)),
                        "ITEM_LOD",
                        "A render alternative must name a geometry control and satisfy its declared type, range and choices.",
                    ));
                }
            }
        }
        match &self.collision {
            Collision::Shape(CollisionShape::Box { center, size })
                if center
                    .iter()
                    .chain(size)
                    .any(|v| !v.is_finite() || v.abs() > 1e7)
                    || size.iter().any(|v| *v <= 0.) =>
            {
                return Err(fail(
                    &self.source,
                    "/collision",
                    "ITEM_COLLISION",
                    "Box collision needs finite center and positive finite size.",
                ));
            }
            Collision::Shape(CollisionShape::Sphere { center, radius })
                if center.iter().any(|v| !v.is_finite() || v.abs() > 1e7)
                    || !radius.is_finite()
                    || *radius <= 0.
                    || *radius > 1e7 =>
            {
                return Err(fail(
                    &self.source,
                    "/collision",
                    "ITEM_COLLISION",
                    "Sphere collision needs a finite center and positive finite radius.",
                ));
            }
            _ => {}
        }
        Ok(values)
    }

    /// The source and effective controls for one render-only alternative. Collision,
    /// attachments and editor handles of the canonical item are never replaced by it.
    pub fn render_alternative(
        &self,
        level: &ItemLodDefinition,
        overrides: &Values,
    ) -> (Self, Values) {
        let mut definition = self.clone();
        definition.lods.clear();
        definition.collision = Collision::default();
        let mut parameters = overrides.clone();
        parameters.extend(level.parameters.clone());
        (definition, parameters)
    }

    pub(super) fn parameter<'a>(
        &self,
        name: &str,
        values: &'a Values,
        pointer: &str,
        geometry: bool,
    ) -> Result<&'a Value, ItemDiagnostic> {
        let declaration = self.parameters.get(name).ok_or_else(|| {
            fail(
                &self.source,
                pointer,
                "ITEM_PARAMETER",
                format!("Unknown parameter {name}."),
            )
        })?;
        if geometry && declaration.role != Role::Geometry {
            return Err(fail(
                &self.source,
                pointer,
                "ITEM_PARAMETER",
                "Geometry cannot depend on a finish control.",
            ));
        }
        values.get(name).ok_or_else(|| {
            fail(
                &self.source,
                pointer,
                "ITEM_PARAMETER",
                "Missing resolved parameter.",
            )
        })
    }
    /// Evaluates every derived value. They follow the geometry rule: an expression reads
    /// numeric geometry controls and other derived values, never finish controls.
    pub fn resolve_derived(&self, values: &Values) -> Result<Derived, ItemDiagnostic> {
        let pointer = |name: &str| format!("/derived/{}", super::pointer_key(name));
        let failure = |name: &str, error: ExpressionError| {
            fail(
                &self.source,
                &pointer(name),
                "ITEM_EXPRESSION",
                format!("{} (character {})", error.message, error.offset + 1),
            )
        };
        let mut parsed = BTreeMap::new();
        for (name, text) in &self.derived {
            let identifier = !name.is_empty()
                && name.len() <= 128
                && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
                && !name.as_bytes()[0].is_ascii_digit();
            if !identifier || self.parameters.contains_key(name) {
                return Err(fail(
                    &self.source,
                    &pointer(name),
                    "ITEM_EXPRESSION",
                    "A derived name uses letters, digits and underscores and differs from every parameter.",
                ));
            }
            parsed.insert(
                name.as_str(),
                Expression::parse(text).map_err(|error| failure(name, error))?,
            );
        }
        struct Evaluation<'a> {
            definition: &'a ItemDefinition,
            values: &'a Values,
            parsed: BTreeMap<&'a str, Expression>,
            // `None` marks a value whose evaluation is in progress.
            done: BTreeMap<&'a str, Option<f64>>,
        }
        impl<'a> Evaluation<'a> {
            fn value(&mut self, name: &'a str) -> Result<f64, (String, ExpressionError)> {
                let expression = self.parsed[name].clone();
                self.done.insert(name, None);
                let names: Vec<(String, usize)> = expression
                    .names()
                    .into_iter()
                    .map(|(n, at)| (n.to_owned(), at))
                    .collect();
                let mut resolved = BTreeMap::new();
                for (used, at) in names {
                    let error = |message: String| {
                        (
                            name.to_owned(),
                            ExpressionError {
                                offset: at,
                                message,
                            },
                        )
                    };
                    let number = if let Some((&key, _)) = self.parsed.get_key_value(used.as_str()) {
                        match self.done.get(key) {
                            Some(Some(value)) => *value,
                            Some(None) => {
                                return Err(error(format!(
                                    "Derived value {used} depends on itself."
                                )));
                            }
                            None => self.value(key)?,
                        }
                    } else if let Some(parameter) = self.definition.parameters.get(&used) {
                        if parameter.role != Role::Geometry {
                            return Err(error(format!(
                                "Derived values cannot read the finish control {used}."
                            )));
                        }
                        // A boolean control reads as 1 or 0, as in inline expressions.
                        self.values
                            .get(&used)
                            .and_then(|value| parameter.number(value))
                            .ok_or_else(|| error(format!("Parameter {used} is not numeric.")))?
                    } else {
                        return Err(error(format!("Unknown name {used}.")));
                    };
                    resolved.insert(used, number);
                }
                let result = expression.evaluate(&mut |used, at| {
                    if used.contains('[') || used.starts_with('#') {
                        return self
                            .definition
                            .grid_value(used, self.values)
                            .map_err(|message| ExpressionError {
                                offset: at,
                                message,
                            });
                    }
                    resolved.get(used).copied().ok_or_else(|| ExpressionError {
                        offset: at,
                        message: format!("Unknown name {used}."),
                    })
                });
                match result {
                    Ok(value) => {
                        self.done.insert(name, Some(value));
                        Ok(value)
                    }
                    Err(error) => Err((name.to_owned(), error)),
                }
            }
        }
        let mut evaluation = Evaluation {
            definition: self,
            values,
            parsed,
            done: BTreeMap::new(),
        };
        let names: Vec<&str> = evaluation.parsed.keys().copied().collect();
        for name in names {
            if !evaluation.done.contains_key(name) {
                evaluation
                    .value(name)
                    .map_err(|(owner, error)| failure(&owner, error))?;
            }
        }
        Ok(evaluation
            .done
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value.unwrap_or(0.)))
            .collect())
    }

    pub(super) fn scalar(
        &self,
        expression: &Scalar,
        values: &Values,
        derived: &Derived,
        pointer: &str,
    ) -> Result<f64, ItemDiagnostic> {
        let number = match expression {
            Scalar::Number(number) => *number,
            Scalar::Derived(reference) => *derived.get(&reference.derived).ok_or_else(|| {
                fail(
                    &self.source,
                    pointer,
                    "ITEM_EXPRESSION",
                    format!("Unknown derived value {}.", reference.derived),
                )
            })?,
            Scalar::Expression(text) => self.expression(text, values, derived, pointer)?,
            Scalar::Parameter(reference) => self
                .parameter(&reference.parameter, values, pointer, true)?
                .as_f64()
                .ok_or_else(|| {
                    fail(
                        &self.source,
                        pointer,
                        "ITEM_PARAMETER",
                        "Geometry requires a numeric control.",
                    )
                })?,
        };
        if !number.is_finite() {
            return Err(fail(
                &self.source,
                pointer,
                "ITEM_PARAMETER",
                "Geometry values must be finite.",
            ));
        }
        Ok(number)
    }
    /// Evaluates inline expression text. Names are derived values (including copy indices)
    /// first, then numeric or boolean geometry controls (a boolean reads as 1 or 0).
    pub(super) fn expression(
        &self,
        text: &str,
        values: &Values,
        derived: &Derived,
        pointer: &str,
    ) -> Result<f64, ItemDiagnostic> {
        let failure = |error: ExpressionError| {
            fail(
                &self.source,
                pointer,
                "ITEM_EXPRESSION",
                format!("{} (character {})", error.message, error.offset + 1),
            )
        };
        let expression = Expression::parse(text).map_err(failure)?;
        expression
            .evaluate(&mut |name, at| {
                let missing = |message: String| ExpressionError {
                    offset: at,
                    message,
                };
                if let Some(value) = derived.get(name) {
                    return Ok(*value);
                }
                // `cell(grid, i, ...)` arrives as `grid[i]...` and `count(grid, i, ...)` as `#grid[i]...`.
                if name.contains('[') || name.starts_with('#') {
                    return self.grid_value(name, values).map_err(missing);
                }
                let parameter = self
                    .parameters
                    .get(name)
                    .ok_or_else(|| missing(format!("Unknown name {name}.")))?;
                if parameter.role != Role::Geometry {
                    return Err(missing(format!(
                        "Geometry cannot read the finish control {name}."
                    )));
                }
                let value = values
                    .get(name)
                    .ok_or_else(|| missing(format!("Missing parameter {name}.")))?;
                parameter
                    .number(value)
                    .ok_or_else(|| missing(format!("Parameter {name} is not numeric.")))
            })
            .map_err(failure)
    }
    /// The value of a grid lookup key, `grid[i][j]` for an element (a text's index among the
    /// control's choices, a number's value, 0 where there is none) or `#grid[i]` for the length of
    /// the list there.
    pub(super) fn grid_value(&self, key: &str, values: &Values) -> Result<f64, String> {
        let (counting, key) = match key.strip_prefix('#') {
            Some(rest) => (true, rest),
            None => (false, key),
        };
        let (grid, rest) = key.split_once('[').unwrap_or((key, ""));
        let parameter = self
            .parameters
            .get(grid)
            .filter(|parameter| parameter.kind == ParameterType::Grid)
            .ok_or_else(|| format!("{grid} is not a grid control."))?;
        let mut element = values.get(grid);
        for index in rest.split("][").filter(|index| !index.is_empty()) {
            element = index
                .trim_end_matches(']')
                .parse::<usize>()
                .ok()
                .and_then(|index| element?.as_array()?.get(index));
        }
        Ok(match element {
            Some(Value::Array(items)) if counting => items.len() as f64,
            Some(Value::Number(number)) if !counting => number.as_f64().unwrap_or(0.),
            Some(Value::String(text)) if !counting => parameter
                .choices
                .iter()
                .position(|choice| choice == text)
                .map_or(0., |index| index as f64),
            _ => 0.,
        })
    }
    pub(super) fn switch(
        &self,
        expression: &Switch,
        values: &Values,
        pointer: &str,
    ) -> Result<bool, ItemDiagnostic> {
        match expression {
            Switch::Boolean(value) => Ok(*value),
            Switch::Parameter(reference) => self
                .parameter(&reference.parameter, values, pointer, true)?
                .as_bool()
                .ok_or_else(|| {
                    fail(
                        &self.source,
                        pointer,
                        "ITEM_PARAMETER",
                        "Geometry requires a boolean control.",
                    )
                }),
        }
    }
    pub(super) fn materials(
        &self,
        values: &Values,
    ) -> Result<BTreeMap<String, MaterialRequest>, ItemDiagnostic> {
        let mut result = BTreeMap::new();
        for (slot, request) in &self.slots {
            let pointer = format!("/slots/{}", super::pointer_key(slot));
            if !bounded_name(slot)
                || request.entry.is_empty()
                || request.entry.len() > 1024
                || request.parameters.len() > 64
            {
                return Err(fail(
                    &self.source,
                    &pointer,
                    "ITEM_MATERIAL",
                    "Invalid material slot or entry.",
                ));
            }
            let mut request = request.clone();
            for (name, value) in &mut request.parameters {
                if let Some(reference) = value
                    .as_object()
                    .filter(|object| object.len() == 1)
                    .and_then(|object| object.get("parameter"))
                    .and_then(Value::as_str)
                {
                    let resolved = self.parameter(
                        reference,
                        values,
                        &format!("{pointer}/parameters/{}", super::pointer_key(name)),
                        false,
                    )?;
                    *value = if self.parameters[reference].kind == ParameterType::Color {
                        resolved
                            .as_str()
                            .map(|hex| serde_json::json!(superspec_types::color::srgb(hex)))
                            .unwrap_or_else(|| resolved.clone())
                    } else {
                        resolved.clone()
                    };
                }
                if serde_json::to_vec(value).map_or(true, |bytes| bytes.len() > 4096) {
                    return Err(fail(
                        &self.source,
                        &pointer,
                        "ITEM_LIMIT",
                        "Material parameter exceeds its size limit.",
                    ));
                }
            }
            result.insert(slot.clone(), request);
        }
        Ok(result)
    }
    /// The texel density each slot declares, in texels per meter.
    pub(super) fn slot_densities(&self) -> Result<BTreeMap<String, f64>, ItemDiagnostic> {
        if self.densities.len() > 128 {
            return Err(fail(
                &self.source,
                "/densities",
                "ITEM_LIMIT",
                "An item states at most 128 slot densities.",
            ));
        }
        for (slot, density) in &self.densities {
            if !bounded_name(slot) || !(MIN_DENSITY..=MAX_DENSITY).contains(density) {
                return Err(fail(
                    &self.source,
                    &format!("/densities/{}", super::pointer_key(slot)),
                    "ITEM_DENSITY",
                    "A slot density is 1 to 100000 texels per meter for a bounded slot name.",
                ));
            }
        }
        Ok(self.densities.clone())
    }
    /// The weathering layers of every slot, with `{"parameter": name}` values bound to the
    /// item's controls (any role: a layer is appearance, never geometry).
    pub(super) fn resolved_layers(
        &self,
        values: &Values,
    ) -> Result<BTreeMap<String, Vec<LayerRequest>>, ItemDiagnostic> {
        let mut result = BTreeMap::new();
        for (slot, layers) in &self.layers {
            let pointer = format!("/layers/{}", super::pointer_key(slot));
            if !bounded_name(slot) || layers.len() > MAX_LAYERS_PER_SLOT {
                return Err(fail(
                    &self.source,
                    &pointer,
                    "ITEM_LAYER",
                    "A slot has at most 16 layers.",
                ));
            }
            let mut resolved = Vec::with_capacity(layers.len());
            for (index, layer) in layers.iter().enumerate() {
                let pointer = format!("{pointer}/{index}");
                if layer.entry.is_empty()
                    || layer.entry.len() > 1024
                    || layer.parameters.len() > 64
                    || layer.follow.len() > 16
                    || layer.follow.iter().any(|(own, theirs)| {
                        !bounded_name(own)
                            || theirs.names().is_empty()
                            || theirs.names().len() > MAX_FOLLOWED
                            || theirs.names().iter().any(|name| !bounded_name(name))
                    })
                {
                    return Err(fail(
                        &self.source,
                        &pointer,
                        "ITEM_LAYER",
                        "Invalid layer entry, parameters or followed features (a name follows one feature or a list of 1 to 16).",
                    ));
                }
                let mut layer = layer.clone();
                // One spelling per set of followed features, so equal layers of sub-items merge.
                for theirs in layer.follow.values_mut() {
                    *theirs = theirs.normalized();
                }
                for (name, value) in &mut layer.parameters {
                    if let Some(reference) = value
                        .as_object()
                        .filter(|object| object.len() == 1)
                        .and_then(|object| object.get("parameter"))
                        .and_then(Value::as_str)
                    {
                        let at = format!("{pointer}/parameters/{}", super::pointer_key(name));
                        let resolved = self.parameter(reference, values, &at, false)?;
                        // A color control's hex text becomes linear RGB, as for materials.
                        *value = match (self.parameters[reference].kind, resolved.as_str()) {
                            (ParameterType::Color, Some(hex)) => {
                                serde_json::json!(superspec_types::color::srgb(hex))
                            }
                            _ => resolved.clone(),
                        };
                    }
                }
                resolved.push(layer);
            }
            result.insert(slot.clone(), resolved);
        }
        Ok(result)
    }
}
