//! The `route` generator: a round or rectangular section swept along a filleted
//! centreline, with fittings placed by station (arc length from the start).
//!
//! Fittings are separate closed solids that overlap the body; nothing is subtracted.
use super::{
    Attachment, Feature, ItemDiagnostic, PatternAnchor,
    parts::Context,
    schema::{Edge, FittingDefinition, Handle, RouteDefinition, Rule, SectionDefinition},
    surfaces::{Frame, Outline, P2, Surfaces, prism},
};
use serde::Serialize;
use std::collections::BTreeSet;
use superspec_geometry::{
    V3,
    math::{acos, add, cos, cross, dot, length, scale, sin, sub, unit},
};

pub(super) const GROUPS: &[&str] = &["body", "fitting", "bracket", "bolt", "end"];
const DEFAULT_BRACKET_WIDTH: f64 = 0.04;
const DEFAULT_BRACKET_THICKNESS: f64 = 0.006;

#[derive(Clone, Debug, Serialize)]
pub(super) enum Section {
    Round { radius: f64, segments: usize },
    Rect { width: f64, height: f64 },
}
/// One point of a section outline: its position and outward normal in the section
/// plane, and its distance around the perimeter.
#[derive(Clone, Copy)]
struct Corner {
    at: P2,
    normal: P2,
    u: f64,
}
impl Section {
    /// Half extents along the section's own axes.
    fn half(&self) -> P2 {
        match self {
            Self::Round { radius, .. } => [*radius, *radius],
            Self::Rect { width, height } => [width / 2., height / 2.],
        }
    }
    /// Distance from the centre to the surface along a unit direction in the section plane.
    pub(super) fn support(&self, direction: P2) -> f64 {
        match self {
            Self::Round { radius, .. } => *radius,
            Self::Rect { width, height } => {
                direction[0].abs() * width / 2. + direction[1].abs() * height / 2.
            }
        }
    }
    fn outline(&self, grow: f64) -> Outline {
        match self {
            Self::Round { radius, segments } => {
                Outline::circle([0., 0.], radius + grow, *segments, true)
            }
            Self::Rect { width, height } => Outline::rectangle(
                [-width / 2. - grow, -height / 2. - grow],
                [width / 2. + grow, height / 2. + grow],
            ),
        }
    }
    /// The outline as strips to sweep; each strip is one flat or smooth side.
    fn strips(&self, shrink: f64) -> Vec<[Corner; 2]> {
        match self {
            Self::Round { radius, segments } => {
                let radius = radius - shrink;
                let ring = Outline::circle([0., 0.], radius, *segments, true);
                let normals = ring.normals.unwrap_or_default();
                let step = std::f64::consts::TAU * radius / *segments as f64;
                (0..*segments)
                    .map(|j| {
                        let k = (j + 1) % segments;
                        [
                            Corner {
                                at: ring.points[j],
                                normal: normals[j],
                                u: j as f64 * step,
                            },
                            Corner {
                                at: ring.points[k],
                                normal: normals[k],
                                u: (j + 1) as f64 * step,
                            },
                        ]
                    })
                    .collect()
            }
            Self::Rect { width, height } => {
                let (w, h) = (width / 2. - shrink, height / 2. - shrink);
                let corners = [[-w, -h], [w, -h], [w, h], [-w, h]];
                let normals = [[0., -1.], [1., 0.], [0., 1.], [-1., 0.]];
                let mut along = 0.;
                (0..4)
                    .map(|side| {
                        let (a, b): (P2, P2) = (corners[side], corners[(side + 1) % 4]);
                        let span = (b[0] - a[0]).abs() + (b[1] - a[1]).abs();
                        let strip = [
                            Corner {
                                at: a,
                                normal: normals[side],
                                u: along,
                            },
                            Corner {
                                at: b,
                                normal: normals[side],
                                u: along + span,
                            },
                        ];
                        along += span;
                        strip
                    })
                    .collect()
            }
        }
    }
}

/// A point on the centreline with its moving frame; `a` and `b` span the section plane
/// and `cross(a, b)` is the tangent.
#[derive(Clone, Copy, Debug)]
pub(super) struct Station {
    pub s: f64,
    pub position: V3,
    pub tangent: V3,
    pub a: V3,
    pub b: V3,
}
impl Station {
    fn frame(&self) -> Frame {
        Frame {
            origin: self.position,
            x: self.a,
            y: self.b,
        }
    }
    fn point(&self, at: P2) -> V3 {
        self.frame().at(at, 0.)
    }
    fn direction(&self, direction: P2) -> V3 {
        self.frame().direction(direction)
    }
}
#[derive(Clone, Copy, Debug)]
struct Arc {
    center: V3,
    axis: V3,
    angle: f64,
}
#[derive(Clone, Debug)]
struct Element {
    start: Station,
    length: f64,
    arc: Option<Arc>,
}
fn rotate(vector: V3, axis: V3, angle: f64) -> V3 {
    let (s, c) = (sin(angle), cos(angle));
    add(
        add(scale(vector, c), scale(cross(axis, vector), s)),
        scale(axis, dot(axis, vector) * (1. - c)),
    )
}
impl Element {
    fn at(&self, distance: f64) -> Station {
        let start = &self.start;
        match self.arc {
            None => Station {
                s: start.s + distance,
                position: add(start.position, scale(start.tangent, distance)),
                ..*start
            },
            Some(arc) => {
                let angle = arc.angle * distance / self.length;
                Station {
                    s: start.s + distance,
                    position: add(
                        arc.center,
                        rotate(sub(start.position, arc.center), arc.axis, angle),
                    ),
                    tangent: rotate(start.tangent, arc.axis, angle),
                    a: rotate(start.a, arc.axis, angle),
                    b: rotate(start.b, arc.axis, angle),
                }
            }
        }
    }
}

