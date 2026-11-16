//! Versioned operation catalog: the single description of every MaterialSpec operation.
//!
//! The checker resolves calls, constant folding and input dependencies through this
//! table. Operator typing is still written in the checker's expression rules; the tests
//! verify the operator table against it in both directions. SDK declarations, graph ports and the AI reference are generated from it.
//! Evaluation semantics stay in the evaluator; `tests` keep both in agreement.
use super::program::Type;
use serde::Serialize;
use std::sync::OnceLock;

/// Version of the operation catalog as a whole. Bump when an operation changes meaning.
pub const CATALOG_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    /// Shared math, coordinates, sampling, masks and channel operations.
    Primitive,
    /// An editable group that expands to primitives and exposes named fields.
    Group,
    /// A live operation consumed by the renderer instead of the sample evaluator.
    Renderer,
}

mod library;
pub mod sdk;
pub use library::{
    Group, GroupArgument, GroupOutput, Operator, RendererOperation, find_group, find_operator,
    find_renderer_operation, groups, operators, renderer_operations,
};

/// Module specifier that authored recipes import the SDK from.
pub const SDK_MODULE: &str = "@superspec/material/v1";

/// Coordinate spaces the SDK declarations track on vectors. "none" marks literals, parameters and
/// results with no space; "direction" (normals, gravity) combines with any position space.
/// Only TypeScript checks them; the compiler ignores spaces.
pub const SPACES: [&str; 6] = ["none", "uv", "surface", "object", "world", "direction"];

/// A host-supplied sample input visible to every recipe.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Input {
    pub ts_name: &'static str,
    pub id: &'static str,
    #[serde(rename = "type")]
    pub kind: Type,
    pub summary: &'static str,
}

pub fn inputs() -> &'static [Input] {
    const fn input(
        ts_name: &'static str,
        id: &'static str,
        kind: Type,
        summary: &'static str,
    ) -> Input {
        Input {
            ts_name,
            id,
            kind,
            summary,
        }
    }
    static INPUTS: [Input; 18] = [
        input(
            "uv",
            "uv",
            Type::Vec2,
            "Dimensionless UV coordinates; a unit interval spans one image repeat.",
        ),
        input(
            "surface",
            "surface",
            Type::Vec2,
            "Mapped surface coordinates in meters.",
        ),
        input(
            "object",
            "object",
            Type::Vec3,
            "Object-space position in meters.",
        ),
        input(
            "world",
            "world",
            Type::Vec3,
            "World-space position in meters, right-handed and Z up.",
        ),
        input(
            "normal",
            "normal",
            Type::Vec3,
            "Receiver normal in world space.",
        ),
        input(
            "gravity",
            "gravity",
            Type::Vec3,
            "World-space gravity direction; defaults to down along Z.",
        ),
        input(
            "footprint",
            "footprint",
            Type::Scalar,
            "Sample diameter in meters.",
        ),
        input(
            "uvFootprint",
            "uv_footprint",
            Type::Scalar,
            "Dimensionless UV sample diameter.",
        ),
        input(
            "boundsMin",
            "bounds_min",
            Type::Vec3,
            "Minimum corner of the receiver's world-space bounding box in meters.",
        ),
        input(
            "boundsMax",
            "bounds_max",
            Type::Vec3,
            "Maximum corner of the receiver's world-space bounding box in meters.",
        ),
        input(
            "time",
            "time",
            Type::Scalar,
            "Render-only: seconds of the renderer's clock (uniform msTime). Only the shader reads it; a bake of a value that reads it fails, naming it.",
        ),
        input(
            "instance",
            "instance",
            Type::Vec3,
            "Render-only: the first three components of the drawn instance's value (varying msInstanceValue). Only the shader reads it; a bake of a value that reads it fails.",
        ),
        input(
            "instanceW",
            "instance_w",
            Type::Scalar,
            "Render-only: the fourth component of the drawn instance's value. Only the shader reads it.",
        ),
        input(
            "sunDirection",
            "sun_direction",
            Type::Vec3,
            "Render-only: the unit vector from the scene toward the sun, from the scene's lighting record (uniform msSunDirection). Only the shader reads it; a bake of a value that reads it fails, naming it.",
        ),
        input(
            "instanceRoot",
            "instance_root",
            Type::Vec3,
            "Render-only: the world position of the drawn instance's origin, in meters (the mesh's origin outside an instanced batch).",
        ),
        input(
            "instanceTint",
            "instance_tint",
            Type::Vec3,
            "Render-only: the drawn instance's tint, linear RGB (white without one).",
        ),
        input(
            "instanceRandom",
            "instance_random",
            Type::Scalar,
            "Render-only: a stable number in [0, 1) per drawn instance (its `random` attribute, or one derived from its index; 0 outside a batch).",
        ),
        input(
            "placedWorld",
            "placed_world",
            Type::Vec3,
            "Render-only: the world position of the drawn point through its instance's placement, in meters; `world` is the batch's own space, where its one bake is made.",
        ),
    ];
    &INPUTS
}

