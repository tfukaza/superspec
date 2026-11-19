//! The `panel` generator: a flat slab with through openings and recesses, frames,
//! glazing and sills. Local frame: width along X, height along Z, thickness along Y.
//! The front face lies on y = 0 and faces -Y; the part origin is its bottom-left corner.
//!
//! The slab is built directly as a polygon with holes swept through the thickness, so
//! every face belongs to its group by construction and stays in f64. It is the same
//! solid as the panel minus its extruded openings.
use super::{
    Feature, ItemDiagnostic, PatternAnchor,
    parts::{Bevel, Context},
    schema::{Edge, OpeningDefinition, OpeningMode, OpeningShape, PanelDefinition, Rule},
    surfaces::{Frame, Outline, P2, Surfaces, inset, prism, rim},
};
use serde::Serialize;
use std::collections::BTreeSet;
use superspec_geometry::{
    V3,
    math::{cos, sin},
    triangulate,
};

pub(super) const GROUPS: &[&str] = &[
    "front", "back", "edge", "reveal", "frame", "glazing", "sill",
];
/// Smallest distance between an opening and the panel edge or another opening.
pub(super) const MARGIN: f64 = 0.02;
const ARCH_SEGMENTS: usize = 16;
const SLIVER: f64 = 1e-3;
const DEFAULT_SILL_THICKNESS: f64 = 0.03;