/// Straight runs joined by circular arcs of the bend radius.
#[derive(Clone, Debug, Default)]
pub(super) struct Path {
    elements: Vec<Element>,
    pub length: f64,
    /// Station of each route point; a filleted corner maps to the middle of its arc.
    points: Vec<f64>,
}
impl Path {
    pub(super) fn at(&self, s: f64) -> Station {
        let s = s.clamp(0., self.length);
        let element = self
            .elements
            .iter()
            .find(|element| s <= element.start.s + element.length)
            .unwrap_or_else(|| self.elements.last().expect("a path has elements"));
        element.at((s - element.start.s).clamp(0., element.length))
    }
    /// Whether the span lies on the route and touches no bend.
    pub(super) fn straight(&self, from: f64, to: f64) -> bool {
        from >= -1e-9
            && to <= self.length + 1e-9
            && !self.elements.iter().any(|element| {
                element.arc.is_some()
                    && element.start.s < to - 1e-9
                    && element.start.s + element.length > from + 1e-9
            })
    }
}

#[derive(Clone, Debug, Serialize)]
pub(super) enum FittingShape {
    Coupling {
        length: f64,
        clearance: f64,
    },
    Flange {
        overhang: f64,
        thickness: f64,
        bolts: usize,
    },
    Valve {
        length: f64,
        lever: bool,
    },
    /// `direction` is a unit vector perpendicular to the route, toward the support.
    Bracket {
        reach: f64,
        direction: V3,
        width: f64,
        thickness: f64,
    },
    Tee {
        direction: V3,
        length: f64,
    },
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct Fitting {
    id: String,
    station: f64,
    shape: FittingShape,
}
impl Fitting {
    /// Half the length the fitting occupies along the route.
    fn half(&self, section: &Section) -> f64 {
        match &self.shape {
            FittingShape::Coupling { length, .. } | FittingShape::Valve { length, .. } => {
                length / 2.
            }
            FittingShape::Flange { thickness, .. } => thickness / 2.,
            FittingShape::Bracket { width, .. } => width / 2.,
            FittingShape::Tee { .. } => section.half()[0],
        }
    }
}

/// The section the body and end caps are drawn with where it is not the route's own.
#[derive(Clone, Debug, Serialize)]
struct Drawn {
    section: Section,
    /// The drawn section's centre from the centreline, along the station's `a` and `b`.
    offset: P2,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct Route {
    section: Section,
    /// Absent when the body is drawn as `section`, which leaves such a route's key as it was.
    #[serde(skip_serializing_if = "Option::is_none")]
    drawn: Option<Drawn>,
    wall: f64,
    points: Vec<V3>,
    bend_radius: f64,
    bend_segments: usize,
    up: V3,
    capped: bool,
    fittings: Vec<Fitting>,
    /// Derived from the fields above, so it stays out of the geometry key.
    #[serde(skip)]
    path: Path,
}

fn integer(
    context: &Context<'_>,
    value: f64,
    range: std::ops::RangeInclusive<f64>,
    pointer: &str,
    code: &str,
    what: &str,
) -> Result<usize, ItemDiagnostic> {
    if value.fract() != 0. || !range.contains(&value) {
        return Err(context.fail(
            pointer,
            code,
            format!(
                "{what} must be an integer from {} to {}.",
                range.start(),
                range.end()
            ),
        ));
    }
    Ok(value as usize)
}

pub(super) fn resolve(
    context: &Context<'_>,
    definition: &RouteDefinition,
    pointer: &str,
    origin: V3,
) -> Result<Route, ItemDiagnostic> {
    let route_error = |field: &str, message: String| {
        context.fail(&format!("{pointer}{field}"), "ITEM_ROUTE", message)
    };
    let positive = |value: &super::schema::Scalar, field: &str| -> Result<f64, ItemDiagnostic> {
        let number = context.scalar(value, &format!("{pointer}{field}"))?;
        if number <= 0. || number > 10_000. {
            return Err(route_error(
                field,
                "Route dimensions must be positive and at most 10000 meters.".into(),
            ));
        }
        Ok(number)
    };
    let wall = context.scalar(
        &definition.wall_thickness,
        &format!("{pointer}/wallThickness"),
    )?;
    let resolve_section =
        |definition_section: &SectionDefinition, field: &str| -> Result<Section, ItemDiagnostic> {
            Ok(match definition_section {
                SectionDefinition::Round { radius } => Section::Round {
                    radius: positive(radius, &format!("{field}/radius"))?,
                    segments: integer(
                        context,
                        context.scalar(&definition.segments, &format!("{pointer}/segments"))?,
                        3. ..=256.,
                        &format!("{pointer}/segments"),
                        "ITEM_ROUTE",
                        "Route segments",
                    )?,
                },
                SectionDefinition::Rect { width, height } => Section::Rect {
                    width: positive(width, &format!("{field}/width"))?,
                    height: positive(height, &format!("{field}/height"))?,
                },
            })
        };
    let section = resolve_section(&definition.section, "/section")?;
    let half = section.half();
    if wall < 0. || wall >= half[0].min(half[1]) {
        return Err(route_error(
            "/wallThickness",
            "Wall thickness must be nonnegative and leave a positive inner section.".into(),
        ));
    }
    let drawn = match &definition.drawn {
        None => None,
        Some(drawn) => {
            let drawn_section = resolve_section(&drawn.section, "/drawn/section")?;
            let mut offset = [0.; 2];
            for (axis, value) in drawn.offset.iter().enumerate() {
                offset[axis] = context.scalar(value, &format!("{pointer}/drawn/offset/{axis}"))?;
                if offset[axis].abs() > 10_000. {
                    return Err(route_error(
                        "/drawn/offset",
                        "A drawn section sits at most 10000 meters from the centreline.".into(),
                    ));
                }
            }
            let drawn_half = drawn_section.half();
            if wall >= drawn_half[0].min(drawn_half[1]) {
                return Err(route_error(
                    "/drawn/section",
                    "Wall thickness must leave the drawn section a positive inner section.".into(),
                ));
            }
            // A drawn section that is the route's own draws the route exactly as without one.
            let same = match (&section, &drawn_section) {
                (Section::Round { radius: a, .. }, Section::Round { radius: b, .. }) => a == b,
                (
                    Section::Rect { width, height },
                    Section::Rect {
                        width: drawn_width,
                        height: drawn_height,
                    },
                ) => width == drawn_width && height == drawn_height,
                _ => false,
            };
            (!same || offset != [0.; 2]).then_some(Drawn {
                section: drawn_section,
                offset,
            })
        }
    };
    let bend_segments = integer(
        context,
        context.scalar(
            &definition.bend_segments,
            &format!("{pointer}/bendSegments"),
        )?,
        1. ..=64.,
        &format!("{pointer}/bendSegments"),
        "ITEM_ROUTE",
        "Bend segments",
    )?;
    let bend_radius = context.scalar(&definition.bend_radius, &format!("{pointer}/bendRadius"))?;
    use super::schema::{RoutePoints, Scalar};
    let supplied;
    let vertices = match &definition.points {
        RoutePoints::Inline(points) => points,
        RoutePoints::Parameter(reference) => {
            let coordinates = context
                .parameters
                .get(&reference.parameter)
                .ok_or_else(|| route_error("/points", "Unknown route point parameter.".into()))?;
            let coordinates: Vec<V3> =
                serde_json::from_value(coordinates.clone()).map_err(|_| {
                    route_error("/points", "Expected a list of XYZ route points.".into())
                })?;
            supplied = coordinates
                .into_iter()
                .map(|p| p.map(Scalar::Number))
                .collect();
            &supplied
        }
    };
    if !(2..=128).contains(&vertices.len()) {
        return Err(route_error(
            "/points",
            "A route has from 2 to 128 points.".into(),
        ));
    }
    let mut points = Vec::with_capacity(vertices.len());
    for (index, point) in vertices.iter().enumerate() {
        let mut resolved = origin;
        for axis in 0..3 {
            resolved[axis] +=
                context.scalar(&point[axis], &format!("{pointer}/points/{index}/{axis}"))?;
            if resolved[axis].abs() > 1e7 {
                return Err(route_error(
                    &format!("/points/{index}"),
                    "Route point exceeds coordinate bounds.".into(),
                ));
            }
        }
        points.push(resolved);
    }
    let capped = context.switch(&definition.capped, &format!("{pointer}/capped"))?;

    // Directions and lengths of the runs between points.
    let mut runs = Vec::with_capacity(points.len() - 1);
    for index in 1..points.len() {
        let delta = sub(points[index], points[index - 1]);
        let span = length(delta);
        if span < 1e-6 {
            return Err(route_error(
                &format!("/points/{index}"),
                format!("Route points {} and {index} coincide.", index - 1),
            ));
        }
        runs.push((scale(delta, 1. / span), span));
    }
    // How much of each neighbouring run a corner's fillet uses, and its turn.
    let mut corners: Vec<Option<(f64, f64, V3)>> = vec![None; points.len()];
    for index in 1..points.len() - 1 {
        let (before, after) = (runs[index - 1].0, runs[index].0);
        let c = dot(before, after).clamp(-1., 1.);
        if c > 1. - 1e-12 {
            continue;
        }
        let field = format!("/points/{index}");
        if c < -1. + 1e-9 {
            return Err(route_error(
                &field,
                format!("The route doubles back at point {index}."),
            ));
        }
        let reach = half[0].max(half[1]);
        if bend_radius <= reach {
            return Err(route_error(
                "/bendRadius",
                format!(
                    "Bend radius {bend_radius} must exceed the section's half extent {reach} to turn at point {index}."
                ),
            ));
        }
        if let Some(drawn) = &drawn {
            let drawn_half = drawn.section.half();
            let reach =
                (drawn_half[0] + drawn.offset[0].abs()).max(drawn_half[1] + drawn.offset[1].abs());
            if bend_radius <= reach {
                return Err(route_error(
                    "/drawn/section",
                    format!(
                        "Bend radius {bend_radius} must exceed the drawn section's reach {reach} to turn at point {index}."
                    ),
                ));
            }
        }
        let tangent = bend_radius * ((1. - c) / (1. + c)).sqrt();
        let axis = unit(cross(before, after)).map_err(|e| route_error(&field, e))?;
        corners[index] = Some((tangent, acos(c), axis));
    }
    let first = runs[0].0;
    let up = match definition.up {
        Some(up) => up,
        None if first[2].abs() > 0.999 => [1., 0., 0.],
        None => [0., 0., 1.],
    };
    let b = unit(sub(up, scale(first, dot(up, first)))).map_err(|_| {
        route_error(
            "/up",
            "The up direction must be finite and not parallel to the first run.".into(),
        )
    })?;
    let mut station = Station {
        s: 0.,
        position: points[0],
        tangent: first,
        a: cross(b, first),
        b,
    };
    let mut path = Path {
        points: vec![0.; points.len()],
        ..Path::default()
    };
    let supports: Vec<V3> = match &definition.supports {
        None => vec![],
        Some(reference) => serde_json::from_value(
            context
                .parameters
                .get(&reference.parameter)
                .cloned()
                .unwrap_or_default(),
        )
        .map_err(|_| {
            route_error(
                "/supports",
                "Expected XYZ offsets to mounting faces.".into(),
            )
        })?,
    };
    if !supports.is_empty() && supports.len() != runs.len() {
        return Err(route_error(
            "/supports",
            "Supply one mounting offset for each segment.".into(),
        ));
    }
    let mut mounts = Vec::new();
    for (index, &(direction, span)) in runs.iter().enumerate() {
        let used = corners[index].map_or(0., |c| c.0) + corners[index + 1].map_or(0., |c| c.0);
        let straight = span - used;
        if straight < -1e-9 {
            return Err(route_error(
                &format!("/points/{}", index + 1),
                format!(
                    "Bend radius {bend_radius} does not fit between points {index} and {}: the bends need {used:.4} m of a {span:.4} m run.",
                    index + 1
                ),
            ));
        }
        if straight > 1e-9 {
            if let Some(offset) = supports.get(index).filter(|offset| **offset != [0.; 3]) {
                let reach = length(*offset);
                if !(0.001..=10.).contains(&reach) || dot(direction, *offset).abs() > 1e-5 {
                    return Err(route_error(
                        "/supports",
                        "Mounting offsets must be perpendicular to the segment and at most 10 m."
                            .into(),
                    ));
                }
                let normal = scale(*offset, 1. / reach);
                let skin = section.support([dot(normal, station.a), dot(normal, station.b)]);
                if reach < skin + 2. * DEFAULT_BRACKET_THICKNESS {
                    return Err(route_error(
                        "/supports",
                        "Mounting face leaves insufficient room for its plate.".into(),
                    ));
                }
                if straight >= 0.25 {
                    let count = (straight / 1.8).ceil() as usize;
                    if count > 256 - mounts.len() {
                        return Err(route_error(
                            "/supports",
                            "A route has at most 256 fittings.".into(),
                        ));
                    }
                    for i in 0..count {
                        mounts.push(Fitting {
                            id: format!("mount-{index}-{i}"),
                            station: station.s + straight * (i as f64 + 0.5) / count as f64,
                            shape: FittingShape::Bracket {
                                reach,
                                direction: normal,
                                width: 0.055,
                                thickness: DEFAULT_BRACKET_THICKNESS,
                            },
                        });
                    }
                }
            }
            let element = Element {
                start: station,
                length: straight,
                arc: None,
            };
            station = element.at(straight);
            path.elements.push(element);
        }
        match corners[index + 1] {
            Some((tangent, angle, axis)) => {
                let inward = unit(sub(
                    runs[index + 1].0,
                    scale(direction, dot(direction, runs[index + 1].0)),
                ))
                .map_err(|e| route_error(&format!("/points/{}", index + 1), e))?;
                let element = Element {
                    start: station,
                    length: bend_radius * angle,
                    arc: Some(Arc {
                        center: add(station.position, scale(inward, bend_radius)),
                        axis,
                        angle,
                    }),
                };
                path.points[index + 1] = station.s + element.length / 2.;
                let mut end = element.at(element.length);
                // The arc ends exactly where the next run begins.
                end.position = add(points[index + 1], scale(runs[index + 1].0, tangent));
                end.tangent = runs[index + 1].0;
                station = end;
                path.elements.push(element);
            }
            None => path.points[index + 1] = station.s,
        }
    }
    path.length = station.s;

    let mut route = Route {
        section,
        drawn,
        wall,
        points,
        bend_radius,
        bend_segments,
        up,
        capped,
        fittings: mounts,
        path,
    };
    if definition.fittings.len() > 256 {
        return Err(context.fail(
            &format!("{pointer}/fittings"),
            "ITEM_LIMIT",
            "A route has at most 256 fittings.",
        ));
    }
    let mut ids = BTreeSet::from(["start".to_owned(), "end".to_owned()]);
    ids.extend(route.fittings.iter().map(|mount| mount.id.clone()));
    for (index, fitting) in definition.fittings.iter().enumerate() {
        let pointer = format!("{pointer}/fittings/{index}");
        let stations = stations(context, fitting.at(), &route.path, &format!("{pointer}/at"))?;
        let repeated = matches!(fitting.at(), Rule::Every(_));
        for (copy, station) in stations.into_iter().enumerate() {
            let id = if repeated {
                format!("{}-{}", fitting.id(), copy + 1)
            } else {
                fitting.id().to_owned()
            };
            if !super::schema::bounded_name(fitting.id()) || !ids.insert(id.clone()) {
                return Err(context.fail(
                    &format!("{pointer}/id"),
                    "ITEM_FITTING",
                    format!("Fitting ID {id} must be a bounded identifier, unique in its route."),
                ));
            }
            if route.fittings.len() >= 256 {
                return Err(context.fail(
                    &pointer,
                    "ITEM_LIMIT",
                    "A route has at most 256 fittings.",
                ));
            }
            let shape = fitting_shape(context, fitting, &route, station, &pointer)?;
            let resolved = Fitting { id, station, shape };
            let half = resolved.half(&route.section);
            if !route.path.straight(station - half, station + half) {
                if repeated && definition.omit_unfittable_repeats {
                    continue;
                }
                return Err(context.fail(
                    &pointer,
                    "ITEM_FITTING",
                    format!(
                        "Fitting {} spans stations {:.4} to {:.4}; it must lie on one straight run of the {:.4} m route.",
                        resolved.id,
                        station - half,
                        station + half,
                        route.path.length
                    ),
                ));
            }
            route.fittings.push(resolved);
        }
    }
    Ok(route)
}

/// Stations a placement rule selects on a route.
pub(super) fn stations(
    context: &Context<'_>,
    rule: &Rule,
    path: &Path,
    pointer: &str,
) -> Result<Vec<f64>, ItemDiagnostic> {
    let fail = |message: &str| context.fail(pointer, "ITEM_FITTING", message);
    Ok(match rule {
        Rule::Offset(rule) => {
            let offset = context.scalar(&rule.offset, &format!("{pointer}/offset"))?;
            vec![match rule.from {
                Edge::Start => offset,
                Edge::End => path.length - offset,
                Edge::Center => path.length / 2. + offset,
            }]
        }
        Rule::Fraction(rule) => {
            let fraction = context.scalar(&rule.fraction, &format!("{pointer}/fraction"))?;
            if !(0. ..=1.).contains(&fraction) {
                return Err(fail("A fraction lies between 0 and 1."));
            }
            vec![fraction * path.length]
        }
        Rule::Every(rule) => {
            let every = context.scalar(&rule.every, &format!("{pointer}/every"))?;
            let inset = context.scalar(&rule.inset, &format!("{pointer}/inset"))?;
            let available = path.length - 2. * inset;
            if every <= 0. || inset < 0. || available < -1e-9 {
                return Err(fail(
                    "A repeat needs a positive spacing and insets that fit the route.",
                ));
            }
            let intervals = (available / every - 1e-9).ceil().max(1.);
            if intervals > 255. {
                return Err(context.fail(
                    pointer,
                    "ITEM_LIMIT",
                    "A route has at most 256 fittings.",
                ));
            }
            if available <= 1e-9 {
                vec![path.length / 2.]
            } else {
                (0..=intervals as usize)
                    .map(|k| inset + available * k as f64 / intervals)
                    .collect()
            }
        }
        Rule::Point(rule) => vec![
            *path
                .points
                .get(rule.point)
                .ok_or_else(|| fail("The route has no point with that index."))?,
        ],
    })
}

fn fitting_shape(
    context: &Context<'_>,
    fitting: &FittingDefinition,
    route: &Route,
    station: f64,
    pointer: &str,
) -> Result<FittingShape, ItemDiagnostic> {
    let fail = |field: &str, message: &str| {
        context.fail(&format!("{pointer}{field}"), "ITEM_FITTING", message)
    };
    let positive = |value, field: &str| -> Result<f64, ItemDiagnostic> {
        let number = context.scalar(value, &format!("{pointer}{field}"))?;
        if number <= 0. || number > 10_000. {
            return Err(fail(field, "Fitting dimensions are positive."));
        }
        Ok(number)
    };
    let frame = route.path.at(station);
    // A unit direction perpendicular to the route, and the section surface along it.
    let across = |direction: V3| -> Result<(V3, f64), ItemDiagnostic> {
        let flat = sub(
            direction,
            scale(frame.tangent, dot(direction, frame.tangent)),
        );
        if direction.iter().any(|v| !v.is_finite())
            || length(flat) < 1e-6 * length(direction).max(1e-300)
        {
            return Err(fail(
                "/direction",
                "A fitting direction must be finite and not parallel to the route.",
            ));
        }
        let flat = unit(flat).map_err(|e| fail("/direction", &e))?;
        Ok((
            flat,
            route
                .section
                .support([dot(flat, frame.a), dot(flat, frame.b)]),
        ))
    };
    Ok(match fitting {
        FittingDefinition::Coupling {
            length, clearance, ..
        } => FittingShape::Coupling {
            length: positive(length, "/length")?,
            clearance: positive(clearance, "/clearance")?,
        },
        FittingDefinition::Flange {
            radius,
            overhang,
            thickness,
            bolts,
            ..
        } => {
            let overhang = match (radius, overhang, &route.section) {
                (Some(radius), None, Section::Round { radius: pipe, .. }) => {
                    let radius = positive(radius, "/radius")?;
                    if radius <= *pipe {
                        return Err(fail("/radius", "A flange is wider than its pipe."));
                    }
                    radius - pipe
                }
                (None, Some(overhang), _) => positive(overhang, "/overhang")?,
                _ => {
                    return Err(fail(
                        "",
                        "A flange gives a radius (round sections) or an overhang, not both.",
                    ));
                }
            };
            FittingShape::Flange {
                overhang,
                thickness: positive(thickness, "/thickness")?,
                bolts: integer(
                    context,
                    context.scalar(bolts, &format!("{pointer}/bolts"))?,
                    0. ..=64.,
                    &format!("{pointer}/bolts"),
                    "ITEM_FITTING",
                    "Flange bolts",
                )?,
            }
        }
        FittingDefinition::Valve { length, handle, .. } => FittingShape::Valve {
            length: positive(length, "/length")?,
            lever: *handle == Handle::Lever,
        },
        FittingDefinition::Bracket {
            reach,
            direction,
            width,
            thickness,
            ..
        } => {
            let (direction, surface) = across(*direction)?;
            let reach = positive(reach, "/reach")?;
            let width = match width {
                Some(width) => positive(width, "/width")?,
                None => DEFAULT_BRACKET_WIDTH,
            };
            let thickness = match thickness {
                Some(thickness) => positive(thickness, "/thickness")?,
                None => DEFAULT_BRACKET_THICKNESS,
            };
            if reach < surface + 2. * thickness {
                return Err(fail(
                    "/reach",
                    "A bracket reaches beyond the section's surface.",
                ));
            }
            FittingShape::Bracket {
                reach,
                direction,
                width,
                thickness,
            }
        }
        FittingDefinition::Tee {
            direction, length, ..
        } => {
            let (direction, surface) = across(*direction)?;
            let length = positive(length, "/length")?;
            if length <= surface {
                return Err(fail(
                    "/length",
                    "A tee's branch is measured from the centreline and leaves the section.",
                ));
            }
            FittingShape::Tee { direction, length }
        }
    })
}

impl Route {
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
    pub(super) fn section(&self) -> &Section {
        &self.section
    }
    /// The span each fitting occupies along the route, as (id, from, to).
    pub(super) fn fitting_spans(&self) -> Vec<(&str, f64, f64)> {
        self.fittings
            .iter()
            .map(|fitting| {
                let half = fitting.half(&self.section);
                (
                    fitting.id.as_str(),
                    fitting.station - half,
                    fitting.station + half,
                )
            })
            .collect()
    }
    /// Stations of the cross-sections the body is swept through.
    fn rings(&self) -> Vec<Station> {
        let mut rings = vec![self.path.at(0.)];
        for element in &self.path.elements {
            let steps = if element.arc.is_some() {
                self.bend_segments
            } else {
                1
            };
            for step in 1..=steps {
                rings.push(element.at(element.length * step as f64 / steps as f64));
            }
        }
        // Use the next element's exact start where two elements meet.
        let mut index = 0;
        for element in &self.path.elements {
            rings[index] = element.start;
            index += if element.arc.is_some() {
                self.bend_segments
            } else {
                1
            };
        }
        rings
    }

