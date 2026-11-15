//! Periodic intrinsic wall coating; exposure wraps before thresholding to keep chips opaque at seams.
use super::{
    metal::Binding,
    noise::{fractal_noise, smooth_step, value_noise},
    surface::{Layer, Sample},
    wall::finish_seed,
};
use serde::{Deserialize, Serialize};
use superspec_geometry::V3;
pub const PERIOD: f64 = 4.8;
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Coat {
    pub material: String,
    pub repeat: f64,
    pub tint: V3,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Settings {
    pub concrete: bool,
    pub coat: Coat,
    pub backing: Binding,
    pub wear: f64,
    pub density: f64,
    pub variant: u32,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finish {
    pub exposed: f64,
    pub bump: f64,
    pub tone: f64,
    pub backing_tone: f64,
}
pub struct Facade<'a> {
    settings: &'a Settings,
    seed: f64,
    grain: f64,
}
fn periodic(value: f64) -> f64 {
    ((value % PERIOD) + PERIOD) % PERIOD
}
fn canonical(value: f64) -> f64 {
    if (-1e-9..=PERIOD + 1e-9).contains(&value) {
        value.clamp(0., PERIOD)
    } else {
        periodic(value)
    }
}
/// Draws one facade receiver; equal full recipes can share the same seamless bake across faces.
pub fn panel(
    drawing: &mut super::fixture::Fixture<'_, '_>,
    slot: &str,
    bounds: [f64; 4],
    depth: f64,
    settings: &Settings,
    group: &str,
) -> Result<(), String> {
    if drawing.mesh.surveying() {
        return drawing.panel(
            slot,
            [bounds[0], bounds[2], bounds[1], bounds[3]],
            depth,
            [1.; 3],
        );
    }
    Facade::new(settings)?;
    if group.len() > 4096 {
        return Err("Facade group exceeds identity budget.".into());
    }
    let mut settings = settings.clone();
    settings.variant %= 2;
    let density = settings.density;
    super::bake_jobs::face(
        drawing,
        slot,
        &[
            [bounds[0], depth, bounds[1]],
            [bounds[2], depth, bounds[1]],
            [bounds[2], depth, bounds[3]],
            [bounds[0], depth, bounds[3]],
        ],
        super::bake_jobs::Job {
            recipe: super::bake_jobs::Recipe::Facade(settings),
            density,
        },
        group.into(),
    )
}
impl<'a> Facade<'a> {
    pub fn new(settings: &'a Settings) -> Result<Self, String> {
        let s = settings;
        if !s.wear.is_finite()
            || !(0. ..=1.).contains(&s.wear)
            || !s.density.is_finite()
            || !(0. ..=4096.).contains(&s.density)
            || [&s.coat.material, &s.backing.material]
                .iter()
                .any(|m| m.is_empty() || m.len() > 4096)
            || [s.coat.repeat, s.backing.repeat]
                .iter()
                .any(|v| !v.is_finite() || *v <= 0. || *v > 10000.)
            || s.coat
                .tint
                .iter()
                .any(|v| !v.is_finite() || !(0. ..=4.).contains(v))
        {
            return Err(
                "Facade appearance requires bounded identities, repeats, colors, wear and density."
                    .into(),
            );
        }
        Ok(Self {
            settings,
            seed: f64::from(finish_seed(&format!(
                "facade-finish/v1/{}",
                settings.variant % 2
            ))),
            grain: (settings.density * 0.35).min(63.),
        })
    }
    pub fn finish(&self, u: f64, v: f64) -> Result<Finish, String> {
        if [u, v].iter().any(|v| !v.is_finite() || v.abs() > 1e7) {
            return Err("Facade coordinates exceed the bounded meter range.".into());
        }
        let x = periodic(u);
        let y = periodic(v);
        let across = smooth_step(0., PERIOD, x);
        let up = smooth_step(0., PERIOD, y);
        let (mut erosion, mut edge, mut tooth, mut tone, mut backing) = (0., 0., 0., 0., 0.);
        let seed = self.seed;
        for row in 0..2 {
            for column in 0..2 {
                let weight = (if column > 0 { across } else { 1. - across })
                    * (if row > 0 { up } else { 1. - up });
                let a = x - column as f64 * PERIOD;
                let b = y - row as f64 * PERIOD;
                let warp_u = (fractal_noise(a * 1.7, b * 1.7, seed + 3., 3) - 0.5) * 0.24;
                let warp_v =
                    (fractal_noise(a * 1.7 + 13., b * 1.7 + 29., seed + 7., 3) - 0.5) * 0.24;
                erosion +=
                    weight * fractal_noise((a + warp_u) * 0.65, (b + warp_v) * 0.78, seed + 11., 4);
                if self.settings.density >= 32. {
                    edge += weight * (value_noise(a * 13., b * 13., seed + 31.) - 0.5) * 0.11;
                }
                if self.settings.density >= 76. {
                    edge += weight * (value_noise(a * 31., b * 31., seed + 41.) - 0.5) * 0.07;
                }
                if self.settings.density >= 180. {
                    edge += weight * (value_noise(a * 73., b * 73., seed + 47.) - 0.5) * 0.035;
                }
                backing += weight * (0.43 + fractal_noise(a * 2.8, b * 2.8, seed + 67., 4) * 0.75);
                tooth += weight
                    * (value_noise(a * self.grain, b * self.grain, seed + 53.) - 0.5)
                    * 0.0008;
                tone += weight * (0.86 + fractal_noise(a * 1.85, b * 1.4, seed + 59., 3) * 0.14);
            }
        }
        let threshold = 0.72 - self.settings.wear * 0.22;
        let exposed = if self.settings.concrete {
            1.
        } else if self.settings.wear <= 0. {
            0.
        } else {
            smooth_step(threshold - 0.004, threshold + 0.004, erosion + edge)
        };
        let rim = if self.settings.concrete || self.settings.wear <= 0. {
            0.
        } else {
            (-(erosion + edge - threshold).abs() / 0.016).exp()
        };
        Ok(Finish {
            exposed,
            bump: tooth + 0.004 * (1. - exposed),
            tone: tone * (1. - rim * 0.16),
            backing_tone: backing * (1. - rim * 0.24),
        })
    }
    pub fn structure(&self) -> Sample {
        let mut layers = Vec::with_capacity(8);
        for _ in 0..4 {
            for material in [
                &self.settings.coat.material,
                &self.settings.backing.material,
            ] {
                layers.push(Layer {
                    material: material.clone(),
                    weight: 0.125,
                    uv: [0.; 2],
                    tint: Some([1.; 3]),
                    roughness: None,
                    coatings: vec![],
                });
            }
        }
        Sample {
            height: 0.,
            layers,
            bump: Some(0.),
            roughness: Some(0.9),
            height_weight: None,
            layer_elevation_weight: None,
        }
    }
    pub fn sample(&self, u: f64, v: f64) -> Result<Sample, String> {
        let finish = self.finish(u, v)?;
        let x = canonical(u);
        let y = canonical(v);
        let across = smooth_step(0., PERIOD, x);
        let up = smooth_step(0., PERIOD, y);
        let s = self.settings;
        let mut sample = self.structure();
        for row in 0..2 {
            for column in 0..2 {
                let weight = (if column > 0 { across } else { 1. - across })
                    * (if row > 0 { up } else { 1. - up });
                let local = [x - column as f64 * PERIOD, y - row as f64 * PERIOD];
                let i = (row * 2 + column) * 2;
                sample.layers[i].weight = weight * (1. - finish.exposed);
                sample.layers[i].uv = local.map(|v| v * s.coat.repeat);
                sample.layers[i].tint = Some(s.coat.tint.map(|v| finish.tone * v));
                sample.layers[i + 1].weight = weight * finish.exposed;
                sample.layers[i + 1].uv = local.map(|v| v * s.backing.repeat);
                sample.layers[i + 1].tint = Some(
                    s.coat
                        .tint
                        .map(|v| finish.backing_tone * 0.84 * if s.concrete { v } else { 1. }),
                );
            }
        }
        sample.bump = Some(finish.bump);
        sample.roughness = Some(0.9 + finish.exposed * 0.07);
        Ok(sample)
    }
}
