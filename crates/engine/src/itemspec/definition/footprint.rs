//! Plan footprints: a counterclockwise polygon whose edges a part can repeat over, with the
//! convex corners between two street edges cut back (a 45 degree cut on a square corner), and
//! the cut outline, optionally inset, for a roof or slab.
//!
//! Edge `i` runs from point `i` to point `i + 1`. Its wall starts after the cut at its start
//! corner and ends before the cut at its end corner; the cut at its end corner is a short edge
//! of its own (the chamfer) from there to the start of edge `i + 1`. A part placed at an edge's
//! start with `along` its direction has its front (-Y) facing out of the footprint.
//! Only `sqrt` is used, which IEEE 754 rounds exactly, so native and Wasm agree.
use super::{
    Derived, ItemDefinition, ItemDiagnostic, Values, fail,
    schema::{FootprintReference, ParameterType},
};
use serde_json::Value;

pub(super) type P2 = [f64; 2];

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
    pub chamfer_start: P2,
    pub chamfer_direction: P2,
    pub chamfer_length: f64,
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
pub(super) fn edges(points: &[P2], streets: &[bool], cut: f64) -> Result<Vec<Edge>, String> {
    let n = points.len();
    if n < 3 {
        return Err("A footprint has at least three points.".into());
    }
    let area: f64 = (0..n)
        .map(|i| cross(points[i], points[(i + 1) % n]))
        .sum::<f64>()
        / 2.;
    if area <= 0. {
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
        if l < 0.01 {
            return Err(format!("Footprint edge {i} is shorter than a centimeter."));
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
    Ok((0..n)
        .map(|i| {
            let (direction, l) = spans[i];
            let before = cuts[(i + n - 1) % n];
            let after = cuts[i];
            let start = add_scaled(points[i], direction, before);
            let end = add_scaled(points[(i + 1) % n], direction, -after);
            let next = add_scaled(points[(i + 1) % n], spans[(i + 1) % n].0, after);
            let chamfer = sub(next, end);
            let chamfer_length = length(chamfer);
            Edge {
                start,
                direction,
                length: l - before - after,
                street: street(i),
                next_street: street(i + 1),
                previous_street: street(i + n - 1),
                cut: after,
                chamfer_start: end,
                chamfer_direction: if chamfer_length > 0. {
                    [chamfer[0] / chamfer_length, chamfer[1] / chamfer_length]
                } else {
                    direction
                },
                chamfer_length,
            }
        })
        .collect())
}

/// The cut outline, moved `inset` inward along every edge (each corner where its two moved
/// edges meet; parallel neighbours keep the moved point).
pub(super) fn outline(edges: &[Edge], inset: f64) -> Vec<P2> {
    let mut lines: Vec<(P2, P2)> = Vec::new();
    for edge in edges {
        lines.push((edge.start, edge.direction));
        if edge.cut > 0. {
            lines.push((edge.chamfer_start, edge.chamfer_direction));
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
) -> Result<Vec<Edge>, ItemDiagnostic> {
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
    let points: Vec<P2> = control(&reference.points, ParameterType::Points)?
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|point| {
            let xy = point.as_array()?;
            Some([xy.first()?.as_f64()?, xy.get(1)?.as_f64()?])
        })
        .collect();
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
    edges(&points, &streets, cut).map_err(failure)
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
        for p in [edge.start, edge.chamfer_start] {
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
        .map(|edge| edge.length + edge.chamfer_length * f64::from(u8::from(edge.cut > 0.)))
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
        ("cx", edge.chamfer_start[0]),
        ("cy", edge.chamfer_start[1]),
        ("cdx", edge.chamfer_direction[0]),
        ("cdy", edge.chamfer_direction[1]),
        ("clength", edge.chamfer_length),
        ("perimeter", perimeter),
        ("cperimeter", perimeter + edge.length),
        ("min_x", bounds[0]),
        ("min_y", bounds[1]),
        ("max_x", bounds[2]),
        ("max_y", bounds[3]),
    ]
    .into_iter()
    .map(|(field, value)| (format!("{name}_{field}"), value))
    .collect()
}
