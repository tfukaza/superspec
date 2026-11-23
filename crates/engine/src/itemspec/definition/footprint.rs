//! Plan footprints and courtyard loops whose edges a part can repeat over, with the
//! convex corners between two street edges cut back (a 45 degree cut on a square corner), and
//! the cut outline, optionally inset, for a roof or slab.
//!
//! Edge `i` runs from point `i` to point `i + 1`. Its wall starts after the cut at its start
//! corner and ends before the cut at its end corner; the cut at its end corner is a short edge
//! of its own (the chamfer) from there to the start of edge `i + 1`. A part placed at an edge's
//! start with `along` its direction has its front (-Y) facing out of the footprint.
//! A round corner is cut into several facets instead: chords of the arc that touches both
//! edges where the cut starts and ends.
//! Only `sqrt`, which IEEE 754 rounds exactly, and for round corners the shared deterministic
//! `sin`, `cos` and `atan2` are used, so native and Wasm agree.
use super::{
    Derived, ItemDefinition, ItemDiagnostic, Values, fail,
    schema::{FootprintReference, ParameterType, Scalar},
};
use serde_json::Value;
use superspec_geometry::{
    V3,
    math::{atan2, cos, sin},
    triangulate,
};

pub(super) type P2 = [f64; 2];

pub(super) struct Footprint {
    rings: Vec<Vec<Edge>>,
    boundary: Vec<Vec<P2>>,
    raw_bounds: [f64; 4],
    shaped: bool,
}

impl Footprint {
    pub(super) fn edge_count(&self) -> usize {
        self.rings.iter().map(Vec::len).sum()
    }

    pub(super) fn edge_locals(&self, name: &str, mut index: usize) -> Vec<(String, f64)> {
        for (ring_index, ring) in self.rings.iter().enumerate() {
            if index < ring.len() {
                let mut values = locals(name, ring, index);
                let inward = if self.shaped && ring[index].street {
                    self.inward_depth(&ring[index])
                } else {
                    0.
                };
                values.extend([
                    (format!("{name}_courtyard"), flag(ring_index > 0)),
                    (format!("{name}_shaped"), flag(self.shaped)),
                    (format!("{name}_inward_depth"), inward),
                ]);
                return values;
            }
            index -= ring.len();
        }
        unreachable!("footprint edge index was checked by the caller")
    }

    /// Distance to the first other boundary across the open width of this edge's inward strip.
    /// Consecutive walls touching the strip's ends do not reduce its available depth.
    fn inward_depth(&self, edge: &Edge) -> f64 {
        let local = |p| {
            let [x, y] = sub(p, edge.start);
            let [dx, dy] = edge.direction;
            [dx * x + dy * y, dx * y - dy * x]
        };
        let ends = [1e-7, edge.length - 1e-7];
        let mut depth = 1e5_f64;
        for [p, q] in self.boundary.iter().flat_map(|ring| pairs(ring)) {
            let (mut a, mut b) = (local(p), local(q));
            if a[0] > b[0] {
                std::mem::swap(&mut a, &mut b);
            }
            if b[0] < ends[0] || a[0] > ends[1] || a[1].max(b[1]) <= 1e-7 {
                continue;
            }
            let [x, y] = a;
            let at = |u| y + (b[1] - y) * (u - x) / (b[0] - x);
            if x < ends[0] {
                a[1] = at(ends[0]);
            }
            if b[0] > ends[1] {
                b[1] = at(ends[1]);
            }
            if a[1].max(b[1]) > 1e-7 {
                depth = depth.min(a[1].min(b[1]).max(0.));
            }
        }
        depth
    }

