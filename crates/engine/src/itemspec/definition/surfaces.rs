//! Face-group mesh buffers and closed prisms shared by the version 2 part generators.
//! Positions stay f64 and unrounded: every input is exact IEEE arithmetic or the shared
//! deterministic `sin`/`cos`, so native and Wasm agree bit for bit and coincident
//! corners of neighbouring faces are identical values.
use super::ItemMesh;
use std::collections::BTreeMap;
use superspec_geometry::{
    V3,
    math::{add, cos, cross, dot, length, scale, sin, sub},
    triangulate,
};

pub(super) type P2 = [f64; 2];
const MAXIMUM_TRIANGLES: usize = 250_000;

#[derive(Default)]
struct Buffer {
    positions: Vec<V3>,
    normals: Vec<V3>,
    uvs: Vec<P2>,
    indices: Vec<u32>,
}

pub(super) struct Surfaces<'a> {
    groups: BTreeMap<&'static str, Buffer>,
    triangles: usize,
    cancelled: &'a dyn Fn() -> bool,
}

fn clean(value: f64) -> f64 {
    if value == 0. { 0. } else { value }
}

impl<'a> Surfaces<'a> {
    pub(super) fn new(cancelled: &'a dyn Fn() -> bool) -> Self {
        Self {
            groups: BTreeMap::new(),
            triangles: 0,
            cancelled,
        }
    }

    /// Appends vertices and triangles as given. Triangles wind counter-clockwise seen
    /// from outside unless `flip`.
    pub(super) fn indexed(
        &mut self,
        group: &'static str,
        positions: &[V3],
        normals: &[V3],
        uvs: &[P2],
        triangles: &[[usize; 3]],
        flip: bool,
    ) -> Result<(), String> {
        if positions.len() != normals.len()
            || positions.len() != uvs.len()
            || positions
                .iter()
                .flatten()
                .chain(normals.iter().flatten())
                .chain(uvs.iter().flatten())
                .any(|v| !v.is_finite())
        {
            return Err("Item geometry has nonfinite or mismatched attributes.".into());
        }
        let before = self.triangles / 4096;
        self.triangles += triangles.len();
        if self.triangles > MAXIMUM_TRIANGLES {
            return Err("Item triangle allowance exceeded.".into());
        }
        if self.triangles / 4096 != before && (self.cancelled)() {
            return Err("Item generation cancelled.".into());
        }
        let buffer = self.groups.entry(group).or_default();
        let base = buffer.positions.len();
        buffer
            .positions
            .extend(positions.iter().map(|p| p.map(clean)));
        buffer.normals.extend(normals.iter().map(|n| n.map(clean)));
        buffer.uvs.extend(uvs.iter().map(|uv| uv.map(clean)));
        for triangle in triangles {
            let [a, b, c] = triangle.map(|i| (base + i) as u32);
            buffer
                .indices
                .extend(if flip { [a, c, b] } else { [a, b, c] });
        }
        Ok(())
    }

    /// A convex polygon as a fan. `outward` fixes the winding; `normals` default to the
    /// polygon's own normal. A polygon without area emits nothing.
    pub(super) fn polygon(
        &mut self,
        group: &'static str,
        points: &[V3],
        outward: V3,
        uvs: &[P2],
        normals: Option<&[V3]>,
    ) -> Result<(), String> {
        let mut normal = [0.; 3];
        for i in 1..points.len().saturating_sub(1) {
            normal = add(
                normal,
                cross(sub(points[i], points[0]), sub(points[i + 1], points[0])),
            );
        }
        let size = length(normal);
        if !size.is_finite() {
            return Err("Item geometry has nonfinite attributes.".into());
        }
        if size < 1e-18 {
            return Ok(());
        }
        let flip = dot(normal, outward) < 0.;
        let flat = scale(normal, if flip { -1. } else { 1. } / size);
        let flat = vec![flat; points.len()];
        let triangles: Vec<_> = (1..points.len() - 1).map(|i| [0, i, i + 1]).collect();
        self.indexed(
            group,
            points,
            normals.unwrap_or(&flat),
            uvs,
            &triangles,
            flip,
        )
    }

    /// One mesh per face group that has a slot. An empty `slots` map gives every face
    /// group a slot of its own name.
    pub(super) fn into_meshes(self, part: &str, slots: &BTreeMap<String, String>) -> Vec<ItemMesh> {
        self.groups
            .into_iter()
            .filter(|(_, buffer)| !buffer.indices.is_empty())
            .filter_map(|(group, buffer)| {
                let slot = if slots.is_empty() {
                    group.to_owned()
                } else {
                    slots.get(group)?.clone()
                };
                Some(ItemMesh {
                    id: format!("{part}/{group}"),
                    positions: buffer.positions,
                    normals: buffer.normals,
                    uvs: buffer.uvs,
                    additional_uvs: BTreeMap::new(),
                    colors: vec![],
                    tangents: vec![],
                    indices: buffer.indices,
                    material_slot: slot,
                })
            })
            .collect()
    }
}

/// A plane with a right-handed normal `cross(x, y)`; `x` and `y` are orthonormal.
#[derive(Clone, Copy)]
pub(super) struct Frame {
    pub origin: V3,
    pub x: V3,
    pub y: V3,
}
impl Frame {
    pub(super) fn normal(&self) -> V3 {
        cross(self.x, self.y)
    }
    pub(super) fn at(&self, point: P2, depth: f64) -> V3 {
        add(
            add(self.origin, scale(self.normal(), depth)),
            add(scale(self.x, point[0]), scale(self.y, point[1])),
        )
    }
    pub(super) fn direction(&self, direction: P2) -> V3 {
        add(scale(self.x, direction[0]), scale(self.y, direction[1]))
    }
}

