//! Intrinsic layered metal appearance. Complete assembly exposure is borrowed immutably while sampling.
use super::{
    noise::{fractal_noise, smooth_step, value_noise},
    surface::{Coat, Layer, Sample, Uv},
};
use serde::{Deserialize, Serialize};
use superspec_geometry::{V3, math::hypot};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Frame,
    Cage,
    Pipe,
    Sheet,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Binding {
    pub material: String,
    pub repeat: f64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Fold {
    pub axis: usize,
    pub at: f64,
    pub concave: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Metric {
    pub scale: Uv,
    pub gravity: Uv,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub kind: Kind,
    pub bounds: [f64; 4],
    pub paint: V3,
    pub seed: f64,
    pub wear: f64,
    pub grime: f64,
    pub rust: f64,
    pub paint_coverage: Option<f64>,
    pub edge_axes: Option<[bool; 2]>,
    pub contacts: Option<Vec<Uv>>,
    pub metric: Option<Metric>,
    #[serde(default)]
    pub folds: Vec<Fold>,
    pub wrap_u: Option<f64>,
    pub substrate: Binding,
    pub coating: Binding,
}
type Runoff<'a> = dyn Fn(f64, f64) -> Result<f64, String> + 'a;
pub struct Metal<'a> {
    settings: &'a Settings,
    runoff: Option<&'a Runoff<'a>>,
}
#[derive(Clone, Copy)]
struct Patina {
    paint: V3,
    paint_coverage: f64,
    mottling: V3,
    substrate_roughness: f64,
    oxide: V3,
    oxide_coverage: f64,
    roughness: f64,
    bump: f64,
    runoff: f64,
}
impl Patina {
    fn blend(a: Self, b: Self, weight: f64) -> Self {
        let blend = |a: f64, b: f64| a * (1. - weight) + b * weight;
        let color = |a: V3, b: V3| std::array::from_fn(|i| blend(a[i], b[i]));
        Self {
            paint: color(a.paint, b.paint),
            paint_coverage: blend(a.paint_coverage, b.paint_coverage),
            mottling: color(a.mottling, b.mottling),
            substrate_roughness: blend(a.substrate_roughness, b.substrate_roughness),
            oxide: color(a.oxide, b.oxide),
            oxide_coverage: blend(a.oxide_coverage, b.oxide_coverage),
            roughness: blend(a.roughness, b.roughness),
            bump: blend(a.bump, b.bump),
            runoff: blend(a.runoff, b.runoff),
        }
    }
}
impl<'a> Metal<'a> {
    /// Construct after contact/fold planning; no callbacks run until an appearance sample is requested.
    pub fn new(settings: &'a Settings, runoff: Option<&'a Runoff<'a>>) -> Result<Self, String> {
        let s = settings;
        if [s.paint_coverage.unwrap_or(1.), s.wear, s.grime, s.rust]
            .iter()
            .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
            || [s.substrate.repeat, s.coating.repeat]
                .iter()
                .any(|v| !v.is_finite() || *v <= 0.)
            || s.paint.iter().any(|v| !v.is_finite() || *v < 0.)
            || s.bounds.iter().any(|v| !v.is_finite())
            || s.bounds[2] <= s.bounds[0]
            || s.bounds[3] <= s.bounds[1]
            || !s.seed.is_finite()
            || s.metric
                .as_ref()
                .is_some_and(|m| m.scale != [1., 1.] || m.gravity.iter().any(|v| !v.is_finite()))
            || s.wrap_u.is_some_and(|v| !v.is_finite() || v <= 0.)
        {
            return Err("Metal surface requires finite increasing metric bounds, nonnegative colors and normalized finish factors.".into());
        }
        if s.folds.len() > 4096
            || s.contacts
                .as_ref()
                .is_some_and(|v| v.len() > 4096 || v.iter().flatten().any(|n| !n.is_finite()))
            || s.folds.iter().any(|f| f.axis > 1 || !f.at.is_finite())
            || s.substrate.material.len() > 65536
            || s.coating.material.len() > 65536
        {
            return Err("Metal assembly exposure or identity budget exceeded.".into());
        }
        Ok(Self { settings, runoff })
    }
    /// Stable layer/coating identities without running noise, exposure or appearance callbacks.
    pub fn structure(&self) -> Sample {
        let s = self.settings;
        let mut coatings = vec![
            Coat {
                material: s.coating.material.clone(),
                uv: [0.; 2],
                opacity: 0.,
                tint: None,
                roughness: None
            };
            2
        ];
        if self.has_runoff() {
            coatings.push(coatings[0].clone());
        }
        Sample {
            height: 0.,
            layers: vec![Layer {
                material: s.substrate.material.clone(),
                weight: 1.,
                uv: [0.; 2],
                tint: None,
                roughness: None,
                coatings,
            }],
            bump: None,
            roughness: None,
            height_weight: None,
            layer_elevation_weight: None,
        }
    }
    fn has_runoff(&self) -> bool {
        self.runoff.is_some() || self.settings.contacts.is_some()
    }
    pub fn sample(&self, u: f64, v: f64) -> Result<Sample, String> {
        if !u.is_finite() || !v.is_finite() {
            return Err("Metal coordinates must be finite.".into());
        }
        let s = self.settings;
        let value = if let Some(period) = s.wrap_u {
            let x = ((u % period) + period) % period;
            let t = x / period;
            let weight = t * t * (3. - 2. * t);
            Patina::blend(self.patina(x, v)?, self.patina(x - period, v)?, weight)
        } else {
            self.patina(u, v)?
        };
        let uv = |repeat: f64| {
            [
                if let Some(period) = s.wrap_u {
                    u / period * super::mesh::rounded(period * repeat, 1.).max(1.)
                } else {
                    u * repeat
                },
                v * repeat,
            ]
        };
        let coating_uv = uv(s.coating.repeat);
        let substrate_uv = uv(s.substrate.repeat);
        if coating_uv
            .iter()
            .chain(&substrate_uv)
            .any(|v| !v.is_finite())
        {
            return Err("Metal source UVs exceed the finite repeat range.".into());
        }
        let mut coatings = vec![
            Coat {
                material: s.coating.material.clone(),
                uv: coating_uv,
                opacity: value.paint_coverage,
                tint: Some(value.paint),
                roughness: Some(value.roughness),
            },
            Coat {
                material: s.coating.material.clone(),
                uv: coating_uv,
                opacity: value.oxide_coverage,
                tint: Some(value.oxide),
                roughness: Some(0.95),
            },
        ];
        if self.has_runoff() {
            coatings.push(Coat {
                material: s.coating.material.clone(),
                uv: coating_uv,
                opacity: s.grime * 0.92 * smooth_step(0.035, 0.75, value.runoff),
                tint: Some([0.045, 0.021, 0.008]),
                roughness: Some(0.92),
            });
        }
        Ok(Sample {
            height: 0.,
            bump: Some(value.bump),
            roughness: Some(value.substrate_roughness),
            height_weight: None,
            layer_elevation_weight: None,
            layers: vec![Layer {
                material: s.substrate.material.clone(),
                weight: 1.,
                uv: substrate_uv,
                tint: Some(value.mottling),
                roughness: None,
                coatings,
            }],
        })
    }
    fn patina(&self, u: f64, v: f64) -> Result<Patina, String> {
        let s = self.settings;
        let [u0, v0, u1, v1] = s.bounds;
        let seed = s.seed;
        let wear = s.wear;
        let grime = s.grime;
        let x = u - u0;
        let y = v - v0;
        let width = u1 - u0;
        let height = v1 - v0;
        // Receivers are unwrapped in physical meters; seam axes are not exposed boundaries.
        let edges = s.edge_axes.unwrap_or([true, true]);
        let edge = if edges[0] {
            (-x.min(width - x).max(0.) / 0.012).exp()
        } else {
            0.
        };
        let joint = if edges[1] {
            (-y.min(height - y).max(0.) / 0.024).exp()
        } else {
            0.
        };
        let gravity = s.metric.as_ref().map_or([0., -1.], |m| m.gravity);
        let downhill = |edge: bool, gravity: f64, size: f64, at: f64| {
            if !edge {
                f64::INFINITY
            } else if gravity > 0.01 {
                (size - at) / gravity
            } else if gravity < -0.01 {
                -at / gravity
            } else {
                f64::INFINITY
            }
        };
        let bottom = (-downhill(edges[0], gravity[0], width, x)
            .min(downhill(edges[1], gravity[1], height, y))
            .max(0.)
            / 0.09)
            .exp();
        let warp = value_noise(x * 7., y * 7., seed + 7.) - 0.5;
        let field_u = x + warp * 0.035;
        let field_v = y - warp * 0.027;
        let islands = fractal_noise(field_u * 14., field_v * 14., seed + 11., 4);
        let fine = value_noise(x * 67. + y * 31., y * 67. - x * 31., seed + 19.);
        let brush = value_noise(x * 55., y * 6., seed + 29.);
        let mut crease = 0f64;
        let mut ridge = 0f64;
        for fold in &s.folds {
            let distance = ([u, v][fold.axis] - fold.at).abs();
            if fold.concave {
                crease = crease.max((-distance / 0.012).exp());
            } else {
                ridge = ridge.max((-distance / 0.009).exp());
            }
        }
        let mut bleed = if let Some(runoff) = self.runoff {
            runoff(u, v)?
        } else {
            0.
        };
        if !bleed.is_finite() || !(0. ..=1.).contains(&bleed) {
            return Err("Metal runoff must be finite coverage in [0,1].".into());
        }
        if let Some(contacts) = &s.contacts {
            let gravity_length = hypot(gravity[0], gravity[1]);
            let trail_noise = 0.45 + value_noise(x * 21., y * 3., seed + 61.) * 0.55;
            for point in contacts {
                let delta = [u - point[0], v - point[1]];
                let spot = (-((delta[0] / 0.025).powi(2) + (delta[1] / 0.035).powi(2))).exp();
                let mut trail = 0.;
                if gravity_length > 0.1 {
                    let down = (delta[0] * gravity[0] + delta[1] * gravity[1]) / gravity_length;
                    let side =
                        (delta[0] * gravity[1] - delta[1] * gravity[0]).abs() / gravity_length;
                    if (0. ..0.45).contains(&down) {
                        trail = (-down / 0.15).exp() * (-side / (0.012 + down * 0.03)).exp();
                    }
                }
                bleed = bleed.max(spot * 0.65).max(trail * trail_noise);
            }
        }
        let exposure =
            edge * 0.42 + joint * 0.33 + bottom * 0.2 + crease * 0.24 + ridge * 0.24 + bleed * 0.55;
        let threshold = 0.86 - wear * (0.12 + exposure * 0.5);
        let chips = if wear == 0. {
            0.
        } else {
            smooth_step(threshold, threshold + 0.075, islands * 0.8 + fine * 0.2)
        };
        let scratches = wear * edge * smooth_step(0.71, 0.86, brush) * 0.6;
        let coverage = s.paint_coverage.unwrap_or(1.);
        let loss = chips.max(scratches);
        let exposed = 1. - coverage * (1. - loss);
        let chalk = fractal_noise(field_u * 4., field_v * 4., seed + 43., 4);
        let drip = smooth_step(0.48, 0.73, value_noise(x * 23., y * 0.7, seed + 47.));
        let edge_deposit = (edge * 0.45 + joint * 0.35 + crease * 0.7 + bleed * 0.65).min(1.)
            * smooth_step(0.3, 0.7, islands);
        let dirt = grime
            * (bottom * (0.23 + drip * 0.3)
                + joint * 0.23
                + (1. - chalk) * 0.32
                + bleed * 0.7
                + crease * 0.3
                + edge_deposit * 0.28)
                .min(0.78);
        let fade = wear * (0.02 + chalk * 0.025);
        let pigment: V3 = std::array::from_fn(|axis| {
            (fractal_noise(
                field_u * 6.7 + field_v * 2.1,
                field_v * 6.7 - field_u * 2.1,
                seed + 101. + axis as f64 * 47.,
                4,
            ) - 0.5)
                * 0.025
        });
        let base_tone = 1. + (chalk - 0.5) * 0.055 + (brush - 0.5) * 0.012;
        let paint = std::array::from_fn(|axis| {
            let faded = s.paint[axis] * (1. + fade) + fade * 0.065;
            faded * base_tone * (1. + pigment[axis]) * (1. - dirt * 0.55)
        });
        let metal_cloud = fractal_noise(field_u * 21., field_v * 21., seed + 211., 3);
        let mottling = [
            0.78 + metal_cloud * 0.2,
            0.8 + metal_cloud * 0.18,
            0.82 + metal_cloud * 0.16,
        ];
        let rust_exposure =
            (edge * 0.6 + joint * 0.55 + crease * 0.65 + ridge * 0.45 + bleed * 0.8).min(1.);
        let fleck_threshold = 0.76 - rust_exposure * 0.16;
        let flecks = smooth_step(
            fleck_threshold,
            fleck_threshold + 0.12,
            value_noise(x * 83., y * 83., seed + 307.),
        ) * smooth_step(0.38, 0.62, islands);
        let oxide_coverage = (s.rust
            * (exposed * rust_exposure * 0.9
                + bleed * 0.4
                + crease * smooth_step(0.53, 0.72, islands) * 0.25
                + flecks * (0.7 + rust_exposure * 0.65)))
            .min(0.8);
        let oxide = [
            0.095 + fine * 0.035,
            0.054 + fine * 0.022,
            0.028 + fine * 0.015,
        ];
        let corrosion_relief =
            oxide_coverage * (0.00045 + (fine - 0.5) * 0.0009 + (metal_cloud - 0.5) * 0.0003);
        let result = Patina {
            paint,
            runoff: bleed,
            paint_coverage: coverage * (1. - loss),
            mottling,
            substrate_roughness: 0.64 + metal_cloud * 0.22,
            oxide,
            oxide_coverage,
            roughness: (0.42 + chalk * 0.32 + dirt * 0.4 + loss * 0.2).min(0.95),
            bump: (brush - 0.5) * 0.000045 - coverage * loss * 0.00022 + corrosion_relief,
        };
        if result
            .paint
            .iter()
            .chain(&result.mottling)
            .chain(&result.oxide)
            .chain([
                &result.bump,
                &result.roughness,
                &result.paint_coverage,
                &result.oxide_coverage,
            ])
            .any(|n| !n.is_finite())
        {
            return Err("Metal sampling exceeds the finite coordinate range.".into());
        }
        Ok(result)
    }
}
