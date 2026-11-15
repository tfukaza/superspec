//! Recast generation and bounded path queries in domain Z-up meters.
use crate::math::{V3, add, dot, length, scale, sub};
use rerecast::{AreaType, ConfigBuilder, HeightfieldBuilder, PolygonNavmesh, TriMesh};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeSet, BinaryHeap};
mod tiles;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Agent {
    pub radius: f64,
    pub height: f64,
    pub step: f64,
    pub slope: f64,
}
impl Default for Agent {
    fn default() -> Self {
        Self {
            radius: 0.35,
            height: 1.8,
            step: 0.45,
            slope: 42.,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mesh {
    pub vertices: Vec<V3>,
    pub polygons: Vec<Vec<usize>>,
    pub neighbors: Vec<Vec<usize>>,
    pub components: Vec<usize>,
    pub cell_size: f64,
    pub cell_height: f64,
    pub tile_count: usize,
    pub walkable_climb: f64,
}

fn checkpoint(cancelled: &dyn Fn() -> bool) -> Result<(), String> {
    if cancelled() {
        Err("Navigation cancelled.".into())
    } else {
        Ok(())
    }
}

/// Generates walkable space from canonical collision triangles, including walls and ceilings.
pub fn build(
    vertices: &[V3],
    triangles: &[[usize; 3]],
    agent: Agent,
    cancelled: &dyn Fn() -> bool,
) -> Result<Mesh, String> {
    build_masked(vertices, triangles, None, agent, cancelled)
}

/// The mask permits support only on movement-specific faces while all triangles remain obstacles.
pub fn build_masked(
    vertices: &[V3],
    triangles: &[[usize; 3]],
    walkable: Option<&[bool]>,
    agent: Agent,
    cancelled: &dyn Fn() -> bool,
) -> Result<Mesh, String> {
    if vertices.len() > 2_000_000
        || triangles.len() > 2_000_000
        || vertices
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 1e7)
        || triangles.iter().flatten().any(|i| *i >= vertices.len())
        || walkable.is_some_and(|m| m.len() != triangles.len())
    {
        return Err("Invalid or oversized navigation geometry.".into());
    }
    if !(0.05..=10.).contains(&agent.radius) {
        return Err("Navigation radius must be between 0.05 and 10 meters.".into());
    }
    if !triangles.is_empty() {
        let cells = |axis: usize| {
            (vertices
                .iter()
                .map(|p| p[axis])
                .fold(f64::NEG_INFINITY, f64::max)
                - vertices
                    .iter()
                    .map(|p| p[axis])
                    .fold(f64::INFINITY, f64::min))
                / (agent.radius / 2.)
        };
        if cells(0) > 256. || cells(1) > 256. {
            return tiles::build(vertices, triangles, walkable, agent, cancelled);
        }
    }
    build_single(vertices, triangles, walkable, agent, cancelled)
}

fn build_single(
    vertices: &[V3],
    triangles: &[[usize; 3]],
    walkable: Option<&[bool]>,
    agent: Agent,
    cancelled: &dyn Fn() -> bool,
) -> Result<Mesh, String> {
    if walkable.is_some_and(|mask| mask.len() != triangles.len()) {
        return Err("Navigation face mask length mismatch.".into());
    }
    if vertices.len() > 2_000_000 || triangles.len() > 2_000_000 {
        return Err("Navigation geometry budget exceeded.".into());
    }
    if !(0.05..=10.).contains(&agent.radius)
        || !(0.1..=100.).contains(&agent.height)
        || !(0. ..=10.).contains(&agent.step)
        || !(0. ..85.).contains(&agent.slope)
    {
        return Err(
            "Navigation agent requires bounded positive dimensions and a slope below 85 degrees."
                .into(),
        );
    }
    checkpoint(cancelled)?;
    if triangles.is_empty() {
        return Ok(Mesh::default());
    }
    let mut input = TriMesh::default();
    for p in vertices {
        if p.iter().any(|v| !v.is_finite() || v.abs() > 1e7) {
            return Err("Navigation requires finite bounded coordinates.".into());
        }
        input
            .vertices
            .push([p[0] as f32, p[2] as f32, -p[1] as f32].into());
    }
    for t in triangles {
        if t.iter().any(|i| *i >= vertices.len()) {
            return Err("Invalid navigation triangle index.".into());
        }
        input.indices.push(t.map(|i| i as u32).into());
        input.area_types.push(AreaType::NOT_WALKABLE);
    }
    let mut aabb = input.compute_aabb().ok_or("Empty navigation bounds.")?;
    let margin = agent.radius as f32 * 4.;
    aabb.min.x -= margin;
    aabb.min.y -= 0.1;
    aabb.min.z -= margin;
    aabb.max.x += margin;
    aabb.max.y += agent.height as f32;
    aabb.max.z += margin;
    let config = ConfigBuilder {
        agent_radius: agent.radius as f32,
        agent_height: agent.height as f32,
        walkable_climb: agent.step as f32,
        walkable_slope_angle: (agent.slope as f32).to_radians(),
        cell_size_fraction: 2.,
        cell_height_fraction: 4.,
        min_region_size: 0,
        merge_region_size: 0,
        aabb,
        ..Default::default()
    }
    .build();
    let columns = ((aabb.max.x - aabb.min.x) / config.cell_size).ceil() as usize;
    let rows = ((aabb.max.z - aabb.min.z) / config.cell_size).ceil() as usize;
    if columns > 16000 || rows > 16000 || columns.saturating_mul(rows) > 4_000_000 {
        return Err(
            "Navigation grid exceeds four million cells. Split this world into smaller projects."
                .into(),
        );
    }
    input.mark_walkable_triangles(config.walkable_slope_angle);
    if let Some(mask) = walkable {
        for (area, enabled) in input.area_types.iter_mut().zip(mask) {
            if !enabled {
                *area = AreaType::NOT_WALKABLE;
            }
        }
    }
    let mut heightfield = HeightfieldBuilder {
        aabb,
        cell_size: config.cell_size,
        cell_height: config.cell_height,
    }
    .build()
    .map_err(|e| e.to_string())?;
    heightfield
        .populate_from_trimesh(input, config.walkable_height, config.walkable_climb)
        .map_err(|e| e.to_string())?;
    checkpoint(cancelled)?;
    let mut compact = heightfield
        .into_compact(config.walkable_height, config.walkable_climb)
        .map_err(|e| e.to_string())?;
    compact.erode_walkable_area(config.walkable_radius);
    checkpoint(cancelled)?;
    compact.build_distance_field();
    compact
        .build_regions(0, config.min_region_area, config.merge_region_area)
        .map_err(|e| e.to_string())?;
    checkpoint(cancelled)?;
    let contours = compact.build_contours(
        config.max_simplification_error,
        config.max_edge_len,
        config.contour_flags,
    );
    let polygons = contours
        .into_polygon_mesh(config.max_vertices_per_polygon)
        .map_err(|e| e.to_string())?;
    checkpoint(cancelled)?;
    let mut out = Mesh {
        cell_size: config.cell_size as f64,
        cell_height: config.cell_height as f64,
        tile_count: 1,
        walkable_climb: config.walkable_climb as f64 * config.cell_height as f64,
        ..Default::default()
    };
    for p in &polygons.vertices {
        out.vertices.push([
            (polygons.aabb.min.x + p.x as f32 * polygons.cell_size) as f64,
            -(polygons.aabb.min.z + p.z as f32 * polygons.cell_size) as f64,
            (polygons.aabb.min.y + p.y as f32 * polygons.cell_height) as f64,
        ]);
    }
    let width = polygons.max_vertices_per_polygon as usize;
    for (face, neighbors) in polygons
        .polygons
        .chunks(width)
        .zip(polygons.polygon_neighbors.chunks(width))
    {
        out.polygons.push(
            face.iter()
                .take_while(|v| **v != PolygonNavmesh::NO_INDEX)
                .map(|v| *v as usize)
                .collect(),
        );
        out.neighbors.push(
            neighbors
                .iter()
                .filter(|v| {
                    **v != PolygonNavmesh::NO_CONNECTION
                        && (**v as usize) < polygons.polygon_count()
                })
                .map(|v| *v as usize)
                .collect(),
        );
    }
    label_components(&mut out);
    Ok(out)
}

fn label_components(out: &mut Mesh) {
    out.components = vec![usize::MAX; out.polygons.len()];
    let mut component = 0;
    for start in 0..out.polygons.len() {
        if out.components[start] != usize::MAX {
            continue;
        }
        let mut stack = vec![start];
        out.components[start] = component;
        while let Some(p) = stack.pop() {
            for &n in &out.neighbors[p] {
                if out.components[n] == usize::MAX {
                    out.components[n] = component;
                    stack.push(n);
                }
            }
        }
        component += 1;
    }
}

/// Closest point on a triangle, including its edges and vertices.
pub fn closest_triangle(p: V3, a: V3, b: V3, c: V3) -> V3 {
    let ab = sub(b, a);
    let ac = sub(c, a);
    let ap = sub(p, a);
    let d1 = dot(ab, ap);
    let d2 = dot(ac, ap);
    if d1 <= 0. && d2 <= 0. {
        return a;
    }
    let bp = sub(p, b);
    let d3 = dot(ab, bp);
    let d4 = dot(ac, bp);
    if d3 >= 0. && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0. && d1 >= 0. && d3 <= 0. {
        return add(a, scale(ab, d1 / (d1 - d3)));
    }
    let cp = sub(p, c);
    let d5 = dot(ab, cp);
    let d6 = dot(ac, cp);
    if d6 >= 0. && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0. && d2 >= 0. && d6 <= 0. {
        return add(a, scale(ac, d2 / (d2 - d6)));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0. && d4 - d3 >= 0. && d5 - d6 >= 0. {
        return add(b, scale(sub(c, b), (d4 - d3) / ((d4 - d3) + (d5 - d6))));
    }
    let sum = va + vb + vc;
    if sum.abs() < 1e-20 {
        return a;
    }
    add(a, add(scale(ab, vb / sum), scale(ac, vc / sum)))
}

#[derive(Clone, Copy)]
struct Queue {
    cost: f64,
    polygon: usize,
}

#[derive(Clone, Debug)]
pub struct Link {
    pub from: V3,
    pub to: V3,
    pub bidirectional: bool,
}

pub struct Path {
    pub points: Vec<V3>,
    pub links: Vec<usize>,
}
impl PartialEq for Queue {
    fn eq(&self, o: &Self) -> bool {
        self.cost == o.cost && self.polygon == o.polygon
    }
}
impl Eq for Queue {}
impl PartialOrd for Queue {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Queue {
    fn cmp(&self, o: &Self) -> Ordering {
        o.cost
            .total_cmp(&self.cost)
            .then_with(|| o.polygon.cmp(&self.polygon))
    }
}

impl Mesh {
    pub fn nearest(&self, p: V3, horizontal: f64, vertical: f64) -> Option<(usize, V3)> {
        let mut nearest = None;
        let mut distance = f64::INFINITY;
        for (i, polygon) in self.polygons.iter().enumerate() {
            for tri in polygon[1..].windows(2) {
                let q = closest_triangle(
                    p,
                    self.vertices[polygon[0]],
                    self.vertices[tri[0]],
                    self.vertices[tri[1]],
                );
                let d = sub(q, p);
                if d[0] * d[0] + d[1] * d[1] <= horizontal * horizontal
                    && d[2].abs() <= vertical
                    && dot(d, d) < distance
                {
                    distance = dot(d, d);
                    nearest = Some((i, q));
                }
            }
        }
        nearest
    }
    fn center(&self, p: usize) -> V3 {
        scale(
            self.polygons[p]
                .iter()
                .fold([0.; 3], |sum, i| add(sum, self.vertices[*i])),
            1. / self.polygons[p].len() as f64,
        )
    }
    fn portal(&self, a: usize, b: usize) -> Option<[V3; 2]> {
        for i in 0..self.polygons[a].len() {
            for j in 0..self.polygons[b].len() {
                let p = self.vertices[self.polygons[a][i]];
                let q = self.vertices[self.polygons[a][(i + 1) % self.polygons[a].len()]];
                let r = self.vertices[self.polygons[b][j]];
                let s = self.vertices[self.polygons[b][(j + 1) % self.polygons[b].len()]];
                let d = sub(q, p);
                let size = (d[0] * d[0] + d[1] * d[1]).sqrt();
                if size < 1e-8 {
                    continue;
                }
                let cross = |v: V3| (d[0] * (v[1] - p[1]) - d[1] * (v[0] - p[0])).abs() / size;
                if cross(r) > 1e-4 || cross(s) > 1e-4 {
                    continue;
                }
                let project = |v: V3| ((v[0] - p[0]) * d[0] + (v[1] - p[1]) * d[1]) / size;
                let (u, v) = (project(r), project(s));
                let lo = 0f64.max(u.min(v));
                let hi = size.min(u.max(v));
                if hi - lo < 1e-6 || (v - u).abs() < 1e-8 {
                    continue;
                }
                let points = [lo, hi].map(|t| crate::math::lerp(p, q, t / size));
                if [lo, hi].iter().all(|t| {
                    (p[2] + d[2] * t / size - (r[2] + (s[2] - r[2]) * (t - u) / (v - u))).abs()
                        <= self.walkable_climb + 1e-4
                }) {
                    return Some(points);
                }
            }
        }
        None
    }
    /// Returns a witnessed polygon corridor path. Length is an upper bound, not a shortest-path proof.
    pub fn path(&self, from: V3, to: V3, horizontal: f64, vertical: f64) -> Option<Vec<V3>> {
        self.path_with_links(from, to, horizontal, vertical, &[])
            .map(|p| p.points)
    }

    /// Authored traversals join otherwise disconnected walkable polygons without changing collision.
    pub fn path_with_links(
        &self,
        from: V3,
        to: V3,
        horizontal: f64,
        vertical: f64,
        links: &[Link],
    ) -> Option<Path> {
        let (start, a) = self.nearest(from, horizontal, vertical)?;
        let (end, b) = self.nearest(to, horizontal, vertical)?;
        if links.is_empty() && self.components[start] != self.components[end] {
            return None;
        }
        let attached: Vec<_> = links
            .iter()
            .map(|link| {
                Some((
                    self.nearest(link.from, horizontal, vertical)?,
                    self.nearest(link.to, horizontal, vertical)?,
                ))
            })
            .collect();
        let mut traversals = vec![vec![]; self.polygons.len()];
        for (i, link) in links.iter().enumerate() {
            if let Some(((from, _), (to, _))) = attached[i] {
                traversals[from].push((to, i, false));
                if link.bidirectional {
                    traversals[to].push((from, i, true));
                }
            }
        }
        let mut costs = vec![f64::INFINITY; self.polygons.len()];
        let mut parent = vec![usize::MAX; costs.len()];
        let mut used_link = vec![None; costs.len()];
        let mut queue = BinaryHeap::new();
        costs[start] = 0.;
        queue.push(Queue {
            cost: 0.,
            polygon: start,
        });
        while let Some(Queue { cost, polygon }) = queue.pop() {
            if polygon == end {
                break;
            }
            if cost > costs[polygon] {
                continue;
            }
            for &next in &self.neighbors[polygon] {
                let value = cost + length(sub(self.center(polygon), self.center(next)));
                if value < costs[next] {
                    costs[next] = value;
                    parent[next] = polygon;
                    used_link[next] = None;
                    queue.push(Queue {
                        cost: value,
                        polygon: next,
                    });
                }
            }
            for &(next, index, reverse) in &traversals[polygon] {
                let ((_, a), (_, b)) = attached[index]?;
                let (a, b) = if reverse { (b, a) } else { (a, b) };
                let value = cost
                    + length(sub(self.center(polygon), a))
                    + length(sub(a, b))
                    + length(sub(b, self.center(next)));
                if value < costs[next] {
                    costs[next] = value;
                    parent[next] = polygon;
                    used_link[next] = Some((index, reverse));
                    queue.push(Queue {
                        cost: value,
                        polygon: next,
                    });
                }
            }
        }
        if !costs[end].is_finite() {
            return None;
        }
        let mut corridor = vec![end];
        while *corridor.last()? != start {
            corridor.push(parent[*corridor.last()?]);
        }
        corridor.reverse();
        let mut path = vec![a];
        let mut used = vec![];
        for pair in corridor.windows(2) {
            if let Some((index, reverse)) = used_link[pair[1]] {
                let ((_, a), (_, b)) = attached[index]?;
                if reverse {
                    path.extend([b, a]);
                } else {
                    path.extend([a, b]);
                }
                used.push(index);
                continue;
            }
            let portal = self.portal(pair[0], pair[1])?;
            path.push(scale(add(portal[0], portal[1]), 0.5));
        }
        path.push(b);
        path.dedup_by(|a, b| length(sub(*a, *b)) < 1e-8);
        Some(Path {
            points: path,
            links: used,
        })
    }
    pub fn component_count(&self) -> usize {
        self.components
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .len()
    }
}
