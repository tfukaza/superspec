use super::Image;
use crate::materialspec::noise::{lattice_hash, smooth_step};
use rustfft::{Fft, FftPlanner, num_complex::Complex64};
use std::{f64::consts::TAU, sync::Arc};
use superspec_types::rng::SeededRng;

fn gaussian_pair(rng: &mut SeededRng) -> [f64; 2] {
    let radius = (-2. * (1. - rng.next_f64()).ln()).sqrt();
    let (sin, cos) = (TAU * rng.next_f64()).sin_cos();
    [radius * cos, radius * sin]
}

struct Spectrum {
    size: usize,
    rng: SeededRng,
    forward: Arc<dyn Fft<f64>>,
    inverse: Arc<dyn Fft<f64>>,
    column: Vec<Complex64>,
    scratch: Vec<Complex64>,
}

impl Spectrum {
    fn new(size: usize, seed: u32) -> Self {
        let mut planner = FftPlanner::new();
        let forward = planner.plan_fft_forward(size);
        let inverse = planner.plan_fft_inverse(size);
        let scratch_size = forward
            .get_inplace_scratch_len()
            .max(inverse.get_inplace_scratch_len());
        Self {
            size,
            rng: SeededRng::from_u32(seed),
            forward,
            inverse,
            column: vec![Complex64::default(); size],
            scratch: vec![Complex64::default(); scratch_size],
        }
    }

    fn field(&mut self, exponent: f64, vertical: f64) -> Vec<f64> {
        let n = self.size;
        let count = n * n;
        let mut data: Vec<_> = (0..count / 2)
            .flat_map(|_| gaussian_pair(&mut self.rng))
            .map(|re| Complex64::new(re, 0.))
            .collect();
        self.transform(&mut data, false);
        for y in 0..n {
            let fy = y.min(n - y) as f64 * vertical;
            for x in 0..n {
                let fx = x.min(n - x) as f64;
                let radius = fx.hypot(fy);
                data[y * n + x] *= if radius == 0. {
                    0.
                } else {
                    radius.powf(-exponent)
                };
            }
        }
        self.transform(&mut data, true);
        let mut sorted: Vec<_> = (0..count).collect();
        sorted.sort_unstable_by(|&a, &b| data[a].re.total_cmp(&data[b].re).then(a.cmp(&b)));
        let mut ranks = vec![0.; count];
        for (rank, index) in sorted.into_iter().enumerate() {
            ranks[index] = rank as f64 / (count - 1) as f64;
        }
        ranks
    }

    fn transform(&mut self, data: &mut [Complex64], inverse: bool) {
        let fft = if inverse {
            &self.inverse
        } else {
            &self.forward
        };
        let n = self.size;
        fft.process_with_scratch(data, &mut self.scratch);
        for x in 0..n {
            for (y, value) in self.column.iter_mut().enumerate() {
                *value = data[y * n + x];
            }
            fft.process_with_scratch(&mut self.column, &mut self.scratch);
            for (y, &value) in self.column.iter().enumerate() {
                data[y * n + x] = value;
            }
        }
    }
}

fn blend<const CHANNELS: usize>(
    a: [f64; CHANNELS],
    b: [f64; CHANNELS],
    opacity: f64,
) -> [f64; CHANNELS] {
    std::array::from_fn(|i| crate::materialspec::authoring::sampling::mix(a[i], b[i], opacity))
}

fn blur<const CHANNELS: usize>(
    source: &[[f64; CHANNELS]],
    n: usize,
    sigma: f64,
) -> Vec<[f64; CHANNELS]> {
    let radius = (sigma * 3.).ceil() as isize;
    let weights: Vec<_> = (-radius..=radius)
        .map(|i| (-0.5 * (i as f64 / sigma).powi(2)).exp())
        .collect();
    let total: f64 = weights.iter().sum();
    let pass = |input: &[[f64; CHANNELS]], stride: usize| {
        (0..n * n)
            .map(|index| {
                let coordinate = index / stride % n;
                let origin = index - coordinate * stride;
                let mut pixel = [0.; CHANNELS];
                for (offset, weight) in (-radius..=radius).zip(&weights) {
                    let sample = (coordinate as isize + offset).rem_euclid(n as isize) as usize;
                    for (result, channel) in pixel.iter_mut().zip(input[origin + sample * stride]) {
                        *result += channel * weight / total;
                    }
                }
                pixel
            })
            .collect::<Vec<_>>()
    };
    pass(&pass(source, 1), n)
}

