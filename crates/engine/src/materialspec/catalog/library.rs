//! Operators, editable groups and renderer operations beside the built-in call catalog.
use super::{Kind, Overload};
use crate::materialspec::program::Type::{self, Bool, Scalar, Vec2, Vec3};
use serde::Serialize;
use std::sync::OnceLock;

/// An infix or prefix operation. TypeScript recipes spell these as functions because a
/// branded value type cannot be used with operators; they lower to the same expression nodes.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Operator {
    pub id: &'static str,
    pub version: u32,
    pub symbol: &'static str,
    pub summary: &'static str,
    pub example: &'static str,
    pub overloads: Vec<Overload>,
}

fn operator(
    id: &'static str,
    symbol: &'static str,
    summary: &'static str,
    example: &'static str,
    overloads: Vec<Overload>,
) -> Operator {
    Operator {
        id,
        version: 1,
        symbol,
        summary,
        example,
        overloads,
    }
}

fn pair(a: Type, b: Type, result: Type) -> Overload {
    Overload {
        args: vec![a, b],
        result,
    }
}

pub fn operators() -> &'static [Operator] {
    static OPERATORS: OnceLock<Vec<Operator>> = OnceLock::new();
    OPERATORS.get_or_init(|| {
        let numeric = [Scalar, Vec2, Vec3];
        let arithmetic: Vec<Overload> = numeric
            .iter()
            .flat_map(|&a| {
                numeric.iter().filter_map(move |&b| {
                    if a == b || b == Scalar {
                        Some(pair(a, b, a))
                    } else if a == Scalar {
                        Some(pair(a, b, b))
                    } else {
                        None
                    }
                })
            })
            .collect();
        let compare = vec![pair(Scalar, Scalar, Bool)];
        let equal: Vec<Overload> = [Scalar, Vec2, Vec3, Bool]
            .into_iter()
            .map(|kind| pair(kind, kind, Bool))
            .collect();
        let logic = vec![pair(Bool, Bool, Bool)];
        let prefix = |kind: Type| Overload {
            args: vec![kind],
            result: kind,
        };
        vec![
            operator(
                "add",
                "+",
                "Componentwise sum with scalar broadcast.",
                "add(a, b)",
                arithmetic.clone(),
            ),
            operator(
                "sub",
                "-",
                "Componentwise difference with scalar broadcast.",
                "sub(a, b)",
                arithmetic.clone(),
            ),
            operator(
                "mul",
                "*",
                "Componentwise product with scalar broadcast.",
                "mul(a, b)",
                arithmetic.clone(),
            ),
            operator(
                "div",
                "/",
                "Componentwise quotient with scalar broadcast; division by zero fails.",
                "div(a, b)",
                arithmetic.clone(),
            ),
            operator(
                "rem",
                "%",
                "Componentwise remainder with scalar broadcast; zero fails.",
                "rem(a, b)",
                arithmetic,
            ),
            operator(
                "neg",
                "-",
                "Componentwise negation.",
                "neg(a)",
                numeric.iter().map(|&k| prefix(k)).collect(),
            ),
            operator(
                "not",
                "!",
                "Boolean negation.",
                "not(flag)",
                vec![prefix(Bool)],
            ),
            operator("lt", "<", "Scalar less-than.", "lt(a, b)", compare.clone()),
            operator(
                "le",
                "<=",
                "Scalar less-than-or-equal.",
                "le(a, b)",
                compare.clone(),
            ),
            operator(
                "gt",
                ">",
                "Scalar greater-than.",
                "gt(a, b)",
                compare.clone(),
            ),
            operator(
                "ge",
                ">=",
                "Scalar greater-than-or-equal.",
                "ge(a, b)",
                compare,
            ),
            operator(
                "eq",
                "==",
                "Equality of like types; vectors compare every component.",
                "eq(a, b)",
                equal.clone(),
            ),
            operator("ne", "!=", "Inequality of like types.", "ne(a, b)", equal),
            operator(
                "and",
                "&&",
                "Short-circuit boolean and.",
                "and(a, b)",
                logic.clone(),
            ),
            operator("or", "||", "Short-circuit boolean or.", "or(a, b)", logic),
        ]
    })
}

pub fn find_operator(id: &str) -> Option<&'static Operator> {
    operators().iter().find(|operator| operator.id == id)
}

