//! Owned offline food meshes, preserving the measured hanging anchor and atlas variants.
use super::super::{fixture::Fixture, pen::Tint};
use serde::Deserialize;
use std::sync::OnceLock;
use superspec_geometry::{
    V3,
    math::{add, sin, unit},
};
#[derive(Deserialize)]
struct Food {
    size: V3,
    #[serde(default)]
    anchor: V3,
    faces: Vec<Vec<[f64; 11]>>,
}
#[derive(Deserialize)]
struct Catalog {
    chicken: Food,
    slices: Food,
}

#[derive(Clone, Copy)]
enum Finish {
    Original,
    RoastGlaze { variant: usize },
    CutMeat,
}

impl Finish {
    fn pigment(self, vertex: &[f64; 11]) -> V3 {
        let [x, y, z, nx, ny, nz, ..] = *vertex;
        match self {
            Self::Original => [1.; 3],
            Self::RoastGlaze { variant } => {
                // Long patches follow the actual body and neck. Broad golden
                // strokes on raised skin give way to russet in the folds.
                let ribbons = sin(51. * x + 18. * y + 4.1 * z + variant as f64 * 1.9);
                let break_up = (0.5 + 0.5 * sin(28. * z + 6. * y)).powi(2);
                let lacquer = (0.35
                    + 0.24 * ny.abs()
                    + 0.32 * nz
                    + 0.87 * z
                    + ribbons.max(0.).powi(3) * break_up * 0.31)
                    .clamp(0., 1.);
                [
                    0.86 + 0.14 * lacquer,
                    0.77 + 0.23 * lacquer,
                    0.96 - 0.11 * lacquer,
                ]
            }
            Self::CutMeat => {
                let fibers = sin(x * 24. + y * 56. + z * 5.);
                let face = (0.45 + 0.48 * nz + 0.07 * nx + 0.055 * fibers).clamp(0., 1.);
                [0.90 + 0.10 * face, 0.86 + 0.14 * face, 0.88 + 0.12 * face]
            }
        }
    }
}
fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("assets/restaurant-food.json"))
            .expect("shipped restaurant food")
    })
}
fn draw(
    d: &mut Fixture<'_, '_>,
    asset: &Food,
    at: V3,
    scale: f64,
    pivot: V3,
    offset: [f64; 2],
    textured: bool,
    proportions: V3,
    pigment: V3,
    finish: Finish,
) -> Result<(), String> {
    if at.iter().any(|v| !v.is_finite() || v.abs() > 1000.)
        || !scale.is_finite()
        || !(0.0001..=100.).contains(&scale)
    {
        return Err(
            "Restaurant food requires a bounded finite position and positive scale.".into(),
        );
    }
    d.mesh.check_work(asset.faces.len() * 2)?;
    for face in &asset.faces {
        let points: Vec<V3> = face
            .iter()
            .map(|v| std::array::from_fn(|i| at[i] + (v[i] - pivot[i]) * scale * proportions[i]))
            .collect();
        let normals: Vec<V3> = face
            .iter()
            .map(|v| unit(std::array::from_fn(|i| v[i + 3] / proportions[i])))
            .collect::<Result<_, _>>()?;
        let hint = normals.iter().fold([0.; 3], |sum, n| add(sum, *n));
        let uv: Vec<_> = face
            .iter()
            .map(|v| [v[6] + offset[0], v[7] + offset[1]])
            .collect();
        let color = |p| {
            let i = points
                .iter()
                .position(|v| *v == p)
                .ok_or("Missing food corner tint.")?;
            let brush = finish.pigment(&face[i]);
            Ok(std::array::from_fn(|axis| {
                let baked = if textured { 1. } else { face[i][axis + 8] };
                baked * pigment[axis] * brush[axis]
            }))
        };
        let tint = if textured && matches!(finish, Finish::Original) {
            Tint::Color(pigment)
        } else {
            Tint::Field(&color)
        };
        let slot = if textured { "restaurant-food" } else { "paint" };
        d.pen.face(
            d.mesh,
            slot,
            &points,
            hint,
            &tint,
            d.repeat(slot),
            Some(&uv),
            Some(&normals),
        )?;
    }
    Ok(())
}
pub fn hanging_chicken(
    d: &mut Fixture<'_, '_>,
    hook: V3,
    scale: f64,
    variant: usize,
    textured: bool,
) -> Result<(), String> {
    hanging_bird(
        d,
        hook,
        scale,
        variant,
        textured,
        [1.; 3],
        [1.; 3],
        Finish::Original,
    )
}

const DUCK_PROPORTIONS: V3 = [0.85, 0.9, 1.22];

pub fn duck_size() -> V3 {
    std::array::from_fn(|axis| catalog().chicken.size[axis] * DUCK_PROPORTIONS[axis])
}

pub fn hanging_duck(
    d: &mut Fixture<'_, '_>,
    hook: V3,
    scale: f64,
    variant: usize,
    textured: bool,
) -> Result<(), String> {
    hanging_bird(
        d,
        hook,
        scale,
        variant,
        textured,
        DUCK_PROPORTIONS,
        [1., 0.95, 0.92],
        Finish::RoastGlaze { variant },
    )
}

fn hanging_bird(
    d: &mut Fixture<'_, '_>,
    hook: V3,
    scale: f64,
    variant: usize,
    textured: bool,
    proportions: V3,
    pigment: V3,
    finish: Finish,
) -> Result<(), String> {
    if variant > 2 {
        return Err("Chicken atlas variant must be 0–2.".into());
    }
    let asset = &catalog().chicken;
    draw(
        d,
        asset,
        hook,
        scale,
        asset.anchor,
        [(variant % 2) as f64 * 0.5, -((variant / 2) as f64) * 0.5],
        textured,
        proportions,
        pigment,
        finish,
    )
}
pub fn sliced_meat(
    d: &mut Fixture<'_, '_>,
    at: V3,
    envelope: V3,
    textured: bool,
) -> Result<(), String> {
    sliced(d, at, envelope, textured, Finish::Original)
}

pub(super) fn sliced_roast(
    d: &mut Fixture<'_, '_>,
    at: V3,
    envelope: V3,
    textured: bool,
) -> Result<(), String> {
    sliced(d, at, envelope, textured, Finish::CutMeat)
}

fn sliced(
    d: &mut Fixture<'_, '_>,
    at: V3,
    envelope: V3,
    textured: bool,
    finish: Finish,
) -> Result<(), String> {
    if envelope
        .iter()
        .any(|v| !v.is_finite() || !(0.001..=100.).contains(v))
    {
        return Err("Meat tray envelope must be positive and bounded.".into());
    }
    let asset = &catalog().slices;
    let scale = (0..3)
        .map(|i| envelope[i] / asset.size[i])
        .fold(f64::INFINITY, f64::min);
    draw(
        d, asset, at, scale, [0.; 3], [0.; 2], textured, [1.; 3], [1.; 3], finish,
    )
}
