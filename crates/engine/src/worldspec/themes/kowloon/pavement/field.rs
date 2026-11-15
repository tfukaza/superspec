//! Convex XY footprint operations retain receiving heights at every cut.
use superspec_geometry::{V3, math::hypot};

pub fn area(points: &[V3]) -> f64 {
    let mut result = 0.;
    for (i, p) in points.iter().enumerate() {
        let q = points[(i + 1) % points.len()];
        result = result + p[0] * q[1] - q[0] * p[1];
    }
    result / 2.
}
pub fn clip(points: &[V3], distance: impl Fn(V3) -> f64) -> Vec<V3> {
    let mut kept = Vec::new();
    for (i, &a) in points.iter().enumerate() {
        let b = points[(i + 1) % points.len()];
        let la = distance(a);
        let lb = distance(b);
        if la >= 0. {
            kept.push(a);
        }
        if (la >= 0.) != (lb >= 0.) {
            let t = la / (la - lb);
            kept.push(std::array::from_fn(|k| a[k] + t * (b[k] - a[k])));
        }
    }
    if kept.len() >= 3 { kept } else { Vec::new() }
}
fn side(a: V3, b: V3, p: V3, sign: f64) -> f64 {
    sign * ((b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]))
}
pub fn intersect(points: &[V3], boundary: &[V3]) -> Vec<V3> {
    let mut result = points.to_vec();
    let value = area(boundary);
    let sign = if value == 0. { 0. } else { value.signum() };
    for (i, &a) in boundary.iter().enumerate() {
        if result.len() < 3 {
            break;
        }
        let b = boundary[(i + 1) % boundary.len()];
        result = clip(&result, |p| side(a, b, p, sign));
    }
    if result.len() >= 3 && area(&result).abs() > 1e-10 {
        result
    } else {
        Vec::new()
    }
}
pub struct Split {
    pub inside: Vec<V3>,
    pub outside: Vec<Vec<V3>>,
}
pub fn split(points: &[V3], boundary: &[V3]) -> Split {
    if intersect(points, boundary).is_empty() {
        return Split {
            inside: Vec::new(),
            outside: vec![points.to_vec()],
        };
    }
    let mut inside = points.to_vec();
    let mut outside = Vec::new();
    let value = area(boundary);
    let sign = if value == 0. { 0. } else { value.signum() };
    for (i, &a) in boundary.iter().enumerate() {
        if inside.len() < 3 {
            break;
        }
        let b = boundary[(i + 1) % boundary.len()];
        let removed = clip(&inside, |p| -side(a, b, p, sign));
        if area(&removed).abs() > 1e-10 {
            outside.push(removed);
        }
        inside = clip(&inside, |p| side(a, b, p, sign));
    }
    if area(&inside).abs() <= 1e-10 {
        inside.clear();
    }
    Split { inside, outside }
}
pub fn rectangle(x: f64, y: f64, width: f64, depth: f64, z: f64) -> Vec<V3> {
    vec![
        [x, y, z],
        [x + width, y, z],
        [x + width, y + depth, z],
        [x, y + depth, z],
    ]
}
pub fn edge_distance(p: V3, a: V3, b: V3) -> f64 {
    let x = b[0] - a[0];
    let y = b[1] - a[1];
    let t = (((p[0] - a[0]) * x + (p[1] - a[1]) * y) / (x * x + y * y).max(1e-20)).clamp(0., 1.);
    hypot(p[0] - a[0] - t * x, p[1] - a[1] - t * y)
}
pub(super) fn valid_point(p: V3) -> bool {
    p.into_iter()
        .all(|v| v.is_finite() && v.abs() <= 1_000_000.)
}
