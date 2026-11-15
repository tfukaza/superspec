use crate::math::*;
use spade::{ConstrainedDelaunayTriangulation, HasPosition, Point2, Triangulation};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default)]
pub struct Profile {
    pub points: Vec<V3>,
    pub triangles: Vec<[usize; 3]>,
    pub loops: Vec<Vec<V3>>,
}
#[derive(Clone, Copy)]
struct Vertex {
    point: V3,
    index: usize,
}
impl HasPosition for Vertex {
    type Scalar = f64;
    fn position(&self) -> Point2<f64> {
        Point2::new(self.point[0], self.point[1])
    }
}

/// Constrained XY triangulation preserves explicit Z and rejects crossing constraints.
pub fn triangulate(
    mut loops: Vec<Vec<V3>>,
    interior: &[V3],
    breaklines: &[Vec<V3>],
) -> Result<Profile, String> {
    let count = loops.iter().map(Vec::len).sum::<usize>()
        + interior.len()
        + breaklines.iter().map(Vec::len).sum::<usize>();
    if count > 4096 || loops.is_empty() {
        return Err("Profile vertex budget exceeded or outer loop missing.".into());
    }
    for ring in &mut loops {
        if ring.len() > 1 && length(sub(ring[0], *ring.last().unwrap())) < 1e-9 {
            ring.pop();
        }
        if ring.len() < 3 || area(ring).abs() < 1e-12 {
            return Err("Profile requires at least three points and nonzero area.".into());
        }
        for i in 0..ring.len() {
            let a = ring[i];
            let b = ring[(i + 1) % ring.len()];
            let c = ring[(i + 2) % ring.len()];
            if on_segment(c, a, b) || on_segment(a, b, c) {
                return Err("Profile boundary doubles back along an adjacent edge.".into());
            }
            for j in i + 1..ring.len() {
                if j == i + 1 || (i == 0 && j == ring.len() - 1) {
                    continue;
                }
                if segments_meet(
                    ring[i],
                    ring[(i + 1) % ring.len()],
                    ring[j],
                    ring[(j + 1) % ring.len()],
                ) {
                    return Err("Profile boundary self-intersects.".into());
                }
            }
        }
    }
    for i in 1..loops.len() {
        if !inside(loops[i][0], &loops[0]) {
            return Err("Profile hole is outside its outer boundary.".into());
        }
        for j in 0..i {
            if j > 0 && (inside(loops[i][0], &loops[j]) || inside(loops[j][0], &loops[i])) {
                return Err("Profile holes overlap or nest.".into());
            }
            for a in 0..loops[i].len() {
                for b in 0..loops[j].len() {
                    if segments_meet(
                        loops[i][a],
                        loops[i][(a + 1) % loops[i].len()],
                        loops[j][b],
                        loops[j][(b + 1) % loops[j].len()],
                    ) {
                        return Err("Profile loops intersect.".into());
                    }
                }
            }
        }
    }
    let mut result = Profile {
        loops,
        ..Profile::default()
    };
    let mut lookup: BTreeMap<(u64, u64), usize> = BTreeMap::new();
    let mut edges = vec![];
    let mut insert_point = |p: V3| -> Result<usize, String> {
        if p.iter().any(|v| !v.is_finite()) {
            return Err("Nonfinite profile coordinate.".into());
        }
        let key = (
            if p[0] == 0. { 0 } else { p[0].to_bits() },
            if p[1] == 0. { 0 } else { p[1].to_bits() },
        );
        if let Some(&i) = lookup.get(&key) {
            if (result.points[i][2] - p[2]).abs() > 1e-7 {
                return Err("Multiple heights at one profile coordinate.".into());
            }
            return Ok(i);
        }
        let i = result.points.len();
        result.points.push(p);
        lookup.insert(key, i);
        Ok(i)
    };
    for ring in &result.loops {
        let ids = ring
            .iter()
            .map(|p| insert_point(*p))
            .collect::<Result<Vec<_>, _>>()?;
        for i in 0..ids.len() {
            edges.push([ids[i], ids[(i + 1) % ids.len()]]);
        }
    }
    for &p in interior {
        if !inside(p, &result.loops[0]) || result.loops[1..].iter().any(|ring| inside(p, ring)) {
            return Err("Interior point is outside its profile.".into());
        }
        insert_point(p)?;
    }
    for line in breaklines {
        if line.len() < 2 {
            return Err("Breakline requires at least two points.".into());
        }
        for pair in line.windows(2) {
            if !segment_in_profile(pair[0], pair[1], &result.loops) {
                return Err("Breakline leaves its profile or crosses a hole.".into());
            }
        }
        let ids = line
            .iter()
            .map(|p| insert_point(*p))
            .collect::<Result<Vec<_>, _>>()?;
        for pair in ids.windows(2) {
            edges.push([pair[0], pair[1]]);
        }
    }
    let mut split = vec![];
    for [a, b] in edges {
        let a = result.points[a];
        let b = result.points[b];
        let d = sub(b, a);
        let len = d[0] * d[0] + d[1] * d[1];
        if len < 1e-20 {
            return Err("Zero-length profile constraint.".into());
        }
        let mut ids: Vec<_> = result
            .points
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                let t = ((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / len;
                if (-1e-9..=1. + 1e-9).contains(&t) && orient(a, b, *p).abs() < 1e-7 {
                    Some((t, i))
                } else {
                    None
                }
            })
            .collect();
        ids.sort_by(|a, b| a.0.total_cmp(&b.0));
        for pair in ids.windows(2) {
            split.push([pair[0].1, pair[1].1]);
        }
    }
    let mut cdt = ConstrainedDelaunayTriangulation::<Vertex>::new();
    let mut handles = vec![];
    for (index, &point) in result.points.iter().enumerate() {
        handles.push(
            cdt.insert(Vertex { point, index })
                .map_err(|e| format!("Triangulation: {e:?}"))?,
        );
    }
    for [a, b] in split {
        if !cdt.can_add_constraint(handles[a], handles[b]) {
            return Err("Profile constraints cross without an authored junction.".into());
        }
        cdt.add_constraint(handles[a], handles[b]);
    }
    for face in cdt.inner_faces() {
        let verts = face.vertices();
        let mut ids = verts.map(|v| v.data().index);
        let [a, b, c] = ids.map(|i| result.points[i]);
        let center = scale(add(add(a, b), c), 1. / 3.);
        if inside(center, &result.loops[0])
            && !result.loops[1..].iter().any(|ring| inside(center, ring))
        {
            if orient(a, b, c) < 0. {
                ids.swap(1, 2);
            }
            result.triangles.push(ids);
        }
    }
    if result.triangles.is_empty() {
        return Err("Profile triangulation is empty.".into());
    }
    Ok(result)
}

