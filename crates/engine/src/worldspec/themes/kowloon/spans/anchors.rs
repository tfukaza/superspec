//! Solid-wall coverage and seeded fixing points. No collision or navigation geometry is changed.
use super::walking::Walking;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use superspec_geometry::obstructions::{Budget, Obstructions, Triangle};
use superspec_geometry::{
    V3,
    math::{cross, dot, hypot, sub},
};
use superspec_types::rng::SeededRng;
// Cosine of 0.35 degrees; freeze it for native/Wasm direction grouping.
const SAME_DIRECTION: f64 = 0.9999813422410571;
const MAX_COVERAGE_ENTRIES: usize = 262_144;
const MAX_PLANES: usize = 8192;

#[derive(Clone, Copy, Debug, Serialize, serde::Deserialize)]
pub struct Rect {
    pub u0: f64,
    pub u1: f64,
    pub z0: f64,
    pub z1: f64,
}
impl Rect {
    fn valid(self) -> bool {
        [self.u0, self.u1, self.z0, self.z1]
            .iter()
            .all(|v| v.is_finite() && v.abs() <= 2e6)
            && self.u0 < self.u1
            && self.z0 < self.z1
    }
    fn points(points: &[[f64; 2]]) -> Self {
        Self {
            u0: points.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min),
            u1: points
                .iter()
                .map(|p| p[0])
                .fold(f64::NEG_INFINITY, f64::max),
            z0: points.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min),
            z1: points
                .iter()
                .map(|p| p[1])
                .fold(f64::NEG_INFINITY, f64::max),
        }
    }
}
fn clipped_area(points: &[[f64; 2]], rect: Rect) -> f64 {
    let mut polygon = points.to_vec();
    for (axis, bound, sign) in [
        (0, rect.u0, 1.),
        (0, rect.u1, -1.),
        (1, rect.z0, 1.),
        (1, rect.z1, -1.),
    ] {
        let mut kept = Vec::new();
        for i in 0..polygon.len() {
            let a = polygon[i];
            let b = polygon[(i + 1) % polygon.len()];
            let da = sign * (a[axis] - bound);
            let db = sign * (b[axis] - bound);
            if da >= 0. {
                kept.push(a);
            }
            if (da >= 0.) != (db >= 0.) {
                kept.push(std::array::from_fn(|k| {
                    a[k] + da / (da - db) * (b[k] - a[k])
                }));
            }
        }
        polygon = kept;
    }
    (1..polygon.len().saturating_sub(1))
        .map(|i| {
            ((polygon[i][0] - polygon[0][0]) * (polygon[i + 1][1] - polygon[0][1])
                - (polygon[i][1] - polygon[0][1]) * (polygon[i + 1][0] - polygon[0][0]))
                .abs()
        })
        .sum::<f64>()
        / 2.
}
#[derive(Serialize)]
pub struct Plane {
    pub(super) host: String,
    pub(super) normal: V3,
    pub(super) tangent: V3,
    pub(super) offset: f64,
    pub(super) bounds: Rect,
    pub(super) windows: Vec<Rect>,
    #[serde(skip)]
    polygons: Vec<[[f64; 2]; 3]>,
    #[serde(skip)]
    cells: BTreeMap<(i32, i32), Vec<usize>>,
    #[serde(skip)]
    entries: usize,
}
impl Plane {
    pub fn host(&self) -> &str {
        &self.host
    }
    pub fn normal(&self) -> V3 {
        self.normal
    }
    pub fn bounds(&self) -> Rect {
        self.bounds
    }
    pub fn windows(&self) -> &[Rect] {
        &self.windows
    }
    /// Copies a vertical wall patch into bounded one-meter coverage cells.
    pub fn new(
        host: &str,
        normal: V3,
        offset: f64,
        faces: &[Triangle],
        budget: &mut Budget<'_>,
    ) -> Result<Self, String> {
        Self::indexed(host, normal, offset, faces, budget, MAX_COVERAGE_ENTRIES)
    }
    fn indexed(
        host: &str,
        normal: V3,
        offset: f64,
        faces: &[Triangle],
        budget: &mut Budget<'_>,
        allowance: usize,
    ) -> Result<Self, String> {
        budget.spend(faces.len() + 1)?;
        if host.is_empty()
            || host.len() > 4096
            || faces.is_empty()
            || faces.len() > 1_048_576
            || !offset.is_finite()
            || offset.abs() > 2e6
            || normal.iter().any(|v| !v.is_finite())
            || normal[2].abs() > 1e-8
            || (hypot(normal[0], normal[1]) - 1.).abs() > 1e-7
            || faces
                .iter()
                .flatten()
                .flatten()
                .any(|v| !v.is_finite() || v.abs() > 1e6)
        {
            return Err(
                "Anchor plane requires bounded wall triangles and a horizontal unit normal.".into(),
            );
        }
        let tangent = [normal[1], -normal[0], 0.];
        let polygons: Vec<_> = faces
            .iter()
            .map(|f| f.map(|p| [dot(p, tangent), p[2]]))
            .collect();
        let mut bounds = Rect {
            u0: f64::INFINITY,
            u1: f64::NEG_INFINITY,
            z0: f64::INFINITY,
            z1: f64::NEG_INFINITY,
        };
        for p in polygons.iter().flatten() {
            bounds.u0 = bounds.u0.min(p[0]);
            bounds.u1 = bounds.u1.max(p[0]);
            bounds.z0 = bounds.z0.min(p[1]);
            bounds.z1 = bounds.z1.max(p[1]);
        }
        if !bounds.valid() {
            return Err("Anchor plane needs nonzero wall coverage.".into());
        }
        let mut plane = Self {
            host: host.into(),
            normal,
            tangent,
            offset,
            bounds,
            windows: vec![],
            polygons,
            cells: BTreeMap::new(),
            entries: 0,
        };
        for i in 0..plane.polygons.len() {
            for key in plane.keys(
                Rect::points(&plane.polygons[i]),
                budget,
                allowance - plane.entries,
            )? {
                plane.entries += 1;
                plane.cells.entry(key).or_default().push(i);
            }
        }
        Ok(plane)
    }
    fn keys(
        &self,
        r: Rect,
        budget: &mut Budget<'_>,
        allowance: usize,
    ) -> Result<Vec<(i32, i32)>, String> {
        let a = (
            (r.u0 - self.bounds.u0).floor() as i32,
            (r.z0 - self.bounds.z0).floor() as i32,
        );
        let b = (
            (r.u1 - self.bounds.u0).floor() as i32,
            (r.z1 - self.bounds.z0).floor() as i32,
        );
        let count = (i64::from(b.0) - i64::from(a.0) + 1) * (i64::from(b.1) - i64::from(a.1) + 1);
        if count < 1 || count > allowance.min(MAX_COVERAGE_ENTRIES) as i64 {
            return Err("Anchor coverage query exceeds its cell allowance.".into());
        }
        budget.spend(count as usize)?;
        Ok((a.0..=b.0)
            .flat_map(|u| (a.1..=b.1).map(move |z| (u, z)))
            .collect())
    }
    /// Coverage admits half a percent of missing area for generated-coordinate rounding.
    pub fn covers(&self, r: Rect, budget: &mut Budget<'_>) -> Result<bool, String> {
        budget.spend(1)?;
        if !r.valid() {
            return Err("Anchor coverage requires an ordered finite rectangle.".into());
        }
        if r.u0 < self.bounds.u0
            || r.u1 > self.bounds.u1
            || r.z0 < self.bounds.z0
            || r.z1 > self.bounds.z1
        {
            return Ok(false);
        }
        let mut seen = BTreeSet::new();
        let mut area = 0.;
        for key in self.keys(r, budget, MAX_COVERAGE_ENTRIES)? {
            if let Some(ids) = self.cells.get(&key) {
                for &i in ids {
                    budget.spend(1)?;
                    if seen.insert(i) {
                        area += clipped_area(&self.polygons[i], r);
                    }
                }
            }
        }
        Ok(area >= (r.u1 - r.u0) * (r.z1 - r.z0) * 0.995)
    }
    pub fn point(&self, u: f64, z: f64, out: f64) -> V3 {
        let along = self.offset + out;
        [
            self.tangent[0] * u + self.normal[0] * along,
            self.tangent[1] * u + self.normal[1] * along,
            z,
        ]
    }
}
#[derive(Clone, Serialize, serde::Deserialize)]
pub struct Anchor {
    pub plane: usize,
    pub u: f64,
    pub z: f64,
    pub position: V3,
    pub height: f64,
    pub window: Option<Rect>,
}
pub struct Surroundings<'a> {
    pub obstructions: &'a Obstructions,
    pub floors: &'a Walking,
}
fn anchor_at(
    plane: &Plane,
    index: usize,
    u: f64,
    z: f64,
    half: [f64; 2],
    s: &Surroundings<'_>,
    budget: &mut Budget<'_>,
) -> Result<Option<Anchor>, String> {
    if !plane.covers(
        Rect {
            u0: u - half[0],
            u1: u + half[0],
            z0: z - half[1],
            z1: z + half[1],
        },
        budget,
    )? || s
        .obstructions
        .blocked(plane.point(u, z, 0.03), plane.point(u, z, 0.45), budget)?
    {
        return Ok(None);
    }
    let floor = s
        .floors
        .below(plane.point(u, z, 0.4), budget)?
        .unwrap_or(plane.bounds.z0);
    let height = z - floor;
    Ok((height >= 2.5).then(|| Anchor {
        plane: index,
        u,
        z,
        position: plane.point(u, z, 0.),
        height,
        window: None,
    }))
}
pub fn utility(
    plane: &Plane,
    index: usize,
    seed: &str,
    s: &Surroundings<'_>,
    budget: &mut Budget<'_>,
) -> Result<Vec<Anchor>, String> {
    if seed.len() > 8192 {
        return Err("Span anchor seed exceeds its identity allowance.".into());
    }
    let mut rng = SeededRng::from_text(&format!("{seed}/utility"));
    let b = plane.bounds;
    let columns = ((b.u1 - b.u0 - 0.6) / 1.5).round().max(1.) as usize;
    let pitch = (b.u1 - b.u0 - 0.6) / columns as f64;
    let top = b.z1 - 0.32 - 0.04;
    let rows = ((top - 0.7 - (b.z0 + 0.6)) / 1.15).ceil().max(0.) as usize;
    let count = columns
        .checked_mul(rows + 1)
        .ok_or("Span anchor count overflow.")?;
    if count > 65_536 {
        return Err("Wall exceeds its fixing-point allowance.".into());
    }
    budget.spend(count)?;
    let mut anchors = vec![];
    for column in 0..columns {
        let center = b.u0 + 0.3 + (column as f64 + 0.5) * pitch;
        let mut z = b.z0 + 0.6;
        while z < top - 0.7 {
            let u = center + rng.float(-0.3, 0.3) * pitch;
            let height = z + rng.float(-0.4, 0.4);
            if let Some(a) = anchor_at(plane, index, u, height, [0.11, 0.32], s, budget)? {
                anchors.push(a);
            }
            z += 1.15;
        }
        if let Some(a) = anchor_at(
            plane,
            index,
            center + rng.float(-0.3, 0.3) * pitch,
            top,
            [0.11, 0.32],
            s,
            budget,
        )? {
            anchors.push(a);
        }
    }
    Ok(anchors)
}
pub fn windows(
    plane: &Plane,
    index: usize,
    seed: &str,
    s: &Surroundings<'_>,
    budget: &mut Budget<'_>,
) -> Result<Vec<Anchor>, String> {
    if seed.len() > 8192 || plane.windows.len() > 16_384 || plane.windows.iter().any(|r| !r.valid())
    {
        return Err("Window anchor input exceeds finite geometry or identity allowance.".into());
    }
    budget.spend(plane.windows.len() + 1)?;
    let mut ordered = plane.windows.clone();
    ordered.sort_by(|a, b| a.z0.total_cmp(&b.z0).then(a.u0.total_cmp(&b.u0)));
    let mut rng = SeededRng::from_text(&format!("{seed}/windows"));
    let mut result = vec![];
    for window in ordered {
        let z = window.z0 + (window.z1 - window.z0) * rng.float(0.6, 0.85);
        let side = if rng.chance(0.5) { 1. } else { -1. };
        'reach: for gap in [0.16, 0.28, 0.42] {
            for sign in [side, -side] {
                let u = if sign > 0. {
                    window.u1 + gap
                } else {
                    window.u0 - gap
                };
                if let Some(mut a) = anchor_at(plane, index, u, z, [0.06, 0.2], s, budget)? {
                    a.window = Some(window);
                    result.push(a);
                    break 'reach;
                }
            }
        }
    }
    Ok(result)
}

