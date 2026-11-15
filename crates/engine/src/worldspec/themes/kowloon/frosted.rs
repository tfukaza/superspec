//! Opaque patterned glass. Fine grain lives in pigment and normal relief, not transparent geometry.
use super::{
    bake_jobs::{self, Job},
    fixture::Fixture,
    noise::value_noise,
    palette::shade,
    surface::{Layer, Sample},
};
use serde::{Deserialize, Serialize};
use superspec_geometry::V3;
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Recipe {
    pub bounds: [f64; 4],
    pub tint: V3,
    pub seed: f64,
    pub material: String,
}
pub struct Frosted<'a> {
    recipe: &'a Recipe,
}
impl<'a> Frosted<'a> {
    pub fn new(recipe: &'a Recipe) -> Result<Self, String> {
        let r = recipe;
        if r.bounds.iter().any(|v| !v.is_finite() || v.abs() > 10000.)
            || r.bounds[2] <= r.bounds[0]
            || r.bounds[3] <= r.bounds[1]
            || !r.seed.is_finite()
            || r.seed.abs() > 4294967296.
            || r.tint
                .iter()
                .any(|v| !v.is_finite() || !(0. ..=4.).contains(v))
            || r.material.is_empty()
            || r.material.len() > 4096
        {
            return Err(
                "Frosted glass requires finite bounded coordinates, tint and identity.".into(),
            );
        }
        Ok(Self { recipe })
    }
    pub fn structure(&self) -> Sample {
        Sample {
            height: 0.,
            bump: Some(0.),
            roughness: Some(0.68),
            height_weight: None,
            layer_elevation_weight: None,
            layers: vec![Layer {
                material: self.recipe.material.clone(),
                weight: 1.,
                uv: [0.; 2],
                tint: Some([1.; 3]),
                roughness: None,
                coatings: vec![],
            }],
        }
    }
    pub fn sample(&self, u: f64, v: f64) -> Result<Sample, String> {
        if [u, v].iter().any(|v| !v.is_finite() || v.abs() > 100000.) {
            return Err("Frosted glass sample exceeds its padded meter range.".into());
        }
        let grain = value_noise(u * 190., v * 190., self.recipe.seed);
        let mottling =
            0.86 + grain * 0.2 + value_noise(u * 7., v * 7., self.recipe.seed + 7.) * 0.1;
        let mut sample = self.structure();
        sample.bump = Some((grain - 0.5) * 0.00065);
        sample.layers[0].uv = [u, v];
        sample.layers[0].tint = Some(shade(self.recipe.tint, mottling));
        Ok(sample)
    }
}
pub fn panel(
    drawing: &mut Fixture<'_, '_>,
    bounds: [f64; 4],
    depth: f64,
    seed: f64,
    group: &str,
    textured: bool,
) -> Result<(), String> {
    let tint = super::palette::srgb("#a59e83");
    if !textured || drawing.mesh.surveying() {
        return drawing.panel(
            "pane",
            [bounds[0], bounds[2], bounds[1], bounds[3]],
            depth,
            tint,
        );
    }
    if group.len() > 4096 {
        return Err("Frosted glass group exceeds identity budget.".into());
    }
    let recipe = Recipe {
        bounds,
        tint,
        seed,
        material: "pane".into(),
    };
    bake_jobs::face(
        drawing,
        "pane",
        &[
            [bounds[0], depth, bounds[1]],
            [bounds[2], depth, bounds[1]],
            [bounds[2], depth, bounds[3]],
            [bounds[0], depth, bounds[3]],
        ],
        Job {
            recipe: bake_jobs::Recipe::Frosted(recipe),
            density: 256.,
        },
        group.to_owned(),
    )
}