#[derive(Clone, Debug, Serialize)]
pub(super) struct Sill {
    depth: f64,
    overhang: f64,
    thickness: f64,
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct Opening {
    id: String,
    arch: bool,
    /// Depth of a recess; `None` cuts through.
    recess: Option<f64>,
    x: f64,
    z: f64,
    width: f64,
    height: f64,
    /// Border and depth.
    frame: Option<[f64; 2]>,
    glazing: Option<f64>,
    sill: Option<Sill>,
    /// The radius its two upright front arrises are rounded with; 0 leaves them sharp.
    #[serde(skip_serializing_if = "is_zero")]
    jambs: f64,
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct Panel {
    width: f64,
    height: f64,
    thickness: f64,
    openings: Vec<Opening>,
    anchor: P2,
    #[serde(skip_serializing_if = "Bevel::none")]
    bevel: Bevel,
    #[serde(skip_serializing_if = "Arris::none")]
    arris: Arris,
}
fn is_zero(value: &f64) -> bool {
    *value == 0.
}

/// The rounding of a panel's upright end edges: its radius and the tangent of half the plan
/// turn at the start and at the end (0 where that end stays sharp). Absent from a geometry key
/// when neither end is rounded.
#[derive(Clone, Copy, Debug, Serialize)]
struct Arris {
    radius: f64,
    turns: [f64; 2],
}
impl Arris {
    fn none(&self) -> bool {
        self.radius == 0. || self.turns == [0.; 2]
    }
    /// How far along the front the round starts from each end.
    fn setbacks(&self) -> [f64; 2] {
        self.turns.map(|turn| self.radius * turn)
    }
    /// One end's half of the round, in (x, y) with y into the panel: where it leaves the front,
    /// its middle (where the neighbour's half takes over) and the outward normal there.
    fn half(&self, end: usize, width: f64) -> Option<[P2; 3]> {
        let turn = self.turns[end];
        if self.radius <= 0. || turn <= 0. {
            return None;
        }
        // The half turn's cosine and sine from its tangent: only a square root.
        let cosine = 1. / (1. + turn * turn).sqrt();
        let sine = turn * cosine;
        let reach = self.radius * ((1. + turn * turn).sqrt() - 1.);
        let setback = self.radius * turn;
        Some(if end == 0 {
            [
                [setback, 0.],
                [reach * sine, reach * cosine],
                [-sine, -cosine],
            ]
        } else {
            [
                [width - setback, 0.],
                [width - reach * sine, reach * cosine],
                [sine, -cosine],
            ]
        })
    }
}

/// Start of a span of `size` inside `available`, by the shared placement rule.
pub(super) fn place(
    context: &Context<'_>,
    rule: &Rule,
    available: f64,
    size: f64,
    pointer: &str,
) -> Result<f64, ItemDiagnostic> {
    Ok(match rule {
        Rule::Offset(rule) => {
            let offset = context.scalar(&rule.offset, &format!("{pointer}/offset"))?;
            match rule.from {
                Edge::Start => offset,
                Edge::End => available - size - offset,
                Edge::Center => (available - size) / 2. + offset,
            }
        }
        Rule::Fraction(rule) => {
            let fraction = context.scalar(&rule.fraction, &format!("{pointer}/fraction"))?;
            if !(0. ..=1.).contains(&fraction) {
                return Err(context.fail(
                    pointer,
                    "ITEM_OPENING",
                    "A fraction lies between 0 and 1.",
                ));
            }
            fraction * (available - size)
        }
        Rule::Every(_) | Rule::Point(_) => {
            return Err(context.fail(
                pointer,
                "ITEM_OPENING",
                "Openings use a from/offset or fraction rule; every and point are for fittings.",
            ));
        }
    })
}

pub(super) fn resolve(
    context: &Context<'_>,
    definition: &PanelDefinition,
    pointer: &str,
) -> Result<Panel, ItemDiagnostic> {
    let width = context.dimension(&definition.width, &format!("{pointer}/width"))?;
    let height = context.dimension(&definition.height, &format!("{pointer}/height"))?;
    let thickness = context.dimension(&definition.thickness, &format!("{pointer}/thickness"))?;
    let (radius, segments) = context.bevel(
        pointer,
        &definition.bevel,
        &definition.bevel_segments,
        (thickness - SLIVER)
            .min(width.min(height) / 2. - 5e-4)
            .max(0.),
    )?;
    let bevel = Bevel { radius, segments };
    let arris = Arris {
        radius: context.scalar(&definition.arris, &format!("{pointer}/arris"))?,
        turns: [
            context.scalar(&definition.arris_start, &format!("{pointer}/arrisStart"))?,
            context.scalar(&definition.arris_end, &format!("{pointer}/arrisEnd"))?,
        ],
    };
    let setbacks = arris.setbacks();
    if arris.radius < 0.
        || arris.turns.iter().any(|turn| !(0. ..=8.).contains(turn))
        || setbacks.iter().any(|setback| *setback > thickness - SLIVER)
        || setbacks[0] + setbacks[1] > width - 2. * MARGIN
        || (!arris.none() && radius > 0.)
    {
        return Err(context.fail(
            &format!("{pointer}/arris"),
            "ITEM_GEOMETRY",
            "An arris is a radius whose rounds (the radius times the tangent of each end's half turn, 0 to 8) fit in the panel's thickness and width; a panel has a bevel or arrises, not both.",
        ));
    }
    // An end that stays sharp keeps the arris out of the geometry key.
    let arris = if arris.none() {
        Arris {
            radius: 0.,
            turns: [0.; 2],
        }
    } else {
        arris
    };
    let setbacks = arris.setbacks();
    // A rounded rim takes its radius from the panel's edge and from round every opening.
    let margin = if radius > 0. {
        MARGIN.max(2. * radius + 0.002)
    } else {
        MARGIN
    };
    if definition.openings.len() > 32 {
        return Err(context.fail(
            &format!("{pointer}/openings"),
            "ITEM_LIMIT",
            "A panel has at most 32 openings.",
        ));
    }
    let mut ids = BTreeSet::new();
    let mut openings: Vec<Opening> = Vec::new();
    // Repeated openings expand into copies whose index joins the derived values they read.
    let mut expanded = Vec::new();
    for (index, opening) in definition.openings.iter().enumerate() {
        let pointer = format!("{pointer}/openings/{index}");
        let copies = match &opening.repeat {
            None => vec![(opening.id.clone(), context.derived.clone())],
            Some(axis) => {
                let identifier = !axis.name.is_empty()
                    && axis.name.len() <= 64
                    && axis
                        .name
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'_')
                    && !axis.name.as_bytes()[0].is_ascii_digit()
                    && !context.definition.parameters.contains_key(&axis.name)
                    && !context.derived.contains_key(&axis.name);
                let count = context.scalar(&axis.count, &format!("{pointer}/repeat/count"))?;
                if !identifier || count.fract() != 0. || !(0. ..=32.).contains(&count) {
                    return Err(context.fail(
                        &format!("{pointer}/repeat"),
                        "ITEM_REPEAT",
                        "An opening repeats up to 32 times, indexed by a new identifier.",
                    ));
                }
                (0..count as usize)
                    .map(|copy| {
                        let mut derived = context.derived.clone();
                        derived.insert(axis.name.clone(), copy as f64);
                        derived.insert(format!("{}_count", axis.name), count);
                        (format!("{}-{copy}", opening.id), derived)
                    })
                    .collect()
            }
        };
        for (id, derived) in copies {
            let copy = Context {
                definition: context.definition,
                parameters: context.parameters,
                derived: &derived,
            };
            if let Some(condition) = &opening.when {
                if copy.scalar(condition, &format!("{pointer}/when"))? <= 0. {
                    continue;
                }
            }
            expanded.push((opening, pointer.clone(), id, derived));
        }
    }
    if expanded.len() > 32 {
        return Err(context.fail(
            &format!("{pointer}/openings"),
            "ITEM_LIMIT",
            "A panel has at most 32 openings, counting every copy.",
        ));
    }
    for (opening, pointer, id, derived) in expanded {
        let copy = Context {
            definition: context.definition,
            parameters: context.parameters,
            derived: &derived,
        };
        let mut resolved = resolve_opening(
            &copy,
            opening,
            [width, height, thickness],
            margin,
            setbacks,
            &pointer,
        )?;
        resolved.id = id;
        if resolved.jambs > 0. && radius > 0. {
            return Err(context.fail(
                &format!("{pointer}/jambs"),
                "ITEM_OPENING",
                "A bevelled panel already rounds its openings' rims.",
            ));
        }
        if resolved.recess.is_some_and(|depth| depth < radius + SLIVER) {
            return Err(context.fail(
                &format!("{pointer}/depth"),
                "ITEM_OPENING",
                "A recess is deeper than the panel's bevel.",
            ));
        }
        if !ids.insert(resolved.id.clone()) {
            return Err(context.fail(
                &format!("{pointer}/id"),
                "ITEM_OPENING",
                "Opening IDs are unique within their panel.",
            ));
        }
        // Rounded jambs take their radius from the wall beside the opening.
        let between = margin + resolved.jambs;
        if let Some(other) = openings.iter().find(|other| {
            resolved.x < other.x + other.width + other.jambs + between - 1e-9
                && other.x < resolved.x + resolved.width + other.jambs + between - 1e-9
                && resolved.z < other.z + other.height + margin - 1e-9
                && other.z < resolved.z + resolved.height + margin - 1e-9
        }) {
            return Err(context.fail(
                &pointer,
                "ITEM_OPENING",
                format!(
                    "Opening {} overlaps opening {} or leaves less than {margin} m between them.",
                    resolved.id, other.id
                ),
            ));
        }
        openings.push(resolved);
    }
    let anchor = match &definition.pattern_anchor {
        Some(anchor) => [
            context.scalar(&anchor[0], &format!("{pointer}/patternAnchor/0"))?,
            context.scalar(&anchor[1], &format!("{pointer}/patternAnchor/1"))?,
        ],
        None => [0., 0.],
    };
    Ok(Panel {
        width,
        height,
        thickness,
        openings,
        anchor,
        bevel,
        arris,
    })
}

fn resolve_opening(
    context: &Context<'_>,
    opening: &OpeningDefinition,
    [panel_width, panel_height, panel_thickness]: [f64; 3],
    margin: f64,
    ends: [f64; 2],
    pointer: &str,
) -> Result<Opening, ItemDiagnostic> {
    let fail = |field: &str, message: String| {
        context.fail(&format!("{pointer}{field}"), "ITEM_OPENING", message)
    };
    if !super::schema::bounded_name(&opening.id) {
        return Err(fail(
            "/id",
            "Opening ID must be a bounded identifier.".into(),
        ));
    }
    let positive = |value, field: &str| -> Result<f64, ItemDiagnostic> {
        let number = context.scalar(value, &format!("{pointer}{field}"))?;
        if number <= 0. {
            return Err(fail(field, "Opening dimensions are positive.".into()));
        }
        Ok(number)
    };
    let width = positive(&opening.width, "/width")?;
    let height = positive(&opening.height, "/height")?;
    let x = place(
        context,
        &opening.u,
        panel_width,
        width,
        &format!("{pointer}/u"),
    )?;
    let z = place(
        context,
        &opening.v,
        panel_height,
        height,
        &format!("{pointer}/v"),
    )?;
    let jambs = context.scalar(&opening.jambs, &format!("{pointer}/jambs"))?;
    // Rounded jambs, and the panel's rounded ends, take their width from the front.
    if x < margin + jambs + ends[0] - 1e-9
        || z < margin - 1e-9
        || x + width > panel_width - margin - jambs - ends[1] + 1e-9
        || z + height > panel_height - margin + 1e-9
    {
        return Err(fail(
            "",
            format!(
                "Opening {} spans u {:.4} to {:.4} and v {:.4} to {:.4}; it must stay {margin} m inside the {panel_width} by {panel_height} m panel.",
                opening.id,
                x,
                x + width,
                z,
                z + height
            ),
        ));
    }
    let arch = opening.shape == OpeningShape::Arch;
    if arch && height - width / 2. < SLIVER {
        return Err(fail(
            "/height",
            "An arch is taller than half its width.".into(),
        ));
    }
    let recess = match (opening.mode, &opening.depth) {
        (OpeningMode::Recess, Some(depth)) => {
            let depth = positive(depth, "/depth")?;
            if depth > panel_thickness - SLIVER {
                return Err(fail(
                    "/depth",
                    "A recess is shallower than the panel thickness.".into(),
                ));
            }
            Some(depth)
        }
        (OpeningMode::Recess, None) => {
            return Err(fail("", "A recess needs a depth.".into()));
        }
        (OpeningMode::Through, Some(_)) => {
            return Err(fail("/depth", "Only a recess has a depth.".into()));
        }
        (OpeningMode::Through, None) => None,
    };
    if jambs < 0. || jambs > recess.unwrap_or(panel_thickness) - SLIVER || (jambs > 0. && arch) {
        return Err(fail(
            "/jambs",
            "Rounded jambs are a radius smaller than the depth of a rectangular opening.".into(),
        ));
    }
    if recess.is_some() && (opening.frame.is_some() || opening.glazing.is_some()) {
        return Err(fail(
            "",
            "Frames and glazing belong to through openings.".into(),
        ));
    }
    let frame = match &opening.frame {
        Some(frame) => {
            let border = positive(&frame.border, "/frame/border")?;
            let depth = positive(&frame.depth, "/frame/depth")?;
            let rise = if arch { height - width / 2. } else { height };
            if 2. * border > width.min(height) - SLIVER || border > rise - SLIVER {
                return Err(fail(
                    "/frame/border",
                    "The frame border leaves no opening.".into(),
                ));
            }
            if depth > panel_thickness {
                return Err(fail(
                    "/frame/depth",
                    "The frame is no deeper than the panel.".into(),
                ));
            }
            Some([border, depth])
        }
        None => None,
    };
    let glazing = match &opening.glazing {
        Some(glazing) => {
            let thickness = positive(&glazing.thickness, "/glazing/thickness")?;
            if thickness > panel_thickness {
                return Err(fail(
                    "/glazing/thickness",
                    "Glazing is no thicker than the panel.".into(),
                ));
            }
            Some(thickness)
        }
        None => None,
    };
    let sill = match &opening.sill {
        Some(sill) => {
            let depth = positive(&sill.depth, "/sill/depth")?;
            let overhang = context.scalar(&sill.overhang, &format!("{pointer}/sill/overhang"))?;
            let thickness = match &sill.thickness {
                Some(thickness) => positive(thickness, "/sill/thickness")?,
                None => DEFAULT_SILL_THICKNESS,
            };
            if depth > recess.unwrap_or(panel_thickness)
                || !(0. ..=1.).contains(&overhang)
                || thickness > height - SLIVER
            {
                return Err(fail(
                    "/sill",
                    "The sill fits inside its opening, with an overhang from 0 to 1 m.".into(),
                ));
            }
            Some(Sill {
                depth,
                overhang,
                thickness,
            })
        }
        None => None,
    };
    Ok(Opening {
        id: opening.id.clone(),
        arch,
        recess,
        x,
        z,
        width,
        height,
        frame,
        glazing,
        sill,
        jambs,
    })
}

impl Opening {
    /// Counter-clockwise in (x, z), shrunk by `inset` on every side.
    fn outline(&self, inset: f64) -> Vec<P2> {
        let (x0, x1) = (self.x + inset, self.x + self.width - inset);
        let (z0, z1) = (self.z + inset, self.z + self.height - inset);
        if !self.arch {
            return vec![[x0, z0], [x1, z0], [x1, z1], [x0, z1]];
        }
        let radius = self.width / 2. - inset;
        let center = [
            self.x + self.width / 2.,
            self.z + self.height - self.width / 2.,
        ];
        let mut points = vec![[x0, z0], [x1, z0], [x1, center[1]]];
        for step in 1..ARCH_SEGMENTS {
            let angle = step as f64 / ARCH_SEGMENTS as f64 * std::f64::consts::PI;
            points.push([
                center[0] + radius * cos(angle),
                center[1] + radius * sin(angle),
            ]);
        }
        points.push([x0, center[1]]);
        points
    }
}

impl Panel {
    /// The rounded jambs of a rectangular opening: on each side a quarter round in two chords
    /// from the front (whose group it joins, with normals that turn to the reveal's) to the
    /// reveal, and the head and the foot of the reveal, which widen to the front between them.
    fn jambs(surfaces: &mut Surfaces<'_>, opening: &Opening, depth: f64) -> Result<(), String> {
        let j = opening.jambs;
        let half = 0.5f64.sqrt();
        let (z0, z1) = (opening.z, opening.z + opening.height);
        let arc = j * std::f64::consts::FRAC_PI_4;
        // Where a side's round leaves the front, its middle, and where it meets the reveal.
        let round = |x: f64, away: f64| -> [P2; 3] {
            [
                [x + away * j, 0.],
                [x + away * j * (1. - half), j * (1. - half)],
                [x, j],
            ]
        };
        // Each side: the jamb's plane, and which way the wall beside it lies.
        for (x, away) in [(opening.x, -1.), (opening.x + opening.width, 1.)] {
            let points = round(x, away);
            let normals: [V3; 3] = [[0., -1., 0.], [-away * half, -half, 0.], [-away, 0., 0.]];
            for k in 0..2 {
                let (a, b) = (points[k], points[k + 1]);
                let u = |k: usize| x + away * (j - arc * k as f64);
                surfaces.polygon(
                    "front",
                    &[
                        [a[0], a[1], z0],
                        [b[0], b[1], z0],
                        [b[0], b[1], z1],
                        [a[0], a[1], z1],
                    ],
                    [-away * half, -half, 0.],
                    &[[u(k), z0], [u(k + 1), z0], [u(k + 1), z1], [u(k), z1]],
                    Some(&[normals[k], normals[k + 1], normals[k + 1], normals[k]]),
                )?;
            }
        }
        // Head and foot, counterclockwise seen from above: along the front, round the right
        // jamb, along the back and round the left jamb.
        let (left, right) = (round(opening.x, -1.), round(opening.x + opening.width, 1.));
        let outline = vec![
            left[0],
            right[0],
            right[1],
            right[2],
            [right[2][0], depth],
            [left[2][0], depth],
            left[2],
            left[1],
        ];
        let profile = triangulate(
            vec![outline.iter().map(|p| [p[0], p[1], 0.]).collect()],
            &[],
            &[],
        )?;
        let points: Vec<P2> = profile.points.iter().map(|p| [p[0], p[1]]).collect();
        for (z, facing) in [(z0, 1.), (z1, -1.)] {
            surfaces.indexed(
                "reveal",
                &points.iter().map(|p| [p[0], p[1], z]).collect::<Vec<_>>(),
                &vec![[0., 0., facing]; points.len()],
                &points,
                &profile.triangles,
                facing < 0.,
            )?;
        }
        Ok(())
    }
    /// Width, height and thickness.
    pub(super) fn size(&self) -> [f64; 3] {
        [self.width, self.height, self.thickness]
    }
    /// What the openings take from a face, as (id, [x, z, width, height]) in front-face
    /// coordinates: every opening on the front, the through ones on the back.
    pub(super) fn cuts(&self, back: bool) -> Vec<(&str, [f64; 4])> {
        self.openings
            .iter()
            .filter(|opening| !back || opening.recess.is_none())
            .map(|opening| {
                (
                    opening.id.as_str(),
                    [opening.x, opening.z, opening.width, opening.height],
                )
            })
            .collect()
    }
    pub(super) fn build(&self, surfaces: &mut Surfaces<'_>) -> Result<(), String> {
        let (w, h, t) = (self.width, self.height, self.thickness);
        // A bevel rounds the front's rim: the front face shrinks by its radius, and the edge
        // faces and reveals start that far behind it.
        let (r, chords) = (self.bevel.radius, self.bevel.segments);
        // This frame's normal is -Y: depth 0 is the front face.
        let across = Frame {
            origin: [0.; 3],
            x: [1., 0., 0.],
            y: [0., 0., 1.],
        };
        // Front and back: the panel rectangle with the openings as holes.
        for (group, y, through_only, facing) in [
            ("front", 0., false, [0., -1., 0.]),
            ("back", t, true, [0., 1., 0.]),
        ] {
            let front = group == "front";
            let g = if front { r } else { 0. };
            // Rounded ends start where the front leaves off.
            let [start, end] = if front {
                self.arris.setbacks()
            } else {
                [0.; 2]
            };
            let mut loops: Vec<Vec<V3>> = vec![vec![
                [g + start, g, 0.],
                [w - g - end, g, 0.],
                [w - g - end, h - g, 0.],
                [g + start, h - g, 0.],
            ]];
            for opening in &self.openings {
                if !through_only || opening.recess.is_none() {
                    let mut outline = opening.outline(0.);
                    if g > 0. {
                        outline = inset(&outline, -g);
                    }
                    if front && opening.jambs > 0. {
                        // Rounded jambs widen the opening in the front face by their radius.
                        let (x0, x1) = (opening.x - opening.jambs, opening.x + opening.width);
                        let (z0, z1) = (opening.z, opening.z + opening.height);
                        outline = vec![
                            [x0, z0],
                            [x1 + opening.jambs, z0],
                            [x1 + opening.jambs, z1],
                            [x0, z1],
                        ];
                    }
                    loops.push(outline.iter().map(|p| [p[0], p[1], 0.]).collect());
                }
            }
            let profile = triangulate(loops, &[], &[])?;
            surfaces.indexed(
                group,
                &profile
                    .points
                    .iter()
                    .map(|p| [p[0], y, p[1]])
                    .collect::<Vec<_>>(),
                &vec![facing; profile.points.len()],
                &profile
                    .points
                    .iter()
                    .map(|p| [if front { p[0] } else { w - p[0] }, p[1]])
                    .collect::<Vec<_>>(),
                &profile.triangles,
                !front,
            )?;
        }
        // The four edge faces, with one texture coordinate running around the perimeter.
        let corners = [[0., 0.], [w, 0.], [w, h], [0., h]];
        let halves = [self.arris.half(0, w), self.arris.half(1, w)];
        let setbacks = self.arris.setbacks();
        let mut along = 0.;
        for side in 0..4 {
            let (a, b): (P2, P2) = (corners[side], corners[(side + 1) % 4]);
            let span = (b[0] - a[0]).abs() + (b[1] - a[1]).abs();
            let outward = [(b[1] - a[1]) / span, 0., -(b[0] - a[0]) / span];
            if !self.arris.none() && side % 2 == 0 {
                // The bottom and the top lose the corners their rounded ends cut off: the front
                // edge from the start's round to the end's, then the back edge.
                let mut outline: Vec<P2> = match halves[0] {
                    Some([leaves, middle, _]) => vec![[0., setbacks[0]], middle, leaves],
                    None => vec![[0., 0.]],
                };
                match halves[1] {
                    Some([leaves, middle, _]) => {
                        outline.extend([leaves, middle, [w, setbacks[1]]]);
                    }
                    None => outline.push([w, 0.]),
                }
                outline.extend([[w, t], [0., t]]);
                surfaces.polygon(
                    "edge",
                    &outline
                        .iter()
                        .map(|p| [p[0], p[1], a[1]])
                        .collect::<Vec<_>>(),
                    outward,
                    &outline
                        .iter()
                        .map(|p| [along + if side == 0 { p[0] } else { w - p[0] }, p[1]])
                        .collect::<Vec<_>>(),
                    None,
                )?;
                along += span;
                continue;
            }
            // An end face starts behind its round.
            let lift = match side {
                1 => r.max(setbacks[1]),
                3 => r.max(setbacks[0]),
                _ => r,
            };
            surfaces.polygon(
                "edge",
                &[
                    [a[0], lift, a[1]],
                    [b[0], lift, b[1]],
                    [b[0], t, b[1]],
                    [a[0], t, a[1]],
                ],
                outward,
                &[
                    [along, lift],
                    [along + span, lift],
                    [along + span, t],
                    [along, t],
                ],
                None,
            )?;
            along += span;
        }
        // Each rounded end: one chord from the front to the middle of the round, which the
        // neighbouring panel's chord continues, with normals that turn across it.
        for (end, half) in halves.into_iter().enumerate() {
            let Some([leaves, middle, normal]) = half else {
                continue;
            };
            let chord = superspec_geometry::math::hypot(middle[0] - leaves[0], middle[1]);
            let u = leaves[0] + if end == 0 { -chord } else { chord };
            let (flat, turned) = ([0., -1., 0.], [normal[0], normal[1], 0.]);
            surfaces.polygon(
                "front",
                &[
                    [leaves[0], 0., 0.],
                    [middle[0], middle[1], 0.],
                    [middle[0], middle[1], h],
                    [leaves[0], 0., h],
                ],
                [normal[0], normal[1] - 1., 0.],
                &[[leaves[0], 0.], [u, 0.], [u, h], [leaves[0], h]],
                Some(&[flat, turned, turned, flat]),
            )?;
        }
        if r > 0. {
            rim(
                surfaces,
                "edge",
                &across,
                &Outline::flat(corners.to_vec()),
                1.,
                -r,
                1.,
                [r, r, -1.],
                chords,
            )?;
        }
        for opening in &self.openings {
            let ring = opening.outline(0.);
            let depth = opening.recess.unwrap_or(t);
            let center = [
                opening.x + opening.width / 2.,
                opening.z + opening.height / 2.,
            ];
            let mut along = 0.;
            for i in 0..ring.len() {
                let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
                let span = superspec_geometry::math::hypot(b[0] - a[0], b[1] - a[1]);
                let middle = [(a[0] + b[0]) / 2., (a[1] + b[1]) / 2.];
                // A rounded jamb's reveal starts behind its round (a rectangle's jambs are its
                // second and fourth edges); the head and the foot between them follow the rounds.
                let r = if opening.jambs > 0. {
                    if i % 2 == 0 {
                        along += span;
                        continue;
                    }
                    opening.jambs
                } else {
                    r
                };
                surfaces.polygon(
                    "reveal",
                    &[
                        [a[0], r, a[1]],
                        [b[0], r, b[1]],
                        [b[0], depth, b[1]],
                        [a[0], depth, a[1]],
                    ],
                    [center[0] - middle[0], 0., center[1] - middle[1]],
                    &[
                        [along, r],
                        [along + span, r],
                        [along + span, depth],
                        [along, depth],
                    ],
                    None,
                )?;
                along += span;
            }
            if opening.jambs > 0. {
                Self::jambs(surfaces, opening, depth)?;
            }
            if r > 0. {
                rim(
                    surfaces,
                    "reveal",
                    &across,
                    &Outline::flat(ring.clone()),
                    -1.,
                    -r,
                    1.,
                    [r, r, -1.],
                    chords,
                )?;
            }
            if opening.recess.is_some() {
                surfaces.polygon(
                    "reveal",
                    &ring.iter().map(|p| [p[0], depth, p[1]]).collect::<Vec<_>>(),
                    [0., -1., 0.],
                    &ring,
                    None,
                )?;
            }
            // Frame and glazing sit at the middle of the thickness.
            let slab = |size: f64| [-(t + size) / 2., -(t - size) / 2.];
            if let Some([border, frame_depth]) = opening.frame {
                prism(
                    surfaces,
                    ["frame"; 3],
                    &across,
                    &Outline::flat(ring.clone()),
                    Some(&Outline::flat(opening.outline(border))),
                    slab(frame_depth),
                )?;
            }
            if let Some(glazing) = opening.glazing {
                prism(
                    surfaces,
                    ["glazing"; 3],
                    &across,
                    &Outline::flat(opening.outline(opening.frame.map_or(0., |frame| frame[0]))),
                    None,
                    slab(glazing),
                )?;
            }
            if let Some(sill) = &opening.sill {
                let (x0, x1) = (opening.x, opening.x + opening.width);
                let o = sill.overhang;
                let outline = if o > 0. {
                    vec![
                        [x0 - o, -o],
                        [x1 + o, -o],
                        [x1 + o, 0.],
                        [x1, 0.],
                        [x1, sill.depth],
                        [x0, sill.depth],
                        [x0, 0.],
                        [x0 - o, 0.],
                    ]
                } else {
                    vec![[x0, 0.], [x1, 0.], [x1, sill.depth], [x0, sill.depth]]
                };
                prism(
                    surfaces,
                    ["sill"; 3],
                    &Frame {
                        origin: [0.; 3],
                        x: [1., 0., 0.],
                        y: [0., 1., 0.],
                    },
                    &Outline::flat(outline),
                    None,
                    [opening.z, opening.z + sill.thickness],
                )?;
            }
        }
        Ok(())
    }