fn runoff(blooms: &[[f64; 1]], n: usize, length: f64) -> Vec<f64> {
    let attenuation = (-1. / length).exp();
    let mut trail = vec![0.; n * n];
    // Two laps let the exponential tail reach rows at the top of the tile.
    for x in 0..n {
        let mut decay: f64 = 0.;
        for y in 0..2 * n {
            let index = (y % n) * n + x;
            decay = blooms[index][0].max(decay * attenuation);
            trail[index] = decay;
        }
    }
    trail
}

fn signboard(n: usize) -> Vec<[f64; 3]> {
    let mut noise = Spectrum::new(n, 21);
    let mut color: Vec<_> = noise
        .field(2.2, 1.)
        .into_iter()
        .map(|v| [0.95 * (0.955 + 0.045 * v); 3])
        .collect();
    let streaks = noise.field(0.95, 22.);
    let washes = noise.field(1.7, 5.);
    let patchiness = noise.field(2.2, 1.);
    for (((pixel, streak), wash), patch) in
        color.iter_mut().zip(streaks).zip(washes).zip(patchiness)
    {
        let dirt = smooth_step(0.55, 0.98, streak)
            * (0.1 + 0.3 * smooth_step(0.35, 0.95, wash) * (0.25 + 0.75 * patch));
        for (channel, multiplier) in pixel.iter_mut().zip([0.85, 0.95, 1.1]) {
            *channel *= 1. - dirt * multiplier;
        }
    }
    let seeds: Vec<_> = noise
        .field(1.1, 1.)
        .into_iter()
        .map(|v| [f64::from(v > 0.9965)])
        .collect();
    let mut blooms = blur(&seeds, n, 2.2);
    for bloom in &mut blooms {
        bloom[0] = (bloom[0] * 3.2).min(1.);
    }
    let runs = runoff(&blooms, n, 26.);
    let variation = noise.field(1.2, 10.);
    let chips = noise.field(0.9, 1.);
    for (index, (pixels, grain)) in color
        .chunks_exact_mut(2)
        .zip(std::iter::repeat_with(|| gaussian_pair(&mut noise.rng)))
        .enumerate()
    {
        for (offset, (pixel, grain)) in pixels.iter_mut().zip(grain).enumerate() {
            let i = index * 2 + offset;
            let stain = (0.85 * blooms[i][0] + 0.42 * runs[i] * (0.5 + 0.5 * variation[i])).min(1.);
            let chip = smooth_step(0.988, 0.994, chips[i]);
            let shade = (1. - 0.3 * chip) * (1. + 0.02 * grain.clamp(-2., 2.));
            *pixel = blend(*pixel, [0.52, 0.28, 0.14], stain).map(|v| v * shade);
        }
    }
    color
}