/// A group argument. `default` is a MaterialSpec expression; `None` makes the argument required.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupArgument {
    pub name: &'static str,
    #[serde(rename = "type")]
    pub kind: Type,
    pub unit: &'static str,
    pub summary: &'static str,
    pub default: Option<&'static str>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupOutput {
    pub name: &'static str,
    #[serde(rename = "type")]
    pub kind: Type,
    pub summary: &'static str,
}

/// An editable group: a named, parameterized set of fields over shared operations.
///
/// `template` is a MaterialSpec module body. Its declarations become fields named
/// `<instance>.<name>`, and its free names that match an argument resolve to the instance's
/// argument fields. `%ID%` is replaced by the instance name so named seeds stay independent.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub id: &'static str,
    pub version: u32,
    pub kind: Kind,
    pub title: &'static str,
    pub summary: &'static str,
    pub example: &'static str,
    pub arguments: Vec<GroupArgument>,
    pub outputs: Vec<GroupOutput>,
    pub requires: &'static [&'static str],
    /// Receiver or renderer capabilities the evaluator cannot supply yet.
    pub unavailable: Option<&'static str>,
    #[serde(skip)]
    pub template: &'static str,
}

fn argument(
    name: &'static str,
    kind: Type,
    unit: &'static str,
    default: Option<&'static str>,
    summary: &'static str,
) -> GroupArgument {
    GroupArgument {
        name,
        kind,
        unit,
        summary,
        default,
    }
}

fn mask(name: &'static str, summary: &'static str) -> GroupOutput {
    GroupOutput {
        name,
        kind: Scalar,
        summary,
    }
}

