use super::field::{area, edge_distance};
use superspec_geometry::{V3, math::hypot};
pub type Bounds = [f64; 4];
pub fn bounds(points: &[V3]) -> Bounds {
    let mut b = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for p in points {
        for i in 0..2 {
            b[i] = b[i].min(p[i]);
            b[i + 2] = b[i + 2].max(p[i]);
        }
    }
    b
}
pub fn overlaps(a: Bounds, b: Bounds, margin: f64) -> bool {
    a[0] <= b[2] + margin && a[2] >= b[0] - margin && a[1] <= b[3] + margin && a[3] >= b[1] - margin
}
pub fn signed_distance(x: f64, y: f64, polygon: &[V3]) -> f64 {
    let mut inside = true;
    let mut distance = f64::INFINITY;
    for (i, &a) in polygon.iter().enumerate() {
        let b = polygon[(i + 1) % polygon.len()];
        if (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0]) < 0. {
            inside = false;
        }
        distance = distance.min(edge_distance([x, y, 0.], a, b));
    }
    if inside { distance } else { -distance }
}
pub fn ccw(polygon: &[V3]) -> Vec<V3> {
    let mut p = polygon.to_vec();
    if area(&p) < 0. {
        p.reverse();
    }
    p
}
struct Plane {
    origin: V3,
    x: f64,
    y: f64,
}
pub struct Footprint {
    pub bounds: Bounds,
    planes: Vec<Plane>,
}
impl Footprint {
    pub fn new(polygon: &[V3]) -> Self {
        let bounds = bounds(polygon);
        let mut result = Self {
            bounds,
            planes: Vec::new(),
        };
        if polygon.len() < 3 || area(polygon) <= 1e-12 {
            return result;
        }
        let tolerance = f64::EPSILON * bounds.iter().map(|v| v.abs()).fold(1., f64::max) * 128.;
        let mut planes = Vec::new();
        for (i, &a) in polygon.iter().enumerate() {
            let b = polygon[(i + 1) % polygon.len()];
            let dx = b[0] - a[0];
            let dy = b[1] - a[1];
            let length = hypot(dx, dy);
            if length == 0. {
                continue;
            }
            if length <= tolerance {
                return result;
            }
            let plane = Plane {
                origin: a,
                x: -dy / length,
                y: dx / length,
            };
            if polygon
                .iter()
                .any(|p| plane.x * (p[0] - a[0]) + plane.y * (p[1] - a[1]) < -tolerance)
            {
                return result;
            }
            planes.push(plane);
        }
        if planes.len() >= 3 {
            result.planes = planes;
        }
        result
    }
    pub fn convex(&self) -> bool {
        !self.planes.is_empty()
    }
    pub fn uniform(&self, area: Bounds, inside_margin: f64, outside_margin: f64) -> bool {
        if self.planes.is_empty() {
            return false;
        }
        let x = (area[0] + area[2]) / 2.;
        let y = (area[1] + area[3]) / 2.;
        let half_width = (area[2] - area[0]) / 2.;
        let half_height = (area[3] - area[1]) / 2.;
        let tolerance = f64::EPSILON
            * area
                .iter()
                .chain(&self.bounds)
                .map(|v| v.abs())
                .fold(1., f64::max)
            * 128.;
        let mut inside = true;
        for p in &self.planes {
            let distance = p.x * (x - p.origin[0]) + p.y * (y - p.origin[1]);
            let extent = p.x.abs() * half_width + p.y.abs() * half_height;
            if distance + extent < -outside_margin - tolerance {
                return true;
            }
            if distance - extent <= inside_margin + tolerance {
                inside = false;
            }
        }
        if inside {
            return true;
        }
        let dx = (self.bounds[0] - area[2])
            .max(area[0] - self.bounds[2])
            .max(0.);
        let dy = (self.bounds[1] - area[3])
            .max(area[1] - self.bounds[3])
            .max(0.);
        hypot(dx, dy) > outside_margin + tolerance
    }
}