fn rust(n: usize) -> Vec<[f64; 3]> {
    const STOPS: [f64; 5] = [0.2, 0.36, 0.5, 0.64, 0.8];
    const COLORS: [[f64; 3]; 5] = [
        [0.12, 0.075, 0.055],
        [0.23, 0.125, 0.075],
        [0.36, 0.18, 0.095],
        [0.5, 0.26, 0.115],
        [0.6, 0.36, 0.16],
    ];
    let mut noise = Spectrum::new(n, 22);
    let broad = noise.field(1.7, 1.);
    let middle = noise.field(1.15, 1.);
    let fine = noise.field(0.65, 1.);
    let scale = noise.field(1., 1.);
    let islands = noise.field(1.5, 1.);
    let flaking = noise.field(0.9, 1.);
    let tracks = noise.field(1.6, 12.);
    let mut color = Vec::with_capacity(n * n);
    for grain in (0..n * n / 2).flat_map(|_| gaussian_pair(&mut noise.rng)) {
        let i = color.len();
        let tone = (0.36 * broad[i] + 0.36 * middle[i] + 0.28 * fine[i]).clamp(STOPS[0], STOPS[4]);
        let step = STOPS.partition_point(|&stop| stop < tone).clamp(1, 4);
        let palette = blend(
            COLORS[step - 1],
            COLORS[step],
            (tone - STOPS[step - 1]) / (STOPS[step] - STOPS[step - 1]),
        );
        let pits = 1. - 0.45 * smooth_step(0.88, 0.95, fine[i]);
        let raised = 1. + 0.2 * smooth_step(0.78, 0.84, scale[i]);
        let islands = 0.6 * islands[i] + 0.4 * flaking[i];
        let paint = smooth_step(0.735, 0.75, islands);
        let rim = smooth_step(0.71, 0.735, islands) * (1. - paint);
        let gray = [0.46, 0.48, 0.47].map(|v| v * (0.8 + 0.2 * fine[i]));
        let base = palette.map(|v| v * pits * raised * (1. - 0.5 * rim));
        let shade =
            (1. - 0.14 * smooth_step(0.6, 1., tracks[i])) * (1. + 0.035 * grain.clamp(-2., 2.));
        color.push(blend(base, gray, 0.8 * paint).map(|v| v * shade));
    }
    color
}

fn pigment(x: usize, y: usize, n: usize, cells: usize, seed: f64) -> f64 {
    let (x, y) = (
        x as f64 * cells as f64 / n as f64,
        y as f64 * cells as f64 / n as f64,
    );
    let lattice = |dx: f64, dy: f64| {
        lattice_hash(
            (x.floor() + dx).rem_euclid(cells as f64),
            (y.floor() + dy).rem_euclid(cells as f64),
            seed,
        )
    };
    let (u, v) = (
        smooth_step(0., 1., x.fract()),
        smooth_step(0., 1., y.fract()),
    );
    let mix = crate::materialspec::authoring::sampling::mix;
    mix(
        mix(lattice(0., 0.), lattice(1., 0.), u),
        mix(lattice(0., 1.), lattice(1., 1.), u),
        v,
    )
}

struct Canvas {
    size: usize,
    pixels: Vec<[f64; 3]>,
}

impl Canvas {
    fn overlay(&mut self, x: isize, y: isize, rgb: [u8; 3], alpha: u8) {
        let wrap = |v: isize| v.rem_euclid(self.size as isize) as usize;
        let pixel = &mut self.pixels[wrap(y) * self.size + wrap(x)];
        *pixel = blend(
            *pixel,
            rgb.map(|v| f64::from(v) / 255.),
            f64::from(alpha) / 255.,
        );
    }
}