/// The identifier a recipe uses for a MaterialSpec name: `sample_color` becomes `sampleColor`.
pub fn ts_name(id: &str) -> String {
    let mut out = String::with_capacity(id.len());
    let mut upper = false;
    for character in id.chars() {
        if character == '_' {
            upper = true;
        } else if upper {
            out.push(character.to_ascii_uppercase());
            upper = false;
        } else {
            out.push(character);
        }
    }
    out
}

/// The MaterialSpec name for a recipe identifier: `sampleColor` becomes `sample_color`.
pub fn ms_name(ts: &str) -> String {
    let mut out = String::with_capacity(ts.len() + 2);
    for character in ts.chars() {
        if character.is_ascii_uppercase() {
            out.push('_');
            out.push(character.to_ascii_lowercase());
        } else {
            out.push(character);
        }
    }
    out
}

/// Every name a recipe may import from the SDK module.
pub fn sdk_exports() -> Vec<String> {
    let mut names: Vec<String> = [
        "defineMaterial",
        "param",
        "layer",
        "Scalar",
        "ScalarInput",
        "Vec2",
        "Vec3",
        "Space",
        "Bool",
        "BoolInput",
        "Layer",
        "LayerOptions",
        "BaseChannels",
        "HeightEdit",
        "LiveEffect",
        "MaterialOptions",
        "MaterialDefinition",
        "defineDecal",
        "DecalOptions",
        "DecalDefinition",
        "defineLayer",
        "WeatheringOptions",
        "WeatheringDefinition",
        "BlendRule",
        "below",
    ]
    .iter()
    .map(|name| (*name).to_owned())
    .collect();
    names.extend(operations().iter().map(|op| ts_name(op.id)));
    names.extend(operators().iter().map(|op| op.id.to_owned()));
    names.extend(groups().iter().map(|group| group.id.to_owned()));
    // The declarations also export each group's result type.
    names.extend(groups().iter().map(|group| sdk::pascal(group.id) + "Group"));
    names.extend(renderer_operations().iter().map(|op| op.id.to_owned()));
    names.extend(inputs().iter().map(|input| input.ts_name.to_owned()));
    names
}

/// One concrete typing of an operation. Broadcast and splat rules are enumerated here
/// so declaration generators and the checker read the same data.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Overload {
    pub args: Vec<Type>,
    pub result: Type,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    pub id: &'static str,
    pub version: u32,
    pub kind: Kind,
    pub family: &'static str,
    pub summary: &'static str,
    pub example: &'static str,
    pub overloads: Vec<Overload>,
    /// May appear in a parameter default or other constant expression.
    pub constant: bool,
    /// The first argument must be a nonempty literal string of at most 1,024 bytes.
    pub literal_first: bool,
    /// Sample inputs and host inputs the operation consumes, by metadata name.
    pub requires: &'static [&'static str],
    /// The literal first argument names an image the host must supply.
    pub image_input: bool,
    /// When nonempty, the literal first argument must be one of these words.
    pub literal_values: &'static [&'static str],
}

use Type::{Bool, Scalar, String as Text, Vec2, Vec3};

