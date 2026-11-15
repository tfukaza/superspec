//! Owned offline food meshes, preserving the measured hanging anchor and atlas variants.
use super::super::{fixture::Fixture, pen::Tint};
use serde::Deserialize;
use std::sync::OnceLock;
use superspec_geometry::{
    V3,
    math::{add, unit},
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
            points
                .iter()
                .position(|v| *v == p)
                .map(|i| std::array::from_fn(|axis| face[i][axis + 8] * pigment[axis]))
                .ok_or_else(|| "Missing food corner tint.".into())
        };
        let tint = if textured {
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
    hanging_bird(d, hook, scale, variant, textured, [1.; 3], [1.; 3])
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
        [0.94, 0.75, 0.56],
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
    )
}
pub fn sliced_meat(
    d: &mut Fixture<'_, '_>,
    at: V3,
    envelope: V3,
    textured: bool,
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
        d, asset, at, scale, [0.; 3], [0.; 2], textured, [1.; 3], [1.; 3],
    )
}