/// A closed 2D loop. `normals` are outward unit normals per point for smooth sides.
#[derive(Clone)]
pub(super) struct Outline {
    pub points: Vec<P2>,
    pub normals: Option<Vec<P2>>,
}
impl Outline {
    pub(super) fn flat(points: Vec<P2>) -> Self {
        Self {
            points,
            normals: None,
        }
    }
    pub(super) fn rectangle(low: P2, high: P2) -> Self {
        Self::flat(vec![low, [high[0], low[1]], high, [low[0], high[1]]])
    }
    pub(super) fn circle(center: P2, radius: f64, segments: usize, smooth: bool) -> Self {
        let normals: Vec<P2> = (0..segments)
            .map(|j| {
                let angle = j as f64 / segments as f64 * std::f64::consts::TAU;
                [cos(angle), sin(angle)]
            })
            .collect();
        Self {
            points: normals
                .iter()
                .map(|n| [center[0] + radius * n[0], center[1] + radius * n[1]])
                .collect(),
            normals: smooth.then_some(normals),
        }
    }
    fn area(&self) -> f64 {
        let p = &self.points;
        (0..p.len())
            .map(|i| {
                let (a, b) = (p[i], p[(i + 1) % p.len()]);
                a[0] * b[1] - a[1] * b[0]
            })
            .sum::<f64>()
            / 2.
    }
    fn counter_clockwise(&self) -> Self {
        let mut result = self.clone();
        if self.area() < 0. {
            result.points.reverse();
            if let Some(normals) = &mut result.normals {
                normals.reverse();
            }
        }
        result
    }
    fn convex(&self) -> bool {
        let p = &self.points;
        (0..p.len()).all(|i| {
            let (a, b, c) = (p[i], p[(i + 1) % p.len()], p[(i + 2) % p.len()]);
            (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]) >= -1e-12
        })
    }
}

/// A closed solid: `outer` (minus `hole`) swept along the frame normal over `depth`.
/// Side texture coordinates are perimeter distance by depth; cap coordinates are the
/// outline's own.
pub(super) fn prism(
    surfaces: &mut Surfaces<'_>,
    groups: [&'static str; 3],
    frame: &Frame,
    outer: &Outline,
    hole: Option<&Outline>,
    depth: [f64; 2],
) -> Result<(), String> {
    let [side, low_cap, high_cap] = groups;
    let depth = [depth[0].min(depth[1]), depth[0].max(depth[1])];
    if outer.points.len() < 3 || hole.is_some_and(|h| h.points.len() < 3) || depth[1] <= depth[0] {
        return Err("A prism needs an outline with area and a positive depth.".into());
    }
    let outer = outer.counter_clockwise();
    let hole = hole.map(Outline::counter_clockwise);
    let normal = frame.normal();
    // Caps.
    let (points, triangles): (Vec<P2>, Vec<[usize; 3]>) = if hole.is_none() && outer.convex() {
        (
            outer.points.clone(),
            (1..outer.points.len() - 1).map(|i| [0, i, i + 1]).collect(),
        )
    } else {
        let ring = |outline: &Outline| outline.points.iter().map(|p| [p[0], p[1], 0.]).collect();
        let mut loops: Vec<Vec<V3>> = vec![ring(&outer)];
        loops.extend(hole.as_ref().map(ring));
        let profile = triangulate(loops, &[], &[])?;
        (
            profile.points.iter().map(|p| [p[0], p[1]]).collect(),
            profile.triangles,
        )
    };
    for (group, at, flip) in [(low_cap, depth[0], true), (high_cap, depth[1], false)] {
        let positions: Vec<V3> = points.iter().map(|p| frame.at(*p, at)).collect();
        let facing = scale(normal, if flip { -1. } else { 1. });
        surfaces.indexed(
            group,
            &positions,
            &vec![facing; points.len()],
            &points,
            &triangles,
            flip,
        )?;
    }
    // Sides. A hole's wall faces into the hole.
    for (outline, sign) in [(Some(&outer), 1.), (hole.as_ref(), -1.)] {
        let Some(outline) = outline else { continue };
        let count = outline.points.len();
        let mut along = 0.;
        for i in 0..count {
            let j = (i + 1) % count;
            let (a, b) = (outline.points[i], outline.points[j]);
            let edge = [b[0] - a[0], b[1] - a[1]];
            let span = superspec_geometry::math::hypot(edge[0], edge[1]);
            if span < 1e-12 {
                continue;
            }
            let face = scale(frame.direction([edge[1] / span, -edge[0] / span]), sign);
            let normals = outline.normals.as_ref().map(|normals| {
                let (na, nb) = (
                    scale(frame.direction(normals[i]), sign),
                    scale(frame.direction(normals[j]), sign),
                );
                [na, nb, nb, na]
            });
            surfaces.polygon(
                side,
                &[
                    frame.at(a, depth[0]),
                    frame.at(b, depth[0]),
                    frame.at(b, depth[1]),
                    frame.at(a, depth[1]),
                ],
                face,
                &[
                    [along, depth[0]],
                    [along + span, depth[0]],
                    [along + span, depth[1]],
                    [along, depth[1]],
                ],
                normals.as_ref().map(|n| n.as_slice()),
            )?;
            along += span;
        }
    }
    Ok(())
}