const NUMERIC: [Type; 3] = [Scalar, Vec2, Vec3];
const VECTORS: [Type; 2] = [Vec2, Vec3];

fn is_numeric(kind: Type) -> bool {
    NUMERIC.contains(&kind)
}

fn broadcast(a: Type, b: Type) -> Option<Type> {
    if is_numeric(a) && is_numeric(b) {
        if a == b || b == Scalar {
            Some(a)
        } else if a == Scalar {
            Some(b)
        } else {
            None
        }
    } else {
        None
    }
}

fn overload(args: &[Type], result: Type) -> Overload {
    Overload {
        args: args.to_vec(),
        result,
    }
}

fn each(kinds: &[Type], build: impl Fn(Type) -> Overload) -> Vec<Overload> {
    kinds.iter().copied().map(build).collect()
}

fn operation(
    id: &'static str,
    family: &'static str,
    summary: &'static str,
    example: &'static str,
    overloads: Vec<Overload>,
) -> Operation {
    Operation {
        id,
        version: 1,
        kind: Kind::Primitive,
        family,
        summary,
        example,
        overloads,
        constant: false,
        literal_first: false,
        requires: &[],
        image_input: false,
        literal_values: &[],
    }
}

fn constant(mut op: Operation) -> Operation {
    op.constant = true;
    op
}

fn literal(mut op: Operation, requires: &'static [&'static str], image: bool) -> Operation {
    op.literal_first = true;
    op.requires = requires;
    op.image_input = image;
    op
}