    pub(super) fn outlines(&self, inset: f64) -> Result<Vec<Vec<P2>>, String> {
        let mut loops = self
            .rings
            .iter()
            .map(|ring| outline(ring, inset))
            .collect::<Vec<_>>();
        if self.shaped {
            for (original, inset) in self.boundary.iter().zip(&loops) {
                if !pairs(original).zip(pairs(inset)).all(|([a, b], [p, q])| {
                    let [ux, uy] = sub(b, a);
                    let [vx, vy] = sub(q, p);
                    ux * vx + uy * vy > 1e-9
                }) {
                    return Err("The footprint inset collapses or reverses an edge.".into());
                }
            }
            loops = profile_loops(&loops)?
                .loops
                .into_iter()
                .map(|ring| ring.into_iter().map(|p| [p[0], p[1]]).collect())
                .collect();
        }
        Ok(loops)
    }

    pub(super) fn rectangles(&self, largest: bool) -> Result<Vec<[f64; 4]>, String> {
        if !self.shaped {
            return Ok(vec![self.raw_bounds]);
        }
        let loops = self.outlines(0.2)?;
        let mut bounds = rectangles(&loops);
        if largest {
            bounds = bounds
                .into_iter()
                .max_by(|a, b| rectangle_area(*a).total_cmp(&rectangle_area(*b)))
                .into_iter()
                .collect();
        }
        Ok(bounds)
    }

    pub(super) fn rectangle_locals(&self, name: &str, bounds: [f64; 4]) -> Vec<(String, f64)> {
        let mut values = self.edge_locals(name, 0);
        for (field, value) in ["min_x", "min_y", "max_x", "max_y"].into_iter().zip(bounds) {
            let field = format!("{name}_{field}");
            values.iter_mut().find(|(key, _)| *key == field).unwrap().1 = value;
        }
        values
    }
}

fn profile_loops(loops: &[Vec<P2>]) -> Result<superspec_geometry::Profile, String> {
    let rings = loops
        .iter()
        .map(|ring| ring.iter().map(|p| [p[0], p[1], 0.] as V3).collect())
        .collect();
    triangulate(rings, &[], &[])
}

fn pairs(points: &[P2]) -> impl Iterator<Item = [P2; 2]> + Clone + '_ {
    points
        .iter()
        .copied()
        .zip(points.iter().copied().cycle().skip(1))
        .map(|(a, b)| [a, b])
}

fn area(points: &[P2]) -> f64 {
    pairs(points).map(|[a, b]| cross(a, b)).sum::<f64>() / 2.
}

fn flag(value: bool) -> f64 {
    f64::from(u8::from(value))
}

fn rectangle_area([x0, y0, x1, y1]: [f64; 4]) -> f64 {
    (x1 - x0) * (y1 - y0)
}

/// Between vertex heights each edge crossing moves linearly. Taking the common width at
/// both ends of a strip keeps every box in the solid and covers rectangular wings exactly.
fn rectangles(loops: &[Vec<P2>]) -> Vec<[f64; 4]> {
    let mut heights: Vec<_> = loops.iter().flatten().map(|p| p[1]).collect();
    heights.sort_by(f64::total_cmp);
    heights.dedup_by(|a, b| (*a - *b).abs() < 1e-8);
    let edges = loops
        .iter()
        .flat_map(|ring| pairs(ring))
        .collect::<Vec<_>>();
    let mut rectangles: Vec<[f64; 4]> = Vec::new();
    for heights in heights.windows(2) {
        let (y0, y1) = (heights[0], heights[1]);
        let mid = (y0 + y1) / 2.;
        let mut sides: Vec<_> = edges
            .iter()
            .filter_map(|&[a, b]| {
                if !(a[1].min(b[1]) < mid && mid < a[1].max(b[1])) {
                    return None;
                }
                let at = |y: f64| a[0] + (b[0] - a[0]) * ((y - a[1]) / (b[1] - a[1]));
                Some([at(mid), at(y0), at(y1)])
            })
            .collect();
        sides.sort_by(|a, b| a[0].total_cmp(&b[0]));
        for [left, right] in sides.as_chunks::<2>().0.iter().copied() {
            let x0 = left[1].max(left[2]);
            let x1 = right[1].min(right[2]);
            if x1 - x0 < 1e-6 {
                continue;
            }
            if let Some(previous) = rectangles.iter_mut().rev().find(|r| {
                (r[3] - y0).abs() < 1e-8 && (r[0] - x0).abs() < 1e-8 && (r[2] - x1).abs() < 1e-8
            }) {
                previous[3] = y1;
            } else {
                rectangles.push([x0, y0, x1, y1]);
            }
        }
    }
    rectangles
}

