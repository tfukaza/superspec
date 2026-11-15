//! Ordered render-repair campaigns and gravity-driven pigment, separate from geometric coat loss.
use super::{
    noise::{fractal_noise, smooth_step, value_noise},
    wall::{Condition, WallSample},
};
use serde::{Deserialize, Serialize};
use superspec_geometry::{
    V3,
    math::{cos, sin},
};
use superspec_types::rng::SeededRng;
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Patch {
    pub regions: Vec<[f64; 4]>,
    pub tint: V3,
    pub seed: u32,
    pub age: f64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Sample {
    pub tint: V3,
    pub age: f64,
    pub height: f64,
    pub coverage: f64,
}
fn rectangle(b: [f64; 4]) -> bool {
    b.iter().all(|v| v.is_finite() && v.abs() <= 10000.) && b[0] < b[2] && b[1] < b[3]
}
pub fn plan(bounds: [f64; 4], seed: u32, openings: &[[f64; 4]]) -> Result<Vec<Patch>, String> {
    if !rectangle(bounds)
        || openings.len() > 256
        || openings.iter().any(|b| !rectangle(*b))
        || ((bounds[2] - bounds[0] + 0.3) / 2.1).ceil()
            * ((bounds[3] - bounds[1] + 0.3) / 1.9).ceil()
            + openings.len() as f64
            > 4096.
    {
        return Err("Render repair planning exceeds bounded rectangles or campaign work.".into());
    }
    let mut rng = SeededRng::from_text(&format!("render-repairs/{seed}"));
    let mut patches = Vec::new();
    let tones = [
        [0.92, 0.94, 0.95],
        [1.06, 1.04, 1.01],
        [0.97, 0.99, 1.02],
        [1.04, 1.02, 0.98],
    ];
    let [u0, v0, u1, v1] = bounds;
    let mut v = v0 - 0.3;
    while v < v1 {
        let mut u = u0 - 0.3;
        while u < u1 {
            if rng.chance(0.65) {
                let x = u + rng.float(0.1, 0.7);
                let y = v + rng.float(0.1, 0.8);
                let width = rng.float(0.45, 1.3);
                let height = rng.float(0.35, 1.05);
                let mut regions = vec![[x, y, x + width, y + height]];
                if rng.chance(0.65) {
                    regions.push([
                        x + width * 0.2,
                        y + height * 0.5,
                        x + width * 0.8,
                        y + height + rng.float(0.18, 0.5),
                    ]);
                }
                patches.push(Patch {
                    regions,
                    tint: rng.pick(&tones),
                    seed: rng.integer(1, 2147483647) as u32,
                    age: rng.pick(&[0.08, 0.18, 0.45, 0.8]),
                });
            }
            u += 2.1;
        }
        v += 1.9;
    }
    for opening in openings.iter().step_by(2) {
        let left = opening[0];
        let top = opening[3];
        let x = (u0 + 0.06).max(left - rng.float(0.45, 0.75));
        let y = top - rng.float(0.45, 0.8);
        let end = (v1 - 0.05).min(top + rng.float(0.45, 0.8));
        if end <= y || left - x < 0.12 {
            continue;
        }
        patches.push(Patch {
            regions: vec![
                [x, y, left + 0.2, end],
                [x + 0.2, y + 0.15, left + 0.45, end + 0.2],
            ],
            tint: rng.pick(&tones),
            seed: rng.integer(1, 2147483647) as u32,
            age: rng.pick(&[0.08, 0.18, 0.5]),
        });
    }
    Ok(patches)
}
pub struct Repairs {
    patches: Vec<Patch>,
}
impl Repairs {
    pub fn new(patches: &[Patch]) -> Result<Self, String> {
        if patches.len() > 4096
            || patches.iter().any(|p| {
                p.regions.is_empty()
                    || p.regions.len() > 8
                    || p.regions.iter().any(|b| !rectangle(*b))
                    || !p.age.is_finite()
                    || !(0. ..=1.).contains(&p.age)
                    || p.tint
                        .iter()
                        .any(|v| !v.is_finite() || !(0. ..=4.).contains(v))
            })
        {
            return Err("Render repairs require bounded ordered regions, colors and ages.".into());
        }
        Ok(Self {
            patches: patches.to_vec(),
        })
    }
    pub fn sample(&self, u: f64, v: f64) -> Result<Sample, String> {
        if [u, v].iter().any(|v| !v.is_finite() || v.abs() > 100000.) {
            return Err("Invalid render repair sample coordinate.".into());
        }
        let mut sample = Sample {
            tint: [1.; 3],
            age: 1.,
            height: 0.,
            coverage: 0.,
        };
        for p in &self.patches {
            let distance = p
                .regions
                .iter()
                .map(|b| (u - b[0]).min(b[2] - u).min(v - b[1]).min(b[3] - v))
                .fold(f64::NEG_INFINITY, f64::max);
            if distance < -0.06 {
                continue;
            }
            let seed = f64::from(p.seed);
            let ragged = (value_noise(u * 47., v * 43., seed) - 0.5) * 0.008;
            let coverage = smooth_step(-0.004, 0.004, distance + ragged);
            let trowel = (value_noise(u * 7. + v * 2.2, v * 24., seed + 7.) - 0.5) * 0.0009;
            for axis in 0..3 {
                sample.tint[axis] = sample.tint[axis] * (1. - coverage) + p.tint[axis] * coverage;
            }
            sample.age = sample.age * (1. - coverage) + p.age * coverage;
            sample.height = sample.height * (1. - coverage) + (0.003 + trowel) * coverage;
            sample.coverage = 1. - (1. - sample.coverage) * (1. - coverage);
        }
        Ok(sample)
    }
}
/// Prepared affine source mapping retains physical scale and a consistent tangent-space normal basis.
pub struct StudyUv {
    repeat: f64,
    cosine: f64,
    sine: f64,
    phase: [f64; 2],
}
impl StudyUv {
    pub fn new(repeat: f64, seed: f64) -> Result<Self, String> {
        if !repeat.is_finite()
            || repeat <= 0.
            || repeat > 10000.
            || !seed.is_finite()
            || seed.abs() > 1e10
        {
            return Err("Invalid study texture transform.".into());
        }
        let angle = (value_noise(0., 0., seed) - 0.5) * std::f64::consts::PI / 10.;
        Ok(Self {
            repeat,
            cosine: cos(angle),
            sine: sin(angle),
            phase: [
                value_noise(0., 0., seed + 1.) * 7.,
                value_noise(0., 0., seed + 2.) * 7.,
            ],
        })
    }
    pub fn sample(&self, u: f64, v: f64) -> [f64; 2] {
        [
            (u * self.cosine - v * self.sine) * self.repeat + self.phase[0],
            (u * self.sine + v * self.cosine) * self.repeat + self.phase[1],
        ]
    }
}
/// The caller supplies an admitted condition and its sample at the same point.
pub fn patina(
    condition: &Condition,
    repair: &Sample,
    tone: f64,
    wall: &WallSample,
    u: f64,
    v: f64,
    seed: u32,
) -> V3 {
    let seed = f64::from(seed);
    let grain = (value_noise(u * 95., v * 95., seed + 223.) - 0.5) * 0.012;
    let mut clean = 1.;
    for ledge in condition.runoff_sources() {
        let drop = ledge.z - v;
        let outside = (ledge.u0 - u).max(u - ledge.u1).max(0.);
        if drop <= 0. || drop > 1.8 || outside >= 0.12 {
            continue;
        }
        let warped = u + (value_noise(u * 3., v * 5., seed + 251.) - 0.5) * 0.025;
        let finger = smooth_step(
            0.28,
            0.8,
            value_noise(warped * 13., ledge.z * 0.31, seed + 257.),
        );
        let hairline = smooth_step(0.38, 0.82, value_noise(warped * 59., v * 0.18, seed + 263.));
        let ends = (-(u - ledge.u0).abs().min((u - ledge.u1).abs()) / 0.035).exp();
        let reach = (0.18 + 1.5 * finger.max(ends)) * (0.45 + 0.55 * ledge.flow);
        let ragged = (fractal_noise(u * 24., v * 20., seed + 269., 2) - 0.5) * 0.2;
        let tail = smooth_step(-0.08, 0.12, 1. - drop / reach + ragged)
            * (1. - drop / reach).max(0.).powf(0.45);
        let thread = (0.25 + 0.75 * finger.max(ends)) * (0.45 + 0.55 * hairline);
        let apron = (-drop / 0.12).exp() * 0.72;
        let breakup = 0.3 + fractal_noise(u * 37., v * 29., seed + 271., 2) * 0.9;
        let concentration = 0.4 + 0.6 * (0.8 / (ledge.u1 - ledge.u0)).min(1.);
        let stain = ((thread * tail + apron) * breakup).min(1.)
            * ledge.flow
            * concentration
            * (1. - smooth_step(0., 0.12, outside))
            * smooth_step(0., 0.015, drop);
        clean *= 1. - stain;
    }
    let runoff = (1. - clean) * condition.grime_level() * repair.age;
    let fresh = wall.tone.min(1.);
    let gray = (wall.tint[0] + wall.tint[1] + wall.tint[2]) / 3.;
    std::array::from_fn(|axis| {
        repair.tint[axis]
            * (tone + grain)
            * (fresh * (1. - repair.age) + (gray + (wall.tint[axis] - gray) * 0.15) * repair.age)
            * (1. - runoff * [0.72, 0.76, 0.8][axis])
    })
}