fn build() -> Vec<Operation> {
    let numeric_pairs: Vec<Overload> = NUMERIC
        .iter()
        .flat_map(|&a| {
            NUMERIC
                .iter()
                .filter_map(move |&b| broadcast(a, b).map(|r| overload(&[a, b], r)))
        })
        .collect();
    let spaced = |op: &'static str, summary: &'static str, example: &'static str| {
        operation(
            op,
            "coordinates",
            summary,
            example,
            VECTORS
                .iter()
                .flat_map(|&a| [Scalar, a].into_iter().map(move |b| overload(&[a, b], a)))
                .collect(),
        )
    };
    let image = |id: &'static str, summary: &'static str, result: Type, example: &'static str| {
        literal(
            operation(
                id,
                "image",
                summary,
                example,
                vec![overload(&[Text, Vec2], result)],
            ),
            &[],
            true,
        )
    };
    let componentwise = |id: &'static str, summary: &'static str, example: &'static str| {
        constant(operation(
            id,
            "math",
            summary,
            example,
            each(&NUMERIC, |a| overload(&[a], a)),
        ))
    };
    vec![
        constant(operation(
            "vec2",
            "vector",
            "Splat, construct, or pass through a two-component vector.",
            "vec2(0.5, 1)",
            vec![
                overload(&[Scalar], Vec2),
                overload(&[Scalar, Scalar], Vec2),
                overload(&[Vec2], Vec2),
            ],
        )),
        constant(operation(
            "vec3",
            "vector",
            "Splat, construct, append a component, or pass through a three-component vector.",
            "vec3(uv, 0)",
            vec![
                overload(&[Scalar], Vec3),
                overload(&[Scalar, Scalar, Scalar], Vec3),
                overload(&[Vec2, Scalar], Vec3),
                overload(&[Vec3], Vec3),
            ],
        )),
        componentwise("abs", "Componentwise absolute value.", "abs(x)"),
        componentwise("floor", "Componentwise round down.", "floor(x)"),
        componentwise(
            "fract",
            "Componentwise fractional part `x - floor(x)`.",
            "fract(x)",
        ),
        componentwise("sin", "Componentwise sine in radians.", "sin(x)"),
        componentwise("cos", "Componentwise cosine in radians.", "cos(x)"),
        componentwise(
            "sqrt",
            "Componentwise square root; negative inputs fail.",
            "sqrt(x)",
        ),
        constant(operation(
            "min",
            "math",
            "Componentwise minimum with scalar broadcast.",
            "min(a, 1)",
            numeric_pairs.clone(),
        )),
        constant(operation(
            "max",
            "math",
            "Componentwise maximum with scalar broadcast.",
            "max(a, 0)",
            numeric_pairs.clone(),
        )),
        operation(
            "length",
            "vector",
            "Euclidean length.",
            "length(surface)",
            each(&VECTORS, |a| overload(&[a], Scalar)),
        ),
        operation(
            "normalize",
            "vector",
            "Unit direction; a length below 1e-12 fails.",
            "normalize(gravity)",
            each(&VECTORS, |a| overload(&[a], a)),
        ),
        operation(
            "dot",
            "vector",
            "Dot product of equal-dimension vectors.",
            "dot(normal, gravity)",
            each(&VECTORS, |a| overload(&[a, a], Scalar)),
        ),
        constant(operation(
            "mix",
            "mix",
            "Linear interpolation; `t` outside [0, 1] extrapolates.",
            "mix(a, b, 0.5)",
            NUMERIC
                .iter()
                .flat_map(|&a| {
                    [Scalar, a]
                        .into_iter()
                        .map(move |t| overload(&[a, a, t], a))
                })
                .collect(),
        )),
        constant(operation(
            "clamp",
            "mask",
            "Clip to inclusive bounds; a reversed bound fails.",
            "clamp(x, 0, 1)",
            NUMERIC
                .iter()
                .flat_map(|&a| {
                    [Scalar, a].into_iter().flat_map(move |lo| {
                        [Scalar, a]
                            .into_iter()
                            .map(move |hi| overload(&[a, lo, hi], a))
                    })
                })
                .collect(),
        )),
        constant(operation(
            "smoothstep",
            "mask",
            "Cubic transition for strictly increasing bounds.",
            "smoothstep(0.4, 0.6, x)",
            NUMERIC
                .iter()
                .flat_map(|&x| {
                    [Scalar, x].into_iter().flat_map(move |lo| {
                        [Scalar, x]
                            .into_iter()
                            .map(move |hi| overload(&[lo, hi, x], x))
                    })
                })
                .collect(),
        )),
        operation(
            "step",
            "mask",
            "Zero below the edge, one at or above it; scalar broadcast applies.",
            "step(0.5, x)",
            numeric_pairs,
        ),
        constant(operation(
            "select",
            "selection",
            "Choose between matching branches; only the chosen branch is evaluated.",
            "select(gt(x, 0.5), a, b)",
            [Scalar, Vec2, Vec3, Bool]
                .into_iter()
                .map(|a| overload(&[Bool, a, a], a))
                .collect(),
        )),
        spaced(
            "scale",
            "Componentwise multiplication.",
            "scale(surface, 2)",
        ),
        spaced(
            "offset",
            "Add a constant displacement.",
            "offset(surface, vec2(1, 0))",
        ),
        spaced(
            "warp",
            "Add a varying displacement.",
            "warp(surface, vec2(n, n))",
        ),
        spaced(
            "repeat",
            "Wrap each component into [0, period); periods are positive.",
            "repeat(surface, 0.5)",
        ),
        operation(
            "rotate",
            "coordinates",
            "Rotate a 2D coordinate around the origin by radians.",
            "rotate(surface, 0.3)",
            vec![overload(&[Vec2, Scalar], Vec2)],
        ),
        literal(
            operation(
                "seed",
                "seed",
                "Derive a stable named seed from the request seed.",
                "seed(\"chips\")",
                vec![overload(&[Text], Scalar)],
            ),
            &["seed"],
            false,
        ),
        operation(
            "noise",
            "noise",
            "Gradient noise in [0, 1]; coordinates are in units of feature size.",
            "noise(div(surface, 0.1), seed(\"a\"))",
            each(&VECTORS, |a| overload(&[a, Scalar], Scalar)),
        ),
        operation(
            "cellular",
            "noise",
            "Nearest-cell distance field in [0, 1].",
            "cellular(div(surface, 0.1), seed(\"a\"))",
            each(&VECTORS, |a| overload(&[a, Scalar], Scalar)),
        ),
        operation(
            "fbm",
            "noise",
            "Filtered gradient noise summed over 1-12 octaves, in [0, 1].",
            "fbm(div(surface, 0.1), seed(\"a\"), 4)",
            each(&VECTORS, |a| overload(&[a, Scalar, Scalar], Scalar)),
        ),
        image(
            "sample_color",
            "Periodic bilinear sample with sRGB decoded before interpolation.",
            Vec3,
            "sampleColor(\"grime\", uv)",
        ),
        image(
            "sample_data",
            "Periodic bilinear sample of linear byte values.",
            Vec3,
            "sampleData(\"grime\", uv)",
        ),
        image(
            "sample_alpha",
            "Periodic bilinear sample of linear alpha.",
            Scalar,
            "sampleAlpha(\"grime\", uv)",
        ),
        {
            let mut op = operation(
                "runoff",
                "mapping",
                "Mask in [0, 1] from finite world-space source edges; width and length in meters.",
                "runoff(0.2, 1.5)",
                vec![overload(&[Scalar, Scalar], Scalar)],
            );
            op.requires = &["world", "normal", "gravity", "edges"];
            op
        },
        literal(
            operation(
                "feature_distance",
                "receiver",
                "Distance in meters from the sample to the nearest segment of the receiver's features of \
                 this name, at most maxDistance. The geometry's owner names features (a sill, a joint, a \
                 base); a receiver with none of the name reports maxDistance. Names are at most 64 \
                 letters, digits, '.', '_' or '-'.",
                "featureDistance(\"sill\", 0.3)",
                vec![overload(&[Text, Scalar], Scalar)],
            ),
            &["world", "named_features"],
            false,
        ),
        literal(
            operation(
                "feature_tangent",
                "receiver",
                "World-space unit direction along the nearest segment of the receiver's features of this \
                 name within maxDistance meters, in the order its points were given; vec3(0) without one.",
                "featureTangent(\"joint\", 0.05)",
                vec![overload(&[Text, Scalar], Vec3)],
            ),
            &["world", "named_features"],
            false,
        ),
        literal(
            operation(
                "external",
                "mapping",
                "A value the host supplies for each texel of a receiver under this name: a plane from \
                 another bake or system, such as a base layer's field or the scene's occlusion. Where no \
                 plane is supplied, or it holds no value at a texel, the result is the fallback. Every \
                 use of a name has one type; names are at most 64 letters, digits, '.', '_' or '-'.",
                "external(\"occlusion\", 1)",
                each(&NUMERIC, |a| overload(&[Text, a], a)),
            ),
            &["planes"],
            false,
        ),
        {
            let mut op = literal(
                operation(
                    "crease_distance",
                    "receiver",
                    "Distance in meters from the sample to the nearest selected receiver edge, at most maxDistance. \
                     Selectors: convex or concave creases at least minAngle degrees sharp, open boundary edges, \
                     exposed (convex or boundary), any (creases and boundaries), seam (edges where UVs differ \
                     across faces) or contact (edges lying on another surface of the scene, supplied by \
                     project builds). Coplanar triangulation edges are never creases; \
                     a receiver with no selected feature reports maxDistance.",
                    "creaseDistance(\"exposed\", 30, 0.05)",
                    vec![overload(&[Text, Scalar, Scalar], Scalar)],
                ),
                &["world", "features"],
                false,
            );
            op.literal_values = SELECTORS;
            op
        },
        {
            let mut op = literal(
                operation(
                    "crease_normal",
                    "receiver",
                    "World-space unit normal at the middle of the nearest selected crease within maxDistance \
                     meters (halfway between its two faces), as if the edge were rounded over. Boundaries have \
                     one face and are never chosen; creases facing the receiver's other side are ignored. \
                     Without a crease the receiver normal is returned, so adjustments toward it have no effect.",
                    "creaseNormal(\"convex\", 30, 0.014)",
                    vec![overload(&[Text, Scalar, Scalar], Vec3)],
                ),
                &["world", "normal", "features"],
                false,
            );
            op.literal_values = SELECTORS;
            op
        },
        {
            let mut op = literal(
                operation(
                    "crease_tangent",
                    "receiver",
                    "World-space unit direction along the nearest selected receiver edge within maxDistance meters \
                     (any selector, including boundaries, seams and contacts). The sign follows the edge's stored \
                     order: stable for equal geometry, but not oriented. Without an edge the result is vec3(0).",
                    "creaseTangent(\"exposed\", 30, 0.05)",
                    vec![overload(&[Text, Scalar, Scalar], Vec3)],
                ),
                &["world", "features"],
                false,
            );
            op.literal_values = SELECTORS;
            op
        },
        {
            let mut op = literal(
                operation(
                    "crease_adjacent_normal",
                    "receiver",
                    "World-space unit normal of the face across the nearest selected crease within maxDistance \
                     meters: the receiver normal reflected across the crease's middle normal. The receiver normal \
                     is the near face's normal. Creases are chosen as for creaseNormal; without one the receiver \
                     normal is returned.",
                    "creaseAdjacentNormal(\"convex\", 30, 0.014)",
                    vec![overload(&[Text, Scalar, Scalar], Vec3)],
                ),
                &["world", "normal", "features"],
                false,
            );
            op.literal_values = SELECTORS;
            op
        },
        operation(
            "cross",
            "math",
            "Cross product of two 3D vectors (right-handed).",
            "cross(normal, gravity)",
            vec![overload(&[Vec3, Vec3], Vec3)],
        ),
        operation(
            "hash",
            "noise",
            "Stable pseudo-random value in [0, 1) for the lattice cell containing a coordinate (its floor) and a seed.",
            "hash(floor(div(surface, 2)), seed(\"panels\"))",
            each(&NUMERIC, |a| overload(&[a, Scalar], Scalar)),
        ),
        operation(
            "value_noise",
            "noise",
            "Smooth value noise in [0, 1] over 2D or 3D coordinates in units of feature size; detail at or below the footprint fades toward 0.5.",
            "valueNoise(mul(world, 34), seed(\"rust\"))",
            each(&VECTORS, |a| overload(&[a, Scalar], Scalar)),
        ),
        operation(
            "pow",
            "math",
            "Componentwise power of nonnegative bases; a negative base fails.",
            "pow(weights, 4)",
            each(&NUMERIC, |a| overload(&[a, Scalar], a)),
        ),
        operation(
            "exp",
            "math",
            "Componentwise natural exponential; arguments above 200 fail.",
            "exp(neg(div(distance, 0.045)))",
            each(&NUMERIC, |a| overload(&[a], a)),
        ),
        operation(
            "triplanar_weights",
            "coordinates",
            "Projection weights from a normal: |n| raised to a positive sharpness, normalized to sum to one (x, y and z projections).",
            "triplanarWeights(normal, 4)",
            vec![overload(&[Vec3, Scalar], Vec3)],
        ),
        literal(
            operation(
                "varied_color",
                "image",
                "Blend rotated, offset copies of a color image over a triangular lattice (one cell per UV unit) to hide \
                 repetition. Each copy turns by up to rotation/2 radians either way; correction in [0, 1] restores the \
                 image's contrast around its mean. Sampling is mip-filtered by the coordinate's footprint and decodes sRGB.",
                "variedColor(\"concrete-plain.png\", div(surface, 2), seed(\"a\"), 0.32, 1)",
                vec![overload(&[Text, Vec2, Scalar, Scalar, Scalar], Vec3)],
            ),
            &[],
            true,
        ),
        {
            let mut op = literal(
                operation(
                    "remap",
                    "mask",
                    "Map a value from a strictly increasing input range to an output range. The mode is explicit: \
                     clamp keeps results inside the output range, extrapolate continues the line.",
                    "remap(\"clamp\", wear.distance, 0, 0.02, 1, 0)",
                    each(&NUMERIC, |a| {
                        overload(&[Text, a, Scalar, Scalar, Scalar, Scalar], a)
                    }),
                ),
                &[],
                false,
            );
            op.literal_values = REMAP_MODES;
            op
        },
        operation(
            "soft_threshold",
            "mask",
            "Mask that rises from zero to one across a threshold. Softness is the positive transition width in the \
             value's units, widened by the value's own variation across the sample.",
            "softThreshold(noise1, 0.6, 0.1)",
            each(&NUMERIC, |a| overload(&[a, Scalar, Scalar], a)),
        ),
        operation(
            "distance_band",
            "mask",
            "Mask that falls from one at the inner distance to zero at the outer distance, in meters, with \
             0 <= inner < outer; the distance's footprint softens both sides.",
            "distanceBand(creaseDistance(\"exposed\", 30, 0.05), 0.005, 0.02)",
            vec![overload(&[Scalar, Scalar, Scalar], Scalar)],
        ),
        operation(
            "soft_union",
            "mask",
            "Union of two masks clamped to [0, 1]: 1 - (1 - a) * (1 - b).",
            "softUnion(wear.coverage, rust.edge)",
            vec![overload(&[Scalar, Scalar], Scalar)],
        ),
        operation(
            "mask_intersect",
            "mask",
            "Intersection of two masks clamped to [0, 1]: a * b.",
            "maskIntersect(tiles.face, wear.coverage)",
            vec![overload(&[Scalar, Scalar], Scalar)],
        ),
        operation(
            "mask_subtract",
            "mask",
            "First mask with the second removed, both clamped to [0, 1]: a * (1 - b).",
            "maskSubtract(tiles.face, tiles.chip)",
            vec![overload(&[Scalar, Scalar], Scalar)],
        ),
        operation(
            "alpha_cutout",
            "output",
            "The material's opacity as a cutout: coverage clamped to [0, 1], drawn where it reaches the \
             cutoff and not at all elsewhere. Valid only as the whole opacity channel, with a constant \
             cutoff in [0, 1]; the material is delivered with alpha mode MASK and that cutoff.",
            "alphaCutout(sticker, 0.5)",
            vec![overload(&[Scalar, Scalar], Scalar)],
        ),
        literal(
            operation(
                "authored_mask",
                "image",
                "An authored mask in [0, 1]: the red channel of a linear image, bilinear, over the unit \
                 square of the coordinate (V up). The image does not repeat; outside the unit square the \
                 mask is zero.",
                "authoredMask(\"sticker-tear.png\", uv)",
                vec![overload(&[Text, Vec2], Scalar)],
            ),
            &[],
            true,
        ),
        {
            let mut op = literal(
                operation(
                    "ramp",
                    "color",
                    "A color ramp: after the mode and the coordinate t come two to seven stops, each a \
                     position and a color, with positions ascending. t is clamped to the stops. linear \
                     interpolates between neighboring stops and constant holds the stop at or below t, both \
                     on linear-light colors. The display modes are for painted materials: stops are \
                     display-referred (sRGB-encoded, in [0, 1]) colors, interpolated as such, and the result \
                     is decoded to linear light.",
                    "ramp(\"linear\", t, 0, vec3(0.1), 0.5, rust, 1, vec3(0.9))",
                    (2..=RAMP_STOPS)
                        .map(|stops| {
                            let mut args = vec![Text, Scalar];
                            for _ in 0..stops {
                                args.extend([Scalar, Vec3]);
                            }
                            overload(&args, Vec3)
                        })
                        .collect(),
                ),
                &[],
                false,
            );
            op.literal_values = RAMP_MODES;
            op
        },
        literal(
            operation(
                "instance_attribute",
                "render",
                "Render-only: a named attribute of the drawn instance (varying msInstance_<name>, such as \
                 root, pivot, shadeNormal, random or variation), typed like the second argument. Only the \
                 shader reads it; a bake of a value that reads it fails, naming it.",
                "instanceAttribute(\"random\", 0)",
                each(&NUMERIC, |a| overload(&[Text, a], a)),
            ),
            &["instance"],
            false,
        ),
        literal(
            operation(
                "vertex_attribute",
                "render",
                "Render-only: a named attribute of the drawn vertex (varying msVertex_<name>, such as \
                 heightRatio or bendWeight), typed like the second argument. Only the shader reads it; \
                 a bake of a value that reads it fails, naming it.",
                "vertexAttribute(\"heightRatio\", 0)",
                each(&NUMERIC, |a| overload(&[Text, a], a)),
            ),
            &["vertex"],
            false,
        ),
        operation(
            "shade_band",
            "lighting",
            "The painted shade factor in [0, 1]: smoothstep(-w, w, dot(normal, sun) - threshold), 0 in \
             shade and 1 lit, where w is the larger of range and softness screen pixels of the \
             difference's change (a bake, which has no pixels, reads softness as zero). Where the \
             material is drawn the sun's cast shadow, in the scene's shadow style, is folded in by \
             the minimum, so painted and lit surfaces share one shadow; a bake has no cast shadow. \
             Add a world-anchored edge break to threshold to bite the boundary (see edgeBreak).",
            "shadeBand(normal, sunDirection, 0.1, 1.5, 0.04)",
            vec![overload(&[Vec3, Vec3, Scalar, Scalar, Scalar], Scalar)],
        ),
        {
            let mut op = literal(
                operation(
                    "sample_field",
                    "render",
                    "Render-only: a named top-down scene field at a world XY position in meters, such as \
                     the ground's linear color (ground.color) or its world normal (ground.normal), which \
                     the renderer makes from the ground receiver's own material and binds as textures. \
                     Bilinear between texel centers, the edge texels continuing outside the field. A \
                     bake, or a renderer without the field, fails where it is read, naming it.",
                    "sampleField(\"ground.color\", instanceRoot.xy)",
                    vec![overload(&[Text, Vec2], Vec3)],
                ),
                &["field"],
                false,
            );
            op.literal_values = SCENE_FIELDS;
            op
        },
        operation(
            "adjust_normal",
            "receiver",
            "Turn a normal toward a target normal along the great circle; weight is clamped to [0, 1], \
             and one reaches the target. Opposite normals are left unchanged; zero vectors fail.",
            "adjustNormal(normal, creaseNormal(\"convex\", 30, 0.014), 0.5)",
            vec![overload(&[Vec3, Vec3, Scalar], Vec3)],
        ),
    ]
}

