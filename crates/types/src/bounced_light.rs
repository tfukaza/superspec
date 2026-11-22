//! Inputs to static diffuse probe baking, in right-handed Z-up meters.
use serde::Deserialize;

type V3 = [f64; 3];

#[derive(serde::Serialize, Deserialize)]
pub struct Reflector {
    pub vertices: [V3; 3],
    /// Diffuse reflectance in linear RGB.
    pub color: V3,
}

#[derive(serde::Serialize, Deserialize)]
pub struct SpotEmitter {
    pub position: V3,
    pub direction: V3,
    /// Linear RGB candela, including the color of the lamp.
    pub intensity: V3,
    pub range: f64,
    pub outer_cosine: f64,
    pub inner_cosine: f64,
}

#[derive(serde::Serialize, Deserialize)]
pub struct ProbeBake {
    pub origin: V3,
    pub dimensions: [usize; 3],
    pub spacing: f64,
    pub sun_direction: V3,
    pub sun_radiance: V3,
    #[serde(default)]
    pub spots: Vec<SpotEmitter>,
    pub surfaces: Vec<Reflector>,
}
