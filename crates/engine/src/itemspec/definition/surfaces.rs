//! Face-group mesh buffers and closed prisms shared by the version 2 part generators.
//! Positions stay f64 and unrounded: every input is exact IEEE arithmetic or the shared
//! deterministic `sin`/`cos`, so native and Wasm agree bit for bit and coincident
//! corners of neighbouring faces are identical values.
use super::{ItemMesh, limits::MAX_SOURCE_TRIANGLES};
use std::collections::BTreeMap;
use superspec_geometry::{
    V3,
    math::{add, cos, cross, dot, length, scale, sin, sub},
    triangulate,
};

pub(super) type P2 = [f64; 2];

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
        self.triangles = self.triangles.saturating_add(triangles.len());
        if self.triangles > MAX_SOURCE_TRIANGLES {
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
    caps(
        surfaces,
        [low_cap, high_cap],
        frame,
        [&outer.points, &outer.points],
        hole.as_ref(),
        depth,
    )?;
    // Sides. A hole's wall faces into the hole.
    walls(surfaces, side, frame, &outer, 1., depth)?;
    if let Some(hole) = &hole {
        walls(surfaces, side, frame, hole, -1., depth)?;
    }
    Ok(())
}

/// The two caps of a prism: counter-clockwise `outlines` (low, high) at their depths, minus
/// `hole`. Cap texture coordinates are the outline's own.
fn caps(
    surfaces: &mut Surfaces<'_>,
    groups: [&'static str; 2],
    frame: &Frame,
    outlines: [&[P2]; 2],
    hole: Option<&Outline>,
    depth: [f64; 2],
) -> Result<(), String> {
    let normal = frame.normal();
    let mut shared: Option<(Vec<P2>, Vec<[usize; 3]>)> = None;
    for (index, flip) in [(0, true), (1, false)] {
        let outline = Outline::flat(outlines[index].to_vec());
        if index == 0 || outlines[0] != outlines[1] {
            shared = Some(if hole.is_none() && outline.convex() {
                (
                    outline.points.clone(),
                    (1..outline.points.len() - 1)
                        .map(|i| [0, i, i + 1])
                        .collect(),
                )
            } else {
                let ring =
                    |outline: &Outline| outline.points.iter().map(|p| [p[0], p[1], 0.]).collect();
                let mut loops: Vec<Vec<V3>> = vec![ring(&outline)];
                loops.extend(hole.map(ring));
                let profile = triangulate(loops, &[], &[])?;
                (
                    profile.points.iter().map(|p| [p[0], p[1]]).collect(),
                    profile.triangles,
                )
            });
        }
        let (points, triangles) = shared.as_ref().unwrap();
        let positions: Vec<V3> = points.iter().map(|p| frame.at(*p, depth[index])).collect();
        let facing = scale(normal, if flip { -1. } else { 1. });
        surfaces.indexed(
            groups[index],
            &positions,
            &vec![facing; points.len()],
            points,
            triangles,
            flip,
        )?;
    }
    Ok(())
}

/// The walls of a counter-clockwise outline over `depth`, facing out of it (`sign` 1) or into
/// it (-1, a hole). Texture coordinates are perimeter distance by depth.
fn walls(
    surfaces: &mut Surfaces<'_>,
    group: &'static str,
    frame: &Frame,
    outline: &Outline,
    sign: f64,
    depth: [f64; 2],
) -> Result<(), String> {
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
            group,
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
    Ok(())
}

/// A counter-clockwise loop with every edge moved `distance` to its left (inward; a negative
/// distance grows it). Each corner is where its two moved edges meet; parallel neighbours keep
/// the moved point.
pub(super) fn inset(points: &[P2], distance: f64) -> Vec<P2> {
    let n = points.len();
    let moved: Vec<(P2, P2)> = (0..n)
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % n]);
            let edge = [b[0] - a[0], b[1] - a[1]];
            let span = superspec_geometry::math::hypot(edge[0], edge[1]).max(1e-300);
            let d = [edge[0] / span, edge[1] / span];
            ([a[0] - d[1] * distance, a[1] + d[0] * distance], d)
        })
        .collect();
    (0..n)
        .map(|i| {
            let (a, d) = moved[(i + n - 1) % n];
            let (b, e) = moved[i];
            let denominator = d[0] * e[1] - d[1] * e[0];
            if denominator.abs() < 1e-9 {
                b
            } else {
                let t = ((b[0] - a[0]) * e[1] - (b[1] - a[1]) * e[0]) / denominator;
                [a[0] + d[0] * t, a[1] + d[1] * t]
            }
        })
        .collect()
}

