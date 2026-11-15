//! Shared leaf-local paint fields. Wear changes pigment and normal relief, never geometry.
use super::{
    bake_jobs::{Job, Recipe as RasterRecipe},
    fixture::Fixture,
    metal::Binding,
    noise::{fractal_noise, lattice_hash, smooth_step, value_noise},
    pen::Tint,
    surface::{Layer, Sample},
    weathered,
};
use serde::{Deserialize, Serialize};
use superspec_geometry::V3;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Settings {
    pub bounds: [f64; 4],
    pub paint: V3,
    pub substrate: V3,
    pub seed: f64,
    pub wear: f64,
    pub grime: f64,
    pub handle: Option<[f64; 2]>,
    #[serde(default)]
    pub panels: Vec<[f64; 4]>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Recipe {
    pub finish: Settings,
    pub binding: Binding,
}
#[derive(Debug, Deserialize, Serialize)]
pub struct Appearance {
    pub tint: V3,
    pub roughness: f64,
    pub bump: f64,
}
pub struct Paint<'a> {
    recipe: &'a Recipe,
}
/// A complete recipe shared by several rails and inset faces in the same local chart.
pub struct Surface {
    pub recipe: Recipe,
    pub group: String,
    pub density: f64,
}
impl Surface {
    fn validate(&self, slot: &str) -> Result<(), String> {
        self.recipe.validate()?;
        if slot.is_empty()
            || self
                .group
                .len()
                .saturating_add(slot.len())
                .saturating_add(1)
                > 4096
        {
            return Err("Paint group exceeds identity budget.".into());
        }
        super::surface::Layout::new(weathered::raster_settings(
            self.recipe.finish.bounds,
            self.density,
        ))?;
        Ok(())
    }
}
/// Replaces inherited appearance on this face, preserving its modeled bevel and winding.
pub fn face(
    drawing: &mut Fixture<'_, '_>,
    slot: &str,
    points: &[V3],
    tint: V3,
    surface: Option<&Surface>,
) -> Result<(), String> {
    let surface = surface.filter(|_| !drawing.mesh.surveying());
    if points.len() < 3 || points.len() > 4096 || points.iter().flatten().any(|v| !coordinate(*v)) {
        return Err("Painted face requires bounded geometry.".into());
    }
    let Some(surface) = surface else {
        return drawing.pen.face(
            drawing.mesh,
            slot,
            points,
            [0., 1., 0.],
            &Tint::Color(tint),
            drawing.repeat(slot),
            None,
            None,
        );
    };
    surface.validate(slot)?;
    super::bake_jobs::face(
        drawing,
        slot,
        points,
        Job {
            recipe: RasterRecipe::Paint(surface.recipe.clone()),
            density: surface.density,
        },
        format!("{}/{slot}", surface.group),
    )
}
pub fn panel(
    drawing: &mut Fixture<'_, '_>,
    slot: &str,
    bounds: [f64; 4],
    depth: f64,
    tint: V3,
    surface: Option<&Surface>,
) -> Result<(), String> {
    face(
        drawing,
        slot,
        &[
            [bounds[0], depth, bounds[1]],
            [bounds[2], depth, bounds[1]],
            [bounds[2], depth, bounds[3]],
            [bounds[0], depth, bounds[3]],
        ],
        tint,
        surface,
    )
}
pub fn cuboid(
    drawing: &mut Fixture<'_, '_>,
    slot: &str,
    low: V3,
    high: V3,
    tint: V3,
    omit: &str,
    surface: Option<&Surface>,
) -> Result<(), String> {
    if surface.is_none() || omit.contains('f') {
        return drawing.cuboid(slot, low, high, tint, omit);
    }
    if !drawing.mesh.surveying() {
        surface.unwrap().validate(slot)?;
    }
    if low.iter().chain(&high).any(|v| !coordinate(*v)) || (0..3).any(|i| low[i] >= high[i]) {
        return Err("Painted box requires positive finite dimensions.".into());
    }
    drawing.cuboid(slot, low, high, tint, &format!("{omit}f"))?;
    panel(
        drawing,
        slot,
        [low[0], low[2], high[0], high[2]],
        high[1],
        tint,
        surface,
    )
}
fn coordinate(v: f64) -> bool {
    v.is_finite() && v.abs() <= 10000.
}
fn rectangle(r: &[f64; 4]) -> bool {
    r.iter().all(|v| coordinate(*v)) && r[2] > r[0] && r[3] > r[1]
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if !rectangle(&self.bounds)
            || self.panels.len() > 256
            || self.panels.iter().any(|r| !rectangle(r))
            || self.handle.is_some_and(|p| !p.into_iter().all(coordinate))
            || !self.seed.is_finite()
            || self.seed.abs() > 4294967296.
            || [self.wear, self.grime]
                .iter()
                .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
            || self
                .paint
                .iter()
                .chain(&self.substrate)
                .any(|v| !v.is_finite() || !(0. ..=4.).contains(v))
        {
            return Err(
                "Paint requires bounded finite colors, wear, contacts and rectangles.".into(),
            );
        }
        Ok(())
    }
    /// Call after admission. A bounded number of perimeter fields is sampled per raster texel.
    fn sample(&self, u: f64, v: f64) -> Result<Appearance, String> {
        // A one-texel chart still samples its two padding texels and derivative halo.
        // Authored coordinates stay within 10 km; sampling permits those bounded extensions.
        if [u, v].iter().any(|v| !v.is_finite() || v.abs() > 100000.) {
            return Err("Paint coordinates exceed local meter bounds.".into());
        }
        let foot = (-(v - self.bounds[1]).max(0.) / 0.22).exp();
        let hand = self.handle.map_or(0., |p| {
            (-(((u - p[0]) / 0.12).powi(2) + ((v - p[1]) / 0.22).powi(2))).exp()
        });
        let mut loss: f64 = 0.;
        for (index, &[left, bottom, right, top]) in std::iter::once(&self.bounds)
            .chain(&self.panels)
            .enumerate()
        {
            for (edge, (distance, along, within)) in [
                ((u - left).abs(), v, v >= bottom && v <= top),
                ((u - right).abs(), v, v >= bottom && v <= top),
                ((v - bottom).abs(), u, u >= left && u <= right),
                ((v - top).abs(), u, u >= left && u <= right),
            ]
            .into_iter()
            .enumerate()
            {
                if !within || distance > 0.045 {
                    continue;
                }
                let seed = self.seed + index as f64 * 197. + edge as f64 * 31.;
                let cluster = smooth_step(0.48, 0.79, value_noise(along * 6., 0., seed));
                let tooth = (along * 110.).floor();
                let fraction = along * 110. - tooth;
                let jagged = lattice_hash(tooth, 0., seed + 7.) * (1. - fraction)
                    + lattice_hash(tooth + 1., 0., seed + 7.) * fraction;
                let width = self.wear
                    * cluster
                    * (0.008 + foot * 0.055 + hand * 0.038)
                    * (0.3 + jagged * 0.9);
                let chip = if width > 0.0002 {
                    1. - smooth_step(width - 0.0006, width + 0.0006, distance)
                } else {
                    0.
                };
                loss = loss.max(chip);
            }
        }
        let cloud = fractal_noise(u * 2.3, v * 1.7, self.seed + 13., 3);
        let grain = value_noise(u * 130., v * 45., self.seed + 17.);
        let brush = value_noise(u * 170., v * 11., self.seed + 37.) - 0.5;
        let fade = self.wear * (0.06 + cloud * 0.3);
        let dirt = self.grime * (foot * (0.22 + cloud * 0.32) + hand * 0.16).min(0.65);
        let tint = std::array::from_fn(|axis| {
            let pigment = fractal_noise(
                u * 6.7 + v * 2.1,
                v * 6.7 - u * 2.1,
                self.seed + 101. + axis as f64 * 47.,
                4,
            ) - 0.5;
            let chalk = self.paint[axis]
                * (1. + (cloud - 0.5) * 0.85)
                * (1. + pigment * 0.28)
                * (1. + fade)
                + fade * 0.06;
            let bare = self.substrate[axis] * (0.75 + grain * 0.35);
            (chalk * (1. - loss) + bare * loss) * (1. - dirt) * (1. + brush * 0.035)
        });
        Ok(Appearance {
            tint,
            roughness: (0.45 + cloud * 0.26 + fade * 0.5 + loss * 0.28 + dirt * 0.22).min(1.),
            bump: (grain - 0.5) * 0.00008 + brush * 0.00008 - loss * 0.00012,
        })
    }
}
impl Recipe {
    pub fn validate(&self) -> Result<(), String> {
        self.finish.validate()?;
        if self.binding.material.is_empty()
            || self.binding.material.len() > 4096
            || !self.binding.repeat.is_finite()
            || self.binding.repeat <= 0.
            || self.binding.repeat > 10000.
        {
            return Err("Paint requires a bounded material binding.".into());
        }
        Ok(())
    }
}
impl<'a> Paint<'a> {
    pub fn new(recipe: &'a Recipe) -> Result<Self, String> {
        recipe.validate()?;
        Ok(Self { recipe })
    }
    pub fn appearance(&self, u: f64, v: f64) -> Result<Appearance, String> {
        self.recipe.finish.sample(u, v)
    }
    pub fn structure(&self) -> Sample {
        Sample {
            height: 0.,
            bump: Some(0.),
            roughness: Some(0.5),
            height_weight: None,
            layer_elevation_weight: None,
            layers: vec![Layer {
                material: self.recipe.binding.material.clone(),
                weight: 1.,
                uv: [0.; 2],
                tint: Some([1.; 3]),
                roughness: None,
                coatings: vec![],
            }],
        }
    }
    pub fn sample(&self, u: f64, v: f64) -> Result<Sample, String> {
        let value = self.appearance(u, v)?;
        let mut sample = self.structure();
        sample.bump = Some(value.bump);
        sample.roughness = Some(value.roughness);
        sample.layers[0].uv = [
            u * self.recipe.binding.repeat,
            v * self.recipe.binding.repeat,
        ];
        sample.layers[0].tint = Some(value.tint);
        Ok(sample)
    }
}