pub fn groups() -> &'static [Group] {
    static GROUPS: OnceLock<Vec<Group>> = OnceLock::new();
    GROUPS.get_or_init(|| {
        vec![
            Group {
                id: "runoffStreaks",
                version: 1,
                kind: Kind::Group,
                title: "Runoff",
                summary: "Downhill streaking from finite source edges, as a clamped coverage mask.",
                example: "const streaks = runoffStreaks({ width: 0.025, length: 0.7 });",
                arguments: vec![
                    argument("width", Scalar, "m", Some("0.025"), "Lateral falloff beside each source edge."),
                    argument("length", Scalar, "m", Some("0.7"), "Downhill reach of the streak."),
                    argument("strength", Scalar, "", Some("1.0"), "Multiplier applied before clamping to [0, 1]."),
                ],
                outputs: vec![mask("coverage", "Runoff coverage in [0, 1].")],
                requires: &["world", "normal", "gravity", "edges"],
                unavailable: None,
                template: "let coverage = clamp(runoff(width, length) * strength, 0.0, 1.0);",
            },
            Group {
                id: "groundWeathering",
                version: 1,
                kind: Kind::Group,
                title: "Ground Weathering",
                summary: "Stain and growth masks that fade with height above the ground plane.",
                example: "const ground = groundWeathering({ scale: 0.6, stainAmount: 0.5 });",
                arguments: vec![
                    argument("scale", Scalar, "m", Some("0.6"), "Feature size of the stain pattern."),
                    argument("stainAmount", Scalar, "", Some("0.5"), "Fraction of the contact zone that is stained."),
                    argument("growthAmount", Scalar, "", Some("0.35"), "Fraction of upward-facing contact zone with growth."),
                    argument("contactHeight", Scalar, "m", Some("0.4"), "World Z height over which the effect fades out."),
                ],
                outputs: vec![
                    mask("stain", "Stain coverage in [0, 1]."),
                    mask("growth", "Growth coverage in [0, 1], limited to upward-facing surfaces."),
                    mask("contact", "Ground-contact weight in [0, 1]."),
                ],
                requires: &["surface", "world", "normal", "seed"],
                unavailable: None,
                template: "let contact = 1.0 - smoothstep(0.0, contactHeight, world.z);\n\
                    let stainField = fbm(surface / scale, seed(\"%ID%.stain\"), 4);\n\
                    let growthField = fbm(surface / (scale * 0.35), seed(\"%ID%.growth\"), 3);\n\
                    let stain = clamp(contact * smoothstep(1.0 - stainAmount - 0.1, 1.0 - stainAmount + 0.1, stainField), 0.0, 1.0);\n\
                    let growth = clamp(contact * max(normal.z, 0.0) * smoothstep(1.0 - growthAmount - 0.1, 1.0 - growthAmount + 0.1, growthField), 0.0, 1.0);",
            },
            Group {
                id: "tileAndGrout",
                version: 1,
                kind: Kind::Group,
                title: "Tile and Grout",
                summary: "Square or staggered rectangular tile with joints, missing tiles and edge chips.",
                example: "const tiles = tileAndGrout({ size: vec2(0.3, 0.15), rowOffset: 0.5 });",
                arguments: vec![
                    argument("size", Vec2, "m", Some("vec2(0.3, 0.3)"), "Tile width and height."),
                    argument("groutWidth", Scalar, "m", Some("0.008"), "Joint width."),
                    argument("rowOffset", Scalar, "", Some("0.0"), "Fraction of a tile that alternate rows shift."),
                    argument("chipAmount", Scalar, "", Some("0.15"), "Share of tile edge that is chipped."),
                    argument("chipSize", Scalar, "m", Some("0.03"), "Chip depth into the tile face and pattern scale."),
                    argument("missingChance", Scalar, "", Some("0.0"), "Chance that a tile is missing."),
                ],
                outputs: vec![
                    mask("face", "Intact tile face coverage."),
                    mask("joint", "Joint coverage, antialiased by the sample footprint."),
                    mask("chip", "Chipped edge coverage on tile faces."),
                    mask("substrate", "Exposed substrate: joints, missing tiles and chips."),
                ],
                requires: &["surface", "footprint", "seed"],
                unavailable: None,
                template: "let cell = vec2(surface.x / size.x + rowOffset * abs(floor(surface.y / size.y) % 2.0), surface.y / size.y);\n\
                    let local = vec2(fract(cell.x), fract(cell.y));\n\
                    let edgeDistance = min(min(local.x, 1.0 - local.x) * size.x, min(local.y, 1.0 - local.y) * size.y);\n\
                    let soft = footprint * 0.5 + 0.0005;\n\
                    let joint = 1.0 - smoothstep(groutWidth * 0.5 - soft, groutWidth * 0.5 + soft, edgeDistance);\n\
                    let tileRandom = smoothstep(0.25, 0.75, noise(vec2(floor(cell.x) * 0.7391 + 13.17, floor(cell.y) * 0.9123 + 5.31), seed(\"%ID%.cell\")));\n\
                    let missing = 1.0 - step(missingChance, tileRandom);\n\
                    let face = (1.0 - joint) * (1.0 - missing);\n\
                    let chipField = fbm(surface / chipSize, seed(\"%ID%.chip\"), 3);\n\
                    let chip = face * (1.0 - smoothstep(0.0, chipSize, edgeDistance)) * smoothstep(1.0 - chipAmount - 0.1, 1.0 - chipAmount + 0.1, chipField);\n\
                    let substrate = clamp(1.0 - face + chip, 0.0, 1.0);",
            },
            Group {
                id: "edgeWear",
                version: 1,
                kind: Kind::Group,
                title: "Edge Wear",
                summary: "Worn, chipped coverage and a height dip along exposed receiver edges: convex creases at least minAngle sharp and open boundaries.",
                example: "const wear = edgeWear({ width: 0.014, minAngle: 30 });",
                arguments: vec![
                    argument("width", Scalar, "m", Some("0.014"), "Wear width from the edge."),
                    argument("minAngle", Scalar, "deg", Some("30.0"), "Softest crease that wears."),
                    argument("depth", Scalar, "m", Some("0.002"), "Height removed where fully worn."),
                    argument("breakup", Scalar, "m", Some("0.04"), "Feature size of the irregular wear outline."),
                    argument("jitter", Scalar, "", Some("0.5"), "How far the outline wanders, as a fraction of width in [0, 1]."),
                ],
                outputs: vec![
                    mask("coverage", "Edge-wear coverage in [0, 1]; zero when the receiver has no exposed edge."),
                    GroupOutput { name: "height", kind: Scalar, summary: "Height delta in meters: -depth where fully worn." },
                    GroupOutput { name: "distance", kind: Scalar, summary: "Distance to the nearest exposed edge in meters, capped beyond the wear shoulder (twice the width plus the sample footprint)." },
                ],
                requires: &["world", "features", "footprint", "seed"],
                unavailable: None,
                template: "let distance = crease_distance(\"exposed\", minAngle, width * 2.0 + footprint);\n\
                    let outline = fbm(world / breakup, seed(\"%ID%.wear\"), 3);\n\
                    let reach = width * (1.0 + (outline - 0.5) * clamp(jitter, 0.0, 1.0));\n\
                    let coverage = 1.0 - smoothstep(reach * 0.6, reach + footprint * 0.5, distance);\n\
                    let height = -depth * coverage;",
            },
            Group {
                id: "oxide",
                version: 1,
                kind: Kind::Group,
                title: "Oxide",
                summary: "Edge, surface and spot oxide coverage and their combined mask.",
                example: "const rust = oxide({ edgeWidth: 0.02, surfaceAmount: 0.25 });",
                arguments: vec![
                    argument("edgeWidth", Scalar, "m", Some("0.02"), "Oxide width from exposed edges."),
                    argument("minAngle", Scalar, "deg", Some("30.0"), "Softest crease that collects edge oxide."),
                    argument("surfaceAmount", Scalar, "", Some("0.25"), "Share of the surface with patchy oxide."),
                    argument("surfaceScale", Scalar, "m", Some("0.15"), "Feature size of surface patches."),
                    argument("spotSize", Scalar, "m", Some("0.03"), "Size of isolated oxide spots."),
                    argument("spotAmount", Scalar, "", Some("0.15"), "Share of spot cells that rust."),
                ],
                outputs: vec![
                    mask("edge", "Edge oxide coverage; zero when the receiver has no exposed edge."),
                    mask("surface", "Surface oxide coverage."),
                    mask("spot", "Spot oxide coverage."),
                    mask("combined", "Soft union of the edge, surface and spot masks."),
                ],
                requires: &["world", "features", "footprint", "seed"],
                unavailable: None,
                template: "let edgeDistance = crease_distance(\"exposed\", minAngle, edgeWidth * 2.0 + footprint);\n\
                    let edgeBreakup = fbm(world / (edgeWidth * 2.0), seed(\"%ID%.edge\"), 3);\n\
                    let edge = clamp((1.0 - smoothstep(edgeWidth * 0.4, edgeWidth + footprint * 0.5, edgeDistance)) * (0.6 + edgeBreakup * 0.8), 0.0, 1.0);\n\
                    let surfaceField = fbm(world / surfaceScale, seed(\"%ID%.surface\"), 4);\n\
                    let surface = smoothstep(1.0 - surfaceAmount - 0.08, 1.0 - surfaceAmount + 0.08, surfaceField);\n\
                    let spotField = cellular(world / spotSize, seed(\"%ID%.spot\"));\n\
                    let spot = (1.0 - smoothstep(0.0, 0.35, spotField)) * step(1.0 - spotAmount, noise(world / (spotSize * 3.0), seed(\"%ID%.spots\")));\n\
                    let combined = 1.0 - (1.0 - edge) * (1.0 - surface) * (1.0 - spot);",
            },
            Group {
                id: "roundedEdges",
                version: 1,
                kind: Kind::Group,
                title: "Rounded Edges",
                summary: "Shading round-over of convex receiver creases: the normal turns toward each crease's middle normal near it, without changing geometry.",
                example: "const edges = roundedEdges({ radius: 0.014, minAngle: 30 });",
                arguments: vec![
                    argument("radius", Scalar, "m", Some("0.014"), "Distance from the crease over which the normal turns."),
                    argument("minAngle", Scalar, "deg", Some("30.0"), "Softest crease that is rounded."),
                    argument("strength", Scalar, "", Some("1.0"), "Fraction of the round-over applied, in [0, 1]."),
                ],
                outputs: vec![
                    GroupOutput { name: "distance", kind: Scalar, summary: "Distance to the nearest rounded crease in meters, capped at the radius plus the sample footprint." },
                    mask("weight", "Round-over weight in [0, 1]: one on the crease, zero at the radius."),
                    GroupOutput { name: "receiverNormal", kind: Vec3, summary: "World-space shading normal for the receiverNormal channel." },
                ],
                requires: &["world", "normal", "features", "footprint"],
                unavailable: None,
                template: "let distance = crease_distance(\"convex\", minAngle, radius + footprint);\n\
                    let weight = clamp(1.0 - distance / (radius + footprint * 0.5), 0.0, 1.0) * clamp(strength, 0.0, 1.0);\n\
                    let receiverNormal = adjust_normal(normal, crease_normal(\"convex\", minAngle, radius + footprint), weight);",
            },
            Group {
                id: "rectangularGrid",
                version: 1,
                kind: Kind::Group,
                title: "Rectangular Grid",
                summary: "Cells of a physical rectangular grid, with alternate rows optionally staggered: signed integer column and row, the position inside the cell and distances to the cell's sides and corners. Cell IDs floor at boundaries, including negative coordinates; nothing depends on the footprint.",
                example: "const bricks = rectangularGrid({ size: vec2(0.24, 0.075), rowOffset: 0.5 });",
                arguments: vec![
                    argument("position", Vec2, "m", Some("surface"), "Physical 2D coordinates."),
                    argument("size", Vec2, "m", Some("vec2(0.3, 0.3)"), "Positive cell width and height."),
                    argument("origin", Vec2, "m", Some("vec2(0.0, 0.0)"), "Corner of cell (0, 0)."),
                    argument("rowOffset", Scalar, "", Some("0.0"), "Fraction of a cell in [0, 1) that odd rows shift along x."),
                ],
                outputs: vec![
                    GroupOutput { name: "sideDistance", kind: Scalar, summary: "Distance in meters to the nearest side of the cell." },
                    GroupOutput { name: "cornerDistance", kind: Scalar, summary: "Distance in meters to the nearest corner of the cell." },
                    mask("random", "Stable value in [0, 1) per cell and instance."),
                    GroupOutput { name: "cell", kind: Vec2, summary: "Integer column and row of the cell." },
                    GroupOutput { name: "local", kind: Vec2, summary: "Position inside the cell in meters, from its lower corner." },
                ],
                requires: &["surface", "seed"],
                unavailable: None,
                template: "let p = position - origin;\n\
                    let row = floor(p.y / size.y);\n\
                    let shifted = p.x / size.x + rowOffset * abs(row % 2.0);\n\
                    let column = floor(shifted);\n\
                    let cell = vec2(column, row);\n\
                    let local = vec2((shifted - column) * size.x, p.y - row * size.y);\n\
                    let sides = vec2(min(local.x, size.x - local.x), min(local.y, size.y - local.y));\n\
                    let sideDistance = min(sides.x, sides.y);\n\
                    let cornerDistance = length(sides);\n\
                    let random = hash(cell, seed(\"%ID%.cell\"));",
            },
            Group {
                id: "periodicStrips",
                version: 1,
                kind: Kind::Group,
                title: "Periodic Strips",
                summary: "Joints `gap` meters wide every `period` meters along one coordinate, centered on multiples of the period. Coverage is integrated over the sample footprint, so coarse bakes keep the joints' average share; a zero footprint gives point coverage.",
                example: "const courses = periodicStrips({ position: surface.y, period: 0.075, gap: 0.01 });",
                arguments: vec![
                    argument("position", Scalar, "m", Some("surface.x"), "Coordinate across the strips."),
                    argument("period", Scalar, "m", Some("0.3"), "Positive distance between joint centers."),
                    argument("gap", Scalar, "m", Some("0.01"), "Joint width, from zero to the period."),
                ],
                outputs: vec![
                    mask("joint", "Joint coverage in [0, 1] over the sample footprint."),
                    mask("solid", "Coverage between joints: one minus the joint coverage."),
                ],
                requires: &["surface", "footprint"],
                unavailable: None,
                template: "let half = footprint * 0.5;\n\
                    let x = position + gap * 0.5;\n\
                    let low = x - half;\n\
                    let high = x + half;\n\
                    let lowJoint = floor(low / period) * gap + min(fract(low / period) * period, gap);\n\
                    let highJoint = floor(high / period) * gap + min(fract(high / period) * period, gap);\n\
                    let point = 1.0 - step(gap, fract(x / period) * period);\n\
                    let joint = clamp(mix(point, (highJoint - lowJoint) / max(high - low, 0.000000001), step(0.000000001, half)), 0.0, 1.0);\n\
                    let solid = 1.0 - joint;",
            },
            Group {
                id: "directionalCoordinates",
                version: 1,
                kind: Kind::Group,
                title: "Directional Coordinates",
                summary: "2D coordinates that follow a direction on the surface, for grain, weave and brushing: `along` the direction projected onto the receiver plane and `across` it, each divided by its feature scale. Where the direction is parallel to the normal both are zero.",
                example: "const grain = directionalCoordinates({ direction: vec3(0, 0, 1), scale: vec2(0.2, 0.004) });",
                arguments: vec![
                    argument("position", Vec3, "m", Some("world"), "Position in meters."),
                    argument("direction", Vec3, "", Some("vec3(1.0, 0.0, 0.0)"), "Direction the coordinates follow, in the position's space."),
                    argument("scale", Vec2, "m", Some("vec2(1.0, 1.0)"), "Positive feature sizes along and across."),
                ],
                outputs: vec![
                    GroupOutput { name: "along", kind: Scalar, summary: "Coordinate along the direction, in units of scale.x." },
                    GroupOutput { name: "across", kind: Scalar, summary: "Coordinate across the direction on the surface, in units of scale.y." },
                    GroupOutput { name: "coordinates", kind: Vec2, summary: "vec2(along, across), ready for noise or image sampling." },
                ],
                requires: &["world", "normal"],
                unavailable: None,
                template: "let projected = direction - normal * dot(direction, normal);\n\
                    let axis = projected / max(length(projected), 0.000001);\n\
                    let side = cross(normal, axis);\n\
                    let along = dot(position, axis) / scale.x;\n\
                    let across = dot(position, side) / scale.y;\n\
                    let coordinates = vec2(along, across);",
            },
            Group {
                id: "edgeBreak",
                version: 1,
                kind: Kind::Group,
                title: "Edge Break",
                summary: "A signed offset for a painted shade boundary's threshold, anchored in the world: fractal noise over the position's XY, so the boundary bites into the surface where it lies and stays put as the sun or the camera moves. Add `offset` to shadeBand's threshold.",
                example: "const edge = edgeBreak({ size: 0.35, amount: 0.12 });",
                arguments: vec![
                    argument("position", Vec3, "m", Some("world"), "Position in meters whose XY anchors the noise."),
                    argument("size", Scalar, "m", Some("0.35"), "Feature size of the bites."),
                    argument("amount", Scalar, "", Some("0.12"), "Largest offset either way, in units of the shade factor's dot product."),
                ],
                outputs: vec![GroupOutput { name: "offset", kind: Scalar, summary: "Offset in [-amount, amount] to add to a threshold." }],
                requires: &["world", "seed"],
                unavailable: None,
                template: "let offset = (fbm(position.xy / size, seed(\"%ID%.edge\"), 3) - 0.5) * 2.0 * amount;",
            },
        ]
    })
}