/// Whether the loop moved by `distance` keeps every edge, running the same way and at least a
/// tenth of a millimeter long: what a bevel of that radius needs.
pub(super) fn insets(points: &[P2], distance: f64) -> bool {
    let moved = inset(points, distance);
    let n = points.len();
    (0..n).all(|i| {
        let j = (i + 1) % n;
        let was = [points[j][0] - points[i][0], points[j][1] - points[i][1]];
        let is = [moved[j][0] - moved[i][0], moved[j][1] - moved[i][1]];
        let span = superspec_geometry::math::hypot(was[0], was[1]);
        span < 1e-12 || (was[0] * is[0] + was[1] * is[1]) / span >= 1e-4
    })
}

/// The steps of a quarter-round bevel of `segments` (1 or 2) chords: how far each ring is
/// inset, how far it has risen toward the cap, and the share of the wall's and the cap's
/// normal in its own. Only `sqrt` is used, so every platform agrees.
fn bevel_steps(radius: f64, segments: usize) -> Vec<[f64; 4]> {
    let half = 0.5f64.sqrt();
    let mut steps = vec![[0., 0., 1., 0.]];
    if segments >= 2 {
        steps.push([radius * (1. - half), radius * half, half, half]);
    }
    steps.push([radius, radius, 0., 1.]);
    steps
}

/// The rounded rim between a prism's wall and a cap. The wall ends at `depth` on the
/// counter-clockwise `outline`; the cap lies `radius` further along `toward` (1 or -1 along
/// the frame normal) on the outline moved `radius` into the solid, which is returned. With
/// `sign` -1 the outline is a hole in the solid. The rim continues the wall's texture
/// coordinates: perimeter distance, and `v` running on from `v` by arc length, up or down by
/// `v_sign`. Its normals turn from the wall's to the cap's.
#[allow(clippy::too_many_arguments)]
pub(super) fn rim(
    surfaces: &mut Surfaces<'_>,
    group: &'static str,
    frame: &Frame,
    outline: &Outline,
    sign: f64,
    depth: f64,
    toward: f64,
    [radius, v, v_sign]: [f64; 3],
    segments: usize,
) -> Result<Vec<P2>, String> {
    let steps = bevel_steps(radius, segments);
    let mut rings: Vec<Vec<P2>> = steps
        .iter()
        .map(|step| inset(&outline.points, sign * step[0]))
        .collect();
    let cap = scale(frame.normal(), toward);
    let count = outline.points.len();
    let arc = radius * std::f64::consts::FRAC_PI_2 / (steps.len() - 1) as f64;
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
        let (na, nb) = match &outline.normals {
            Some(normals) => (
                scale(frame.direction(normals[i]), sign),
                scale(frame.direction(normals[j]), sign),
            ),
            None => (face, face),
        };
        for k in 0..steps.len() - 1 {
            let (low, high) = (steps[k], steps[k + 1]);
            let turned = |wall: V3, step: [f64; 4]| add(scale(wall, step[2]), scale(cap, step[3]));
            surfaces.polygon(
                group,
                &[
                    frame.at(rings[k][i], depth + toward * low[1]),
                    frame.at(rings[k][j], depth + toward * low[1]),
                    frame.at(rings[k + 1][j], depth + toward * high[1]),
                    frame.at(rings[k + 1][i], depth + toward * high[1]),
                ],
                add(face, cap),
                &[
                    [along, v + v_sign * arc * k as f64],
                    [along + span, v + v_sign * arc * k as f64],
                    [along + span, v + v_sign * arc * (k + 1) as f64],
                    [along, v + v_sign * arc * (k + 1) as f64],
                ],
                Some(&[
                    turned(na, low),
                    turned(nb, low),
                    turned(nb, high),
                    turned(na, high),
                ]),
            )?;
        }
        along += span;
    }
    Ok(rings.pop().unwrap_or_default())
}

