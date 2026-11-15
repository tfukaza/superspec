//! Finish-aware shape composition for Kowloon items and receiver surveys.
#![allow(clippy::too_many_arguments)]
use super::mesh::Mesh;
use superspec_geometry::{
    Matrix, V3,
    math::{add, cross, dot, length, scale, sub, unit},
};

/// Tint callbacks receive local positions for every emitted corner, before triangle admission.
pub enum Tint<'a> {
    Color(V3),
    Field(&'a dyn Fn(V3) -> Result<V3, String>),
}
impl Tint<'_> {
    pub(super) fn at(&self, p: V3) -> Result<V3, String> {
        match self {
            Self::Color(c) => Ok(*c),
            Self::Field(f) => f(p),
        }
    }
}
pub use crate::itemspec::primitives::PatchVertex;

/// Orthonormal local meters, including reflection. Facade X runs right, Y outward, Z upward.
#[derive(Clone)]
pub struct Pen {
    pub matrix: Matrix,
    pub part: String,
    pub mirrored: bool,
    pub(super) timber: Option<super::timber::Timber>,
    pub(super) weathered: Option<super::weathered::Style>,
    brushed: Option<super::brushed::Brushed>,
    room_patina: Option<super::shop::patina::Room>,
    shelf_patina: Option<super::shop::patina::Shelf>,
}
impl Pen {
    pub fn new(matrix: Matrix, part: &str) -> Result<Self, String> {
        crate::itemspec::primitives::check_frame(matrix, part)?;
        Ok(Self {
            matrix,
            part: part.into(),
            mirrored: matrix.determinant() < 0.,
            timber: None,
            weathered: None,
            brushed: None,
            room_patina: None,
            shelf_patina: None,
        })
    }
    pub fn moved(&self, offset: V3) -> Result<Self, String> {
        let mut matrix = self.matrix;
        matrix.0[3] = matrix.point(offset);
        self.reframed(matrix, &self.part)
    }
    /// Attaches an owned timber finish; member scopes remain local to each shape emission.
    pub fn with_timber(&self, settings: super::timber::Settings) -> Result<Self, String> {
        let mut pen = self.clone();
        pen.timber = Some(super::timber::Timber::new(settings)?);
        pen.weathered = None;
        pen.brushed = None;
        Ok(pen)
    }
    /// Replaces inherited finishes with the stainless slot and physical-scale planar charts.
    pub fn with_brushed_metal(&self, repeat: f64) -> Result<Self, String> {
        self.with_stainless(repeat, "brushed-metal")
    }
    pub(super) fn with_kitchen_metal(&self, repeat: f64) -> Result<Self, String> {
        self.with_stainless(repeat, "kitchen-metal")
    }
    fn with_stainless(&self, repeat: f64, material: &'static str) -> Result<Self, String> {
        let mut pen = self.clone();
        pen.brushed = Some(super::brushed::Brushed::new(self, repeat, material)?);
        pen.weathered = None;
        pen.timber = None;
        Ok(pen)
    }
    /// Starts a metal assembly in this detached drawing; derived pens share its exposure collector.
    pub fn with_weathering(
        &self,
        mesh: &mut Mesh<'_>,
        settings: super::weathered::Settings,
        kind: super::metal::Kind,
        mapping: Option<super::weathered::Mapping>,
        slots: Vec<String>,
    ) -> Result<Self, String> {
        if mesh.surveying() {
            return Ok(self.clone());
        }
        let mut pen = self.clone();
        let existing = if mapping.is_none() {
            self.weathered
                .as_ref()
                .map(|s| s.reuse(mesh, &settings, kind, &slots))
                .transpose()?
                .flatten()
        } else {
            None
        };
        pen.weathered = Some(if let Some(style) = existing {
            style
        } else {
            super::weathered::Style::attach(self, mesh, settings, kind, mapping, slots)?
        });
        pen.timber = None;
        pen.brushed = None;
        Ok(pen)
    }
    pub fn into_part(&self, part: &str) -> Result<Self, String> {
        self.reframed(self.matrix, part)
    }
    pub fn facing(&self, origin: V3, tangent: V3, outward: V3) -> Result<Self, String> {
        self.reframed(
            self.matrix
                .then(Matrix([tangent, outward, [0., 0., 1.], origin])),
            &self.part,
        )
    }
    /// Facade handedness does not change the host's lettering reflection state.
    pub(super) fn reframed(&self, matrix: Matrix, part: &str) -> Result<Self, String> {
        let mut pen = self.shape_frame(matrix, part)?;
        pen.timber = self.timber.as_ref().map(super::timber::Timber::derived);
        pen.weathered = self.weathered.clone();
        pen.brushed = self.brushed.as_ref().map(|s| s.derived(&pen)).transpose()?;
        Ok(pen)
    }
    /// Fresh shape scopes reset material adapters while retaining ambient assembly shading.
    pub(super) fn shape_frame(&self, matrix: Matrix, part: &str) -> Result<Self, String> {
        let mut pen = Self::new(matrix, part)?;
        pen.room_patina = self.room_patina.clone();
        pen.shelf_patina = self.shelf_patina.clone();
        pen.mirrored = self.mirrored;
        Ok(pen)
    }
    pub(super) fn with_room_patina_frame(
        &self,
        frame: &Self,
        size: V3,
        seed: u32,
    ) -> Result<Self, String> {
        let mut pen = self.clone();
        pen.room_patina = Some(super::shop::patina::Room {
            inverse: frame.matrix.inverse()?,
            size,
            seed: seed as f64,
        });
        Ok(pen)
    }

