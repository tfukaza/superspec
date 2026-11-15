//! Seeded pours, access covers and localized damage, before surface tessellation.
use super::{
    context::{self, Wall},
    field,
    layout::{self, Layout, Pour},
    surface::Finish,
};
use crate::worldspec::themes::kowloon::{
    floor,
    noise::{lattice_hash, value_noise},
};
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;
use superspec_geometry::obstructions::Budget;
use superspec_geometry::{
    V3,
    math::{cos, hypot, sin},
};
use superspec_types::rng::SeededRng;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub patch_size: f64,
    pub material_size: f64,
    pub relief: f64,
    pub wetness: f64,
    pub litter: f64,
    pub repairs: f64,
    pub tile_share: f64,
    pub timber_share: f64,
    pub joined_pours: f64,
    pub asphalt: f64,
    pub manholes: f64,
    pub breakup: f64,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            patch_size: 1.9,
            material_size: 7.,
            relief: 0.028,
            wetness: 0.45,
            litter: 0.35,
            repairs: 0.6,
            tile_share: 0.1,
            timber_share: 0.025,
            joined_pours: 0.65,
            asphalt: 0.18,
            manholes: 0.35,
            breakup: 0.5,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if !self.patch_size.is_finite()
            || !(0.5..=4.).contains(&self.patch_size)
            || !self.material_size.is_finite()
            || !(3. ..=24.).contains(&self.material_size)
            || !self.relief.is_finite()
            || !(0. ..=0.04).contains(&self.relief)
            || [
                self.wetness,
                self.litter,
                self.repairs,
                self.tile_share,
                self.timber_share,
                self.joined_pours,
                self.asphalt,
                self.manholes,
                self.breakup,
            ]
            .into_iter()
            .any(|v| !v.is_finite() || !(0. ..=1.).contains(&v))
            || self.tile_share + self.timber_share > 1.
        {
            Err("Invalid pavement construction or finish settings.".into())
        } else {
            Ok(())
        }
    }
}
#[derive(Clone, Deserialize, Serialize)]
pub struct Damage {
    pub polygon: Vec<V3>,
    pub center: V3,
    pub radius: f64,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct Manhole {
    pub center: V3,
    pub outer: Vec<V3>,
    pub inner: Vec<V3>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Patch {
    pub id: String,
    pub polygon: Vec<V3>,
    pub center: V3,
    pub kind: Finish,
    pub tone: V3,
    pub phase: f64,
    pub wet: bool,
    pub angle: f64,
    pub cracks: Vec<Vec<V3>>,
    pub worn_edges: Vec<bool>,
    pub pour: Pour,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asphalt: Option<Vec<V3>>,
    pub damage: Vec<Damage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manhole: Option<Manhole>,
}
impl Patch {
    pub fn validate(&self) -> Result<(), String> {
        let polygons = std::iter::once(&self.polygon)
            .chain(&self.cracks)
            .chain(self.asphalt.iter())
            .chain(self.damage.iter().map(|d| &d.polygon));
        if self.id.len() > 4096
            || self.polygon.len() != 4
            || self.worn_edges.len() != 4
            || self.cracks.len() > 4096
            || self.damage.len() > 4096
            || polygons.clone().map(Vec::len).sum::<usize>() > 65_536
            || polygons
                .into_iter()
                .any(|p| p.len() < 2 || p.len() > 4096 || p.iter().any(|p| !field::valid_point(*p)))
            || !field::valid_point(self.center)
            || !self.phase.is_finite()
            || !(0. ..100.).contains(&self.phase)
            || !self.angle.is_finite()
            || self.angle.abs() > 1000.
            || self
                .tone
                .iter()
                .any(|v| !v.is_finite() || !(0. ..=4.).contains(v))
            || self.damage.iter().any(|d| {
                !field::valid_point(d.center)
                    || !d.radius.is_finite()
                    || !(0.001..=100.).contains(&d.radius)
            })
            || self.manhole.as_ref().is_some_and(|m| {
                !field::valid_point(m.center)
                    || !(3..=128).contains(&m.inner.len())
                    || m.inner.len() != m.outer.len()
                    || m.inner
                        .iter()
                        .chain(&m.outer)
                        .any(|p| !field::valid_point(*p))
                    || m.inner.iter().enumerate().any(|(i, a)| {
                        let b = m.inner[(i + 1) % m.inner.len()];
                        hypot(b[0] - a[0], b[1] - a[1]) <= 1e-12
                    })
            })
        {
            Err("Ground patch requires bounded geometry and finite finish parameters.".into())
        } else {
            Ok(())
        }
    }
}
pub struct Planner<'a> {
    pub layout: &'a Layout,
    pub settings: &'a Settings,
    pub seed: u32,
    pub walls: &'a [Wall],
}
impl Planner<'_> {
    fn corner(&self, x: usize, y: usize) -> V3 {
        [self.layout.columns[x], self.layout.rows[y], 0.]
    }
    fn zone(&self, x: isize, y: isize) -> Finish {
        if x < 0
            || y < 0
            || x as usize + 1 >= self.layout.columns.len()
            || y as usize + 1 >= self.layout.rows.len()
        {
            return Finish::Concrete;
        }
        let a = self.corner(x as usize, y as usize);
        let b = self.corner(x as usize + 1, y as usize + 1);
        let zx = self
            .layout
            .zones_x
            .iter()
            .rposition(|v| *v <= (a[0] + b[0]) / 2.)
            .map_or(-1., |i| i as f64);
        let zy = self
            .layout
            .zones_y
            .iter()
            .rposition(|v| *v <= (a[1] + b[1]) / 2.)
            .map_or(-1., |i| i as f64);
        let roll = lattice_hash(zx, zy, f64::from(self.seed) + 24.);
        if roll < self.settings.tile_share {
            Finish::Tile
        } else if roll < self.settings.tile_share + self.settings.timber_share {
            Finish::Timber
        } else {
            Finish::Concrete
        }
    }
    fn patch(&self, pour: Pour) -> Patch {
        let Pour { x, y, end_x, end_y } = pour;
        let mut rng = SeededRng::from_text(&format!("{}/{x}/{y}", self.seed));
        let polygon = vec![
            self.corner(x, y),
            self.corner(end_x, y),
            self.corner(end_x, end_y),
            self.corner(x, end_y),
        ];
        let mut center = [0.; 3];
        for p in &polygon {
            for i in 0..3 {
                center[i] += p[i] / 4.;
            }
        }
        let repair = rng.next_f64() < self.settings.repairs;
        let zone = self.zone(x as isize, y as isize);
        let kind = if zone == Finish::Concrete && repair {
            Finish::Repair
        } else {
            zone
        };
        let tone = rng.float(if repair { 0.84 } else { 0.89 }, 1.);
        let mut patch = Patch {
            id: format!("{x}/{y}"),
            polygon,
            center,
            kind,
            tone: [tone * rng.float(0.97, 1.), tone, tone * rng.float(0.94, 1.)],
            phase: rng.float(0., 100.),
            wet: self.settings.wetness > 0.,
            angle: rng.integer(0, 3) as f64 * PI / 2.,
            cracks: vec![],
            worn_edges: [
                [x as isize, y as isize - 1],
                [end_x as isize, y as isize],
                [x as isize, end_y as isize],
                [x as isize - 1, y as isize],
            ]
            .map(|[nx, ny]| self.zone(nx, ny) != Finish::Tile)
            .to_vec(),
            pour,
            asphalt: None,
            damage: vec![],
            manhole: None,
        };
        if zone == Finish::Concrete
            && lattice_hash(x as f64, y as f64, f64::from(self.seed) + 119.) < self.settings.asphalt
        {
            patch.asphalt = Some(
                patch
                    .polygon
                    .iter()
                    .map(|p| {
                        [
                            p[0] + (p[0] - center[0]).signum() * 0.30,
                            p[1] + (p[1] - center[1]).signum() * 0.30,
                            0.,
                        ]
                    })
                    .collect(),
            );
        }
        if matches!(kind, Finish::Repair | Finish::Concrete) {
            let edge = rng.integer(0, 3);
            let start: V3 = std::array::from_fn(|i| {
                patch.polygon[edge][i] * 0.6 + patch.polygon[(edge + 1) % 4][i] * 0.4
            });
            let size = self.settings.patch_size;
            let end = [
                center[0] + rng.float(-0.25, 0.25) * size,
                center[1] + rng.float(-0.25, 0.25) * size,
                0.,
            ];
            let crack: Vec<_> = (0..7)
                .map(|i| {
                    [
                        start[0]
                            + (end[0] - start[0]) * i as f64 / 6.
                            + if i > 0 {
                                rng.float(-0.06, 0.06) * size
                            } else {
                                0.
                            },
                        start[1]
                            + (end[1] - start[1]) * i as f64 / 6.
                            + if i > 0 {
                                rng.float(-0.06, 0.06) * size
                            } else {
                                0.
                            },
                        0.,
                    ]
                })
                .collect();
            if rng.next_f64() < 0.6 {
                patch.cracks.push(crack.clone());
            }
            if rng.next_f64() < 0.3 {
                patch.cracks.push(vec![
                    crack[3],
                    [
                        crack[3][0] + rng.float(-0.2, 0.2) * size,
                        crack[3][1] + rng.float(-0.2, 0.2) * size,
                        0.,
                    ],
                    end,
                ]);
            }
        }
        patch
    }
    fn manhole(
        &self,
        patch: &mut Patch,
        faces: &[&Vec<V3>],
        budget: &mut Budget<'_>,
    ) -> Result<(), String> {
        if self.settings.manholes == 0. {
            return Ok(());
        }
        let mut rng = SeededRng::from_text(&format!("{}/{}/manhole", self.seed, patch.id));
        let lo = patch.polygon[0];
        let hi = patch.polygon[2];
        let width = hi[0] - lo[0];
        let depth = hi[1] - lo[1];
        if width.min(depth) < 1.12
            || rng.next_f64() > 1. - (-width * depth * self.settings.manholes / 24.).exp()
        {
            return Ok(());
        }
        for _ in 0..5 {
            budget.spend(1)?;
            let center = [
                rng.float(lo[0] + 0.56, hi[0] - 0.56),
                rng.float(lo[1] + 0.56, hi[1] - 0.56),
                0.,
            ];
            let outer = disk(center, 0.44, 32);
            if !supported(&disk(center, 0.56, 32), faces, budget)? {
                continue;
            }
            budget.spend(self.walls.len())?;
            if self
                .walls
                .iter()
                .any(|w| field::edge_distance(center, w.seam[0], w.seam[1]) < 0.56)
            {
                continue;
            }
            patch.manhole = Some(Manhole {
                center,
                outer,
                inner: disk(center, 0.375, 32),
            });
            return Ok(());
        }
        Ok(())
    }
    fn damage(&self, patch: &mut Patch, budget: &mut Budget<'_>) -> Result<(), String> {
        if self.settings.breakup == 0.
            || patch.asphalt.is_some()
            || !matches!(patch.kind, Finish::Concrete | Finish::Repair)
        {
            return Ok(());
        }
        let Pour { x, y, end_x, end_y } = patch.pour;
        for cx in x..end_x {
            for cy in y..end_y {
                budget.spend(1)?;
                let mut rng = SeededRng::from_text(&format!("{}/{cx}/{cy}/breakup", self.seed));
                let lo = self.corner(cx, cy);
                let hi = self.corner(cx + 1, cy + 1);
                let mut best: Option<(V3, f64)> = None;
                for _ in 0..4 {
                    let point = [
                        rng.float(lo[0] + 0.06, hi[0] - 0.06),
                        rng.float(lo[1] + 0.06, hi[1] - 0.06),
                        0.,
                    ];
                    let exposure = context::exposure(point, self.walls, budget)?;
                    let wall = (1. - exposure.wall_distance / 0.9).max(0.);
                    let narrow = ((3. - exposure.corridor_width) / 2.).clamp(0., 1.);
                    let bias = wall * 0.45 + narrow * 0.5;
                    if best.is_none_or(|(_, before)| bias > before) {
                        best = Some((point, bias));
                    }
                }
                let (point, bias) = best.unwrap();
                if rng.next_f64() > self.settings.breakup * (0.04 + bias) {
                    continue;
                }
                let radius = rng.float(0.18, 0.32) + bias * 0.48;
                let aspect = rng.float(0.7, 0.9);
                patch.damage.push(Damage {
                    center: point,
                    radius,
                    polygon: [
                        [-1., -0.45],
                        [-0.65, -0.9],
                        [0.55, -0.8],
                        [1., -0.3],
                        [0.82, 0.72],
                        [-0.45, 0.9],
                        [-0.95, 0.35],
                    ]
                    .map(|[x, y]| [point[0] + x * radius, point[1] + y * radius * aspect, 0.])
                    .to_vec(),
                });
            }
        }
        Ok(())
    }
    /// Plans all pours, retaining even empty pours so neighboring asphalt overflow remains available.
    pub fn patches(
        &self,
        faces: &[Vec<V3>],
        budget: &mut Budget<'_>,
    ) -> Result<Vec<Patch>, String> {
        self.settings.validate()?;
        budget.spend(faces.iter().map(Vec::len).sum::<usize>() + self.walls.len())?;
        if faces.len() > 65_536
            || faces
                .iter()
                .any(|f| f.len() != 3 || f.iter().any(|p| !field::valid_point(*p)))
            || self.walls.len() > 20_000
        {
            return Err("Pavement planning requires bounded receiving triangles and walls.".into());
        }
        let mut patches =
            layout::pours(self.layout, self.seed, self.settings.joined_pours, budget)?
                .into_iter()
                .map(|p| self.patch(p))
                .collect::<Vec<_>>();
        for patch in &mut patches {
            let mut receiving = Vec::new();
            for face in faces {
                budget.spend(face.len() * patch.polygon.len())?;
                if !field::intersect(face, &patch.polygon).is_empty() {
                    receiving.push(face);
                }
            }
            if receiving.is_empty() {
                continue;
            }
            self.manhole(patch, &receiving, budget)?;
            self.damage(patch, budget)?;
        }
        Ok(patches)
    }
}
fn disk(center: V3, radius: f64, count: usize) -> Vec<V3> {
    (0..count)
        .map(|i| {
            let angle = i as f64 * PI * 2. / count as f64;
            [
                center[0] + cos(angle) * radius,
                center[1] + sin(angle) * radius,
                0.,
            ]
        })
        .collect()
}
pub(super) fn supported(
    polygon: &[V3],
    faces: &[&Vec<V3>],
    budget: &mut Budget<'_>,
) -> Result<bool, String> {
    let area = field::area(polygon).abs();
    let mut covered = 0.;
    for face in faces {
        budget.spend(polygon.len() * face.len())?;
        covered += field::area(&field::intersect(polygon, face)).abs();
    }
    Ok(area > 1e-8 && (area - covered).abs() < 1e-7)
}
/// Complete color/height program for a patch. Physical floor elevation remains independent.
pub struct Pigment<'a> {
    pub patch: &'a Patch,
    pub settings: &'a Settings,
    pub seed: u32,
    pub grime: f64,
    pub wear: f64,
    pub tile_size: f64,
}
impl Pigment<'_> {
    pub fn validate(&self) -> Result<(), String> {
        self.patch.validate()?;
        self.settings.validate()?;
        if [self.grime, self.wear]
            .into_iter()
            .any(|v| !v.is_finite() || !(0. ..=1.).contains(&v))
            || !self.tile_size.is_finite()
            || !(0.01..=10.).contains(&self.tile_size)
        {
            Err("Ground pigment needs bounded wear, grime and tile size.".into())
        } else {
            Ok(())
        }
    }
    pub fn height(&self, p: V3) -> f64 {
        let patch = self.patch;
        let seed = f64::from(self.seed);
        let edge = patch
            .polygon
            .iter()
            .enumerate()
            .map(|(i, &a)| field::edge_distance(p, a, patch.polygon[(i + 1) % 4]))
            .fold(f64::INFINITY, f64::min);
        let texture = value_noise(p[0] * 7.7, p[1] * 7.7, seed + 5.);
        let mut value = 0.06
            + value_noise(p[0] * 0.43, p[1] * 0.43, seed) * 0.5
            + value_noise(p[0] * 1.72, p[1] * 1.72, seed + 7.) * 0.15
            + texture * 0.045
            - (-edge / 0.012).exp() * 0.035;
        let fade = (edge / 0.08).min(1.);
        if patch.asphalt.is_some() {
            value += fade * (value_noise(p[0] * 13., p[1] * 13., seed + 121.) - 0.5) * 0.2;
        }
        for damage in &patch.damage {
            let influence = (1.
                - hypot(p[0] - damage.center[0], p[1] - damage.center[1]) / damage.radius)
                .max(0.);
            value +=
                influence * fade * (value_noise(p[0] * 21., p[1] * 21., seed + 124.) * 0.22 - 0.3);
        }
        value.clamp(0., 1.) * self.settings.relief
    }
    pub fn color(&self, p: V3, finish: Finish) -> V3 {
        let seed = f64::from(self.seed);
        let patch = self.patch;
        let settings = self.settings;
        let mut tone = 1.;
        if matches!(finish, Finish::Asphalt | Finish::Spall | Finish::Chip) {
            tone *= 0.92 + value_noise(p[0] * 2.8, p[1] * 2.8, seed + 126.) * 0.08;
        }
        if finish == Finish::Timber {
            tone *= 0.7 + 0.3 * value_noise(p[0] * 26., p[1] * 0.6, seed + 4.);
        }
        if finish == Finish::Tile {
            tone *= 0.8
                + lattice_hash(
                    (p[0] / self.tile_size).floor(),
                    (p[1] / self.tile_size).floor(),
                    seed,
                ) * 0.2;
        }
        if finish == Finish::Asphalt {
            return [tone; 3];
        }
        let noise = value_noise(p[0] * 2.3, p[1] * 2.3, seed + 3.);
        let water = settings.relief * (0.13 + settings.wetness * 0.16);
        let wet_edge = ((water + settings.relief * 0.15 - self.height(p))
            / (settings.relief * 0.2).max(0.001))
        .clamp(0., 1.);
        let damp = if patch.wet { 1. - wet_edge * 0.35 } else { 1. };
        let weather = floor::Finish {
            grime: self.grime,
            wear: self.wear,
            phase: patch.phase,
            repeat: 1.,
        }
        .color(p);
        std::array::from_fn(|i| weather[i] * patch.tone[i] * (0.85 + noise * 0.15) * damp * tone)
    }
}