    /// The strips the body is swept from: the drawn section's where the route gives one.
    fn body_strips(&self, shrink: f64) -> Vec<[Corner; 2]> {
        match &self.drawn {
            None => self.section.strips(shrink),
            Some(drawn) => drawn
                .section
                .strips(shrink)
                .into_iter()
                .map(|strip| {
                    strip.map(|corner| Corner {
                        at: [
                            corner.at[0] + drawn.offset[0],
                            corner.at[1] + drawn.offset[1],
                        ],
                        ..corner
                    })
                })
                .collect(),
        }
    }

    pub(super) fn build(&self, surfaces: &mut Surfaces<'_>) -> Result<(), String> {
        let rings = self.rings();
        let outer = self.body_strips(0.);
        let inner = (self.wall > 0.).then(|| self.body_strips(self.wall));
        for (strips, sign) in [(Some(&outer), 1.), (inner.as_ref(), -1.)] {
            let Some(strips) = strips else { continue };
            for pair in rings.windows(2) {
                for [p, q] in strips {
                    let normals = [
                        scale(pair[0].direction(p.normal), sign),
                        scale(pair[0].direction(q.normal), sign),
                        scale(pair[1].direction(q.normal), sign),
                        scale(pair[1].direction(p.normal), sign),
                    ];
                    surfaces.polygon(
                        "body",
                        &[
                            pair[0].point(p.at),
                            pair[0].point(q.at),
                            pair[1].point(q.at),
                            pair[1].point(p.at),
                        ],
                        add(add(normals[0], normals[1]), add(normals[2], normals[3])),
                        &[
                            [p.u, pair[0].s],
                            [q.u, pair[0].s],
                            [q.u, pair[1].s],
                            [p.u, pair[1].s],
                        ],
                        Some(&normals),
                    )?;
                }
            }
        }
        if self.capped {
            for (station, sign) in [(rings[0], -1.), (rings[rings.len() - 1], 1.)] {
                let outward = scale(station.tangent, sign);
                match &inner {
                    None => {
                        let loop_: Vec<P2> = outer.iter().map(|strip| strip[0].at).collect();
                        surfaces.polygon(
                            "end",
                            &loop_.iter().map(|p| station.point(*p)).collect::<Vec<_>>(),
                            outward,
                            &loop_,
                            None,
                        )?;
                    }
                    Some(inner) => {
                        for ([p, q], [pi, qi]) in outer.iter().zip(inner) {
                            let corners = [p.at, q.at, qi.at, pi.at];
                            surfaces.polygon(
                                "end",
                                &corners.map(|c| station.point(c)),
                                outward,
                                &corners,
                                None,
                            )?;
                        }
                    }
                }
            }
        }
        for fitting in &self.fittings {
            self.build_fitting(fitting, surfaces)?;
        }
        Ok(())
    }