    pub(super) fn with_shelf_patina(&self, patina: super::shop::patina::Shelf) -> Self {
        let mut pen = self.clone();
        pen.shelf_patina = Some(patina);
        pen
    }

    fn shade(&self, slot: &str, position: V3, normal: V3, mut color: V3) -> V3 {
        if matches!(
            slot,
            "pane" | "glass" | "display-glass" | "interior-lit" | "lettering"
        ) || self.part.contains("bulb-emitter")
            || self.part.contains("tube-emitter")
        {
            return color;
        }
        if let Some(room) = &self.room_patina {
            let tint = room.tint(position, normal);
            color = std::array::from_fn(|axis| color[axis] * tint[axis]);
        }
        if let Some(shelf) = &self.shelf_patina {
            let tint = shelf.tint(position, normal);
            color = std::array::from_fn(|axis| color[axis] * tint[axis]);
        }
        color
    }

    pub fn world(&self, p: V3) -> V3 {
        self.matrix.point(p)
    }

    /// Convex polygon fan; explicit outward hint controls winding, never UV orientation.
    pub fn face(
        &self,
        mesh: &mut Mesh<'_>,
        slot: &str,
        points: &[V3],
        hint: V3,
        tint: &Tint<'_>,
        repeat: f64,
        uv: Option<&[[f64; 2]]>,
        normals: Option<&[V3]>,
    ) -> Result<(), String> {
        if let Some(brushed) = &self.brushed {
            return brushed.face(self, mesh, slot, points, hint, tint, repeat, uv, normals);
        }
        if let Some(timber) = &self.timber {
            return timber.face(self, mesh, slot, points, hint, tint, repeat, uv, normals);
        }
        if let Some(weathered) = &self.weathered {
            return weathered.face(self, mesh, slot, points, hint, tint, repeat, uv, normals);
        }
        self.face_raw(mesh, slot, points, hint, tint, repeat, uv, normals)
    }
    pub(super) fn face_raw(
        &self,
        mesh: &mut Mesh<'_>,
        slot: &str,
        points: &[V3],
        hint: V3,
        tint: &Tint<'_>,
        repeat: f64,
        uv: Option<&[[f64; 2]]>,
        normals: Option<&[V3]>,
    ) -> Result<(), String> {
        crate::itemspec::primitives::face_admitted(
            mesh,
            self.matrix,
            &self.part,
            slot,
            points,
            hint,
            repeat,
            uv,
            normals,
            &|local, world, normal| Ok(self.shade(slot, world, normal, tint.at(local)?)),
        )
    }
    pub fn panel(
        &self,
        mesh: &mut Mesh<'_>,
        slot: &str,
        span: [f64; 4],
        y: f64,
        tint: &Tint<'_>,
        repeat: f64,
    ) -> Result<(), String> {
        let [x0, x1, z0, z1] = span;
        self.face(
            mesh,
            slot,
            &[[x0, y, z0], [x1, y, z0], [x1, y, z1], [x0, y, z1]],
            [0., 1., 0.],
            tint,
            repeat,
            None,
            None,
        )
    }
    /// Missing face letters: left/right, back/front, down/up. An empty string closes every side.
    pub fn cuboid(
        &self,
        mesh: &mut Mesh<'_>,
        slot: &str,
        lo: V3,
        hi: V3,
        tint: &Tint<'_>,
        repeat: f64,
        omit: &str,
    ) -> Result<(), String> {
        if let Some(timber) = &self.timber {
            let mut pen = self.clone();
            pen.timber = Some(timber.for_box(self, lo, hi)?);
            return pen.cuboid_raw(mesh, slot, lo, hi, tint, repeat, omit);
        }
        if let Some(style) = &self.weathered
            && style.selected(slot, mesh)
        {
            let size = sub(hi, lo);
            if size.iter().any(|v| !v.is_finite() || *v <= 0.) {
                return Err("Invalid metal box extents.".into());
            }
            let mut axis = 0;
            for i in 1..3 {
                if size[i] > size[axis] {
                    axis = i;
                }
            }
            let a = std::array::from_fn(|i| {
                if i == axis {
                    lo[i]
                } else {
                    (lo[i] + hi[i]) / 2.
                }
            });
            let mut b = a;
            b[axis] = hi[axis];
            let section: Vec<_> = (0..3).filter(|i| *i != axis).map(|i| size[i]).collect();
            let pen = style.scoped(
                self,
                mesh,
                slot,
                a,
                b,
                section[0].min(section[1]) / 2.,
                section[0].max(section[1]),
                false,
                None,
            )?;
            return pen.cuboid_raw(mesh, slot, lo, hi, tint, repeat, omit);
        }
        self.cuboid_raw(mesh, slot, lo, hi, tint, repeat, omit)
    }
    fn cuboid_raw(
        &self,
        mesh: &mut Mesh<'_>,
        slot: &str,
        lo: V3,
        hi: V3,
        tint: &Tint<'_>,
        repeat: f64,
        omit: &str,
    ) -> Result<(), String> {
        if (0..3).any(|i| !lo[i].is_finite() || !hi[i].is_finite() || hi[i] <= lo[i])
            || omit.chars().any(|c| !"lrbfdu".contains(c))
        {
            return Err(
                "Procedural box requires positive finite extents and known face omissions.".into(),
            );
        }
        let p = [
            [lo[0], lo[1], lo[2]],
            [hi[0], lo[1], lo[2]],
            [hi[0], lo[1], hi[2]],
            [lo[0], lo[1], hi[2]],
            [lo[0], hi[1], lo[2]],
            [hi[0], hi[1], lo[2]],
            [hi[0], hi[1], hi[2]],
            [lo[0], hi[1], hi[2]],
        ];
        for (letter, indices, hint) in [
            ('l', [0, 4, 7, 3], [-1., 0., 0.]),
            ('r', [1, 2, 6, 5], [1., 0., 0.]),
            ('b', [0, 3, 2, 1], [0., -1., 0.]),
            ('f', [4, 5, 6, 7], [0., 1., 0.]),
            ('d', [0, 1, 5, 4], [0., 0., -1.]),
            ('u', [3, 7, 6, 2], [0., 0., 1.]),
        ] {
            if !omit.contains(letter) {
                self.face(
                    mesh,
                    slot,
                    &indices.map(|i| p[i]),
                    hint,
                    tint,
                    repeat,
                    None,
                    None,
                )?;
            }
        }
        Ok(())
    }
    pub fn sheet(
        &self,
        mesh: &mut Mesh<'_>,
        slot: &str,
        points: &[V3],
        front: &Tint<'_>,
        back: &Tint<'_>,
        repeat: f64,
    ) -> Result<(), String> {
        if points.len() < 3 {
            return Err("Sheet requires at least three points.".into());
        }
        let normal = unit(cross(sub(points[1], points[0]), sub(points[2], points[0])))?;
        self.face(mesh, slot, points, normal, front, repeat, None, None)?;
        self.face(
            mesh,
            slot,
            points,
            scale(normal, -1.),
            back,
            repeat,
            None,
            None,
        )
    }
    /// Square section without end caps; the first axis determines a reproducible roll.
    pub fn bar(
        &self,
        mesh: &mut Mesh<'_>,
        slot: &str,
        a: V3,
        b: V3,
        thickness: f64,
        tint: &Tint<'_>,
        repeat: f64,
    ) -> Result<(), String> {
        positive(thickness)?;
        if let Some(style) = &self.weathered {
            let pen = style.scoped(
                self,
                mesh,
                slot,
                a,
                b,
                thickness / 2.,
                thickness,
                false,
                None,
            )?;
            return pen.bar_raw(mesh, slot, a, b, thickness, tint, repeat);
        }
        self.bar_raw(mesh, slot, a, b, thickness, tint, repeat)
    }
    fn bar_raw(
        &self,
        mesh: &mut Mesh<'_>,
        slot: &str,
        a: V3,
        b: V3,
        thickness: f64,
        tint: &Tint<'_>,
        repeat: f64,
    ) -> Result<(), String> {
        let (_, side, other) = basis(sub(b, a))?;
        let h = thickness / 2.;
        for (u, v) in [
            (side, other),
            (other, scale(side, -1.)),
            (scale(side, -1.), scale(other, -1.)),
            (scale(other, -1.), side),
        ] {
            let offsets = [
                add(scale(u, h), scale(v, -h)),
                add(scale(u, h), scale(v, h)),
            ];
            self.face(
                mesh,
                slot,
                &[
                    add(a, offsets[0]),
                    add(b, offsets[0]),
                    add(b, offsets[1]),
                    add(a, offsets[1]),
                ],
                u,
                tint,
                repeat,
                None,
                None,
            )?;
        }
        Ok(())
    }
    pub fn beveled_bar(
        &self,
        mesh: &mut Mesh<'_>,
        slot: &str,
        a: V3,
        b: V3,
        side: V3,
        width: f64,
        depth: f64,
        bevel: f64,
        tint: &Tint<'_>,
        repeat: f64,
    ) -> Result<(), String> {
        if let Some(timber) = &self.timber {
            return timber.beveled_bar(
                self, mesh, slot, a, b, side, width, depth, bevel, tint, repeat,
            );
        }
        if let Some(style) = &self.weathered {
            let pen = style.scoped(
                self,
                mesh,
                slot,
                a,
                b,
                width.min(depth) / 2.,
                width.max(depth),
                true,
                None,
            )?;
            return pen.beveled_bar_raw(mesh, slot, a, b, side, width, depth, bevel, tint, repeat);
        }
        self.beveled_bar_raw(mesh, slot, a, b, side, width, depth, bevel, tint, repeat)
    }
    pub(super) fn beveled_bar_raw(
        &self,
        mesh: &mut Mesh<'_>,
        slot: &str,
        a: V3,
        b: V3,
        side: V3,
        width: f64,
        depth: f64,
        bevel: f64,
        tint: &Tint<'_>,
        repeat: f64,
    ) -> Result<(), String> {
        positive(width)?;
        positive(depth)?;
        let delta = sub(b, a);
        let size = length(delta);
        if !size.is_finite()
            || size < 1e-6
            || !bevel.is_finite()
            || bevel < 0.
            || side.iter().any(|v| !v.is_finite())
            || (length(side) - 1.).abs() > 1e-5
            || (dot(side, delta) / size).abs() > 1e-5
        {
            return Err("Invalid beveled member dimensions or orthogonal side vector.".into());
        }
        let axis = scale(delta, 1. / size);
        let other = unit(cross(axis, side))?;
        let u = scale(side, width / 2.);
        let v = scale(other, depth / 2.);
        let inset = bevel.min(width.min(depth) * 0.18);
        let sx = 1. - inset * 2. / width;
        let sy = 1. - inset * 2. / depth;
        let section = if inset > 0. {
            vec![
                [-sx, -1.],
                [sx, -1.],
                [1., -sy],
                [1., sy],
                [sx, 1.],
                [-sx, 1.],
                [-1., sy],
                [-1., -sy],
            ]
        } else {
            vec![[-1., -1.], [1., -1.], [1., 1.], [-1., 1.]]
        };
        let ring = |at| {
            section
                .iter()
                .map(|[x, y]| add(at, add(scale(u, *x), scale(v, *y))))
                .collect::<Vec<_>>()
        };
        let start = ring(a);
        let end = ring(b);
        for k in 0..section.len() {
            let next = (k + 1) % section.len();
            let outward = sub(scale(add(start[k], start[next]), 0.5), a);
            self.face(
                mesh,
                slot,
                &[start[k], start[next], end[next], end[k]],
                outward,
                tint,
                repeat,
                None,
                None,
            )?;
        }
        self.face(
            mesh,
            slot,
            &start,
            scale(axis, -1.),
            tint,
            repeat,
            None,
            None,
        )?;
        self.face(mesh, slot, &end, axis, tint, repeat, None, None)
    }
    pub fn tube(
        &self,
        mesh: &mut Mesh<'_>,
        slot: &str,
        path: &[V3],
        radius: f64,
        sides: usize,
        tint: &Tint<'_>,
        repeat: f64,
    ) -> Result<(), String> {
        positive(radius)?;
        segments(sides)?;
        if path.len() < 2 || path.len() > 65536 || path.iter().flatten().any(|v| !v.is_finite()) {
            return Err("Invalid tube path.".into());
        }
        mesh.check_work((path.len() - 1) * sides * 2)?;
        for pair in path.windows(2) {
            mesh.check_work(0)?;
            let [a, b] = [pair[0], pair[1]];
            if length(sub(b, a)) < 1e-6 {
                continue;
            }
            let (_, side, other) = basis(sub(b, a))?;
            let scoped;
            let pen = if let Some(style) = &self.weathered {
                scoped = style.scoped(
                    self,
                    mesh,
                    slot,
                    a,
                    b,
                    radius,
                    radius * 2.,
                    false,
                    Some(unit(sub(b, a))?),
                )?;
                &scoped
            } else {
                self
            };
            let ring: Vec<_> = (0..=sides)
                .map(|k| {
                    let angle = (k as f64 / sides as f64) * std::f64::consts::PI * 2.;
                    add(
                        scale(side, superspec_geometry::math::cos(angle)),
                        scale(other, superspec_geometry::math::sin(angle)),
                    )
                })
                .collect();
            for k in 0..sides {
                let n0 = ring[k];
                let n1 = ring[k + 1];
                pen.face(
                    mesh,
                    slot,
                    &[
                        add(a, scale(n0, radius)),
                        add(b, scale(n0, radius)),
                        add(b, scale(n1, radius)),
                        add(a, scale(n1, radius)),
                    ],
                    add(n0, n1),
                    tint,
                    repeat,
                    None,
                    Some(&[n0, n0, n1, n1]),
                )?;
            }
        }
        Ok(())
    }
    pub fn disc(
        &self,
        mesh: &mut Mesh<'_>,
        slot: &str,
        center: V3,
        radius: f64,
        hint: V3,
        sides: usize,
        tint: &Tint<'_>,
        repeat: f64,
    ) -> Result<(), String> {
        positive(radius)?;
        segments(sides)?;
        let (_, side, other) = basis(hint)?;
        let rim: Vec<_> = (0..sides)
            .map(|k| {
                let angle = (k as f64 / sides as f64) * std::f64::consts::PI * 2.;
                add(
                    center,
                    add(
                        scale(side, superspec_geometry::math::cos(angle) * radius),
                        scale(other, superspec_geometry::math::sin(angle) * radius),
                    ),
                )
            })
            .collect();
        self.face(mesh, slot, &rim, hint, tint, repeat, None, None)
    }
    pub fn drum(
        &self,
        mesh: &mut Mesh<'_>,
        slot: &str,
        center: V3,
        radius: f64,
        height: f64,
        sides: usize,
        tint: &Tint<'_>,
        repeat: f64,
    ) -> Result<(), String> {
        positive(height)?;
        let top = add(center, [0., 0., height]);
        self.tube(mesh, slot, &[center, top], radius, sides, tint, repeat)?;
        self.disc(mesh, slot, top, radius, [0., 0., 1.], sides, tint, repeat)
    }
    /// Smooth grid; columns move right, rows down from its front side. No cross-cell welding.
    pub fn patch(
        &self,
        mesh: &mut Mesh<'_>,
        slot: &str,
        grid: &[Vec<PatchVertex>],
        flipped: bool,
    ) -> Result<(), String> {
        crate::itemspec::primitives::patch_admitted(
            mesh,
            self.matrix,
            &self.part,
            slot,
            grid,
            flipped,
        )
    }
}

fn positive(v: f64) -> Result<(), String> {
    if !v.is_finite() || v <= 0. {
        Err("Procedural dimensions must be positive and finite.".into())
    } else {
        Ok(())
    }
}
fn segments(n: usize) -> Result<(), String> {
    if !(3..=4096).contains(&n) {
        Err("Round section requires 3–4096 sides.".into())
    } else {
        Ok(())
    }
}
fn basis(direction: V3) -> Result<(V3, V3, V3), String> {
    let axis = unit(direction)?;
    let side = unit(cross(
        axis,
        if axis[2].abs() > 0.9 {
            [1., 0., 0.]
        } else {
            [0., 0., 1.]
        },
    ))?;
    Ok((axis, side, cross(axis, side)))
}
