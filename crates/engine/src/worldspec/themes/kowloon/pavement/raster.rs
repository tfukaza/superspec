//! Owned ground pigment programs cross worker boundaries without closures or host I/O.
use super::{
    plan::{Patch, Pigment, Settings},
    surface,
};
use crate::worldspec::themes::kowloon::{
    surface::{Layout, Sample},
    weathered,
};
use serde::Serialize;
use superspec_geometry::obstructions::Budget;

#[derive(Clone, Serialize)]
pub struct Recipe {
    pub bounds: [f64; 4],
    pub surface: surface::Recipe,
    pub patch: Patch,
    pub settings: Settings,
    pub grime: f64,
    pub wear: f64,
    pub tile_size: f64,
    pub elevation: f64,
    pub density: f64,
}
impl Recipe {
    fn pigment(&self) -> Pigment<'_> {
        Pigment {
            patch: &self.patch,
            settings: &self.settings,
            seed: self.surface.seed,
            grime: self.grime,
            wear: self.wear,
            tile_size: self.tile_size,
        }
    }
    pub fn source(&self) -> &str {
        self.surface
            .sources
            .get(&self.surface.kind)
            .map_or("", |s| s.id.as_str())
    }
    pub(in crate::worldspec::themes::kowloon) fn owned_bytes(&self) -> usize {
        // Producer-owned snapshots use exact clones. JSON is also the immutable identity and
        // conservatively exceeds these small numeric/enum programs' retained representation.
        serde_json::to_vec(self).map_or(usize::MAX / 4, |v| {
            v.len() * 2 + std::mem::size_of::<Self>()
        })
    }
}
pub struct Field<'a> {
    recipe: &'a Recipe,
    surface: surface::Field<'a>,
}
impl<'a> Field<'a> {
    pub fn new(recipe: &'a Recipe, cancelled: &dyn Fn() -> bool) -> Result<Self, String> {
        recipe.pigment().validate()?;
        if !recipe.elevation.is_finite() || recipe.elevation.abs() > 1e6 {
            return Err("Pavement raster needs a finite floor elevation.".into());
        }
        Layout::new(weathered::raster_settings(recipe.bounds, recipe.density))?;
        let surface =
            surface::Field::new(&recipe.surface, &mut Budget::new(8_000_000, cancelled)?)?;
        Ok(Self { recipe, surface })
    }
    pub fn structure(&self) -> Result<Sample, String> {
        self.sample(self.recipe.bounds[0], self.recipe.bounds[1])
    }
    pub fn sample(&self, u: f64, v: f64) -> Result<Sample, String> {
        let pigment = self.recipe.pigment();
        self.surface
            .raster_sample(u, v, 1. / self.recipe.density, &|p, f| {
                pigment.color([p[0], p[1], self.recipe.elevation], f)
            })
    }
}
