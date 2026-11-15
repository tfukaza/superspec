//! Shared height lattice and disjoint service-cover cuts, before material baking.
use super::{
    field,
    layout::Layout,
    plan::{Manhole, Patch, Pigment},
    surface::{Deposit, Finish},
};
use crate::worldspec::themes::kowloon::noise::value_noise;
use serde::Serialize;
use superspec_geometry::obstructions::Budget;
use superspec_geometry::{V3, math::hypot};
use superspec_types::rng::SeededRng;

#[derive(Serialize)]
pub struct Piece {
    pub points: Vec<V3>,
    pub finish: Finish,
}
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Terrain {
    pub patch_faces: Vec<Vec<V3>>,
    pub pieces: Vec<Piece>,
    pub paints: Vec<Deposit>,
}
struct Builder<'a, 'b, 'c> {
    patch: &'a Patch,
    pigment: &'a Pigment<'a>,
    sampling: f64,
    budget: &'b mut Budget<'c>,
    output: Terrain,
}
impl Builder<'_, '_, '_> {
    fn queue(&mut self, points: Vec<V3>, finish: Finish) -> Result<(), String> {
        self.budget.spend(points.len())?;
        if self.output.pieces.len() >= 1_000_000 {
            return Err("Ground pieces exceed allowance.".into());
        }
        self.output.pieces.push(Piece { points, finish });
        Ok(())
    }
    fn finish(&mut self, points: Vec<V3>, elevation: f64) -> Result<(), String> {
        if let Some(cover) = &self.patch.manhole {
            self.budget.spend(points.len() * cover.outer.len())?;
            let divided = field::split(&points, &cover.outer);
            for outside in divided.outside {
                self.queue(outside, self.patch.kind)?;
            }
            if !divided.inside.is_empty() {
                self.manhole(divided.inside, cover, elevation)?;
            }
        } else {
            self.queue(points, self.patch.kind)?;
        }
        Ok(())
    }
    fn manhole(&mut self, points: Vec<V3>, cover: &Manhole, elevation: f64) -> Result<(), String> {
        self.budget.spend(points.len() * cover.inner.len())?;
        let divided = field::split(&points, &cover.inner);
        let height = elevation + self.pigment.settings.relief;
        for (i, &a) in cover.inner.iter().enumerate() {
            let next = (i + 1) % cover.inner.len();
            let b = cover.inner[next];
            let dx = b[0] - a[0];
            let dy = b[1] - a[1];
            let length = hypot(dx, dy);
            let sector = [cover.outer[i], cover.outer[next], b, a];
            let shoulder = |p: V3| height + (dx * (p[1] - a[1]) - dy * (p[0] - a[0])) / length;
            for ring in &divided.outside {
                self.budget.spend(ring.len() * 4)?;
                let portion = field::intersect(ring, &sector);
                if portion.is_empty() {
                    continue;
                }
                let original = field::clip(&portion, |p| p[2] - shoulder(p));
                let raised = field::clip(&portion, |p| shoulder(p) - p[2]);
                if field::area(&original).abs() > 1e-10 {
                    self.queue(original, Finish::ManholeRim)?;
                }
                if field::area(&raised).abs() > 1e-10 {
                    self.queue(
                        raised
                            .into_iter()
                            .map(|p| [p[0], p[1], shoulder(p)])
                            .collect(),
                        Finish::ManholeRim,
                    )?;
                }
            }
        }
        if divided.inside.is_empty() {
            return Ok(());
        }
        let mut regions = vec![Piece {
            points: divided
                .inside
                .into_iter()
                .map(|p| [p[0], p[1], height])
                .collect(),
            finish: Finish::ManholeCover,
        }];
        let [x, y, _] = cover.center;
        let mut slots: Vec<_> = (0..7)
            .map(|i| field::rectangle(x - 0.36, y - 0.285 + i as f64 * 0.09, 0.72, 0.014, 0.))
            .collect();
        slots.push(field::rectangle(x - 0.24, y - 0.035, 0.055, 0.07, 0.));
        slots.push(field::rectangle(x + 0.185, y - 0.035, 0.055, 0.07, 0.));
        for slot in slots {
            let mut next = Vec::new();
            for region in regions {
                self.budget.spend(region.points.len() * slot.len())?;
                if region.finish == Finish::ManholeSlot {
                    next.push(region);
                    continue;
                }
                let cut = field::split(&region.points, &slot);
                next.extend(cut.outside.into_iter().map(|points| Piece {
                    points,
                    finish: region.finish,
                }));
                if !cut.inside.is_empty() {
                    next.push(Piece {
                        points: cut.inside,
                        finish: Finish::ManholeSlot,
                    });
                }
            }
            regions = next;
        }
        for region in regions {
            self.queue(region.points, region.finish)?;
        }
        Ok(())
    }
    fn detail(&mut self, polygon: &[V3], material: Finish) -> Result<(), String> {
        self.budget
            .spend(polygon.len() * self.patch.polygon.len())?;
        let clipped = field::intersect(polygon, &self.patch.polygon);
        if !clipped.is_empty() {
            if self.output.paints.len() >= 4096 {
                return Err("Ground deposit count exceeds allowance.".into());
            }
            self.output.paints.push(Deposit {
                polygon: clipped,
                material,
            });
        }
        Ok(())
    }
    fn joints(&mut self) -> Result<(), String> {
        for edge in 0..4 {
            let a = self.patch.polygon[edge];
            let b = self.patch.polygon[(edge + 1) % 4];
            let dx = b[0] - a[0];
            let dy = b[1] - a[1];
            let length = hypot(dx, dy);
            let count = (length / 0.35_f64.max(self.sampling * 1.9)).ceil() as usize;
            self.budget.spend(count)?;
            for n in 0..count {
                let p = std::array::from_fn(|i| a[i] + (b[i] - a[i]) * n as f64 / count as f64);
                let q =
                    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * (n + 1) as f64 / count as f64);
                let inset = |p: V3| {
                    let width = 0.004
                        + value_noise(p[0] * 9., p[1] * 9., f64::from(self.pigment.seed) + 68.)
                            * 0.011;
                    [p[0] - dy / length * width, p[1] + dx / length * width, 0.]
                };
                self.detail(&[p, q, inset(q), inset(p)], Finish::Joint)?;
            }
        }
        Ok(())
    }
    fn aging(&mut self, fine_marks: bool) -> Result<(), String> {
        for (i, damage) in self.patch.damage.iter().enumerate() {
            let mut rng = SeededRng::from_text(&format!(
                "{}/{}/chips/{i}",
                self.pigment.seed, self.patch.id
            ));
            self.detail(&damage.polygon, Finish::Spall)?;
            let cut_x = rng.float(-0.25, 0.25) * damage.radius + damage.center[0];
            let cut_y = rng.float(-0.25, 0.25) * damage.radius + damage.center[1];
            let xs = [
                damage.center[0] - damage.radius,
                cut_x,
                damage.center[0] + damage.radius,
            ];
            let ys = [
                damage.center[1] - damage.radius,
                cut_y,
                damage.center[1] + damage.radius,
            ];
            for x in 0..2 {
                for y in 0..2 {
                    if rng.next_f64() < 0.2 {
                        continue;
                    }
                    self.budget.spend(damage.polygon.len() * 4)?;
                    let fragment = field::intersect(
                        &damage.polygon,
                        &field::rectangle(xs[x], ys[y], xs[x + 1] - xs[x], ys[y + 1] - ys[y], 0.),
                    );
                    if fragment.is_empty() {
                        continue;
                    }
                    let mut center = [0.; 3];
                    for p in &fragment {
                        for i in 0..3 {
                            center[i] += p[i] / fragment.len() as f64;
                        }
                    }
                    let remaining = rng.float(0.72, 0.91);
                    let chip: Vec<_> = fragment
                        .iter()
                        .map(|p| {
                            std::array::from_fn(|i| center[i] + (p[i] - center[i]) * remaining)
                        })
                        .collect();
                    self.detail(&chip, Finish::Chip)?;
                }
            }
        }
        if fine_marks && matches!(self.patch.kind, Finish::Concrete | Finish::Repair) {
            self.joints()?;
        }
        if fine_marks {
            for crack in &self.patch.cracks {
                for edge in crack.windows(2) {
                    let [a, b] = [edge[0], edge[1]];
                    let length = hypot(b[0] - a[0], b[1] - a[1]);
                    if length <= 1e-12 {
                        return Err("Ground crack has no extent.".into());
                    }
                    let dx = (b[1] - a[1]) / length * 0.004;
                    let dy = (a[0] - b[0]) / length * 0.004;
                    self.detail(
                        &[
                            [a[0] + dx, a[1] + dy, 0.],
                            [a[0] - dx, a[1] - dy, 0.],
                            [b[0] - dx * 0.25, b[1] - dy * 0.25, 0.],
                            [b[0] + dx * 0.25, b[1] + dy * 0.25, 0.],
                        ],
                        Finish::Crack,
                    )?;
                }
            }
        }
        Ok(())
    }
}
pub fn build(
    layout: &Layout,
    patch: &Patch,
    pigment: &Pigment<'_>,
    receiving: &[Vec<V3>],
    sampling: f64,
    fine_marks: bool,
    budget: &mut Budget<'_>,
) -> Result<Terrain, String> {
    budget.spend(receiving.len())?;
    pigment.validate()?;
    if !sampling.is_finite()
        || !(0.02..=4.).contains(&sampling)
        || receiving.is_empty()
        || receiving.len() > 65_536
        || receiving
            .iter()
            .any(|p| p.len() != 3 || p.iter().any(|p| !field::valid_point(*p)))
        || [&layout.columns, &layout.rows].into_iter().any(|a| {
            a.len() < 2
                || a.len() > 256
                || a.iter().any(|v| !v.is_finite() || v.abs() > 1e6)
                || a.windows(2).any(|p| p[0] >= p[1])
        })
        || patch.pour.end_x >= layout.columns.len()
        || patch.pour.end_y >= layout.rows.len()
        || patch.pour.x >= patch.pour.end_x
        || patch.pour.y >= patch.pour.end_y
        || patch.polygon.len() != 4
        || !std::ptr::eq(patch, pigment.patch)
    {
        return Err(
            "Ground terrain requires a bounded admitted patch and receiving triangles.".into(),
        );
    }
    let mut builder = Builder {
        patch,
        pigment,
        sampling,
        budget,
        output: Terrain::default(),
    };
    let elevation = receiving[0][0][2];
    for x in patch.pour.x..patch.pour.end_x {
        for y in patch.pour.y..patch.pour.end_y {
            let lo = [layout.columns[x], layout.rows[y], 0.];
            let hi = [layout.columns[x + 1], layout.rows[y + 1], 0.];
            let nx = ((hi[0] - lo[0] - 1e-6) / sampling).ceil().max(1.) as usize;
            let ny = ((hi[1] - lo[1] - 1e-6) / sampling).ceil().max(1.) as usize;
            builder
                .budget
                .spend(nx.checked_mul(ny).ok_or("Ground lattice size overflow.")?)?;
            let point = |x: usize, y: usize| {
                let p = [
                    lo[0] + (hi[0] - lo[0]) * x as f64 / nx as f64,
                    lo[1] + (hi[1] - lo[1]) * y as f64 / ny as f64,
                    elevation,
                ];
                [p[0], p[1], elevation + pigment.height(p)]
            };
            for x in 0..nx {
                for y in 0..ny {
                    let a = point(x, y);
                    let b = point(x + 1, y);
                    let c = point(x + 1, y + 1);
                    let d = point(x, y + 1);
                    for triangle in [[a, b, c], [a, c, d]] {
                        for face in receiving {
                            builder.budget.spend(9)?;
                            let polygon = field::intersect(&triangle, face);
                            if polygon.is_empty() {
                                continue;
                            }
                            builder.output.patch_faces.push(polygon.clone());
                            builder.finish(polygon, elevation)?;
                        }
                    }
                }
            }
        }
    }
    builder.aging(fine_marks)?;
    Ok(builder.output)
}
