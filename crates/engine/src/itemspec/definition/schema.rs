use super::{
    ItemDiagnostic,
    expression::{Expression, ExpressionError},
    fail,
};
use crate::materialspec::program::MaterialRequest;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use superspec_geometry::Matrix;

pub type Values = BTreeMap<String, Value>;
/// Evaluated derived values by name.
pub type Derived = BTreeMap<String, f64>;
pub type Transform = [[f64; 3]; 4];
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
    #[serde(skip)]
    pub source: String,
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
}
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
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub follow: BTreeMap<String, String>,
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
    /// Nested lists, at most three deep and 4096 entries in all, of numbers or (with `choices`)
    /// texts among the choices: an explicit plan, such as what each bay of each storey holds.
    /// Expressions read it with `cell(grid, i, j, ...)`.
    Grid,
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
fn segments() -> Scalar {
    Scalar::Number(16.)
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
    /// Version 2 part generator. The box spans from the part origin to `size`.
    Box {
        size: [Scalar; 3],
    },
    /// Version 2 part generator.
    Panel(PanelDefinition),
    /// Version 2 part generator.
    Route(RouteDefinition),
    /// Version 2 part generator: a footprint swept upward, with its street corners cut and
    /// an optional inset. Face groups `side`, `top`, `bottom`.
    Prism {
        footprint: FootprintReference,
        height: Scalar,
        #[serde(default)]
        inset: Scalar,
    },
    /// Version 2 part generator: another item definition, placed as a sub-item.
    Item(ItemReference),
    /// Version 2 part generator: a generator supplied by the host (for example a theme's
    /// fixtures), named and given parameters.
    Native(NativeReference),
    /// The value of an absent root `generator`: the item is defined by `parts`.
    #[default]
    Parts,
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
}

/// A footprint read from controls: `points` names a `points` control, `streets` an optional
/// `flags` control (one per edge, edge i running from point i to point i + 1), and `cut` the
/// length a corner between two street edges is cut back along each, at 45 degrees on a square
/// corner. Only convex corners are cut.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FootprintReference {
    pub points: String,
    pub streets: Option<String>,
    #[serde(default)]
    pub cut: Scalar,
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
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RouteDefinition {
    pub section: SectionDefinition,
    #[serde(default)]
    pub wall_thickness: Scalar,
    pub points: Vec<[Scalar; 3]>,
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
    /// Starts the route at another part's attachment; `points` are then relative to it.
    pub from: Option<AttachmentReference>,
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
            ParameterType::Grid => value.is_array() && grid(value, &self.choices, 3, &mut 0),
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
        if self.parameters.len() > 64 || self.slots.len() > 128 || self.attachments.len() > 256 {
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
                            .and_then(|value| {
                                value.as_f64().or_else(|| value.as_bool().map(f64::from))
                            })
                            .ok_or_else(|| error(format!("Parameter {used} is not numeric.")))?
                    } else {
                        return Err(error(format!("Unknown name {used}.")));
                    };
                    resolved.insert(used, number);
                }
                let result = expression.evaluate(&mut |used, at| {
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
                // `cell(grid, i, ...)` arrives as `grid[i]...`: its element, or 0 where it has none.
                if let Some((grid, rest)) = name.split_once('[') {
                    let parameter = self
                        .parameters
                        .get(grid)
                        .filter(|parameter| parameter.kind == ParameterType::Grid)
                        .ok_or_else(|| missing(format!("{grid} is not a grid control.")))?;
                    let mut element = values.get(grid);
                    for index in rest.trim_end_matches(']').split("][") {
                        element = index
                            .parse::<usize>()
                            .ok()
                            .and_then(|index| element?.as_array()?.get(index));
                    }
                    return Ok(match element {
                        Some(Value::Number(number)) => number.as_f64().unwrap_or(0.),
                        Some(Value::String(text)) => parameter
                            .choices
                            .iter()
                            .position(|choice| choice == text)
                            .map_or(0., |index| index as f64),
                        _ => 0.,
                    });
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
                value
                    .as_f64()
                    .or_else(|| value.as_bool().map(f64::from))
                    .ok_or_else(|| missing(format!("Parameter {name} is not numeric.")))
            })
            .map_err(failure)
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
    /// The weathering layers of every slot, with `{"parameter": name}` values bound to the
    /// item's controls (any role: a layer is appearance, never geometry).
    pub(super) fn resolved_layers(
        &self,
        values: &Values,
    ) -> Result<BTreeMap<String, Vec<LayerRequest>>, ItemDiagnostic> {
        let mut result = BTreeMap::new();
        for (slot, layers) in &self.layers {
            let pointer = format!("/layers/{}", super::pointer_key(slot));
            if !bounded_name(slot) || layers.len() > 16 {
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
                    || layer
                        .follow
                        .iter()
                        .any(|(a, b)| !bounded_name(a) || !bounded_name(b))
                {
                    return Err(fail(
                        &self.source,
                        &pointer,
                        "ITEM_LAYER",
                        "Invalid layer entry, parameters or followed features.",
                    ));
                }
                let mut layer = layer.clone();
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