/// One edge of a footprint and the cut corner at its end.
#[derive(Clone, Debug)]
pub(super) struct Edge {
    pub start: P2,
    pub direction: P2,
    pub length: f64,
    pub street: bool,
    pub next_street: bool,
    pub previous_street: bool,
    /// How far the end corner is cut back along each edge; 0 when it is not cut.
    pub cut: f64,
    /// Where the wall ends: the corner, or where its cut starts.
    pub end: P2,
    /// The cut at the end corner, in order: one chamfer, or the facets of a round corner.
    pub facets: Vec<Facet>,
    /// The facet a copy reads as its cut corner (the wall's end and direction when not cut).
    pub chamfer_start: P2,
    pub chamfer_direction: P2,
    pub chamfer_length: f64,
    /// Which facet that is, and the distance along the cut to its start.
    pub facet: usize,
    pub facet_offset: f64,
    /// The tangent of half the plan turn where the wall starts and ends, and where the read
    /// facet does: what a moulding mitres by.
    pub miters: [f64; 2],
    pub facet_miters: [f64; 2],
    /// The plan turn at the end corner in degrees (positive at a convex corner), and the radius
    /// of the arc that touches both edges where its cut starts and ends (0 when not cut).
    pub turn: f64,
    pub radius: f64,
}

/// One flat side of a cut corner.
#[derive(Clone, Copy, Debug)]
pub(super) struct Facet {
    pub start: P2,
    pub direction: P2,
    pub length: f64,
}

fn sub(a: P2, b: P2) -> P2 {
    [a[0] - b[0], a[1] - b[1]]
}
fn add_scaled(a: P2, d: P2, s: f64) -> P2 {
    [a[0] + d[0] * s, a[1] + d[1] * s]
}
fn length(a: P2) -> f64 {
    (a[0] * a[0] + a[1] * a[1]).sqrt()
}
fn cross(a: P2, b: P2) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

