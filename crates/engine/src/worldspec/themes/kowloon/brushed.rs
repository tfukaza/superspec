//! Repeating stainless material charts in fixture-local meters. Curved explicit UVs pass through.
use super::{
    mesh::Mesh,
    pen::{Pen, Tint},
    wall::finish_seed,
};
use superspec_geometry::{
    V3,
    math::{cross, dot, scale, sub},
};

#[derive(Clone)]
pub(super) struct Brushed {
    repeat: f64,
    phase: [f64; 2],
    material: &'static str,
}
impl Brushed {
    pub(super) fn new(pen: &Pen, repeat: f64, material: &'static str) -> Result<Self, String> {
        if !repeat.is_finite() || repeat <= 0. {
            return Err("Brushed metal repeat must be positive and finite.".into());
        }
        // The board and stainless finishes use the same exact Three column-major frame serialization.
        let seed = finish_seed(&super::timber::seed_key(pen, &[])?);
        Ok(Self {
            repeat,
            phase: [(seed & 65535) as f64 / 65536., (seed >> 16) as f64 / 65536.],
            material,
        })
    }
    pub(super) fn derived(&self, pen: &Pen) -> Result<Self, String> {
        Self::new(pen, self.repeat, self.material)
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn face(
        &self,
        pen: &Pen,
        mesh: &mut Mesh<'_>,
        slot: &str,
        points: &[V3],
        hint: V3,
        tint: &Tint<'_>,
        repeat: f64,
        uv: Option<&[[f64; 2]]>,
        normals: Option<&[V3]>,
    ) -> Result<(), String> {
        if slot != "metal" {
            return pen.face_raw(mesh, slot, points, hint, tint, repeat, uv, normals);
        }
        if uv.is_some() || mesh.surveying() || !(3..=4096).contains(&points.len()) {
            return pen.face_raw(mesh, self.material, points, hint, tint, repeat, uv, normals);
        }
        let mut normal = [0.; 3];
        for i in 1..points.len() - 1 {
            normal = cross(sub(points[i], points[0]), sub(points[i + 1], points[0]));
            if dot(normal, normal) > 1e-18 {
                break;
            }
        }
        if dot(normal, hint) < 0. {
            normal = scale(normal, -1.);
        }
        let normalize = |v| {
            let length = dot(v, v).sqrt();
            scale(v, 1. / if length == 0. { 1. } else { length })
        };
        normal = normalize(normal);
        let tangent = if normal[0].abs() < 0.9 {
            [1., 0., 0.]
        } else {
            [0., 1., 0.]
        };
        let tangent = normalize(sub(tangent, scale(normal, dot(tangent, normal))));
        let across = cross(normal, tangent);
        let coordinates: Vec<_> = points
            .iter()
            .map(|p| {
                [
                    dot(*p, tangent) * self.repeat + self.phase[0],
                    dot(*p, across) * self.repeat + self.phase[1],
                ]
            })
            .collect();
        pen.face_raw(
            mesh,
            self.material,
            points,
            hint,
            tint,
            repeat,
            Some(&coordinates),
            normals,
        )
    }
}