/// Receiver edge selectors accepted by `crease_distance`.
pub const SELECTORS: &[&str] = &[
    "convex", "concave", "boundary", "exposed", "any", "seam", "contact",
];

/// Out-of-range behavior accepted by `remap`.
pub const REMAP_MODES: &[&str] = &["clamp", "extrapolate"];

/// Interpolation and color space accepted by `ramp`.
pub const RAMP_MODES: &[&str] = &["linear", "constant", "display-linear", "display-constant"];

/// Top-down scene fields `sample_field` reads, each a vec3: the ground's linear color and its
/// world-space unit normal.
pub const SCENE_FIELDS: &[&str] = &["ground.color", "ground.normal"];

/// Stops a `ramp` takes at most: with the mode and the coordinate, a call's sixteen arguments.
pub const RAMP_STOPS: usize = 7;

/// Every operation, in documentation order.
pub fn operations() -> &'static [Operation] {
    static OPERATIONS: OnceLock<Vec<Operation>> = OnceLock::new();
    OPERATIONS.get_or_init(|| {
        let mut operations = build();
        for operation in &mut operations {
            // Broadcast enumeration repeats scalar-only combinations; keep first occurrences.
            let mut unique: Vec<Overload> = Vec::with_capacity(operation.overloads.len());
            for overload in operation.overloads.drain(..) {
                if !unique.contains(&overload) {
                    unique.push(overload);
                }
            }
            operation.overloads = unique;
        }
        operations
    })
}

pub fn find(id: &str) -> Option<&'static Operation> {
    operations().iter().find(|operation| operation.id == id)
}

/// Resolve a call's result type, or `None` when no overload matches.
pub fn resolve(id: &str, args: &[Type]) -> Option<Type> {
    find(id)?
        .overloads
        .iter()
        .find(|overload| overload.args == args)
        .map(|overload| overload.result)
}

#[cfg(test)]
mod tests;