    fn build_fitting(&self, fitting: &Fitting, surfaces: &mut Surfaces<'_>) -> Result<(), String> {
        let station = self.path.at(fitting.station);
        let frame = station.frame();
        let half = self.section.half();
        let small = half[0].min(half[1]);
        let pipe = self.section.outline(0.);
        match &fitting.shape {
            FittingShape::Coupling { length, clearance } => prism(
                surfaces,
                ["fitting"; 3],
                &frame,
                &self.section.outline(*clearance),
                Some(&pipe),
                [-length / 2., length / 2.],
            ),
            FittingShape::Flange {
                overhang,
                thickness,
                bolts,
            } => {
                prism(
                    surfaces,
                    ["fitting"; 3],
                    &frame,
                    &self.section.outline(*overhang),
                    Some(&pipe),
                    [-thickness / 2., thickness / 2.],
                )?;
                let head = 0.25 * overhang;
                for bolt in 0..*bolts {
                    let center = match &self.section {
                        Section::Round { radius, .. } => {
                            let angle = bolt as f64 / *bolts as f64 * std::f64::consts::TAU;
                            let circle = radius + overhang / 2.;
                            [circle * cos(angle), circle * sin(angle)]
                        }
                        Section::Rect { width, height } => {
                            // Evenly along the rectangle midway between section and flange edge.
                            let (w, h) = (width + overhang, height + overhang);
                            let mut along = (bolt as f64 + 0.5) / *bolts as f64 * 2. * (w + h);
                            let mut center = [-w / 2., -h / 2.];
                            for (axis, sign, span) in
                                [(0, 1., w), (1, 1., h), (0, -1., w), (1, -1., h)]
                            {
                                let step = along.min(span);
                                center[axis] += sign * step;
                                along -= step;
                            }
                            center
                        }
                    };
                    prism(
                        surfaces,
                        ["bolt"; 3],
                        &frame,
                        &Outline::circle(center, head, 6, false),
                        None,
                        [-thickness / 2. - 0.6 * head, thickness / 2. + 0.6 * head],
                    )?;
                }
                Ok(())
            }
            FittingShape::Valve { length, lever } => {
                prism(
                    surfaces,
                    ["fitting"; 3],
                    &frame,
                    &self.section.outline(0.5 * small),
                    Some(&pipe),
                    [-length / 2., length / 2.],
                )?;
                // The stem rises along the section's `b` axis; this frame's normal is `b`.
                let stem = Frame {
                    origin: station.position,
                    x: station.tangent,
                    y: station.a,
                };
                let top = half[1] + 1.7 * small;
                prism(
                    surfaces,
                    ["fitting"; 3],
                    &stem,
                    &Outline::circle([0., 0.], 0.35 * small, 12, true),
                    None,
                    [0., top],
                )?;
                let handle = if *lever {
                    Outline::rectangle([-0.3 * small, -0.25 * small], [3. * small, 0.25 * small])
                } else {
                    Outline::circle([0., 0.], 1.6 * small, 16, true)
                };
                prism(
                    surfaces,
                    ["fitting"; 3],
                    &stem,
                    &handle,
                    None,
                    [top, top + 0.25 * small],
                )
            }
            FittingShape::Bracket {
                reach,
                direction,
                width,
                thickness,
            } => {
                prism(
                    surfaces,
                    ["bracket"; 3],
                    &frame,
                    &self.section.outline(*thickness),
                    Some(&pipe),
                    [-width / 2., width / 2.],
                )?;
                // This frame's normal is `direction`.
                let arm = Frame {
                    origin: station.position,
                    x: station.tangent,
                    y: cross(*direction, station.tangent),
                };
                let surface = self
                    .section
                    .support([dot(*direction, station.a), dot(*direction, station.b)]);
                prism(
                    surfaces,
                    ["bracket"; 3],
                    &arm,
                    &Outline::rectangle(
                        [-width / 2., -thickness / 2.],
                        [width / 2., thickness / 2.],
                    ),
                    None,
                    [surface, *reach],
                )?;
                prism(
                    surfaces,
                    ["bracket"; 3],
                    &arm,
                    &Outline::rectangle([-width / 2., -*width], [width / 2., *width]),
                    None,
                    [reach - thickness, *reach],
                )
            }
            FittingShape::Tee { direction, length } => {
                let branch = Frame {
                    origin: station.position,
                    x: station.tangent,
                    y: cross(*direction, station.tangent),
                };
                let hole = (self.wall > 0.).then(|| match &self.section {
                    Section::Round { radius, segments } => {
                        Outline::circle([0., 0.], radius - self.wall, *segments, true)
                    }
                    Section::Rect { width, height } => Outline::rectangle(
                        [-width / 2. + self.wall, -height / 2. + self.wall],
                        [width / 2. - self.wall, height / 2. - self.wall],
                    ),
                });
                prism(
                    surfaces,
                    ["fitting"; 3],
                    &branch,
                    &pipe,
                    hole.as_ref(),
                    [0., *length],
                )
            }
        }
    }

