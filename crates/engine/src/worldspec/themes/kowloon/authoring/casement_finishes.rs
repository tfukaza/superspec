//! Metric surface maps for painted casement sections and their nickel hardware.
use super::Files;
use crate::materialspec::authoring::{
    Maps,
    sampling::{anisotropic_lattice, lattice, mix},
};

const SIZE: usize = 512;

fn ramp(low: f64, high: f64, value: f64) -> f64 {
    let t = ((value - low) / (high - low)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

// Favor the neighborhood with least internal variation, retaining distinct planes of color.
// A wrapped quadrant filter keeps the brush fields continuous on adjoining UV tiles.
fn kuwahara(field: &[f64], x: usize, y: usize) -> f64 {
    let mut best = (f64::INFINITY, 0.);
    for sy in [-1, 1] {
        for sx in [-1, 1] {
            let mut sum = 0.;
            let mut squares = 0.;
            for dy in [0, 3, 6] {
                for dx in [0, 3, 6] {
                    let i = (x as isize + sx * dx).rem_euclid(SIZE as isize) as usize;
                    let j = (y as isize + sy * dy).rem_euclid(SIZE as isize) as usize;
                    let sample = field[j * SIZE + i];
                    sum += sample;
                    squares += sample * sample;
                }
            }
            let mean = sum / 9.;
            let variance = squares / 9. - mean * mean;
            if variance < best.0 {
                best = (variance, mean);
            }
        }
    }
    best.1
}

pub fn bake() -> Result<Files, String> {
    let mut field = Vec::with_capacity(SIZE * SIZE);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let u = x as f64 / SIZE as f64;
            let v = y as f64 / SIZE as f64;
            field.push(
                0.56 * lattice(u, v, 7, 61.)
                    + 0.30 * anisotropic_lattice(u, v, [19, 27], 81.)
                    + 0.14 * lattice(u, v, 43, 19.),
            );
        }
    }
    let mut paint = Maps::new(SIZE, SIZE);
    let mut nickel = Maps::new(SIZE, SIZE);
    let mut surround = Maps::new(SIZE, SIZE);
    let mut paint_height = Vec::with_capacity(field.len());
    let mut nickel_height = Vec::with_capacity(field.len());
    let mut surround_height = Vec::with_capacity(field.len());
    for y in 0..SIZE {
        for x in 0..SIZE {
            let u = x as f64 / SIZE as f64;
            let v = y as f64 / SIZE as f64;
            let facet = kuwahara(&field, x, y);
            let warm = ramp(0.36, 0.67, facet);
            let cool = ramp(0.61, 0.82, lattice(u, v, 5, 931.));
            let brush = anisotropic_lattice(u, v, [11, 89], 27.) - 0.5;
            let grain = lattice(u, v, 157, 207.) - 0.5;
            // Long box sections have metric U and one longitudinal lip at V=0.
            // Fade the exposed metal along its run instead of outlining every edge evenly.
            let lip = 1. - ramp(0.0015, 0.006, v.min(1. - v));
            let wear = lip * (0.16 + 0.84 * ramp(0.30, 0.71, lattice(u, v, 17, 739.)));
            let tone = 0.06 * (facet - 0.5) + 0.028 * brush + 0.014 * grain;
            let paint_color = [
                0.695 + 0.15 * warm + 0.015 * cool,
                0.795 + 0.054 * warm - 0.011 * cool,
                0.776 - 0.033 * warm + 0.043 * cool,
            ];
            paint.pixel(
                0,
                std::array::from_fn(|k| {
                    mix(paint_color[k] + tone, [0.87, 0.88, 0.84][k], 0.74 * wear)
                }),
            );
            paint.pixel(
                2,
                [
                    1.,
                    0.42 - 0.08 * warm + 0.043 * brush - 0.19 * wear,
                    0.19 + 0.08 * warm + 0.46 * wear,
                ],
            );
            paint_height.push(0.0009 * facet + 0.00020 * brush + 0.000055 * grain - 0.00023 * wear);

            // The heavier folded liner is a separate blue-grey powder coat. Iron shows
            // through rubbed sections as a low-saturation green-grey sheen.
            let undercoat = [0.39, 0.44, 0.48];
            let rubbed = [0.50, 0.53, 0.50];
            surround.pixel(
                0,
                std::array::from_fn(|k| {
                    mix(undercoat[k], rubbed[k], warm) + 0.06 * (facet - 0.5) + 0.025 * brush
                }),
            );
            surround.pixel(
                2,
                [1., 0.47 - 0.11 * warm + 0.04 * brush, 0.36 + 0.19 * warm],
            );

            surround_height.push(0.00080 * facet + 0.00017 * brush + 0.00024 * grain);

            let polish = ramp(0.28, 0.78, facet + 0.14 * brush);
            let nickel_color = [
                0.70 + 0.17 * polish,
                0.74 + 0.115 * polish,
                0.74 + 0.065 * polish,
            ];
            nickel.pixel(0, nickel_color.map(|c| c + 0.033 * brush));
            nickel.pixel(
                2,
                [
                    1.,
                    0.34 - 0.12 * polish + 0.050 * brush,
                    0.79 + 0.12 * polish,
                ],
            );
            nickel_height.push(0.00052 * brush + 0.00013 * grain);
        }
    }
    paint.normals(&paint_height, SIZE as f64, true);
    nickel.normals(&nickel_height, SIZE as f64, true);
    surround.normals(&surround_height, SIZE as f64, true);
    let mut files = paint.encode("casement-enamel")?;
    files.extend(nickel.encode("casement-nickel")?);
    files.extend(surround.encode("casement-surround")?);
    Ok(files)
}
