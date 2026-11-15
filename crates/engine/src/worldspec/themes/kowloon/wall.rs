//! Intrinsic wall aging in face-local meters. Deposits never cut geometry or encode illumination.
use super::{
    noise::{fractal_noise, fractal_noise_range, smooth_step, value_noise, value_noise_range},
    surface::{Layer, Sample},
};
use serde::{Deserialize, Serialize};
use superspec_geometry::V3;

pub const PLASTER_DEPTH: f64 = 0.006;
pub const LEDGE_SPREAD: f64 = 0.15;
const LONGEST_RUN: f64 = 2.8;
const SOOT_REACH: f64 = 0.65;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Ledge {
    pub u0: f64,
    pub u1: f64,
    pub z: f64,
    pub flow: f64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Opening {
    pub u0: f64,
    pub u1: f64,
    pub z0: f64,
    pub z1: f64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Settings {
    pub base: Option<f64>,
    pub ledges: Vec<Ledge>,
    #[serde(default)]
    pub openings: Vec<Opening>,
    pub grime: f64,
    pub wear: f64,
    pub seed: f64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WallSample {
    pub tint: V3,
    pub loss: f64,
    pub tone: f64,
    pub damp: f64,
    pub runoff: f64,
}
#[derive(Deserialize)]
pub struct Coating {
    pub material: String,
    pub uv: [f64; 2],
    pub tint: V3,
}
#[derive(Deserialize)]
pub struct Backing {
    pub material: String,
    pub uv: [f64; 2],
}

/// Matches the finish hash's first UTF-16 unit per Unicode character, distinct from the placement RNG.
pub fn finish_seed(text: &str) -> u32 {
    text.chars().fold(2166136261, |hash, c| {
        let mut units = [0; 2];
        (hash ^ u32::from(c.encode_utf16(&mut units)[0])).wrapping_mul(16777619)
    })
}

fn coordinate(value: f64) -> bool {
    value.is_finite() && value.abs() <= 1e7
}
fn point(u: f64, z: f64) -> Result<(), String> {
    if coordinate(u) && coordinate(z) {
        Ok(())
    } else {
        Err("Wall coordinates must be finite and within 10,000 km.".into())
    }
}
fn rectangle(bounds: [f64; 4]) -> Result<(), String> {
    if bounds.iter().all(|v| coordinate(*v)) && bounds[0] <= bounds[2] && bounds[1] <= bounds[3] {
        Ok(())
    } else {
        Err("Wall bounds must be finite, bounded and ordered.".into())
    }
}
fn damp_amount(height: f64, front: f64) -> f64 {
    if height <= 0. {
        return 1.;
    }
    let rising = (1. - height / front).max(0.).powf(1.6);
    let splash = (-height / 0.16).exp();
    (rising * 0.8 + splash * 0.6).min(1.)
}
fn detailed_loss(broad: f64, u: f64, z: f64, seed: f64) -> f64 {
    if broad <= -0.9 {
        return broad;
    }
    broad
        + (value_noise(u * 14., z * 14., seed) - 0.5) * 0.025
        + (value_noise(u * 55., z * 55., seed + 17.) - 0.5) * 0.011
        + (value_noise(u * 180., z * 180., seed + 31.) - 0.5) * 0.004
}

/// Owns a snapshot of the complete exposure program, preserving ordered overlapping ledges.
pub struct Condition {
    settings: Settings,
}
impl Condition {
    pub fn new(settings: &Settings) -> Result<Self, String> {
        if settings.ledges.len() > 4096 || settings.openings.len() > 4096 {
            return Err("Wall exposure count exceeds budget.".into());
        }
        if !settings.seed.is_finite()
            || settings.base.is_some_and(|v| !coordinate(v))
            || [settings.grime, settings.wear]
                .iter()
                .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
            || settings.ledges.iter().any(|l| {
                l.u0.is_nan()
                    || l.u1.is_nan()
                    || !coordinate(l.z)
                    || !l.flow.is_finite()
                    || l.flow > 1.
                    || [l.u0, l.u1]
                        .iter()
                        .any(|v| v.is_finite() && !coordinate(*v))
            })
            || settings
                .openings
                .iter()
                .any(|o| [o.u0, o.u1, o.z0, o.z1].iter().any(|v| !coordinate(*v)))
        {
            return Err("Invalid wall aging or exposure parameters.".into());
        }
        let mut settings = settings.clone();
        settings.ledges.retain(|l| l.flow > 0. && l.u1 > l.u0);
        settings.openings.retain(|o| o.u1 > o.u0 && o.z1 > o.z0);
        Ok(Self { settings })
    }
    pub fn grime_level(&self) -> f64 {
        self.settings.grime
    }
    pub fn wear_level(&self) -> f64 {
        self.settings.wear
    }
    pub fn runoff_sources(&self) -> &[Ledge] {
        &self.settings.ledges
    }

    fn deposit_detail(&self, u: f64, z: f64) -> f64 {
        let seed = self.settings.seed;
        let wander = (value_noise(u * 3.1, z * 1.4, seed + 419.) - 0.5) * 0.06;
        let fine = fractal_noise((u + wander) * 17., z * 2.3, seed + 421., 3);
        let broken = fractal_noise(u * 13., z * 11., seed + 423., 2);
        let grain = value_noise(u * 71., z * 67., seed + 431.);
        0.12 + 0.62 * smooth_step(0.36, 0.68, fine) + 0.18 * broken + 0.08 * grain
    }
    fn opening(&self, u: f64, z: f64) -> V3 {
        if self.settings.grime <= 0. {
            return [1.; 3];
        }
        let mut clean = 1.;
        for o in &self.settings.openings {
            let outside_u = (o.u0 - u).max(u - o.u1).max(0.);
            let outside_z = (o.z0 - z).max(z - o.z1).max(0.);
            if outside_u >= SOOT_REACH || outside_z >= SOOT_REACH {
                continue;
            }
            let edge = (u - o.u0).abs().min((u - o.u1).abs());
            let across = 1. - smooth_step(0.02, 0.32, outside_u);
            let within = smooth_step(o.z0 - 0.14, o.z0 + 0.08, z)
                * (1. - smooth_step(o.z1 - 0.04, o.z1 + 0.2, z));
            let jamb = (-edge / 0.12).exp() * within * 0.58;
            let head = (-((z - o.z1 - 0.035) / 0.21).powi(2)).exp() * across;
            let drop = (o.z0 - z).max(0.);
            let sill = if z < o.z0 {
                (-drop / 0.2).exp()
                    * across
                    * (0.2 + 0.5 * value_noise(u * 9., drop * 1.4, self.settings.seed + 403.))
            } else {
                0.
            };
            let mottling = (0.58 + 0.42 * value_noise(u * 4.8, z * 3.4, self.settings.seed + 397.))
                * self.deposit_detail(u, z);
            let fade = (1. - smooth_step(0.4, SOOT_REACH, outside_u))
                * (1. - smooth_step(0.4, SOOT_REACH, outside_z));
            let deposit = ((head * 0.85 + jamb + sill) * mottling * fade).min(1.);
            clean *= 1. - deposit;
        }
        let amount = (1. - clean) * self.settings.grime;
        [0.24, 0.21, 0.17].map(|pigment| 1. - amount * (1. - pigment))
    }
    pub fn opening_tint(&self, u: f64, z: f64) -> Result<V3, String> {
        point(u, z)?;
        Ok(self.opening(u, z))
    }
    fn deposits(&self, u: f64, z: f64, damp: f64, runoff: f64) -> V3 {
        let rain = smooth_step(
            0.4,
            0.9,
            value_noise(u * 2.2, z * 0.17, self.settings.seed + 29.),
        );
        let dirt = self.settings.grime
            * (damp * 0.75 + runoff * 0.85 + rain * 0.22).min(1.)
            * self.deposit_detail(u, z);
        let soot = self.opening(u, z);
        let pigment = [0.36, 0.34, 0.31];
        std::array::from_fn(|i| (1. + (pigment[i] - 1.) * dirt) * soot[i])
    }
    pub fn deposit_tint(&self, u: f64, z: f64) -> Result<V3, String> {
        point(u, z)?;
        Ok(self.deposits(u, z, self.damp(u, z), self.runoff_at(u, z)))
    }
    fn damp(&self, u: f64, z: f64) -> f64 {
        let Some(base) = self.settings.base else {
            return 0.;
        };
        if z <= base {
            return 1.;
        }
        let front = 0.2 + 1.1 * fractal_noise(u * 0.55, 0.37, self.settings.seed + 37., 2);
        damp_amount(z - base, front)
    }
    fn runoff_at(&self, u: f64, z: f64) -> f64 {
        let mut clean = 1.;
        for l in &self.settings.ledges {
            let drop = l.z - z;
            if !(0. ..=LONGEST_RUN).contains(&drop) {
                continue;
            }
            let outside = (l.u0 - u).max(u - l.u1).max(0.);
            if outside >= LEDGE_SPREAD {
                continue;
            }
            let ends = (-(u - l.u0).abs().min((u - l.u1).abs()) / 0.14).exp();
            let streak = smooth_step(
                0.3,
                0.78,
                value_noise(u * 3.4, l.z * 0.7, self.settings.seed + 53.),
            );
            let strength = (0.2 + 0.8 * streak.max(ends * 0.9)).min(1.);
            let reach = (0.3
                + (LONGEST_RUN - 0.3)
                    * strength
                    * value_noise(u * 2.1, l.z * 1.3, self.settings.seed + 71.))
                * (0.4 + 0.6 * l.flow);
            let streaked = strength * (1. - drop / reach).max(0.).powf(1.4);
            let beneath = 0.55 * (-drop / 0.1).exp();
            let stain = l.flow
                * (1. - outside / LEDGE_SPREAD)
                * (streaked + beneath).min(1.)
                * smooth_step(0., 0.008, drop);
            clean *= 1. - stain;
        }
        1. - clean
    }
    pub fn runoff(&self, u: f64, z: f64) -> Result<f64, String> {
        point(u, z)?;
        Ok(self.runoff_at(u, z))
    }
    pub fn sample(&self, u: f64, z: f64) -> Result<WallSample, String> {
        point(u, z)?;
        let s = &self.settings;
        let damp = self.damp(u, z);
        let runoff = self.runoff_at(u, z);
        let broad = fractal_noise(u * 0.37, z * 0.31, s.seed, 3);
        let grain = value_noise(u * 2.9, z * 2.3, s.seed + 19.);
        let tone =
            0.9 + (broad - 0.5) * (0.26 + s.wear * 0.1) + (grain - 0.5) * 0.08 + s.wear * 0.04;
        let loss = if s.wear <= 0. {
            -1.
        } else {
            let patch = fractal_noise(u * 0.5, z * 0.42, s.seed + 91., 4);
            let ragged = value_noise(u * 4.7, z * 4.1, s.seed + 113.);
            let exposure = 0.5 + (patch - 0.5) * 1.5 + (ragged - 0.5) * 0.16;
            exposure + damp * 0.2 + runoff * 0.14 - (1. - s.wear * 0.45)
        };
        Ok(WallSample {
            tint: self
                .deposits(u, z, damp, runoff)
                .map(|v| (tone * v).clamp(0.16, 1.)),
            loss,
            tone,
            damp,
            runoff,
        })
    }
    /// Conservatively encloses coarse loss over the whole rectangle; no sampled ownership guess.
    pub fn loss_range(&self, bounds: [f64; 4]) -> Result<[f64; 2], String> {
        rectangle(bounds)?;
        let s = &self.settings;
        if s.wear <= 0. {
            return Ok([-1.; 2]);
        }
        let [u0, z0, u1, z1] = bounds;
        let patch =
            fractal_noise_range([u0 * 0.5, z0 * 0.42, u1 * 0.5, z1 * 0.42], s.seed + 91., 4)?;
        let ragged = value_noise_range([u0 * 4.7, z0 * 4.1, u1 * 4.7, z1 * 4.1], s.seed + 113.)?;
        let damp = if let Some(base) = s.base {
            let front = fractal_noise_range([u0 * 0.55, 0.37, u1 * 0.55, 0.37], s.seed + 37., 2)?;
            [
                damp_amount(z1 - base, 0.2 + 1.1 * front[0]),
                damp_amount(z0 - base, 0.2 + 1.1 * front[1]),
            ]
        } else {
            [0.; 2]
        };
        let mut clean_low: f64 = 1.;
        let mut clean_high: f64 = 1.;
        for l in &s.ledges {
            let low_drop = (l.z - z1).max(0.);
            let high_drop = (l.z - z0).min(LONGEST_RUN);
            let outside = (l.u0 - u1).max(u0 - l.u1).max(0.);
            if low_drop > high_drop || outside >= LEDGE_SPREAD {
                continue;
            }
            let reach = LONGEST_RUN * (0.4 + 0.6 * l.flow);
            let streak = (1. - low_drop / reach).max(0.).powf(1.4);
            let beneath = 0.55 * (-low_drop / 0.1).exp();
            let maximum = l.flow
                * (1. - outside / LEDGE_SPREAD)
                * (streak + beneath).min(1.)
                * smooth_step(0., 0.008, high_drop);
            let products = [
                clean_low,
                clean_high,
                clean_low * (1. - maximum),
                clean_high * (1. - maximum),
            ];
            clean_low = products.into_iter().fold(f64::INFINITY, f64::min);
            clean_high = products.into_iter().fold(f64::NEG_INFINITY, f64::max);
        }
        let runoff = [1. - clean_high, 1. - clean_low];
        let values: [f64; 2] = std::array::from_fn(|i| {
            0.5 + (patch[i] - 0.5) * 1.5
                + (ragged[i] - 0.5) * 0.16
                + damp[i] * 0.2
                + runoff[i] * 0.14
                - (1. - s.wear * 0.45)
        });
        Ok([values[0] - 1e-12, values[1] + 1e-12])
    }
    pub fn plaster_loss(&self, u: f64, z: f64, seed: f64) -> Result<f64, String> {
        if !seed.is_finite() {
            return Err("Plaster seed must be finite.".into());
        }
        Ok(detailed_loss(self.sample(u, z)?.loss, u, z, seed))
    }
    pub fn plaster_range(&self, bounds: [f64; 4]) -> Result<[f64; 2], String> {
        let range = self.loss_range(bounds)?;
        if range[1] <= -0.9 {
            return Ok(range);
        }
        let detail = (0.025 + 0.011 + 0.004) / 2.;
        Ok([range[0] - detail, range[1] + detail])
    }
    pub fn needs_fracture(&self, bounds: [f64; 4]) -> Result<bool, String> {
        let [low, high] = self.plaster_range(bounds)?;
        Ok(low <= 0. && high >= 0.)
    }
    /// Layer elevations remain receiver-owned; this sample does not displace the architectural wall.
    pub fn plaster_sample(
        &self,
        u: f64,
        z: f64,
        seed: f64,
        coat: &Coating,
        backing: &Backing,
    ) -> Result<Sample, String> {
        let sample = self.sample(u, z)?;
        self.plaster_sample_from([u, z], seed, coat, backing, &sample)
    }
    /// Reuses an already evaluated condition sample without reevaluating deposits or moisture.
    pub fn plaster_sample_from(
        &self,
        at: [f64; 2],
        seed: f64,
        coat: &Coating,
        backing: &Backing,
        sample: &WallSample,
    ) -> Result<Sample, String> {
        let [u, z] = at;
        point(u, z)?;
        if !seed.is_finite()
            || coat.material.is_empty()
            || backing.material.is_empty()
            || coat.material.len() > 4096
            || backing.material.len() > 4096
            || coat.uv.iter().chain(&backing.uv).any(|v| !v.is_finite())
            || coat.tint.iter().any(|v| !v.is_finite() || *v < 0.)
            || !sample.loss.is_finite()
            || !sample.tone.is_finite()
            || sample
                .tint
                .iter()
                .chain([&sample.damp, &sample.runoff])
                .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
        {
            return Err("Invalid plaster material binding.".into());
        }
        let exposed = smooth_step(-0.004, 0.004, detailed_loss(sample.loss, u, z, seed));
        let moisture = sample.damp.max(sample.runoff) * self.settings.grime;
        Ok(Sample {
            layers: vec![
                Layer {
                    material: coat.material.clone(),
                    weight: 1. - exposed,
                    uv: coat.uv,
                    tint: Some(std::array::from_fn(|i| sample.tint[i] * coat.tint[i])),
                    roughness: None,
                    coatings: vec![],
                },
                Layer {
                    material: backing.material.clone(),
                    weight: exposed,
                    uv: backing.uv,
                    tint: Some(sample.tint.map(|v| v * 0.84)),
                    roughness: None,
                    coatings: vec![],
                },
            ],
            height: 0.,
            roughness: Some((0.9 + exposed * 0.07 - moisture * 0.09).min(1.)),
            bump: None,
            height_weight: None,
            layer_elevation_weight: None,
        })
    }
}