fn painted_surface(name: &str, n: usize) -> Vec<[f64; 3]> {
    let (seed, base, cells, low) = match name {
        "metal" => (11, [238., 238., 236.], 6, 112.),
        "sheet" => (12, [236., 238., 238.], 5, 108.),
        "timber" => (13, [236., 234., 228.], 5, 114.),
        _ => unreachable!(),
    };
    let mut rng = SeededRng::from_u32(seed);
    let roughness: Vec<_> = (0..n * n / 2)
        .flat_map(|_| gaussian_pair(&mut rng))
        .collect();
    let streaks: Vec<_> = (0..n / 2).flat_map(|_| gaussian_pair(&mut rng)).collect();
    let shade = |x, y| {
        let blotch = (low + (128. - low) * pigment(x, y, n, cells, seed as f64)) / 128.;
        let x_fraction = x as f64 / n as f64;
        let surface = match name {
            "sheet" => ((120. - 26. * (TAU * 8. * x_fraction).cos()) / 128.).min(1.),
            "timber" => {
                let index = y * n + x;
                let long = pigment(x, 0, n, 64, 13.);
                ((128. + (long - 0.5) * 28.) / 128.).min(1.)
                    * ((128. + streaks[x] * 3.) / 128.).min(1.)
                    * ((128. + roughness[index]) / 128.).min(1.)
            }
            _ => 1.,
        };
        let grain = if name == "timber" {
            1.
        } else {
            ((128. + roughness[y * n + x] * 3.) / 128.).min(1.)
        };
        base.map(|v| v * blotch * surface * grain / 255.)
    };
    let mut canvas = Canvas {
        size: n,
        pixels: (0..n)
            .flat_map(|y| (0..n).map(move |x| shade(x, y)))
            .collect(),
    };
    let mut coordinate = || rng.integer(0, n - 1) as isize;
    match name {
        "metal" => {
            let mut marks = SeededRng::from_u32(11);
            for _ in 0..90 {
                let (x, y) = (coordinate(), coordinate());
                let radius = marks.integer(1, 4) as isize;
                for dy in -radius..=radius {
                    for dx in -radius..=radius {
                        if dx * dx + dy * dy <= radius * radius {
                            canvas.overlay(x + dx, y + dy, [120, 70, 40], 150);
                        }
                    }
                }
                for dy in 0..=marks.integer(4, 40) as isize {
                    for dx in -1..=1 {
                        canvas.overlay(x + dx, y + dy, [130, 80, 45], 40);
                    }
                }
            }
            for _ in 0..40 {
                let (x, y) = (coordinate(), coordinate());
                let length = marks.integer(10, 70) as isize;
                let drop = marks.integer(0, 12) as f64 - 6.;
                for dx in 0..=length {
                    canvas.overlay(
                        x + dx,
                        y + (dx as f64 * drop / length as f64).round() as isize,
                        [255; 3],
                        70,
                    );
                }
            }
            blur(&canvas.pixels, n, 0.6)
        }
        "sheet" => {
            let mut marks = SeededRng::from_u32(12);
            for _ in 0..46 {
                let (x, y) = (coordinate(), coordinate());
                let run = marks.integer(30, 220) as isize;
                let width = marks.integer(1, 3) as isize;
                let alpha = marks.integer(25, 70) as u8;
                for dy in 0..=run {
                    for dx in 0..=width {
                        canvas.overlay(x + dx, y + dy, [120, 72, 40], alpha);
                    }
                }
            }
            blur(&canvas.pixels, n, 0.5)
        }
        "timber" => {
            for board in 0..4 {
                let x = (board * n / 4) as isize;
                for y in 0..n as isize {
                    for dx in 0..=2 {
                        canvas.overlay(x + dx, y, [40, 34, 28], 170);
                    }
                    for dx in 3..=4 {
                        canvas.overlay(x + dx, y, [255; 3], 60);
                    }
                }
            }
            for _ in 0..30 {
                let (x, y) = (coordinate(), coordinate());
                for dy in -6..=6 {
                    for dx in -3..=3 {
                        let r = (dx as f64 / 3.).hypot(dy as f64 / 6.);
                        if (0.78..=1.15).contains(&r) {
                            canvas.overlay(x + dx, y + dy, [90, 74, 60], 90);
                        }
                    }
                }
            }
            canvas.pixels
        }
        _ => unreachable!(),
    }
}

pub fn bake(name: &str) -> Image {
    let (size, pixels) = match name {
        "signboard" => (1024, signboard(1024)),
        "rust" => (1024, rust(1024)),
        _ => (512, painted_surface(name, 512)),
    };
    let rgba = pixels
        .into_iter()
        .flat_map(|rgb| {
            let [r, g, b] = rgb.map(|v| (v * 255.).round().clamp(0., 255.) as u8);
            [r, g, b, 255]
        })
        .collect();
    Image {
        width: size,
        height: size,
        rgba,
        mime: "image/png",
    }
}
