//! Local assembly contacts and folds, finalized before immutable appearance sampling.
use super::{mesh::rounded, metal::Fold};
use std::collections::BTreeMap;
use superspec_geometry::{
    V3,
    math::{cross, dot, length, scale, sub},
};

#[derive(Clone, Copy, Debug)]
pub struct Member {
    pub a: V3,
    pub b: V3,
    pub radius: f64,
}
struct Edge {
    normal: V3,
    direction: V3,
}
#[derive(Default)]
pub struct Assembly {
    members: Vec<Member>,
    edges: BTreeMap<[[i64; 3]; 2], Edge>,
    front: Option<V3>,
    folds: Vec<Fold>,
}
pub struct Exposure {
    pub members: Vec<Member>,
    pub folds: Vec<Fold>,
    contacts: Vec<Vec<V3>>,
}
impl Exposure {
    /// Indices are scoped to this assembly; out-of-range indices return no joint list.
    pub fn joints(&self, member: usize) -> Option<&[V3]> {
        self.contacts.get(member).map(Vec::as_slice)
    }
}
impl Assembly {
    /// Adds the actual member's inscribed centerline. Returns its stable assembly-local index.
    pub fn member(&mut self, a: V3, b: V3, radius: f64) -> Result<usize, String> {
        if a.iter().chain(&b).any(|v| !v.is_finite() || v.abs() > 1e7)
            || !radius.is_finite()
            || !(0. ..=1e4).contains(&radius)
        {
            return Err("Weathering member requires finite world coordinates and a bounded nonnegative radius.".into());
        }
        if self.members.len() >= 2048 {
            return Err("Weathering assembly exceeds 2048 members.".into());
        }
        let size = length(sub(b, a));
        let inset = radius.min(size / 2.);
        let fraction = if size != 0. { inset / size } else { 0. };
        let start = std::array::from_fn(|i| a[i] + (b[i] - a[i]) * fraction);
        let end = std::array::from_fn(|i| b[i] + (a[i] - b[i]) * fraction);
        let index = self.members.len();
        self.members.push(Member {
            a: start,
            b: end,
            radius: radius.min(inset.max(0.001)),
        });
        Ok(index)
    }
    /// Detects front-side shared-edge folds. UV coordinates identify the unfolded physical seam.
    pub fn face(&mut self, points: &[V3], hint: V3, uv: &[[f64; 2]]) -> Result<(), String> {
        if points.len() < 3
            || points.len() > 4096
            || points.len() != uv.len()
            || points
                .iter()
                .flatten()
                .chain(&hint)
                .any(|v| !v.is_finite() || v.abs() > 1e7)
            || uv.iter().flatten().any(|v| !v.is_finite() || v.abs() > 1e7)
        {
            return Err("Weathering faces require matching finite geometry and UVs.".into());
        }
        let size = length(hint);
        if size < 1e-9 {
            return Ok(());
        }
        if self.edges.len() + points.len() > 65536 || self.folds.len() + points.len() > 4096 {
            return Err("Weathering edge/fold budget exceeded.".into());
        }
        let normal = scale(hint, 1. / size);
        let front = self.front.unwrap_or(normal);
        if dot(normal, front) <= 0. {
            return Ok(());
        }
        let geometric = cross(sub(points[1], points[0]), sub(points[2], points[0]));
        let facing = dot(geometric, normal);
        let winding = if facing == 0. { 0. } else { facing.signum() };
        self.front = Some(front);
        for i in 0..points.len() {
            let next = (i + 1) % points.len();
            let mut key =
                [points[i], points[next]].map(|point| point.map(|v| rounded(v * 1e5, 1.) as i64));
            key.sort();
            let direction = scale(sub(points[next], points[i]), winding);
            let Some(previous) = self.edges.get(&key) else {
                self.edges.insert(key, Edge { normal, direction });
                continue;
            };
            if dot(previous.normal, normal) > 0.995 {
                continue;
            }
            let axis = if (uv[i][0] - uv[next][0]).abs() < 1e-5 {
                0
            } else if (uv[i][1] - uv[next][1]).abs() < 1e-5 {
                1
            } else {
                continue;
            };
            let at = (uv[i][axis] + uv[next][axis]) / 2.;
            if !self
                .folds
                .iter()
                .any(|fold| fold.axis == axis && (fold.at - at).abs() < 1e-5)
            {
                self.folds.push(Fold {
                    axis,
                    at,
                    concave: dot(cross(previous.normal, normal), previous.direction) < 0.,
                });
            }
        }
        Ok(())
    }
    /// Consumes mutable planning state; cancellation and contact limits never publish a partial field.
    pub fn finish(self, cancelled: &dyn Fn() -> bool) -> Result<Exposure, String> {
        let mut contacts = vec![vec![]; self.members.len()];
        let mut count = 0;
        if cancelled() {
            return Err("Weathering assembly cancelled.".into());
        }
        for i in 0..self.members.len() {
            if cancelled() {
                return Err("Weathering assembly cancelled.".into());
            }
            for j in i + 1..self.members.len() {
                if j.is_multiple_of(128) && cancelled() {
                    return Err("Weathering assembly cancelled.".into());
                }
                if let Some([a, b]) = contact(self.members[i], self.members[j]) {
                    if count >= 65536 {
                        return Err("Weathering assembly contact budget exceeded.".into());
                    }
                    contacts[i].push(a);
                    contacts[j].push(b);
                    count += 2;
                }
            }
        }
        Ok(Exposure {
            members: self.members,
            folds: self.folds,
            contacts,
        })
    }
}
fn contact(a: Member, b: Member) -> Option<[V3; 2]> {
    let u = sub(a.b, a.a);
    let v = sub(b.b, b.a);
    let w = sub(a.a, b.a);
    let aa = dot(u, u);
    let bb = dot(u, v);
    let cc = dot(v, v);
    let dd = dot(u, w);
    let ee = dot(v, w);
    let determinant = aa * cc - bb * bb;
    let mut s = if determinant > 1e-12 {
        ((bb * ee - cc * dd) / determinant).clamp(0., 1.)
    } else {
        0.
    };
    let mut t = if cc > 1e-12 {
        ((bb * s + ee) / cc).clamp(0., 1.)
    } else {
        0.
    };
    s = if aa > 1e-12 {
        ((bb * t - dd) / aa).clamp(0., 1.)
    } else {
        0.
    };
    t = if cc > 1e-12 {
        ((bb * s + ee) / cc).clamp(0., 1.)
    } else {
        0.
    };
    let pa = std::array::from_fn(|i| a.a[i] + u[i] * s);
    let pb = std::array::from_fn(|i| b.a[i] + v[i] * t);
    (length(sub(pa, pb)) <= a.radius + b.radius + 0.001).then_some([pa, pb])
}
