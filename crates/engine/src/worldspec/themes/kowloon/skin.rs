//! Panelled plaster/concrete skins. Intrinsic finish repeats; local deposits remain in face meters.
use super::{
    bake_jobs::{self, Job, Recipe},
    facade_material::{self, Facade, PERIOD},
    fixture::Fixture,
    massing::Rect,
    surface::Sample,
    wall::{self, Condition},
};
use serde::{Deserialize, Serialize};
use superspec_geometry::V3;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Deposits {
    pub finish: facade_material::Settings,
    pub condition: wall::Settings,
    pub bounds: [f64; 4],
    pub origin: [f64; 2],
}
impl Deposits {
    pub(super) fn owned_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.finish.coat.material.capacity()
            + self.finish.backing.material.capacity()
            + self.condition.ledges.capacity() * std::mem::size_of::<wall::Ledge>()
            + self.condition.openings.capacity() * std::mem::size_of::<wall::Opening>()
    }
}
pub struct Deposited<'a> {
    recipe: &'a Deposits,
    finish: Facade<'a>,
    condition: Condition,
}
impl<'a> Deposited<'a> {
    pub fn new(recipe: &'a Deposits) -> Result<Self, String> {
        rectangle(recipe.bounds)?;
        if recipe
            .origin
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 10000.)
            || recipe
                .condition
                .ledges
                .iter()
                .any(|l| !l.u0.is_finite() || !l.u1.is_finite())
            || (0..2).any(|axis| {
                recipe.bounds[axis] < recipe.origin[axis] - 1e-9
                    || recipe.bounds[axis + 2] > recipe.origin[axis] + PERIOD + 1e-9
            })
        {
            return Err(
                "Finite facade deposits require one canonical cell and finite exposure.".into(),
            );
        }
        Ok(Self {
            recipe,
            finish: Facade::new(&recipe.finish)?,
            condition: Condition::new(&recipe.condition)?,
        })
    }
    pub fn structure(&self) -> Sample {
        self.finish.structure()
    }
    pub fn sample(&self, u: f64, v: f64) -> Result<Sample, String> {
        let mut sample = self
            .finish
            .sample(u - self.recipe.origin[0], v - self.recipe.origin[1])?;
        let deposit = self.condition.deposit_tint(u, v)?;
        for layer in &mut sample.layers {
            let tint = layer.tint.get_or_insert([1.; 3]);
            for axis in 0..3 {
                tint[axis] *= deposit[axis];
            }
        }
        Ok(sample)
    }
}

