//! Immutable facade endpoints shared by ground plinth and upper-band miter queries.
use super::rect;
use crate::worldspec::themes::kowloon::{massing::Rect, pen::Pen};
use superspec_geometry::{
    Matrix, V3,
    math::{dot, length, sub},
};
#[derive(Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Zone {
    Ground,
    Body,
}
pub struct Face {
    pub pen: Pen,
    pub shift: f64,
    pub ground: Vec<Rect>,
    pub body: Vec<Rect>,
}
struct End {
    point: V3,
    rect: Rect,
}
struct Plane {
    matrix: Matrix,
    tangent: V3,
    normal: V3,
    ends: Vec<End>,
}
impl Plane {
    fn new(pen: Pen, rects: Vec<Rect>) -> Self {
        let origin = pen.matrix.point([0.; 3]);
        Self {
            matrix: pen.matrix,
            tangent: sub(pen.matrix.point([1., 0., 0.]), origin),
            normal: sub(pen.matrix.point([0., 1., 0.]), origin),
            ends: rects
                .into_iter()
                .flat_map(|rect| {
                    [rect.u0, rect.u1].map(|u| End {
                        point: pen.matrix.point([u, 0., 0.]),
                        rect,
                    })
                })
                .collect(),
        }
    }
}
pub struct Joins {
    faces: Vec<[Plane; 2]>,
}
impl Joins {
    /// Freezes drawing frames and exposed endpoints; caller clearance belongs to the same scene revision.
    pub fn new(faces: Vec<Face>) -> Result<Self, String> {
        if faces.len() > 256
            || faces
                .iter()
                .map(|f| f.ground.len() + f.body.len())
                .sum::<usize>()
                > 4096
            || faces.iter().any(|f| {
                !f.shift.is_finite()
                    || f.shift.abs() > 1000.
                    || f.ground.iter().chain(f.body.iter()).any(|r| !rect(r))
            })
        {
            return Err("Facade corner collection exceeds its finite geometry budget.".into());
        }
        let mut prepared = Vec::with_capacity(faces.len());
        for face in faces {
            let body = face.pen.moved([0., face.shift, 0.])?;
            prepared.push([
                Plane::new(face.pen, face.ground),
                Plane::new(body, face.body),
            ]);
        }
        Ok(Self { faces: prepared })
    }
    /// Returns the first matching exposed neighbor's lateral shift per meter of trim projection.
    pub fn miter(
        &self,
        index: usize,
        zone: Zone,
        u: f64,
        heights: [f64; 2],
        reach: &dyn Fn(usize, Zone, Rect, f64) -> Result<f64, String>,
    ) -> Result<Option<f64>, String> {
        if index >= self.faces.len()
            || [u, heights[0], heights[1]]
                .iter()
                .any(|v| !v.is_finite() || v.abs() > 10000.)
            || heights[1] <= heights[0]
        {
            return Err("Invalid facade corner query.".into());
        }
        let axis = match zone {
            Zone::Ground => 0,
            Zone::Body => 1,
        };
        let depth = if axis == 0 { 0.065 } else { 0.12 };
        let own = &self.faces[index][axis];
        let point = own.matrix.point([u, 0., 0.]);
        for (other_index, face) in self.faces.iter().enumerate() {
            if index == other_index {
                continue;
            }
            let other = &face[axis];
            for end in &other.ends {
                let r = end.rect;
                if r.v0 > heights[0] + 1e-6 || r.v1 < heights[1] - 1e-6 {
                    continue;
                }
                let room = reach(
                    other_index,
                    zone,
                    Rect {
                        v0: heights[0],
                        v1: heights[1],
                        ..r
                    },
                    depth,
                )?;
                if !room.is_finite() || room < 0. || room > depth + 1e-6 {
                    return Err("Invalid corner clearance result.".into());
                }
                if room < depth - 1e-6 || length(sub(point, end.point)) > 1e-5 {
                    continue;
                }
                let along = dot(other.normal, own.tangent);
                let across = dot(other.normal, own.normal);
                if along.abs() < 1e-5 {
                    continue;
                }
                let shift = (1. - across) / along;
                if shift.abs() <= 2.5 {
                    return Ok(Some(shift));
                }
            }
        }
        Ok(None)
    }
}