/// Already classified visible, static host-owned wall/glazing triangles from the finished mesh.
pub struct Face {
    pub host: usize,
    pub glazing: bool,
    pub points: Triangle,
    pub stored_normal: V3,
}
#[derive(Clone)]
struct Facet {
    points: Triangle,
    normal: V3,
    area: f64,
}
struct Group {
    normal: V3,
    offset: f64,
    facets: Vec<Facet>,
}
fn groups(mut facets: Vec<Facet>, budget: &mut Budget<'_>) -> Result<Vec<Group>, String> {
    facets.sort_by(|a, b| b.area.total_cmp(&a.area));
    let mut directions: Vec<(V3, Vec<Facet>)> = vec![];
    for facet in facets {
        budget.spend(directions.len() + 1)?;
        if let Some(d) = directions
            .iter_mut()
            .find(|d| dot(d.0, facet.normal) > SAME_DIRECTION)
        {
            d.1.push(facet);
        } else {
            directions.push((facet.normal, vec![facet]));
        }
    }
    let mut result = vec![];
    for (normal, mut facets) in directions {
        facets.sort_by(|a, b| dot(normal, a.points[0]).total_cmp(&dot(normal, b.points[0])));
        let mut run: Vec<Facet> = vec![];
        for facet in facets {
            budget.spend(1)?;
            if run.last().is_some_and(|last| {
                dot(normal, facet.points[0]) - dot(normal, last.points[0]) > 0.004
            }) {
                result.push(Group {
                    normal,
                    offset: dot(normal, run[run.len() / 2].points[0]),
                    facets: std::mem::take(&mut run),
                });
            }
            run.push(facet);
        }
        if !run.is_empty() {
            result.push(Group {
                normal,
                offset: dot(normal, run[run.len() / 2].points[0]),
                facets: run,
            });
        }
    }
    Ok(result)
}
fn merge_rects(rects: Vec<Rect>, budget: &mut Budget<'_>) -> Result<Vec<Rect>, String> {
    let mut merged: Vec<Rect> = vec![];
    for mut r in rects {
        let mut i = merged.len();
        while i > 0 {
            budget.spend(1)?;
            i -= 1;
            let o = merged[i];
            if o.u0 > r.u1 + 0.15 || o.u1 < r.u0 - 0.15 || o.z0 > r.z1 + 0.15 || o.z1 < r.z0 - 0.15
            {
                continue;
            }
            r = Rect {
                u0: r.u0.min(o.u0),
                u1: r.u1.max(o.u1),
                z0: r.z0.min(o.z0),
                z1: r.z1.max(o.z1),
            };
            merged.remove(i);
            i = merged.len();
        }
        merged.push(r);
    }
    Ok(merged)
}
/// UTF-8 host ordering is deterministic across native/Wasm, independent of the host locale.
pub fn planes(
    hosts: &[String],
    faces: &[Face],
    budget: &mut Budget<'_>,
) -> Result<Vec<Plane>, String> {
    if hosts.len() > 512
        || faces.len() > 1_048_576
        || hosts.iter().any(|h| h.is_empty() || h.len() > 4096)
        || hosts.iter().collect::<BTreeSet<_>>().len() != hosts.len()
    {
        return Err("Span hosts require unique bounded identities and triangle counts.".into());
    }
    budget.spend(faces.len() + 1)?;
    let mut walls = vec![vec![]; hosts.len()];
    let mut panes = walls.clone();
    for f in faces {
        if f.host >= hosts.len()
            || f.points
                .iter()
                .flatten()
                .chain(&f.stored_normal)
                .any(|v| !v.is_finite() || v.abs() > 1e6)
        {
            return Err("Span facets require known hosts and finite bounded data.".into());
        }
        let n = cross(sub(f.points[1], f.points[0]), sub(f.points[2], f.points[0]));
        let horizontal = hypot(n[0], n[1]);
        let length = hypot(horizontal, n[2]);
        if length < 2e-4 || n[2].abs() > length * 0.02 {
            continue;
        }
        let flip = if n[0] * f.stored_normal[0] + n[1] * f.stored_normal[1] < 0. {
            -1.
        } else {
            1.
        };
        let facet = Facet {
            points: f.points,
            normal: [n[0] / horizontal * flip, n[1] / horizontal * flip, 0.],
            area: length / 2.,
        };
        if f.glazing {
            panes[f.host].push(facet);
        } else {
            walls[f.host].push(facet);
        }
    }
    let mut result = vec![];
    let mut entries = 0usize;
    for (i, host) in hosts.iter().enumerate() {
        let mut own = vec![];
        for group in groups(std::mem::take(&mut walls[i]), budget)? {
            if result.len() + own.len() >= MAX_PLANES {
                return Err("Span extraction exceeds its plane allowance.".into());
            }
            let plane = Plane::indexed(
                host,
                group.normal,
                group.offset,
                &group.facets.iter().map(|f| f.points).collect::<Vec<_>>(),
                budget,
                MAX_COVERAGE_ENTRIES - entries,
            )?;
            let width = plane.bounds.u1 - plane.bounds.u0;
            let height = plane.bounds.z1 - plane.bounds.z0;
            let area: f64 = group.facets.iter().map(|f| f.area).sum();
            if width >= 0.8 && height >= 1.6 && area >= 1.2_f64.max(width * height * 0.15) {
                entries += plane.entries;
                own.push(plane);
            }
        }
        for group in groups(std::mem::take(&mut panes[i]), budget)? {
            for facet in group.facets {
                let center: V3 = std::array::from_fn(|axis| {
                    (facet.points[0][axis] + facet.points[1][axis] + facet.points[2][axis]) / 3.
                });
                let mut nearest: Option<usize> = None;
                for (j, p) in own.iter().enumerate() {
                    budget.spend(1)?;
                    let behind = p.offset - group.offset;
                    let u = dot(center, p.tangent);
                    let b = p.bounds;
                    if dot(p.normal, group.normal) < 0.999
                        || !(-0.05..=0.7).contains(&behind)
                        || u < b.u0
                        || u > b.u1
                        || center[2] < b.z0
                        || center[2] > b.z1
                    {
                        continue;
                    }
                    if nearest.is_none_or(|k| p.offset < own[k].offset) {
                        nearest = Some(j);
                    }
                }
                if let Some(j) = nearest {
                    let p = &mut own[j];
                    p.windows.push(Rect::points(
                        &facet.points.map(|v| [dot(v, p.tangent), v[2]]),
                    ));
                }
            }
        }
        for p in &mut own {
            p.windows = merge_rects(std::mem::take(&mut p.windows), budget)?
                .into_iter()
                .filter(|r| r.u1 - r.u0 > 0.4 && r.z1 - r.z0 > 0.5 && r.z1 - r.z0 < 2.8)
                .collect();
        }
        result.extend(own);
    }
    result.sort_by(|a, b| {
        a.host
            .cmp(&b.host)
            .then(a.normal[0].partial_cmp(&b.normal[0]).unwrap())
            .then(a.normal[1].partial_cmp(&b.normal[1]).unwrap())
            .then(a.offset.partial_cmp(&b.offset).unwrap())
    });
    Ok(result)
}
