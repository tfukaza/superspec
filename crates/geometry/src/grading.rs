use crate::{Profile, math::*};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
pub struct Cell {
    pub points: [V3; 3],
    pub weights: [f64; 3],
}
pub struct Ribbon {
    pub cells: Vec<Cell>,
    pub path: Vec<V3>,
}
pub struct Budget(usize);
impl Default for Budget {
    fn default() -> Self {
        Self(20_000_000)
    }
}
impl Budget {
    pub fn spend(&mut self) -> Result<(), String> {
        self.0 = self
            .0
            .checked_sub(1)
            .ok_or("Grading work budget exceeded.")?;
        Ok(())
    }
}
pub fn barycentric(q: V3, p: [V3; 3]) -> Option<[f64; 3]> {
    let den = orient(p[0], p[1], p[2]);
    if den.abs() < 1e-14 {
        return None;
    }
    let a = orient(p[1], p[2], q) / den;
    let b = orient(p[2], p[0], q) / den;
    let c = 1. - a - b;
    if a.min(b).min(c) < -1e-8 {
        None
    } else {
        Some([a, b, c])
    }
}
pub fn bounds(points: &[V3]) -> [f64; 4] {
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
pub fn overlaps(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] <= b[2] && a[2] >= b[0] && a[1] <= b[3] && a[3] >= b[1]
}
pub fn crossings(a: V3, b: V3, c: V3, d: V3) -> Vec<[f64; 2]> {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let ex = d[0] - c[0];
    let ey = d[1] - c[1];
    let den = dx * ey - dy * ex;
    if den.abs() > 1e-12 {
        let t = ((c[0] - a[0]) * ey - (c[1] - a[1]) * ex) / den;
        let u = ((c[0] - a[0]) * dy - (c[1] - a[1]) * dx) / den;
        return if (0.0..=1.).contains(&t) && (0.0..=1.).contains(&u) {
            vec![[t, u]]
        } else {
            vec![]
        };
    }
    if orient(a, b, c).abs() > 1e-8 {
        return vec![];
    }
    let length = dx * dx + dy * dy;
    let other = ex * ex + ey * ey;
    if length == 0. || other == 0. {
        return vec![];
    }
    let first = ((c[0] - a[0]) * dx + (c[1] - a[1]) * dy) / length;
    let last = ((d[0] - a[0]) * dx + (d[1] - a[1]) * dy) / length;
    let lo = 0.0_f64.max(first.min(last));
    let hi = 1.0_f64.min(first.max(last));
    if lo > hi {
        vec![]
    } else {
        [lo, hi]
            .iter()
            .map(|t| {
                [
                    *t,
                    ((a[0] + t * dx - c[0]) * ex + (a[1] + t * dy - c[1]) * ey) / other,
                ]
            })
            .collect()
    }
}
/// Area of a triangle clipped by another triangle, independent of winding.
pub fn overlap_area(a: [V3; 3], b: [V3; 3]) -> f64 {
    let mut polygon = a.to_vec();
    let sign = orient(b[0], b[1], b[2]).signum();
    for i in 0..3 {
        let mut next = vec![];
        for j in 0..polygon.len() {
            let p = polygon[j];
            let q = polygon[(j + 1) % polygon.len()];
            let d = sign * orient(b[i], b[(i + 1) % 3], p);
            let e = sign * orient(b[i], b[(i + 1) % 3], q);
            if d >= 0. {
                next.push(p);
            }
            if (d > 0. && e < 0.) || (d < 0. && e > 0.) {
                next.push(lerp(p, q, d / (d - e)));
            }
        }
        polygon = next;
    }
    area(&polygon).abs()
}
fn grade_height(p: V3, base: f64, ribbon: &Ribbon, budget: &mut Budget) -> Result<f64, String> {
    for cell in &ribbon.cells {
        budget.spend()?;
        if let Some(w) = barycentric(p, cell.points) {
            let blend = (0..3)
                .map(|i| w[i] * cell.weights[i])
                .sum::<f64>()
                .clamp(0., 1.);
            let target = (0..3).map(|i| w[i] * cell.points[i][2]).sum::<f64>();
            return Ok(base + (target - base) * blend);
        }
    }
    Ok(base)
}
struct Line {
    a: V3,
    b: V3,
    bounds: [f64; 4],
    equation: [f64; 3],
}
struct Overlay {
    points: Vec<V3>,
    base: Vec<f64>,
    lines: Vec<BTreeSet<usize>>,
    intersections: BTreeMap<(usize, usize), usize>,
}
impl Overlay {
    fn split(
        &mut self,
        polygon: &[usize],
        line: &Line,
        id: usize,
        budget: &mut Budget,
    ) -> Result<Vec<Vec<usize>>, String> {
        let mut distances = vec![];
        for &i in polygon {
            let p = self.points[i];
            let mut d = if self.lines[i].contains(&id) {
                0.
            } else {
                orient(line.a, line.b, p)
            };
            let scale = line
                .a
                .iter()
                .chain(&line.b)
                .chain(&p)
                .map(|v| v.abs())
                .fold(1., f64::max);
            let error =
                scale * hypot(line.b[0] - line.a[0], line.b[1] - line.a[1]) * f64::EPSILON * 64.;
            if d.abs() <= error {
                self.lines[i].insert(id);
                d = 0.;
            }
            distances.push(d);
        }
        let mut positive = vec![];
        let mut negative = vec![];
        for j in 0..polygon.len() {
            budget.spend()?;
            let a = polygon[j];
            let b = polygon[(j + 1) % polygon.len()];
            let da = distances[j];
            let db = distances[(j + 1) % polygon.len()];
            if da >= 0. {
                positive.push(a);
            }
            if da <= 0. {
                negative.push(a);
            }
            if (da > 0. && db < 0.) || (da < 0. && db > 0.) {
                let common = *self.lines[a]
                    .intersection(&self.lines[b])
                    .next()
                    .ok_or("Grading lost a shared edge identity.")?;
                let key = (common.min(id), common.max(id));
                let vertex = if let Some(i) = self.intersections.get(&key) {
                    *i
                } else {
                    let p = lerp(self.points[a], self.points[b], da / (da - db));
                    let i = self.points.len();
                    self.points.push(p);
                    self.base.push(p[2]);
                    self.lines.push(BTreeSet::from([common, id]));
                    self.intersections.insert(key, i);
                    i
                };
                positive.push(vertex);
                negative.push(vertex);
            }
        }
        Ok([positive, negative]
            .into_iter()
            .filter(|p| {
                p.len() >= 3
                    && area(&p.iter().map(|i| self.points[*i]).collect::<Vec<_>>()).abs() > 5e-13
            })
            .collect())
    }
}
/// Overlay all ribbon boundaries, then refine shared edges until vertical interpolation
/// error is bounded. No independent triangle tessellation or terrain resnapping occurs.
pub fn apply(
    profile: &Profile,
    ribbon: &Ribbon,
    tolerance: f64,
    budget: &mut Budget,
) -> Result<Profile, String> {
    let mut lines: Vec<Line> = vec![];
    for cell in &ribbon.cells {
        for i in 0..3 {
            let a = cell.points[i];
            let b = cell.points[(i + 1) % 3];
            let len = hypot(b[0] - a[0], b[1] - a[1]);
            let mut nx = (b[1] - a[1]) / len;
            let mut ny = (a[0] - b[0]) / len;
            if nx < 0. || (nx == 0. && ny < 0.) {
                nx = -nx;
                ny = -ny;
            }
            let offset = nx * a[0] + ny * a[1];
            let scale = a.iter().chain(&b).map(|v| v.abs()).fold(1., f64::max);
            let found = lines.iter().position(|l| {
                (l.equation[0] - nx).abs() <= f64::EPSILON * 128.
                    && (l.equation[1] - ny).abs() <= f64::EPSILON * 128.
                    && (l.equation[2] - offset).abs() <= f64::EPSILON * 128. * scale
            });
            let bound = bounds(&[a, b]);
            if let Some(i) = found {
                for axis in 0..2 {
                    lines[i].bounds[axis] = lines[i].bounds[axis].min(bound[axis]);
                    lines[i].bounds[axis + 2] = lines[i].bounds[axis + 2].max(bound[axis + 2]);
                }
            } else {
                lines.push(Line {
                    a,
                    b,
                    bounds: bound,
                    equation: [nx, ny, offset],
                });
            }
        }
    }
    let grade_bounds = bounds(
        &ribbon
            .cells
            .iter()
            .flat_map(|c| c.points)
            .collect::<Vec<_>>(),
    );
    let mut overlay = Overlay {
        points: profile.points.clone(),
        base: profile.points.iter().map(|p| p[2]).collect(),
        lines: vec![BTreeSet::new(); profile.points.len()],
        intersections: BTreeMap::new(),
    };
    let mut edge_ids = BTreeMap::new();
    let mut next_id = lines.len();
    let mut triangles = vec![];
    for &triangle in &profile.triangles {
        budget.spend()?;
        let bound = bounds(&triangle.map(|i| overlay.points[i]));
        if !overlaps(bound, grade_bounds) {
            triangles.push(triangle);
            continue;
        }
        for i in 0..3 {
            let (a, b) = (triangle[i], triangle[(i + 1) % 3]);
            let id = *edge_ids.entry((a.min(b), a.max(b))).or_insert_with(|| {
                let id = next_id;
                next_id += 1;
                id
            });
            overlay.lines[a].insert(id);
            overlay.lines[b].insert(id);
        }
        let mut fragments = vec![triangle.to_vec()];
        for (id, line) in lines.iter().enumerate() {
            if overlaps(bound, line.bounds) {
                let mut next = vec![];
                for fragment in &fragments {
                    budget.spend()?;
                    next.extend(overlay.split(fragment, line, id, budget)?);
                    if next.len() > 1_000_000 {
                        return Err("Grading fragment budget exceeded.".into());
                    }
                }
                fragments = next;
            }
        }
        for fragment in fragments {
            for i in 1..fragment.len() - 1 {
                triangles.push([fragment[0], fragment[i], fragment[i + 1]]);
            }
        }
        if triangles.len() > 1_000_000 {
            return Err("Grading triangle budget exceeded.".into());
        }
    }
    for i in 0..overlay.points.len() {
        let p = overlay.points[i];
        if overlaps(bounds(&[p]), grade_bounds) {
            overlay.points[i][2] = grade_height(p, overlay.base[i], ribbon, budget)?;
        }
    }
    let edge = |a: usize, b: usize| (a.min(b), a.max(b));
    let mut midpoints = BTreeMap::new();
    for _ in 0..20 {
        let mut marked = BTreeSet::new();
        for t in &triangles {
            if !overlaps(bounds(&t.map(|i| overlay.points[i])), grade_bounds) {
                continue;
            }
            budget.spend()?;
            let mut refine = false;
            for i in 0..3 {
                let (a, b) = (t[i], t[(i + 1) % 3]);
                let p = lerp(overlay.points[a], overlay.points[b], 0.5);
                let z = grade_height(p, (overlay.base[a] + overlay.base[b]) / 2., ribbon, budget)?;
                if (z - p[2]).abs() > tolerance / 2. {
                    refine = true;
                }
            }
            let center = scale(
                add(
                    add(overlay.points[t[0]], overlay.points[t[1]]),
                    overlay.points[t[2]],
                ),
                1. / 3.,
            );
            let base = t.iter().map(|i| overlay.base[*i] / 3.).sum();
            if (grade_height(center, base, ribbon, budget)? - center[2]).abs() > tolerance / 2. {
                refine = true;
            }
            if refine {
                for i in 0..3 {
                    marked.insert(edge(t[i], t[(i + 1) % 3]));
                }
            }
        }
        if marked.is_empty() {
            return Ok(Profile {
                points: overlay.points,
                triangles,
                loops: profile.loops.clone(),
            });
        }
        for &(a, b) in &marked {
            if let std::collections::btree_map::Entry::Vacant(entry) = midpoints.entry((a, b)) {
                let p = lerp(overlay.points[a], overlay.points[b], 0.5);
                let base = (overlay.base[a] + overlay.base[b]) / 2.;
                let z = grade_height(p, base, ribbon, budget)?;
                entry.insert(overlay.points.len());
                overlay.points.push([p[0], p[1], z]);
                overlay.base.push(base);
            }
        }
        let mut refined = vec![];
        for t in triangles {
            let mids: [Option<usize>; 3] = std::array::from_fn(|i| {
                let key = edge(t[i], t[(i + 1) % 3]);
                if marked.contains(&key) {
                    Some(midpoints[&key])
                } else {
                    None
                }
            });
            let count = mids.iter().filter(|m| m.is_some()).count();
            if count == 0 {
                refined.push(t);
            } else if count == 3 {
                let [a, b, c] = t;
                let [ab, bc, ca] = mids.map(Option::unwrap);
                refined.extend([[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]]);
            } else {
                let first = mids
                    .iter()
                    .position(|m| m.is_some() == (count == 1))
                    .unwrap();
                let (a, b, c) = (t[first], t[(first + 1) % 3], t[(first + 2) % 3]);
                if count == 1 {
                    let ab = mids[first].unwrap();
                    refined.extend([[a, ab, c], [ab, b, c]]);
                } else {
                    let bc = mids[(first + 1) % 3].unwrap();
                    let ca = mids[(first + 2) % 3].unwrap();
                    refined.extend([[c, ca, bc], [a, b, ca], [b, bc, ca]]);
                }
            }
            if refined.len() > 1_000_000 {
                return Err("Grading refinement budget exceeded.".into());
            }
        }
        triangles = refined;
    }
    Err("Grading approximation did not converge within its refinement limit.".into())
}
