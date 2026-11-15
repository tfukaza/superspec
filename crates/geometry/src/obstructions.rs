//! Immutable bounded BVH for visual placement. These queries never change physical collision.
use crate::{
    V3,
    math::{cross, dot, sub},
};
pub type Triangle = [V3; 3];
pub struct Budget<'a> {
    remaining: usize,
    cancelled: &'a dyn Fn() -> bool,
}
impl<'a> Budget<'a> {
    pub fn remaining(&self) -> usize {
        self.remaining
    }
    pub fn new(work: usize, cancelled: &'a dyn Fn() -> bool) -> Result<Self, String> {
        if work == 0 || work > 128 * 1024 * 1024 {
            return Err("Spatial work requires a bounded nonzero allowance.".into());
        }
        Ok(Self {
            remaining: work,
            cancelled,
        })
    }
    pub fn spend(&mut self, work: usize) -> Result<(), String> {
        if (self.cancelled)() {
            return Err("Spatial query cancelled.".into());
        }
        self.remaining = self
            .remaining
            .checked_sub(work)
            .ok_or("Spatial query exhausted its work allowance.")?;
        Ok(())
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub low: V3,
    pub high: V3,
}
impl Bounds {
    pub fn points(points: impl IntoIterator<Item = V3>, margin: f64) -> Self {
        let mut result = Self {
            low: [f64::INFINITY; 3],
            high: [f64::NEG_INFINITY; 3],
        };
        for p in points {
            for (i, v) in p.into_iter().enumerate() {
                result.low[i] = result.low[i].min(v - margin);
                result.high[i] = result.high[i].max(v + margin)
            }
        }
        result
    }
    pub fn overlaps(&self, b: Self) -> bool {
        (0..3).all(|i| self.low[i] <= b.high[i] && self.high[i] >= b.low[i])
    }
    fn valid(&self) -> bool {
        point(self.low) && point(self.high) && (0..3).all(|i| self.low[i] <= self.high[i])
    }
}
fn point(p: V3) -> bool {
    p.into_iter()
        .all(|v| v.is_finite() && v.abs() <= 1_000_000.)
}
struct Node {
    bounds: Bounds,
    start: usize,
    end: usize,
    children: Option<[usize; 2]>,
}
/// Original triangles are owned once. Each occurs in one leaf, even when it spans the whole scene.
pub struct Obstructions {
    triangles: Vec<Triangle>,
    order: Vec<usize>,
    nodes: Vec<Node>,
    reserved: Vec<Triangle>,
}
impl Obstructions {
    pub fn new(triangles: &[Triangle], budget: &mut Budget<'_>) -> Result<Self, String> {
        if triangles.len() > 1_048_576 {
            return Err("Visual obstruction index exceeds its triangle allowance.".into());
        }
        budget.spend(triangles.len() + 1)?;
        if triangles.iter().flatten().any(|p| !point(*p)) {
            return Err(
                "Visual obstruction triangles require bounded finite world coordinates.".into(),
            );
        }
        let mut index = Self {
            triangles: triangles.to_vec(),
            order: (0..triangles.len()).collect(),
            nodes: Vec::with_capacity(triangles.len().div_ceil(4)),
            reserved: Vec::new(),
        };
        if !triangles.is_empty() {
            index.build(0, triangles.len(), budget)?;
        }
        Ok(index)
    }
    fn build(
        &mut self,
        start: usize,
        end: usize,
        budget: &mut Budget<'_>,
    ) -> Result<usize, String> {
        budget.spend(end - start)?;
        let bounds = Bounds::points(
            self.order[start..end]
                .iter()
                .flat_map(|i| self.triangles[*i]),
            0.,
        );
        let id = self.nodes.len();
        self.nodes.push(Node {
            bounds,
            start,
            end,
            children: None,
        });
        if end - start > 8 {
            let extent = sub(bounds.high, bounds.low);
            let axis = (0..3)
                .max_by(|a, b| extent[*a].total_cmp(&extent[*b]))
                .unwrap();
            let mid = (start + end) / 2;
            let triangles = &self.triangles;
            self.order[start..end].select_nth_unstable_by(mid - start, |a, b| {
                let center = |i: usize| {
                    triangles[i][0][axis] + triangles[i][1][axis] + triangles[i][2][axis]
                };
                center(*a).total_cmp(&center(*b)).then(a.cmp(b))
            });
            let children = [
                self.build(start, mid, budget)?,
                self.build(mid, end, budget)?,
            ];
            self.nodes[id].children = Some(children);
        }
        Ok(id)
    }
    fn visit(
        &self,
        bounds: Bounds,
        budget: &mut Budget<'_>,
        mut test: impl FnMut(usize) -> bool,
    ) -> Result<bool, String> {
        if !bounds.valid() {
            return Err("Spatial query requires a finite ordered world box.".into());
        }
        budget.spend(1)?;
        if self.nodes.is_empty() {
            return Ok(false);
        }
        let mut stack = vec![0];
        while let Some(id) = stack.pop() {
            budget.spend(1)?;
            let node = &self.nodes[id];
            if !node.bounds.overlaps(bounds) {
                continue;
            }
            if let Some(children) = node.children {
                stack.extend(children)
            } else {
                for &i in &self.order[node.start..node.end] {
                    budget.spend(1)?;
                    if Bounds::points(self.triangles[i], 0.).overlaps(bounds) && test(i) {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }
    /// Tight world-box candidates in stable source order. Reserved sheets intentionally stay separate.
    pub fn near(&self, bounds: Bounds, budget: &mut Budget<'_>) -> Result<Vec<usize>, String> {
        let mut indices = Vec::new();
        self.visit(bounds, budget, |i| {
            indices.push(i);
            false
        })?;
        indices.sort_unstable();
        Ok(indices)
    }
    pub fn triangle(&self, index: usize) -> Option<&Triangle> {
        self.triangles.get(index)
    }
    pub fn blocked(&self, a: V3, b: V3, budget: &mut Budget<'_>) -> Result<bool, String> {
        if !point(a) || !point(b) {
            return Err("Sight line exceeds finite world coordinates.".into());
        }
        if self.visit(Bounds::points([a, b], 0.), budget, |i| {
            segment_crosses(a, b, &self.triangles[i])
        })? {
            return Ok(true);
        }
        for t in &self.reserved {
            budget.spend(1)?;
            if segment_crosses(a, b, t) {
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub fn blocked_path(&self, points: &[V3], budget: &mut Budget<'_>) -> Result<bool, String> {
        if points.len() > 16384 || points.iter().any(|p| !point(*p)) {
            return Err("Polyline exceeds bounded spatial input.".into());
        }
        for pair in points.windows(2) {
            if self.blocked(pair[0], pair[1], budget)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub fn pierced(&self, quad: [V3; 4], budget: &mut Budget<'_>) -> Result<bool, String> {
        if quad.into_iter().any(|p| !point(p)) {
            return Err("Sheet exceeds finite world coordinates.".into());
        }
        let sheet = [[quad[0], quad[1], quad[2]], [quad[0], quad[2], quad[3]]];
        self.visit(Bounds::points(quad, 0.), budget, |i| {
            let t = &self.triangles[i];
            (0..3).any(|e| {
                sheet
                    .iter()
                    .any(|s| segment_crosses(t[e], t[(e + 1) % 3], s))
            })
        })
    }
    pub fn reserve(&mut self, quad: [V3; 4]) -> Result<(), String> {
        if self.reserved.len() >= 8192 || quad.into_iter().any(|p| !point(p)) {
            return Err("Reserved sheets exceed bounded spatial input.".into());
        }
        self.reserved
            .extend([[quad[0], quad[1], quad[2]], [quad[0], quad[2], quad[3]]]);
        Ok(())
    }
}
/// Inclusive segment/triangle intersection; coplanar segments do not cross either face.
fn segment_crosses(a: V3, b: V3, t: &Triangle) -> bool {
    let e1 = sub(t[1], t[0]);
    let e2 = sub(t[2], t[0]);
    let d = sub(b, a);
    let p = cross(d, e2);
    let determinant = dot(e1, p);
    if determinant.abs() < 1e-12 {
        return false;
    }
    let inverse = 1. / determinant;
    let s = sub(a, t[0]);
    let u = dot(s, p) * inverse;
    if !(0. ..=1.).contains(&u) {
        return false;
    }
    let q = cross(s, e1);
    let v = dot(d, q) * inverse;
    if v < 0. || u + v > 1. {
        return false;
    }
    let t = dot(e2, q) * inverse;
    (0. ..=1.).contains(&t)
}
/// Separating-axis contact test; inputs are admitted by the owning spatial query.
pub fn triangle_touches_box(triangle: &Triangle, center: V3, half: V3) -> bool {
    let v = triangle.map(|p| sub(p, center));
    for axis in 0..3 {
        if v.iter().map(|p| p[axis]).fold(f64::INFINITY, f64::min) > half[axis]
            || v.iter().map(|p| p[axis]).fold(f64::NEG_INFINITY, f64::max) < -half[axis]
        {
            return false;
        }
    }
    let edges = std::array::from_fn::<_, 3, _>(|i| sub(v[(i + 1) % 3], v[i]));
    let separated = |axis: V3| {
        let projected = v.map(|p| dot(p, axis));
        let reach = half[0] * axis[0].abs() + half[1] * axis[1].abs() + half[2] * axis[2].abs();
        projected.into_iter().fold(f64::INFINITY, f64::min) > reach
            || projected.into_iter().fold(f64::NEG_INFINITY, f64::max) < -reach
    };
    for e in edges {
        for axis in [[0., -e[2], e[1]], [e[2], 0., -e[0]], [-e[1], e[0], 0.]] {
            if separated(axis) {
                return false;
            }
        }
    }
    !separated(cross(edges[0], edges[1]))
}
