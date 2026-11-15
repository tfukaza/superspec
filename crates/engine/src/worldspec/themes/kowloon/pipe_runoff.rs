//! Periodic, world-oriented pipe corrosion. Transport only reads higher cells, preventing cycles.
use super::noise::{smooth_step, value_noise};
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;
use superspec_geometry::{
    V3,
    math::{cos, dot, length, sin, sub},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Ring {
    pub center: V3,
    pub radial: V3,
    pub across: V3,
    pub radius: f64,
    pub distance: f64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Recipe {
    pub rings: Vec<Ring>,
    pub circumference: f64,
    pub seams: Vec<f64>,
    pub seed: f64,
}
impl Recipe {
    pub fn working_bytes(&self) -> Result<usize, String> {
        if !(2..=4096).contains(&self.rings.len()) || self.seams.len() > 4096 {
            return Err("Pipe runoff recipe exceeds its input budget.".into());
        }
        let surface = Surface::new(self.rings.clone(), self.circumference)?;
        if !self.seed.is_finite()
            || self.seams.len() > 4096
            || self.seams.iter().any(|s| !s.is_finite())
        {
            return Err("Invalid pipe runoff recipe.".into());
        }
        if self.seams.is_empty() {
            return Ok(0);
        }
        let columns = (surface.circumference * 128.).ceil().clamp(96., 192.) as usize;
        let rows = (surface.length * 48.).ceil().clamp(8., 384.) as usize;
        let count = columns * (rows + 1);
        if count * self.seams.len() > 8_000_000 {
            return Err("Pipe runoff seam work budget exceeded.".into());
        }
        Ok(count * 64 + self.rings.len() * std::mem::size_of::<Ring>())
    }
    pub fn build(&self, cancelled: &dyn Fn() -> bool) -> Result<Field, String> {
        self.working_bytes()?;
        Field::new(
            &Surface::new(self.rings.clone(), self.circumference)?,
            &self.seams,
            self.seed,
            cancelled,
        )
    }
}
pub struct Surface {
    rings: Vec<Ring>,
    circumference: f64,
    length: f64,
}
impl Surface {
    pub fn new(rings: Vec<Ring>, circumference: f64) -> Result<Self, String> {
        if !(2..=4096).contains(&rings.len())
            || !circumference.is_finite()
            || !(0. ..=1e7).contains(&circumference)
            || circumference == 0.
            || rings[0].distance != 0.
        {
            return Err("Pipe runoff needs bounded rings and positive circumference.".into());
        }
        for (i, r) in rings.iter().enumerate() {
            if r.center
                .iter()
                .chain(&r.radial)
                .chain(&r.across)
                .chain([&r.radius, &r.distance])
                .any(|v| !v.is_finite() || v.abs() > 1e7)
                || r.radius <= 0.
                || (i > 0 && r.distance <= rings[i - 1].distance)
                || dot(r.radial, r.radial) < 1e-10
                || dot(r.across, r.across) < 1e-10
                || dot(r.radial, r.radial) * dot(r.across, r.across)
                    - dot(r.radial, r.across).powi(2)
                    < 1e-10
            {
                return Err(
                    "Pipe runoff needs finite nondegenerate rings with increasing distances."
                        .into(),
                );
            }
        }
        let length = rings.last().unwrap().distance;
        Ok(Self {
            rings,
            circumference,
            length,
        })
    }
    pub fn circumference(&self) -> f64 {
        self.circumference
    }
    pub fn length(&self) -> f64 {
        self.length
    }
    fn segment(&self, v: f64) -> (&Ring, &Ring) {
        let i = self.rings[1..self.rings.len() - 1].partition_point(|r| r.distance < v);
        (&self.rings[i], &self.rings[i + 1])
    }
    pub fn point(&self, u: f64, v: f64) -> Result<V3, String> {
        if !u.is_finite() || !v.is_finite() {
            return Err("Pipe coordinates must be finite.".into());
        }
        let (a, b) = self.segment(v);
        let t = (v - a.distance) / (b.distance - a.distance);
        let remaining = 1. - t;
        let angle = 2. * PI * u / self.circumference;
        let radius = a.radius * remaining + b.radius * t;
        let radial: V3 = std::array::from_fn(|i| a.radial[i] * remaining + b.radial[i] * t);
        let across: V3 = std::array::from_fn(|i| a.across[i] * remaining + b.across[i] * t);
        let cosine = cos(angle) / length(radial);
        let sine = sin(angle) / length(across);
        let point = std::array::from_fn(|i| {
            a.center[i] * remaining
                + b.center[i] * t
                + radius * (radial[i] * cosine + across[i] * sine)
        });
        if point.iter().any(|v| !v.is_finite()) {
            return Err("Pipe runoff has a nonfinite interpolated surface.".into());
        }
        Ok(point)
    }
    pub fn downhill(&self, u: f64, v: f64) -> Result<[f64; 2], String> {
        if !u.is_finite() || !v.is_finite() {
            return Err("Pipe coordinates must be finite.".into());
        }
        let (a, b) = self.segment(v);
        let span = b.distance - a.distance;
        let t = (v - a.distance) / span;
        let angle = 2. * PI * u / self.circumference;
        let cosine = cos(angle);
        let sine = sin(angle);
        let radius = a.radius * (1. - t) + b.radius * t;
        let radius_rate = (b.radius - a.radius) / span;
        let radial: V3 = std::array::from_fn(|i| a.radial[i] * (1. - t) + b.radial[i] * t);
        let across: V3 = std::array::from_fn(|i| a.across[i] * (1. - t) + b.across[i] * t);
        let radial_rate: V3 = std::array::from_fn(|i| (b.radial[i] - a.radial[i]) / span);
        let across_rate: V3 = std::array::from_fn(|i| (b.across[i] - a.across[i]) / span);
        let rl = length(radial);
        let al = length(across);
        let rs = dot(radial, radial_rate) / (rl * rl);
        let acs = dot(across, across_rate) / (al * al);
        let (mut aa, mut ab, mut bb, mut uz, mut vz) = (0., 0., 0., 0., 0.);
        for i in 0..3 {
            let r = radial[i] / rl;
            let q = across[i] / al;
            let dr = (radial_rate[i] - radial[i] * rs) / rl;
            let dq = (across_rate[i] - across[i] * acs) / al;
            let du = radius * 2. * PI / self.circumference * (-r * sine + q * cosine);
            let dv = (b.center[i] - a.center[i]) / span
                + radius_rate * (r * cosine + q * sine)
                + radius * (dr * cosine + dq * sine);
            aa += du * du;
            ab += du * dv;
            bb += dv * dv;
            if i == 2 {
                uz = du;
                vz = dv;
            }
        }
        let determinant = aa * bb - ab * ab;
        if !determinant.is_finite() {
            return Err("Pipe runoff has a nonfinite interpolated metric.".into());
        }
        if determinant < 1e-10 {
            return Ok([0., 0.]);
        }
        let x = (-uz * bb + vz * ab) / determinant;
        let y = (-vz * aa + uz * ab) / determinant;
        let speed = (aa * x * x + 2. * ab * x * y + bb * y * y).max(0.).sqrt();
        let down = if speed < 0.02 {
            [0., 0.]
        } else {
            [x / speed, y / speed]
        };
        if down.iter().any(|v| !v.is_finite()) {
            return Err("Pipe runoff has a nonfinite downhill projection.".into());
        }
        Ok(down)
    }
}

pub struct Field {
    circumference: f64,
    length: f64,
    columns: usize,
    rows: usize,
    values: Vec<f32>,
}
impl Field {
    /// Builds one bounded immutable field. Empty seams produce zero without allocating a grid.
    pub fn new(
        surface: &Surface,
        seams: &[f64],
        seed: f64,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, String> {
        if !seed.is_finite() || seams.len() > 4096 || seams.iter().any(|v| !v.is_finite()) {
            return Err("Pipe runoff requires finite, bounded seams and seed.".into());
        }
        if cancelled() {
            return Err("Pipe runoff cancelled.".into());
        }
        let columns = (surface.circumference * 128.).ceil().clamp(96., 192.) as usize;
        let rows = (surface.length * 48.).ceil().clamp(8., 384.) as usize;
        let count = columns * (rows + 1);
        let mut result = Self {
            circumference: surface.circumference,
            length: surface.length,
            columns,
            rows,
            values: vec![],
        };
        if seams.is_empty() {
            return Ok(result);
        }
        if count * seams.len() > 8_000_000 {
            return Err("Pipe runoff seam work budget exceeded.".into());
        }
        let du = surface.circumference / columns as f64;
        let dv = surface.length / rows as f64;
        let mut values = vec![0f32; count];
        let mut points = vec![[0.; 3]; count];
        let mut upstream = vec![[0.; 2]; count];
        let mut order: Vec<_> = (0..count).collect();
        for row in 0..=rows {
            if cancelled() {
                return Err("Pipe runoff cancelled.".into());
            }
            for column in 0..columns {
                let index = row * columns + column;
                let u = column as f64 * du;
                let v = row as f64 * dv;
                points[index] = surface.point(u, v)?;
                let down = surface.downhill(u, v)?;
                let rate = (down[0].abs() / du).max(down[1].abs() / dv);
                upstream[index] = if rate > 0. {
                    [
                        column as f64 - down[0] / du / rate,
                        row as f64 - down[1] / dv / rate,
                    ]
                } else {
                    [column as f64, row as f64]
                };
                let distance = seams
                    .iter()
                    .map(|s| (v - s).abs())
                    .fold(f64::INFINITY, f64::min);
                let angle = 2. * PI * column as f64 / columns as f64;
                let cosine = cos(angle);
                let sine = sin(angle);
                let variation = smooth_step(
                    0.38,
                    0.72,
                    value_noise(cosine * 7. + 7., sine * 7. + v * 2., seed),
                );
                let strength = 0.35
                    + 1.4
                        * smooth_step(
                            0.2,
                            0.8,
                            value_noise(cosine * 3. + 13., sine * 3. + v * 2., seed + 73.),
                        );
                values[index] = ((-distance / 0.012).exp()
                    * (0.04 + variation * strength * 0.96).min(1.))
                    as f32;
            }
        }
        // Stable sorting retains source-cell order on equal world heights, matching the source field.
        order.sort_by(|a, b| points[*b][2].partial_cmp(&points[*a][2]).unwrap());
        for (step, index) in order.into_iter().enumerate() {
            if step.is_multiple_of(128) && cancelled() {
                return Err("Pipe runoff cancelled.".into());
            }
            let [x, y] = upstream[index];
            if y < 0. || y > rows as f64 {
                continue;
            }
            let x0 = x.floor();
            let y0 = y.floor();
            let tx = x - x0;
            let ty = y - y0;
            let mut carried = 0.;
            let mut weight = 0.;
            for dy in 0..=1 {
                for dx in 0..=1 {
                    let contribution =
                        if dx == 1 { tx } else { 1. - tx } * if dy == 1 { ty } else { 1. - ty };
                    if contribution == 0. || y0 + dy as f64 > rows as f64 {
                        continue;
                    }
                    let column = ((x0 as isize + dx) % (columns as isize) + columns as isize)
                        % columns as isize;
                    let neighbor = (y0 as usize + dy) * columns + column as usize;
                    if points[neighbor][2] <= points[index][2] + 1e-8 {
                        continue;
                    }
                    let distance = length(sub(points[neighbor], points[index]));
                    carried += contribution * f64::from(values[neighbor]) * (-distance / 0.3).exp();
                    weight += contribution;
                }
            }
            if weight > 0. {
                values[index] = f64::from(values[index]).max(carried / weight) as f32;
            }
        }
        result.values = values;
        Ok(result)
    }
    pub fn sample(&self, u: f64, v: f64) -> Result<f64, String> {
        if !u.is_finite() || !v.is_finite() || !(u / self.circumference).is_finite() {
            return Err("Pipe runoff coordinates must be finite.".into());
        }
        if self.values.is_empty() {
            return Ok(0.);
        }
        let x = (((u / self.circumference) % 1. + 1.) % 1.) * self.columns as f64;
        let y = (v / self.length * self.rows as f64).clamp(0., self.rows as f64);
        let x0 = x.floor() as usize;
        let y0 = (y.floor() as usize).min(self.rows - 1);
        let tx = x - x0 as f64;
        let ty = y - y0 as f64;
        let left = y0 * self.columns + x0;
        let right = y0 * self.columns + (x0 + 1) % self.columns;
        Ok(
            (f64::from(self.values[left]) * (1. - tx) + f64::from(self.values[right]) * tx)
                * (1. - ty)
                + (f64::from(self.values[left + self.columns]) * (1. - tx)
                    + f64::from(self.values[right + self.columns]) * tx)
                    * ty,
        )
    }
}
