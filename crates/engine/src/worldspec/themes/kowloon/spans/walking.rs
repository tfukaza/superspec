//! Physical floor/tread height queries for hanging clearance, not a navigation solver.
use superspec_geometry::obstructions::{Bounds, Budget, Obstructions, Triangle};
use superspec_geometry::{V3, math::hypot};
pub struct Surface {
    pub points: Triangle,
    pub floor_or_stairs: bool,
    pub collidable: bool,
}
pub struct Walking {
    index: Obstructions,
}
impl Walking {
    pub fn new(surfaces: &[Surface], budget: &mut Budget<'_>) -> Result<Self, String> {
        if surfaces.len() > 1_048_576 {
            return Err("Walking surface query exceeds triangle allowance.".into());
        }
        let mut triangles = Vec::new();
        for surface in surfaces {
            budget.spend(1)?;
            if surface
                .points
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || v.abs() > 1e6)
            {
                return Err("Walking surfaces require bounded finite coordinates.".into());
            }
            if !surface.floor_or_stairs || !surface.collidable {
                continue;
            }
            let [a, b, c] = surface.points;
            let across = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            let steepness = hypot(
                (b[1] - a[1]) * (c[2] - a[2]) - (b[2] - a[2]) * (c[1] - a[1]),
                (b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2]),
            );
            if across > steepness {
                triangles.push(surface.points);
            }
        }
        Ok(Self {
            index: Obstructions::new(&triangles, budget)?,
        })
    }
    /// Highest eligible floor at most 5 cm above the point, with source barycentric tolerance.
    pub fn below(&self, point: V3, budget: &mut Budget<'_>) -> Result<Option<f64>, String> {
        budget.spend(0)?;
        if point.iter().any(|v| !v.is_finite() || v.abs() > 999_998.) {
            return Err("Walking height requires a bounded finite point.".into());
        }
        let cell = [(point[0] / 2.).floor(), (point[1] / 2.).floor()];
        let bounds = Bounds {
            low: [cell[0] * 2., cell[1] * 2., -1e6],
            high: [cell[0] * 2. + 2., cell[1] * 2. + 2., 1e6],
        };
        let mut highest: Option<f64> = None;
        for i in self.index.near(bounds, budget)? {
            budget.spend(1)?;
            let [a, b, c] = *self.index.triangle(i).unwrap();
            if (0..2).any(|axis| {
                (a[axis].min(b[axis]).min(c[axis]) / 2.).floor() > cell[axis]
                    || (a[axis].max(b[axis]).max(c[axis]) / 2.).floor() < cell[axis]
            }) {
                continue;
            }
            let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            let wb = ((point[0] - a[0]) * (c[1] - a[1]) - (point[1] - a[1]) * (c[0] - a[0])) / area;
            let wc = ((b[0] - a[0]) * (point[1] - a[1]) - (b[1] - a[1]) * (point[0] - a[0])) / area;
            if wb < -1e-9 || wc < -1e-9 || wb + wc > 1. + 1e-9 {
                continue;
            }
            let z = a[2] + wb * (b[2] - a[2]) + wc * (c[2] - a[2]);
            if z <= point[2] + 0.05 && highest.is_none_or(|h| z > h) {
                highest = Some(z);
            }
        }
        Ok(highest)
    }
    pub fn clears(
        &self,
        point: V3,
        clearance: f64,
        budget: &mut Budget<'_>,
    ) -> Result<bool, String> {
        if !clearance.is_finite() || !(0. ..=100.).contains(&clearance) {
            return Err("Walking clearance must be finite and bounded.".into());
        }
        Ok(self
            .below(point, budget)?
            .is_none_or(|z| point[2] - z >= clearance))
    }
}
