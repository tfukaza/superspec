//! Legacy continuous floor weathering, preserving world-space texture phase and physical triangles.
use super::{mesh::Vertex, noise::value_noise3};
use superspec_geometry::math::*;

#[derive(Clone)]
pub struct Finish {
    pub grime: f64,
    pub wear: f64,
    pub phase: f64,
    pub repeat: f64,
}

impl Finish {
    pub fn color(&self, p: V3) -> V3 {
        let noise = ((value_noise3(
            p[0] * 0.4,
            p[1] * 0.4,
            p[2] * 0.4,
            (self.phase * 1e5 + 0.5).floor(),
        ) - 0.5)
            * 1.8)
            .clamp(-0.75, 0.75);
        let deposit = self.grime * (0.12 + (noise + 0.75) * 0.08);
        let value = (0.88 + noise * 0.12 - deposit + self.wear * 0.06).clamp(0.18, 1.);
        [value * 0.98, value, value * 0.93]
    }
    /// Emits in source stack order. The shared budget counts attempted splits, not just final triangles.
    pub fn triangle(
        &self,
        points: [V3; 3],
        normal: V3,
        work: &mut usize,
        vertices: &mut Vec<Vertex>,
    ) -> Result<(), String> {
        if ![self.grime, self.wear, self.phase, self.repeat]
            .iter()
            .all(|v| v.is_finite())
            || !(0.0..=1.0).contains(&self.grime)
            || !(0.0..=1.0).contains(&self.wear)
            || !(0.0..100.0).contains(&self.phase)
            || points
                .iter()
                .flatten()
                .chain(&normal)
                .any(|v| !v.is_finite() || v.abs() > 1e8)
        {
            return Err("Invalid floor finish input.".into());
        }
        let axes = if normal[2].abs() > 0.7 {
            [0, 1]
        } else if normal[1].abs() > 0.7 {
            [0, 2]
        } else {
            [1, 2]
        };
        let (cosine, sine) = match (self.phase.floor() as i64).rem_euclid(4) {
            0 => (1., 0.),
            1 => (0., 1.),
            2 => (-1., 0.),
            _ => (0., -1.),
        };
        let mut stack = vec![points];
        while let Some(face) = stack.pop() {
            if *work >= 2_000_000 || vertices.len().saturating_add(3) > 6_000_000 {
                return Err("Floor dressing work budget exceeded.".into());
            }
            *work += 1;
            let mut edge = 0;
            let mut longest = 0.;
            for i in 0..3 {
                let size = length(sub(face[i], face[(i + 1) % 3]));
                if size > longest {
                    longest = size;
                    edge = i;
                }
            }
            if longest > 0.65 {
                let a = face[edge];
                let b = face[(edge + 1) % 3];
                let c = face[(edge + 2) % 3];
                let middle = scale(add(a, b), 0.5);
                stack.extend([[a, middle, c], [middle, b, c]]);
                continue;
            }
            let emitted = face.map(|p| {
                let u = p[axes[0]] * self.repeat;
                let v = p[axes[1]] * self.repeat;
                Vertex {
                    position: p,
                    normal,
                    uv: [
                        u * cosine - v * sine + self.phase,
                        u * sine + v * cosine + self.phase * 0.37,
                    ],
                    color: self.color(p),
                }
            });
            if emitted.iter().any(|v| {
                v.position
                    .iter()
                    .chain(&v.normal)
                    .chain(&v.uv)
                    .chain(&v.color)
                    .any(|v| !v.is_finite() || !(*v as f32).is_finite())
            }) {
                return Err("Floor vertex attributes exceed finite f32 range.".into());
            }
            vertices.extend(emitted);
        }
        Ok(())
    }
}