    /// Features and the pattern anchor in part coordinates.
    pub(super) fn features(&self, part: &str) -> (Vec<Feature>, PatternAnchor) {
        let (w, h, t) = (self.width, self.height, self.thickness);
        let mut features = Vec::new();
        let mut feature = |name: &str, source: Option<&str>, y: f64, points: &[P2], closed| {
            features.push(Feature {
                name: name.into(),
                part: part.into(),
                source: source.map(str::to_owned),
                face: if y == 0. { "front" } else { "back" }.into(),
                points: points.iter().map(|p| [p[0], y, p[1]]).collect(),
                closed,
                normal: [0., if y == 0. { -1. } else { 1. }, 0.],
                width: 0.,
                reach: Default::default(),
                within: 0.,
            });
        };
        for y in [0., t] {
            feature("base", None, y, &[[0., 0.], [w, 0.]], false);
            feature("top", None, y, &[[0., h], [w, h]], false);
        }
        for opening in &self.openings {
            for y in [0., t] {
                if y > 0. && opening.recess.is_some() {
                    continue;
                }
                let id = Some(opening.id.as_str());
                feature("opening-edge", id, y, &opening.outline(0.), true);
                // The line water leaves from: the opening's bottom edge, or the sill's nose.
                let reach = match &opening.sill {
                    Some(sill) if y == 0. => sill.overhang,
                    _ => 0.,
                };
                feature(
                    "sill",
                    id,
                    y,
                    &[
                        [opening.x - reach, opening.z],
                        [opening.x + opening.width + reach, opening.z],
                    ],
                    false,
                );
            }
        }
        (
            features,
            PatternAnchor {
                part: part.into(),
                origin: [self.anchor[0], 0., self.anchor[1]],
                u: [1., 0., 0.],
                v: [0., 0., 1.],
                normal: [0., -1., 0.],
            },
        )
    }
}