/// The footprint's edges. Fails unless the polygon is simple enough to use: at least three
/// points, counterclockwise (positive area), no edge shorter than a centimeter.
pub(super) fn edges(
    points: &[P2],
    streets: &[bool],
    cut: f64,
    round: usize,
    facet: usize,
    courtyard: bool,
) -> Result<Vec<Edge>, String> {
    let n = points.len();
    if n < 3 {
        return Err("A footprint has at least three points.".into());
    }
    if !courtyard && area(points) <= 0. {
        return Err("A footprint runs counterclockwise seen from above (positive area).".into());
    }
    if !cut.is_finite() || cut < 0. {
        return Err("A corner cut is a nonnegative length.".into());
    }
    let street = |i: usize| streets.get(i % n).copied().unwrap_or(false);
    let mut spans = Vec::with_capacity(n);
    for i in 0..n {
        let span = sub(points[(i + 1) % n], points[i]);
        let l = length(span);
        let minimum = if courtyard { 0.3 } else { 0.01 };
        if l < minimum {
            return Err(format!("Footprint edge {i} is shorter than {minimum} m."));
        }
        spans.push(([span[0] / l, span[1] / l], l));
    }
    // The cut at the end corner of each edge: convex, between two street edges, and at most
    // a third of either edge.
    let cuts: Vec<f64> = (0..n)
        .map(|i| {
            let (d, l) = spans[i];
            let (e, m) = spans[(i + 1) % n];
            if cross(d, e) > 1e-9 && street(i) && street(i + 1) {
                cut.min(l / 3.).min(m / 3.)
            } else {
                0.
            }
        })
        .collect();
    let mut result: Vec<Edge> = (0..n)
        .map(|i| {
            let (direction, l) = spans[i];
            let before = cuts[(i + n - 1) % n];
            let after = cuts[i];
            let start = add_scaled(points[i], direction, before);
            let end = add_scaled(points[(i + 1) % n], direction, -after);
            let onward = spans[(i + 1) % n].0;
            let next = add_scaled(points[(i + 1) % n], onward, after);
            let chamfer = sub(next, end);
            let chamfer_length = length(chamfer);
            let chamfer_direction = if chamfer_length > 0. {
                [chamfer[0] / chamfer_length, chamfer[1] / chamfer_length]
            } else {
                direction
            };
            let facets = if after <= 0. {
                vec![]
            } else if round < 2 {
                vec![Facet {
                    start: end,
                    direction: chamfer_direction,
                    length: chamfer_length,
                }]
            } else {
                arc(end, next, direction, onward, round)
            };
            let turn = atan2(
                cross(direction, onward),
                direction[0] * onward[0] + direction[1] * onward[1],
            );
            let radius = if after > 0. {
                chamfer_length / (2. * sin(turn / 2.))
            } else {
                0.
            };
            let facet = facet.min(facets.len().saturating_sub(1));
            let read = facets.get(facet).copied().unwrap_or(Facet {
                start: end,
                direction: chamfer_direction,
                length: chamfer_length,
            });
            Edge {
                start,
                direction,
                length: l - before - after,
                street: street(i),
                next_street: street(i + 1),
                previous_street: street(i + n - 1),
                cut: after,
                end,
                chamfer_start: read.start,
                chamfer_direction: read.direction,
                chamfer_length: read.length,
                facet,
                facet_offset: facets[..facet].iter().map(|facet| facet.length).sum(),
                facets,
                miters: [0.; 2],
                facet_miters: [0.; 2],
                turn: turn * (180. / std::f64::consts::PI),
                radius,
            }
        })
        .collect();
    // Mitres: half the turn from what comes before a wall or facet, and to what follows it.
    for i in 0..n {
        let previous = &result[(i + n - 1) % n];
        let before = previous
            .facets
            .last()
            .map_or(previous.direction, |facet| facet.direction);
        let edge = &result[i];
        let onward = result[(i + 1) % n].direction;
        let after = edge.facets.first().map_or(onward, |facet| facet.direction);
        let miters = [
            half_turn(before, edge.direction),
            half_turn(edge.direction, after),
        ];
        let facet_miters = if edge.facets.is_empty() {
            [0.; 2]
        } else {
            let into = match edge.facet.checked_sub(1) {
                Some(k) => edge.facets[k].direction,
                None => edge.direction,
            };
            let out = edge
                .facets
                .get(edge.facet + 1)
                .map_or(onward, |facet| facet.direction);
            [
                half_turn(into, edge.chamfer_direction),
                half_turn(edge.chamfer_direction, out),
            ]
        };
        result[i].miters = miters;
        result[i].facet_miters = facet_miters;
    }
    Ok(result)
}

/// The tangent of half the turn from direction `a` to `b` (unit vectors): 1 at a square
/// corner, negative at a concave one. A half-angle identity, so no trigonometry.
fn half_turn(a: P2, b: P2) -> f64 {
    let along = 1. + a[0] * b[0] + a[1] * b[1];
    if along < 1e-9 {
        return 0.;
    }
    cross(a, b) / along
}