pub fn find_group(id: &str) -> Option<&'static Group> {
    groups().iter().find(|group| group.id == id)
}

/// A live operation consumed by the renderer. The program records it and its bound inputs;
/// the evaluator does not run it.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RendererOperation {
    pub id: &'static str,
    pub version: u32,
    pub kind: Kind,
    pub summary: &'static str,
    pub example: &'static str,
    pub stage: &'static str,
    pub arguments: Vec<GroupArgument>,
    /// Renderer-supplied inputs and capabilities, never authored values.
    pub requires: &'static [&'static str],
}

pub fn renderer_operations() -> &'static [RendererOperation] {
    static OPERATIONS: OnceLock<Vec<RendererOperation>> = OnceLock::new();
    OPERATIONS.get_or_init(|| {
        let op = |id, summary, example, stage, arguments, requires| RendererOperation {
            id,
            version: 1,
            kind: Kind::Renderer,
            summary,
            example,
            stage,
            arguments,
            requires,
        };
        vec![
            op(
                "grazingFalloff",
                "Raises specular F90 toward 1.08 on directly sunlit surfaces. The grazing weight ramps from zero at view cosine `from` to `mask` at `to`.",
                "grazingFalloff({ from: 0.2, to: 0.8 })",
                "shading-normal",
                vec![
                    argument(
                        "from",
                        Scalar,
                        "",
                        None,
                        "Facing value where the mask begins.",
                    ),
                    argument(
                        "to",
                        Scalar,
                        "",
                        None,
                        "Facing value where the mask is full.",
                    ),
                    argument(
                        "mask",
                        Scalar,
                        "",
                        Some("1.0"),
                        "Mask in [0, 1] that gates the lift.",
                    ),
                ],
                &["view-direction", "shading-normal"],
            ),
            op(
                "filterEnvironment",
                "Environment reflection from nine taps spread `spread` radians around the reflection at `roughness`, keeping the mean of the lowest-variance quadrant (a directional Kuwahara filter). A spread of zero is the plain reflection.",
                "filterEnvironment({ roughness: 0.3, spread: 0.1 })",
                "environment-reflection",
                vec![
                    argument("roughness", Scalar, "", None, "Roughness in [0, 1]."),
                    argument("spread", Scalar, "rad", None, "Angular filter offset."),
                ],
                &["reflection-direction", "environment-texture"],
            ),
            op(
                "bandLuminance",
                "Steps reflected luminance into bands `spacing` apart in square-root luminance, with transitions `softness` wide (as a fraction of a band), gated by `mask`. Bands whose middle lies between 0.60 and 0.95 blend toward `brightSoftness`; `(liftRed, liftGreen, liftBlue)` is added to bands below 0.42 in square-root luminance, fading out by 0.87.",
                "bandLuminance({ spacing: 0.25, softness: 0.05 })",
                "reflection-response",
                vec![
                    argument(
                        "spacing",
                        Scalar,
                        "",
                        None,
                        "Band spacing in linear radiance.",
                    ),
                    argument(
                        "softness",
                        Scalar,
                        "",
                        None,
                        "Transition width between bands.",
                    ),
                    argument(
                        "mask",
                        Scalar,
                        "",
                        Some("1.0"),
                        "Mask in [0, 1] that gates the banding.",
                    ),
                    argument(
                        "brightSoftness",
                        Scalar,
                        "",
                        Some("-1.0"),
                        "Transition width of bright bands; negative keeps `softness`.",
                    ),
                    argument("liftRed", Scalar, "", Some("0.0"), "Red radiance added to dark bands."),
                    argument("liftGreen", Scalar, "", Some("0.0"), "Green radiance added to dark bands."),
                    argument("liftBlue", Scalar, "", Some("0.0"), "Blue radiance added to dark bands."),
                ],
                &["reflected-radiance"],
            ),
            op(
                "modulateDiffuse",
                "Scales only the diffuse lighting, direct and indirect, by `mix(1, factor, mask)`.",
                "modulateDiffuse({ factor: 0.9, mask: grain })",
                "lighting-composition",
                vec![
                    argument(
                        "factor",
                        Scalar,
                        "",
                        None,
                        "Nonnegative diffuse multiplier.",
                    ),
                    argument(
                        "mask",
                        Scalar,
                        "",
                        Some("1.0"),
                        "Mask in [0, 1] that gates the multiplier.",
                    ),
                ],
                &["diffuse-contribution"],
            ),
            op(
                "scaleSpecular",
                "Scales blended specular color by a masked factor. Increases use 12% of the authored gain on directly sunlit surfaces. Reductions apply at full strength.",
                "scaleSpecular({ factor: add(1, mul(0.45, rim)) })",
                "lighting-composition",
                vec![
                    argument(
                        "factor",
                        Scalar,
                        "",
                        None,
                        "Nonnegative specular color multiplier.",
                    ),
                    argument(
                        "mask",
                        Scalar,
                        "",
                        Some("1.0"),
                        "Mask in [0, 1] that gates the multiplier.",
                    ),
                ],
                &["specular-contribution"],
            ),
            op(
                "lightFloor",
                "Adds linear radiance `(red, green, blue)` times `mask`, a floor under the lit result. When `from` and `to` differ, the mask also ramps from 0 where the view-facing cosine is `from` to 1 where it is `to`.",
                "lightFloor({ red: 0.105, green: 0.111, blue: 0.12, mask: rim })",
                "lighting-composition",
                vec![
                    argument("red", Scalar, "", None, "Added linear red radiance."),
                    argument("green", Scalar, "", None, "Added linear green radiance."),
                    argument("blue", Scalar, "", None, "Added linear blue radiance."),
                    argument(
                        "mask",
                        Scalar,
                        "",
                        Some("1.0"),
                        "Mask in [0, 1] that gates the added light.",
                    ),
                    argument(
                        "from",
                        Scalar,
                        "",
                        Some("0.0"),
                        "Facing value where the ramp begins.",
                    ),
                    argument(
                        "to",
                        Scalar,
                        "",
                        Some("0.0"),
                        "Facing value where the ramp is full; equal to `from` for no ramp.",
                    ),
                ],
                &["view-direction", "lit-radiance"],
            ),
        ]
    })
}

pub fn find_renderer_operation(id: &str) -> Option<&'static RendererOperation> {
    renderer_operations()
        .iter()
        .find(|operation| operation.id == id)
}