fn on_segment(p: V3, a: V3, b: V3) -> bool {
    let extent = hypot(b[0] - a[0], b[1] - a[1]);
    orient(a, b, p).abs() <= 1e-9 * extent
        && p[0] >= a[0].min(b[0]) - 1e-9
        && p[0] <= a[0].max(b[0]) + 1e-9
        && p[1] >= a[1].min(b[1]) - 1e-9
        && p[1] <= a[1].max(b[1]) + 1e-9
}
fn segments_meet(a: V3, b: V3, c: V3, d: V3) -> bool {
    crossing(a, b, c, d)
        || on_segment(a, c, d)
        || on_segment(b, c, d)
        || on_segment(c, a, b)
        || on_segment(d, a, b)
}
fn boundary(p: V3, ring: &[V3]) -> bool {
    (0..ring.len()).any(|i| on_segment(p, ring[i], ring[(i + 1) % ring.len()]))
}
/// Splits at every boundary crossing, including vertices, then checks every open interval.
fn segment_in_profile(a: V3, b: V3, loops: &[Vec<V3>]) -> bool {
    let allowed = |p| {
        (inside(p, &loops[0]) || boundary(p, &loops[0]))
            && !loops[1..]
                .iter()
                .any(|ring| inside(p, ring) && !boundary(p, ring))
    };
    if !allowed(a) || !allowed(b) {
        return false;
    }
    let d = sub(b, a);
    let squared = d[0] * d[0] + d[1] * d[1];
    if squared < 1e-20 {
        return false;
    }
    let mut cuts = vec![0., 1.];
    for ring in loops {
        for i in 0..ring.len() {
            let p = ring[i];
            let q = ring[(i + 1) % ring.len()];
            if on_segment(p, a, b) {
                cuts.push(((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / squared);
            }
            if crossing(a, b, p, q) {
                let e = sub(q, p);
                cuts.push(
                    ((p[0] - a[0]) * e[1] - (p[1] - a[1]) * e[0]) / (d[0] * e[1] - d[1] * e[0]),
                );
            }
        }
    }
    cuts.sort_by(f64::total_cmp);
    cuts.windows(2)
        .all(|pair| allowed(add(a, scale(d, (pair[0] + pair[1]) * 0.5))))
}

impl Profile {
    /// Exact intervals where a plan segment lies on each authored triangle.
    /// Elevation is z0 + t*dz; retaining contacts keep stepped sides distinct.
    pub fn boundary_spans(&self, a: V3, b: V3) -> Vec<[f64; 4]> {
        let mut spans = vec![];
        for triangle in &self.triangles {
            let p = triangle.map(|i| self.points[i]);
            let denominator = orient(p[0], p[1], p[2]);
            if denominator.abs() < 1e-12 {
                continue;
            }
            let weights = |q| {
                [
                    orient(p[1], p[2], q) / denominator,
                    orient(p[2], p[0], q) / denominator,
                    orient(p[0], p[1], q) / denominator,
                ]
            };
            let wa = weights(a);
            let wb = weights(b);
            let mut lo = 0.0_f64;
            let mut hi = 1.0_f64;
            for i in 0..3 {
                let u = p[i];
                let v = p[(i + 1) % 3];
                let f0 = denominator.signum() * orient(u, v, a);
                let f1 = denominator.signum() * orient(u, v, b);
                let tolerance = 1e-5 * hypot(v[0] - u[0], v[1] - u[1]);
                let d = f1 - f0;
                if d.abs() < 1e-12 || (f0.abs() <= tolerance && f1.abs() <= tolerance) {
                    if f0 < -tolerance {
                        hi = -1.;
                    }
                } else if d > 0. {
                    lo = lo.max(-f0 / d);
                } else {
                    hi = hi.min(-f0 / d);
                }
            }
            if hi - lo > 1e-9 {
                let z0 = (0..3).map(|i| wa[i] * p[i][2]).sum::<f64>();
                let z1 = (0..3).map(|i| wb[i] * p[i][2]).sum::<f64>();
                spans.push([lo, hi, z0, z1 - z0]);
            }
        }
        spans
    }
    pub fn transformed(&self, matrix: Matrix) -> Self {
        Self {
            points: self.points.iter().map(|p| matrix.point(*p)).collect(),
            triangles: self
                .triangles
                .iter()
                .map(|t| {
                    if matrix.determinant() < 0. {
                        [t[2], t[1], t[0]]
                    } else {
                        *t
                    }
                })
                .collect(),
            loops: self
                .loops
                .iter()
                .map(|ring| ring.iter().map(|p| matrix.point(*p)).collect())
                .collect(),
        }
    }
    pub fn height_at(&self, x: f64, y: f64) -> Option<f64> {
        for t in &self.triangles {
            let [a, b, c] = t.map(|i| self.points[i]);
            let p = [x, y, 0.];
            let denominator = orient(a, b, c);
            if denominator.abs() < 1e-16 {
                continue;
            }
            let u = orient(b, c, p) / denominator;
            let v = orient(c, a, p) / denominator;
            let w = 1. - u - v;
            if u >= -1e-7 && v >= -1e-7 && w >= -1e-7 {
                return Some(u * a[2] + v * b[2] + w * c[2]);
            }
        }
        None
    }
}
