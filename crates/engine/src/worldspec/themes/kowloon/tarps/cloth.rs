use super::{Pattern, smooth_normals};
use crate::worldspec::themes::kowloon::{
    fixture::Fixture,
    noise::{fractal_noise, smooth_step, value_noise},
    palette::{mix, srgb},
    pen::Tint,
};
use superspec_geometry::{
    V3,
    math::{hypot, length, sin, sub},
};
#[derive(Clone, Copy)]
pub(super) struct Vertex {
    pub point: V3,
    pub uv: [f64; 2],
    pub normal: V3,
}
fn interpolate(a: Vertex, b: Vertex, t: f64) -> Vertex {
    let n = std::array::from_fn(|i| a.normal[i] + (b.normal[i] - a.normal[i]) * t);
    let magnitude = length(n);
    Vertex {
        point: std::array::from_fn(|i| a.point[i] + (b.point[i] - a.point[i]) * t),
        uv: std::array::from_fn(|i| a.uv[i] + (b.uv[i] - a.uv[i]) * t),
        normal: if magnitude > 1e-10 {
            n.map(|v| v / magnitude)
        } else {
            a.normal
        },
    }
}
fn clip(vertices: &[Vertex], boundary: f64, greater: bool) -> Vec<Vertex> {
    let mut result: Vec<Vertex> = vec![];
    let inside = |v: Vertex| {
        if greater {
            v.uv[0] >= boundary
        } else {
            v.uv[0] <= boundary
        }
    };
    let mut append = |v: Vertex| {
        if result
            .last()
            .is_none_or(|last| hypot(v.uv[0] - last.uv[0], v.uv[1] - last.uv[1]) > 1e-10)
        {
            result.push(v);
        }
    };
    for i in 0..vertices.len() {
        let a = vertices[i];
        let b = vertices[(i + 1) % vertices.len()];
        if inside(a) {
            append(a);
        }
        if inside(a) != inside(b) {
            append(interpolate(
                a,
                b,
                (boundary - a.uv[0]) / (b.uv[0] - a.uv[0]),
            ));
        }
    }
    if result.len() > 1 {
        let a = result[0].uv;
        let b = result.last().unwrap().uv;
        if hypot(a[0] - b[0], a[1] - b[1]) <= 1e-10 {
            result.pop();
        }
    }
    result
}
pub(super) struct Field {
    pub width: f64,
    pub run: f64,
    pub base: V3,
    pub grime: f64,
    pub seed: f64,
    pub repeat: f64,
    pub offset: [f64; 2],
    pub module_offset: f64,
    pub pattern: Option<Pattern>,
}
impl Field {
    fn tint(&self, uv: [f64; 2], base: V3) -> V3 {
        let [u, v] = uv;
        let edge = u.min(self.width - u).min(v).min(self.run - v).max(0.);
        let hem = (-edge / 0.085).exp();
        let streak = 0.45 + value_noise(u * 8., v * 1.3, self.seed) * 0.55;
        let fading = value_noise(u * 1.4, v * 1.7, self.seed + 1.) * 0.12
            + smooth_step(
                0.54,
                0.74,
                fractal_noise(u * 7., v * 11., self.seed + 3., 3),
            ) * 0.22;
        let tint = mix(base, srgb("#a99d88"), fading);
        let soil = self.grime
            * (hem * streak
                + smooth_step(0.58, 0.77, value_noise(u * 19., v * 2., self.seed + 4.)) * 0.25);
        std::array::from_fn(|i| tint[i] * (1. - soil * [0.42, 0.49, 0.55][i]))
    }
    fn polygon(
        &self,
        d: &mut Fixture<'_, '_>,
        vertices: &[Vertex],
        base: V3,
    ) -> Result<(), String> {
        let points: Vec<_> = vertices.iter().map(|v| v.point).collect();
        let colors: Vec<_> = vertices.iter().map(|v| self.tint(v.uv, base)).collect();
        let uv: Vec<_> = vertices
            .iter()
            .map(|v| std::array::from_fn(|i| v.uv[i] * self.repeat + self.offset[i]))
            .collect();
        let color = |point: V3| {
            points
                .iter()
                .position(|p| *p == point)
                .map(|i| colors[i])
                .ok_or_else(|| "Cloth color lookup lost its fitted vertex.".to_string())
        };
        for sign in [1., -1.] {
            let normals: Vec<_> = vertices
                .iter()
                .map(|v| v.normal.map(|n| n * sign))
                .collect();
            let hint = normals
                .iter()
                .fold([0.; 3], |a, n| std::array::from_fn(|i| a[i] + n[i]));
            d.pen.face(
                d.mesh,
                "tarp",
                &points,
                hint,
                &Tint::Field(&color),
                self.repeat,
                Some(&uv),
                Some(&normals),
            )?;
        }
        Ok(())
    }
    pub fn triangle(&self, d: &mut Fixture<'_, '_>, vertices: &[Vertex]) -> Result<(), String> {
        let Some(pattern) = self.pattern else {
            return self.polygon(d, vertices, self.base);
        };
        let first = (vertices
            .iter()
            .map(|v| v.uv[0] + self.module_offset)
            .fold(f64::INFINITY, f64::min)
            / pattern.width)
            .floor() as i64;
        let last = (vertices
            .iter()
            .map(|v| v.uv[0] + self.module_offset)
            .fold(f64::NEG_INFINITY, f64::max)
            / pattern.width)
            .ceil() as i64
            - 1;
        if last - first > 4096 {
            return Err("Canvas stripe work exceeds bounded envelope.".into());
        }
        for stripe in first..=last {
            d.mesh.spend_work(1)?;
            let lower = stripe as f64 * pattern.width - self.module_offset;
            let upper = lower + pattern.width;
            let polygon = clip(&clip(vertices, lower, true), upper, false);
            if polygon.len() < 3 {
                continue;
            }
            let color = if stripe % 2 == 0 {
                self.base
            } else {
                mix(pattern.color, srgb("#a99d88"), 0.08)
            };
            self.polygon(d, &polygon, color)?;
        }
        Ok(())
    }
}
#[allow(clippy::too_many_arguments)]
pub(super) fn valance(
    d: &mut Fixture<'_, '_>,
    front: &[[usize; 2]],
    points: &[V3],
    uv: &[[f64; 2]],
    height: f64,
    field: &Field,
    edge: Option<V3>,
) -> Result<(), String> {
    let mut ids = vec![];
    for pair in front {
        for id in pair {
            if !ids.contains(id) {
                ids.push(*id);
            }
        }
    }
    if ids.is_empty() {
        return Ok(());
    }
    let columns = ids.len();
    let column = |id: usize| ids.iter().position(|i| *i == id).unwrap();
    let mut vertices: Vec<Vertex> = vec![];
    let scallop = field.pattern.map_or(0.24, |p| p.width);
    for (row_index, row) in [0., 0.5, 1.].into_iter().enumerate() {
        for id in &ids {
            let top = points[*id];
            let rest = uv[*id];
            let u = rest[0] + field.module_offset;
            let shortening =
                0.028_f64.min(height * 0.18) * sin(std::f64::consts::PI * u / scallop).powi(2);
            let bend = 0.02_f64.min(height * 0.08)
                * (0.4 + value_noise(u * 2., 0., field.seed + 211.) * 0.6);
            let point = if row_index == 0 {
                top
            } else {
                [
                    top[0],
                    top[1] + bend * sin(row * std::f64::consts::PI * 0.7),
                    top[2] - (height - shortening) * row,
                ]
            };
            let v = if row_index == 0 {
                rest[1]
            } else {
                let previous = vertices[(row_index - 1) * columns + column(*id)];
                previous.uv[1] + length(sub(point, previous.point))
            };
            vertices.push(Vertex {
                point,
                uv: [rest[0], v],
                normal: [0.; 3],
            });
        }
    }
    let mut indices = vec![];
    for row in 0..2 {
        for [a, b] in front {
            let a0 = row * columns + column(*a);
            let b0 = row * columns + column(*b);
            let a1 = a0 + columns;
            let b1 = b0 + columns;
            indices.extend([a0, b0, b1, a0, b1, a1]);
        }
    }
    let normals = smooth_normals(
        &vertices.iter().map(|v| v.point).collect::<Vec<_>>(),
        &indices,
    );
    for (v, n) in vertices.iter_mut().zip(normals) {
        v.normal = n;
    }
    let field = Field {
        run: vertices
            .iter()
            .map(|v| v.uv[1])
            .fold(f64::NEG_INFINITY, f64::max),
        ..*field
    };
    for tri in indices.as_chunks::<3>().0 {
        field.triangle(d, &tri.iter().map(|i| vertices[*i]).collect::<Vec<_>>())?;
    }
    // The bound hem is a hairline: a reduced level leaves it out.
    let Some(edge) = edge else {
        return Ok(());
    };
    for [a, b] in front {
        d.tube(
            "tarp",
            &[
                vertices[columns * 2 + column(*a)].point,
                vertices[columns * 2 + column(*b)].point,
            ],
            0.0025,
            edge,
            4,
        )?;
    }
    Ok(())
}