    /// Both ends, every fitting and every tee branch, in part coordinates.
    pub(super) fn attachments(&self, part: &str) -> Vec<Attachment> {
        let (start, end) = (self.path.at(0.), self.path.at(self.path.length));
        let mut result = vec![
            Attachment {
                name: format!("{part}/start"),
                position: start.position,
                direction: scale(start.tangent, -1.),
            },
            Attachment {
                name: format!("{part}/end"),
                position: end.position,
                direction: end.tangent,
            },
        ];
        for fitting in &self.fittings {
            let station = self.path.at(fitting.station);
            result.push(Attachment {
                name: format!("{part}/{}", fitting.id),
                position: station.position,
                direction: station.tangent,
            });
            if let FittingShape::Tee { direction, length } = &fitting.shape {
                result.push(Attachment {
                    name: format!("{part}/{}/branch", fitting.id),
                    position: add(station.position, scale(*direction, *length)),
                    direction: *direction,
                });
            }
        }
        result
    }

    /// Features and the pattern anchor in part coordinates.
    pub(super) fn features(&self, part: &str) -> (Vec<Feature>, PatternAnchor) {
        let outline: Vec<P2> = self
            .section
            .strips(0.)
            .iter()
            .map(|strip| strip[0].at)
            .collect();
        let ring =
            |name: &str, source: Option<&str>, face: &str, station: &Station, sign: f64, width| {
                Feature {
                    name: name.into(),
                    part: part.into(),
                    source: source.map(str::to_owned),
                    face: face.into(),
                    points: outline.iter().map(|p| station.point(*p)).collect(),
                    closed: true,
                    normal: scale(station.tangent, sign),
                    width,
                    reach: Default::default(),
                    within: 0.,
                    zone: None,
                }
            };
        let start = self.path.at(0.);
        let mut features = vec![
            ring("end", None, "end", &start, -1., 0.),
            ring("end", None, "end", &self.path.at(self.path.length), 1., 0.),
        ];
        for fitting in &self.fittings {
            let station = self.path.at(fitting.station);
            let id = Some(fitting.id.as_str());
            features.push(ring(
                "joint",
                id,
                "body",
                &station,
                1.,
                2. * fitting.half(&self.section),
            ));
            if let FittingShape::Bracket {
                reach,
                direction,
                width,
                ..
            } = &fitting.shape
            {
                let side = cross(*direction, station.tangent);
                let center = add(station.position, scale(*direction, *reach));
                features.push(Feature {
                    name: "bracket-contact".into(),
                    part: part.into(),
                    source: id.map(str::to_owned),
                    face: "bracket".into(),
                    points: [[-0.5, -1.], [0.5, -1.], [0.5, 1.], [-0.5, 1.]]
                        .iter()
                        .map(|[along, across]: &P2| {
                            add(
                                center,
                                add(
                                    scale(station.tangent, along * width),
                                    scale(side, across * width),
                                ),
                            )
                        })
                        .collect(),
                    closed: true,
                    normal: scale(*direction, -1.),
                    width: 0.,
                    reach: Default::default(),
                    within: 0.,
                    zone: None,
                });
            }
        }
        (
            features,
            PatternAnchor {
                part: part.into(),
                origin: start.position,
                u: start.a,
                v: start.tangent,
                normal: start.b,
            },
        )
    }
}
