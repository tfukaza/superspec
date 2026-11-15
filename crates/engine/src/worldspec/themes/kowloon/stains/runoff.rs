use crate::materialspec::noise::{fractal_noise, smooth_step, value_noise};
use serde::Serialize;
use superspec_geometry::V3;
use superspec_types::rng::SeededRng;

pub(crate) struct Settings<'a> {
    pub seed: &'a str,
    pub coarseness: f64,
    pub spread: f64,
    pub density: f64,
    pub strength: f64,
    pub tint: V3,
    pub variation: f64,
}

#[derive(Serialize)]
struct Trail {
    x: f64,
    start: f64,
    length: f64,
    radius: f64,
    bend: f64,
    phase: f64,
    seed: f64,
    tint: V3,
    opacity: f64,
    fork: bool,
}

#[derive(Serialize)]
pub(super) struct Runoff {
    seed: f64,
    coarseness: f64,
    spread: f64,
    strength: f64,
    tint: V3,
    trails: Vec<Trail>,
}

impl Runoff {
    pub fn new(s: Settings<'_>) -> Result<Self, String> {
        if [s.coarseness, s.spread, s.density, s.strength, s.variation]
            .into_iter()
            .chain(s.tint)
            .any(|v| !(0. ..=1.).contains(&v))
            || s.seed.len() > 4096
        {
            return Err("Runoff controls must be between zero and one with a bounded seed.".into());
        }
        let mut rng = SeededRng::from_text(&format!("{}/runoff-v4", s.seed));
        let seed = rng.integer(1, 2147483647) as f64;
        let trails = (0..(s.density * 16.).round() as usize)
            .map(|index| {
                let warm = rng.float(-1., 1.) * s.variation;
                let value = rng.float(-0.28, 0.28) * s.variation;
                Trail {
                    x: rng.float(-0.39, 0.39),
                    start: rng.float(0.01, 0.075),
                    length: if index % 2 == 1 {
                        rng.float(0.28, 0.62)
                    } else {
                        rng.float(0.6, 0.93)
                    },
                    radius: rng.float(0.012, 0.025) + s.coarseness * rng.float(0.025, 0.07),
                    bend: rng.float(-0.07, 0.07),
                    phase: rng.float(0., 10.),
                    seed: rng.integer(1, 2147483647) as f64,
                    tint: std::array::from_fn(|i| {
                        (s.tint[i] * (1. + value + warm * [0.3, 0.04, -0.23][i])).clamp(0.015, 0.98)
                    }),
                    opacity: rng.float(0.55, 0.95) * if index % 2 == 1 { 0.65 } else { 1. },
                    fork: index % 3 == 0,
                }
            })
            .collect();
        Ok(Self {
            seed,
            trails,
            coarseness: s.coarseness,
            spread: s.spread,
            strength: s.strength,
            tint: s.tint,
        })
    }

    pub fn sample(&self, u: f64, v: f64) -> (f64, V3) {
        if u.abs() >= 0.5 || v <= 0. || v >= 1. {
            return (0., [1.; 3]);
        }
        let drift = (value_noise(v * 3., 0., self.seed) - 0.5) * 0.12;
        let edge =
            0.46 * (1. - v * 0.68) + (value_noise(u * 15., v * 6., self.seed + 1.) - 0.5) * 0.12;
        let wash = (1. - smooth_step(edge * 0.3, edge, (u + drift).abs()))
            * (1. - smooth_step(0.12, 0.92, v))
            * (0.45 + value_noise(u * 14., v * 12., self.seed + 2.) * 0.55);
        let mut alpha = wash * self.spread * self.strength * 0.95;
        let mut pigment = self.tint.map(|c| (0.35 + c * 0.65) * alpha);
        for trail in &self.trails {
            let progress = (v - trail.start) / trail.length;
            if progress <= 0. || progress >= 1. {
                continue;
            }
            let center = trail.x
                + trail.bend * progress * 0.55
                + (progress * 5. + trail.phase).sin() * trail.radius * 0.4;
            let radius = trail.radius
                * (1. - progress).powf(0.12 + self.coarseness * 0.55)
                * (0.65 + value_noise(progress * 11., 0., trail.seed) * 0.7);
            let erosion = (value_noise(u * 55., v * 33., trail.seed + 71.)
                + value_noise(u * 19., v * 17., trail.seed + 73.)
                - 1.)
                * 0.35;
            let main = 1. - smooth_step(0.12, 1.4, (u - center).abs() / radius + erosion);
            let branch_center = center + trail.phase.sin() * 0.04 * smooth_step(0.2, 0.8, progress);
            let branch = if trail.fork {
                (1. - smooth_step(radius * 0.12, radius * 0.65, (u - branch_center).abs()))
                    * smooth_step(0.15, 0.35, progress)
                    * 0.2
            } else {
                0.
            };
            let broken = 0.4
                + smooth_step(
                    0.25,
                    0.75,
                    fractal_noise(u * 24., progress * 11., trail.seed + 9., 2),
                ) * 0.6;
            let amount = (main.max(branch)
                * broken
                * (1. - smooth_step(0.7, 1., progress))
                * trail.opacity
                * self.strength
                * 1.55
                * (1. - self.spread * 0.25))
                .min(0.96);
            for (i, c) in pigment.iter_mut().enumerate() {
                *c = *c * (1. - amount) + trail.tint[i] * amount;
            }
            alpha = alpha * (1. - amount) + amount;
        }
        let fade = smooth_step(0., 0.035, (0.5 - u.abs()).min(v).min(1. - v));
        (
            alpha * fade,
            if alpha > 0. {
                pigment.map(|c| c / alpha)
            } else {
                [1.; 3]
            },
        )
    }
}