/// The facets of a round corner: `count` chords of the arc that leaves `from` along `d` and
/// arrives at `to` along `e`, touching both edges there. The angles go through the shared
/// deterministic `sin` and `cos`; the first and last points are the given ones exactly.
fn arc(from: P2, to: P2, d: P2, e: P2, count: usize) -> Vec<Facet> {
    let turn = atan2(cross(d, e), d[0] * e[0] + d[1] * e[1]);
    // The centre is to the left of the incoming edge, as far as both tangent points.
    let chord = length(sub(to, from));
    let radius = chord / (2. * sin(turn / 2.));
    let center = add_scaled(from, [-d[1], d[0]], radius);
    let spoke = [d[1] * radius, -d[0] * radius];
    let points: Vec<P2> = (0..=count)
        .map(|k| {
            if k == 0 {
                from
            } else if k == count {
                to
            } else {
                let angle = turn * k as f64 / count as f64;
                let (s, c) = (sin(angle), cos(angle));
                [
                    center[0] + spoke[0] * c - spoke[1] * s,
                    center[1] + spoke[0] * s + spoke[1] * c,
                ]
            }
        })
        .collect();
    points
        .windows(2)
        .map(|pair| {
            let span = sub(pair[1], pair[0]);
            let l = length(span);
            Facet {
                start: pair[0],
                direction: [span[0] / l, span[1] / l],
                length: l,
            }
        })
        .collect()
}

/// The cut outline, moved `inset` inward along every edge (each corner where its two moved
/// edges meet; parallel neighbours keep the moved point).
pub(super) fn outline(edges: &[Edge], inset: f64) -> Vec<P2> {
    let mut lines: Vec<(P2, P2)> = Vec::new();
    for edge in edges {
        lines.push((edge.start, edge.direction));
        for facet in &edge.facets {
            lines.push((facet.start, facet.direction));
        }
    }
    // Inward is to the left of a counterclockwise edge.
    let moved: Vec<(P2, P2)> = lines
        .iter()
        .map(|(point, d)| (add_scaled(*point, [-d[1], d[0]], inset), *d))
        .collect();
    let n = moved.len();
    (0..n)
        .map(|i| {
            let (a, d) = moved[(i + n - 1) % n];
            let (b, e) = moved[i];
            let denominator = cross(d, e);
            if denominator.abs() < 1e-9 {
                b
            } else {
                add_scaled(a, d, cross(sub(b, a), e) / denominator)
            }
        })
        .collect()
}

/// Reads a footprint's controls: its points, street flags and cut.
pub(super) fn resolve(
    definition: &ItemDefinition,
    reference: &FootprintReference,
    values: &Values,
    derived: &Derived,
    pointer: &str,
) -> Result<Footprint, ItemDiagnostic> {
    let failure = |message: String| fail(&definition.source, pointer, "ITEM_FOOTPRINT", message);
    let control = |name: &str, kind: ParameterType| -> Result<&Value, ItemDiagnostic> {
        if definition
            .parameters
            .get(name)
            .is_none_or(|parameter| parameter.kind != kind)
        {
            return Err(failure(format!("{name} is not a {kind:?} control.")));
        }
        definition.parameter(name, values, pointer, true)
    };
    let read_points = |value: &Value| {
        value
            .as_array()?
            .iter()
            .map(|point| {
                let [x, y] = point.as_array()?.as_slice() else {
                    return None;
                };
                let xy = [x.as_f64()?, y.as_f64()?];
                xy.iter()
                    .all(|v| v.is_finite() && v.abs() <= 1e5)
                    .then_some(xy)
            })
            .collect::<Option<Vec<P2>>>()
    };
    let points = read_points(control(&reference.points, ParameterType::Points)?)
        .expect("validated points parameter");
    let mut loops = vec![points];
    if let Some(name) = &reference.holes {
        let holes = control(name, ParameterType::Grid)?;
        for hole in holes.as_array().expect("validated grid parameter") {
            let Some(mut points) = read_points(hole).filter(|p| p.len() >= 3) else {
                return Err(failure(
                    "A courtyard is a loop of at least three finite XY points.".into(),
                ));
            };
            if area(&points) > 0. {
                points[1..].reverse();
            }
            loops.push(points);
        }
        if loops.iter().map(Vec::len).sum::<usize>() > 64 {
            return Err(failure(
                "A footprint and its courtyards have at most 64 corners in total.".into(),
            ));
        }
    }
    let streets: Vec<bool> = match &reference.streets {
        None => vec![],
        Some(name) => control(name, ParameterType::Flags)?
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_bool)
            .collect(),
    };
    let cut = definition.scalar(&reference.cut, values, derived, &format!("{pointer}/cut"))?;
    let whole = |value: &Scalar, field: &str, most: f64| -> Result<usize, ItemDiagnostic> {
        let number = definition.scalar(value, values, derived, &format!("{pointer}/{field}"))?;
        if number.fract() != 0. || !(0. ..=most).contains(&number) {
            return Err(fail(
                &definition.source,
                &format!("{pointer}/{field}"),
                "ITEM_FOOTPRINT",
                format!("A footprint's {field} is an integer from 0 to {most}."),
            ));
        }
        Ok(number as usize)
    };
    let round = whole(&reference.round, "round", 16.)?;
    let facet = whole(&reference.facet, "facet", 255.)?;
    let outer = &loops[0];
    let shaped = loops.len() > 1
        || pairs(outer)
            .zip(pairs(outer).cycle().skip(1))
            .any(|([a, b], [_, c])| cross(sub(b, a), sub(c, b)) < -1e-9);
    let mut rings = vec![edges(outer, &streets, cut, round, facet, false).map_err(failure)?];
    if shaped || outer.len() > 4 {
        profile_loops(&loops).map_err(failure)?;
    }
    for hole in &loops[1..] {
        rings.push(edges(hole, &[], 0., 0, 0, true).map_err(failure)?);
    }
    let raw_bounds = outer.iter().fold(
        [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ],
        |b, [x, y]| [b[0].min(*x), b[1].min(*y), b[2].max(*x), b[3].max(*y)],
    );
    let boundary = if shaped {
        rings.iter().map(|ring| outline(ring, 0.)).collect()
    } else {
        vec![]
    };
    Ok(Footprint {
        rings,
        boundary,
        raw_bounds,
        shaped,
    })
}