/// Ordered rectangular color changes. The last matching wash wins; boundaries become mandatory marks.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Wash {
    pub bounds: [f64; 4],
    pub tint: V3,
    /// Facade households own their lower bay/story boundary; ordinary patches keep open edges.
    #[serde(default)]
    pub lower_inclusive: bool,
}
impl Wash {
    pub(super) fn contains(&self, u: f64, v: f64) -> bool {
        let [left, bottom, right, top] = self.bounds;
        (if self.lower_inclusive {
            u >= left && v >= bottom
        } else {
            u > left && v > bottom
        }) && u < right
            && v < top
    }
}
#[derive(Clone, Debug, Deserialize)]
pub struct Settings {
    pub rects: Vec<Rect>,
    pub openings: Vec<Rect>,
    pub u_marks: Vec<f64>,
    pub z_marks: Vec<f64>,
    pub finish: facade_material::Settings,
    pub condition: wall::Settings,
    #[serde(default)]
    pub washes: Vec<Wash>,
    pub tiles: Option<super::tiled_skin::Settings>,
}
fn rectangle(b: [f64; 4]) -> Result<(), String> {
    if b.iter().any(|v| !v.is_finite() || v.abs() > 10000.) || b[0] >= b[2] || b[1] >= b[3] {
        return Err("Skin bounds require positive finite rectangles within 10 km.".into());
    }
    Ok(())
}
fn bounds(r: &Rect) -> [f64; 4] {
    [r.u0, r.v0, r.u1, r.v1]
}
fn contains(b: [f64; 4], u: f64, v: f64) -> bool {
    u > b[0] && u < b[2] && v > b[1] && v < b[3]
}
fn cuts(lo: f64, hi: f64, marks: &[f64]) -> Vec<f64> {
    let mut marks = marks.to_vec();
    marks.sort_by(f64::total_cmp);
    let mut fixed = vec![lo];
    for mark in marks {
        if mark - fixed.last().unwrap() >= 0.001 && hi - mark >= 0.001 {
            fixed.push(mark);
        }
    }
    fixed.push(hi);
    let mut result = vec![lo];
    for pair in fixed.windows(2) {
        let span = pair[1] - pair[0];
        let pieces = (span / 0.9 - 1e-9).ceil().max(1.) as usize;
        for piece in 1..pieces {
            result.push(pair[0] + span * piece as f64 / pieces as f64);
        }
        result.push(pair[1]);
    }
    result
}
fn cells(lo: f64, hi: f64) -> Vec<f64> {
    let mut result = vec![lo];
    let mut cell = (lo / PERIOD).floor() + 1.;
    while cell * PERIOD < hi {
        result.push(cell * PERIOD);
        cell += 1.;
    }
    result.push(hi);
    result
}
/// Emits all planned panels only after bounded planning and appearance admission succeed.
/// Ceramic cladding uses a finite combined field; geometric height-map finishes remain a separate path.
pub fn draw(
    drawing: &mut Fixture<'_, '_>,
    slot: &str,
    settings: &Settings,
    group: &str,
) -> Result<(), String> {
    if settings.rects.len() > 256
        || settings.openings.len() > 256
        || settings.washes.len() > 256
        || settings.u_marks.len() > 4096
        || settings.z_marks.len() > 4096
        || slot.is_empty()
        || slot.len() > 4096
        || group.len() > 4096
        || settings
            .u_marks
            .iter()
            .chain(&settings.z_marks)
            .any(|v| !v.is_finite() || v.abs() > 10000.)
    {
        return Err("Skin plan exceeds its bounded geometry or identity allowance.".into());
    }
    for r in settings.rects.iter().chain(&settings.openings) {
        rectangle(bounds(r))?;
    }
    for wash in &settings.washes {
        rectangle(wash.bounds)?;
        if wash
            .tint
            .iter()
            .any(|v| !v.is_finite() || !(0. ..=4.).contains(v))
        {
            return Err("Invalid skin wash.".into());
        }
    }
    // Bound panel planning before allocating cuts, including adversarially large but finite rectangles.
    let work: f64 = settings
        .rects
        .iter()
        .map(|r| {
            ((r.u1 - r.u0) / 0.9
                + settings.u_marks.len() as f64
                + settings.washes.len() as f64 * 2.
                + 3.)
                * ((r.v1 - r.v0) / 0.9
                    + settings.z_marks.len() as f64
                    + settings.washes.len() as f64 * 2.
                    + 3.)
        })
        .sum();
    if work > 65536. {
        return Err("Skin panel planning exceeds work allowance.".into());
    }
    if settings.rects.is_empty() {
        return Ok(());
    }
    let all = settings.rects.iter().fold(
        [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ],
        |mut a, r| {
            let b = bounds(r);
            for i in 0..2 {
                a[i] = a[i].min(b[i]);
                a[i + 2] = a[i + 2].max(b[i + 2]);
            }
            a
        },
    );
    let mut u_marks = settings.u_marks.clone();
    let mut z_marks = settings.z_marks.clone();
    for wash in &settings.washes {
        u_marks.extend([wash.bounds[0], wash.bounds[2]]);
        z_marks.extend([wash.bounds[1], wash.bounds[3]]);
    }
    // The caller supplies every opening edge as a mark, as in the source model. Reject an incomplete program.
    for r in &settings.openings {
        for edge in [r.u0, r.u1] {
            if edge > all[0] && edge < all[2] && !u_marks.contains(&edge) {
                return Err("Skin opening edge is missing its U construction mark.".into());
            }
        }
        for edge in [r.v0, r.v1] {
            if edge > all[1] && edge < all[3] && !z_marks.contains(&edge) {
                return Err("Skin opening edge is missing its Z construction mark.".into());
            }
        }
    }
    let mut panels = Vec::new();
    for rect in &settings.rects {
        drawing.mesh.check_work(0)?;
        let us = if settings.tiles.is_some() {
            vec![rect.u0, rect.u1]
        } else {
            cells(rect.u0, rect.u1)
        };
        let zs = if settings.tiles.is_some() {
            vec![rect.v0, rect.v1]
        } else {
            cells(rect.v0, rect.v1)
        };
        for uc in us.windows(2) {
            for zc in zs.windows(2) {
                let us = cuts(uc[0], uc[1], &u_marks);
                let zs = cuts(zc[0], zc[1], &z_marks);
                for u in us.windows(2) {
                    for z in zs.windows(2) {
                        let mid = [(u[0] + u[1]) / 2., (z[0] + z[1]) / 2.];
                        if settings
                            .openings
                            .iter()
                            .any(|r| contains(bounds(r), mid[0], mid[1]))
                        {
                            continue;
                        }
                        panels.push(([u[0], z[0], u[1], z[1]], mid));
                        if panels.len() > 4096 {
                            return Err("Skin exceeds its 4096 panel allowance.".into());
                        }
                    }
                }
            }
        }
    }
    if !drawing.mesh.surveying() {
        Facade::new(&settings.finish)?;
        Condition::new(&settings.condition)?;
        if settings.finish.density <= 0.
            || settings.finish.wear != settings.condition.wear
            || settings
                .condition
                .ledges
                .iter()
                .any(|l| !l.u0.is_finite() || !l.u1.is_finite())
        {
            return Err(
                "Raster skin requires positive density and consistent finite wall exposure.".into(),
            );
        }
    }
    // Prepare immutable jobs before mutation. The collector owns identity/memory and aggregate pixel budgets.
    let mut planned = Vec::new();
    let mut planned_bytes = 0;
    for (bounds, mid) in panels {
        drawing.mesh.check_work(0)?;
        if drawing.mesh.surveying() {
            planned.push((bounds, None, None));
            continue;
        }
        let mut finish = settings.finish.clone();
        finish.variant %= 2;
        if let Some(wash) = settings
            .washes
            .iter()
            .rev()
            .find(|w| w.contains(mid[0], mid[1]))
            && settings.tiles.is_none()
        {
            finish.coat.tint = wash.tint;
        }
        let fallback = if settings.tiles.is_none() && settings.condition.grime == 0. {
            let origin = mid.map(|v| (v / PERIOD).floor() * PERIOD);
            Some(Deposits {
                finish: finish.clone(),
                condition: settings.condition.clone(),
                origin,
                bounds: [
                    all[0].max(origin[0]),
                    all[1].max(origin[1]),
                    all[2].min(origin[0] + PERIOD),
                    all[3].min(origin[1] + PERIOD),
                ],
            })
        } else {
            None
        };
        let job = if let Some(tiles) = &settings.tiles {
            Job {
                density: finish.density,
                recipe: Recipe::TiledSkin(Box::new(super::tiled_skin::Recipe {
                    bounds: all,
                    finish,
                    condition: settings.condition.clone(),
                    tiles: tiles.clone(),
                    washes: settings.washes.clone(),
                    openings: settings.openings.iter().map(super::skin::bounds).collect(),
                    mapping_seed: wall::finish_seed(&format!(
                        "{}/{}/{}",
                        drawing.pen.part,
                        drawing.mesh.source_material(&settings.finish.coat.material),
                        drawing
                            .mesh
                            .source_material(&settings.finish.backing.material)
                    )),
                })),
            }
        } else if settings.condition.grime == 0. {
            Job {
                density: finish.density,
                recipe: Recipe::Facade(finish),
            }
        } else {
            let origin = mid.map(|v| (v / PERIOD).floor() * PERIOD);
            Job {
                density: finish.density.min(128.),
                recipe: Recipe::FacadeDeposits(Deposits {
                    finish,
                    condition: settings.condition.clone(),
                    origin,
                    bounds: [
                        all[0].max(origin[0]),
                        all[1].max(origin[1]),
                        all[2].min(origin[0] + PERIOD),
                        all[3].min(origin[1] + PERIOD),
                    ],
                }),
            }
        };
        job.validate()?;
        planned_bytes += serde_json::to_string(&job.recipe)
            .map_err(|e| e.to_string())?
            .len()
            + settings.condition.ledges.len() * std::mem::size_of::<wall::Ledge>()
            + settings.condition.openings.len() * std::mem::size_of::<wall::Opening>()
            + std::mem::size_of::<Job>();
        if let Recipe::TiledSkin(recipe) = &job.recipe {
            planned_bytes += recipe.owned_bytes();
        }
        if planned_bytes > 4 * 1024 * 1024 {
            return Err("Skin recipe planning exceeds 4 MiB allowance.".into());
        }
        planned_bytes += fallback.as_ref().map_or(0, Deposits::owned_bytes);
        if planned_bytes > 4 * 1024 * 1024 {
            return Err("Skin fallback planning exceeds 4 MiB allowance.".into());
        }
        planned.push((bounds, Some(job), fallback));
    }
    for (b, job, fallback) in planned {
        drawing.mesh.check_work(0)?;
        if let Some(job) = job {
            bake_jobs::face_with_fallback(
                drawing,
                slot,
                &[
                    [b[0], 0., b[1]],
                    [b[2], 0., b[1]],
                    [b[2], 0., b[3]],
                    [b[0], 0., b[3]],
                ],
                job,
                group.into(),
                fallback,
            )?;
        } else {
            drawing.panel(slot, [b[0], b[2], b[1], b[3]], 0., [1.; 3])?;
        }
    }
    Ok(())
}