/// A prism whose rims (where its walls meet its caps) are rounded with `radius` in `segments`
/// chords. The rims join the `side` group and continue its texture coordinates.
pub(super) fn beveled_prism(
    surfaces: &mut Surfaces<'_>,
    groups: [&'static str; 3],
    frame: &Frame,
    outer: &Outline,
    depth: [f64; 2],
    radius: f64,
    segments: usize,
) -> Result<(), String> {
    if radius <= 0. {
        return prism(surfaces, groups, frame, outer, None, depth);
    }
    let [side, low_cap, high_cap] = groups;
    let depth = [depth[0].min(depth[1]), depth[0].max(depth[1])];
    let outer = outer.counter_clockwise();
    if outer.points.len() < 3
        || depth[1] - depth[0] <= 2. * radius
        || !insets(&outer.points, radius)
    {
        return Err("A bevel is smaller than half of every dimension of its solid.".into());
    }
    let belt = [depth[0] + radius, depth[1] - radius];
    let low = rim(
        surfaces,
        side,
        frame,
        &outer,
        1.,
        belt[0],
        -1.,
        [radius, belt[0], -1.],
        segments,
    )?;
    let high = rim(
        surfaces,
        side,
        frame,
        &outer,
        1.,
        belt[1],
        1.,
        [radius, belt[1], 1.],
        segments,
    )?;
    caps(
        surfaces,
        [low_cap, high_cap],
        frame,
        [&low, &high],
        None,
        depth,
    )?;
    walls(surfaces, side, frame, &outer, 1., belt)
}

/// A box whose four edges along X (`along_x`) or Y are rounded: one section swept along that
/// axis, so boxes laid end to end keep it. Faces keep the box's groups and texture
/// coordinates (sides: distance round the box by height; top and bottom: x, y). Each rounded
/// strip joins the side it adjoins and runs that side's `v` on by arc length.
pub(super) fn bar(
    surfaces: &mut Surfaces<'_>,
    [sx, sy, sz]: V3,
    along_x: bool,
    radius: f64,
    segments: usize,
    miters: P2,
) -> Result<(), String> {
    let (length, across) = if along_x { (sx, sy) } else { (sy, sx) };
    let section = if radius > 0. {
        Outline::rounded_rectangle([0., 0.], [across, sz], radius, segments)
    } else {
        Outline::rectangle([0., 0.], [across, sz])
    };
    let at = |a: f64, p: P2| {
        if along_x {
            let x = if a == 0. {
                miters[0] * p[0]
            } else {
                a - miters[1] * p[0]
            };
            [x, p[0], p[1]]
        } else {
            [p[0], a, p[1]]
        }
    };
    let direction = |n: P2| {
        if along_x {
            [0., n[0], n[1]]
        } else {
            [n[0], 0., n[1]]
        }
    };
    // Distance round the box (front, right, back, left) of a point `a` along the bar on its
    // low or high side face, and of a point `h` across it on its first or last end face.
    let round = |high: bool, a: f64| match (along_x, high) {
        (true, false) => a,
        (true, true) => sx + sy + (sx - a),
        (false, false) => 2. * sx + sy + (sy - a),
        (false, true) => sx + a,
    };
    let end = |last: bool, h: f64| match (along_x, last) {
        (true, false) => 2. * sx + sy + (sy - h),
        (true, true) => sx + h,
        (false, false) => h,
        (false, true) => sx + sy + (sx - h),
    };
    // Height, run on by the arc turned so far on a rounded corner.
    let arc = radius * std::f64::consts::FRAC_PI_2;
    let v = |p: P2, n: P2| {
        let share = if n[1] == 0. {
            0.
        } else if n[1].abs() == 1. {
            1.
        } else {
            0.5
        };
        if n[1] > 0. {
            sz - radius + arc * share
        } else if n[1] < 0. {
            radius - arc * share
        } else {
            p[1]
        }
    };
    let count = section.points.len();
    for i in 0..count {
        let j = (i + 1) % count;
        let (p, q) = (section.points[i], section.points[j]);
        let (n, m) = match &section.normals {
            Some(normals) => (normals[i], normals[j]),
            None => {
                let span = (q[0] - p[0]).abs() + (q[1] - p[1]).abs();
                let normal = [(q[1] - p[1]) / span, (p[0] - q[0]) / span];
                (normal, normal)
            }
        };
        let group = if n == m && n[1] == 1. {
            "top"
        } else if n == m && n[1] == -1. {
            "bottom"
        } else {
            "side"
        };
        let corners = [(0., p, n), (0., q, m), (length, q, m), (length, p, n)];
        let high = (p[0] + q[0]) / 2. > across / 2.;
        surfaces.polygon(
            group,
            &corners.map(|(a, p, _)| at(a, p)),
            add(direction(n), direction(m)),
            &corners.map(|(a, p, n)| {
                let position = at(a, p);
                if group == "side" {
                    [round(high, position[usize::from(!along_x)]), v(p, n)]
                } else {
                    [position[0], position[1]]
                }
            }),
            Some(&corners.map(|(_, _, n)| direction(n))),
        )?;
    }
    for (last, a) in [(false, 0.), (true, length)] {
        let outward = if last { 1. } else { -1. };
        surfaces.polygon(
            "side",
            &section.points.iter().map(|p| at(a, *p)).collect::<Vec<_>>(),
            if along_x {
                [outward, miters[usize::from(last)], 0.]
            } else {
                [0., outward, 0.]
            },
            &section
                .points
                .iter()
                .map(|p| [end(last, p[0]), p[1]])
                .collect::<Vec<_>>(),
            None,
        )?;
    }
    Ok(())
}

impl Outline {
    /// A rectangle whose corners are rounded with `radius` in `segments` (1 or 2) chords, with
    /// normals that turn round each corner.
    pub(super) fn rounded_rectangle(low: P2, high: P2, radius: f64, segments: usize) -> Self {
        let corners = [low, [high[0], low[1]], high, [low[0], high[1]]];
        let half = 0.5f64.sqrt();
        let (mut points, mut normals) = (Vec::new(), Vec::new());
        for i in 0..4 {
            let (before, corner, after) = (corners[(i + 3) % 4], corners[i], corners[(i + 1) % 4]);
            let unit = |a: P2, b: P2| {
                let span = (b[0] - a[0]).abs() + (b[1] - a[1]).abs();
                [(b[0] - a[0]) / span, (b[1] - a[1]) / span]
            };
            let (d, e) = (unit(before, corner), unit(corner, after));
            let (n, m) = ([d[1], -d[0]], [e[1], -e[0]]);
            let start = [corner[0] - d[0] * radius, corner[1] - d[1] * radius];
            points.push(start);
            normals.push(n);
            if segments >= 2 {
                let center = [start[0] + e[0] * radius, start[1] + e[1] * radius];
                let middle = [(n[0] + m[0]) * half, (n[1] + m[1]) * half];
                points.push([
                    center[0] + middle[0] * radius,
                    center[1] + middle[1] * radius,
                ]);
                normals.push(middle);
            }
            points.push([corner[0] + e[0] * radius, corner[1] + e[1] * radius]);
            normals.push(m);
        }
        Self {
            points,
            normals: Some(normals),
        }
    }
}