/// The values a copy along edge `edge` reads, as `<name>_<field>`.
pub(super) fn locals(name: &str, edges: &[Edge], index: usize) -> Vec<(String, f64)> {
    let edge = &edges[index];
    let mut bounds = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for edge in edges {
        for p in [edge.start, edge.end] {
            bounds = [
                bounds[0].min(p[0]),
                bounds[1].min(p[1]),
                bounds[2].max(p[0]),
                bounds[3].max(p[1]),
            ];
        }
    }
    let flag = |b: bool| f64::from(u8::from(b));
    // Distance along the cut outline from point 0 to this wall's start, so a pattern measured from it
    // runs on round every corner without a seam.
    let perimeter: f64 = edges[..index]
        .iter()
        .map(|edge| edge.length + edge.facets.iter().map(|facet| facet.length).sum::<f64>())
        .sum();
    [
        ("x", edge.start[0]),
        ("y", edge.start[1]),
        ("dx", edge.direction[0]),
        ("dy", edge.direction[1]),
        ("length", edge.length),
        ("street", flag(edge.street)),
        ("next_street", flag(edge.next_street)),
        ("previous_street", flag(edge.previous_street)),
        ("cut", edge.cut),
        (
            "previous_cut",
            edges[(index + edges.len() - 1) % edges.len()].cut,
        ),
        ("cx", edge.chamfer_start[0]),
        ("cy", edge.chamfer_start[1]),
        ("cdx", edge.chamfer_direction[0]),
        ("cdy", edge.chamfer_direction[1]),
        ("clength", edge.chamfer_length),
        ("perimeter", perimeter),
        ("cperimeter", perimeter + edge.length + edge.facet_offset),
        ("facets", edge.facets.len() as f64),
        ("miter_start", edge.miters[0]),
        ("miter_end", edge.miters[1]),
        ("cmiter_start", edge.facet_miters[0]),
        ("cmiter_end", edge.facet_miters[1]),
        ("turn", edge.turn),
        ("cradius", edge.radius),
        ("min_x", bounds[0]),
        ("min_y", bounds[1]),
        ("max_x", bounds[2]),
        ("max_y", bounds[3]),
    ]
    .into_iter()
    .map(|(field, value)| (format!("{name}_{field}"), value))
    .collect()
}
