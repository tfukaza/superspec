//! Kowloon kitchen, vent and restaurant material recipes.
use super::super::tile::{Settings, Tile};
use crate::materialspec::{
    authoring::{
        Maps,
        sampling::{lattice, mix},
    },
    color::srgb,
    noise::{fractal_noise, smooth_step, value_noise},
};
use superspec_geometry::image::Image;

pub fn kitchen() -> Maps {
    let size = 512;
    let mut maps = Maps::new(size, size);
    let mut heights = Vec::with_capacity(size * size);
    for y in 0..size {
        for x in 0..size {
            let u = x as f64 / size as f64;
            let v = y as f64 / size as f64;
            let cloud = lattice(u, v, 3, 173.) * 0.65 + lattice(u, v, 7, 619.) * 0.35;
            let fine = lattice(u, v, 23, 91.);
            let fluid = lattice(u, v, 3, 539.) * 0.8 + lattice(u, v, 7, 29.) * 0.2;
            let tide = (-((fluid - 0.51) / 0.035).powi(2)).exp() * ((cloud - 0.35) * 1.5).max(0.);
            let grease = ((fluid - 0.52) * 2.).max(0.);
            let silver = 0.77 + (cloud - 0.5) * 0.032 - tide * 0.008;
            maps.pixel(
                0,
                [
                    silver - grease * 0.004,
                    silver + 0.004 - grease * 0.009,
                    silver + 0.006 - grease * 0.017,
                ],
            );
            maps.pixel(
                2,
                [
                    1.,
                    (0.35 + cloud * 0.33 + tide * 0.14 - grease * 0.18).clamp(0.35, 0.65),
                    0.96 - tide * 0.04 - grease * 0.025,
                ],
            );
            heights.push((cloud - 0.5) * 0.000001 + (fine - 0.5) * 0.00000025);
        }
    }
    maps.normals(&heights, 1024., true);
    maps
}

fn periodic(x: f64, y: f64, frequency: f64, seed: f64) -> f64 {
    let (x, y) = (x.rem_euclid(2.), y.rem_euclid(2.));
    let (u, v) = (smooth_step(0., 2., x), smooth_step(0., 2., y));
    let noise = |x, y| value_noise(x * frequency, y * frequency, seed);
    mix(
        mix(noise(x, y), noise(x - 2., y), u),
        mix(noise(x, y - 2.), noise(x - 2., y - 2.), u),
        v,
    )
}

fn oxidation(x: f64, y: f64) -> f64 {
    let warped_x = x + (periodic(x, y, 1.2, 421.) - 0.5) * 0.24;
    let warped_y = y + (periodic(x, y, 1.2, 422.) - 0.5) * 0.24;
    let noise = |frequency, seed| periodic(warped_x, warped_y, frequency, seed);
    let mut total = 0.;
    let mut weight = 0.;
    let (mut amplitude, mut frequency) = (1., 1.8);
    for octave in 0..6 {
        total += noise(frequency, 432. + octave as f64 * 37.) * amplitude;
        weight += amplitude;
        amplitude *= 0.6;
        frequency *= 2.;
    }
    let coverage = total / weight;
    let middle = noise(7.2, 733.) * 0.5 + noise(14.4, 734.) * 0.3 + noise(28.8, 735.) * 0.2;
    smooth_step(0.36, 0.73, coverage) * 0.12
        + smooth_step(0.465, 0.535, coverage + (middle - 0.5) * 0.18) * 0.085
}

pub fn vent(source: &Image) -> Result<Maps, String> {
    const FIELD: usize = 32;
    if source.width < FIELD || source.height < FIELD {
        return Err("Vent pigment source must be at least 32 × 32 pixels.".into());
    }
    let mut pigment = [[0.; 3]; FIELD * FIELD];
    for y in 0..FIELD {
        for x in 0..FIELD {
            let mut count = 0;
            let cell = &mut pigment[y * FIELD + x];
            for sy in y * source.height / FIELD..(y + 1) * source.height / FIELD {
                for sx in x * source.width / FIELD..(x + 1) * source.width / FIELD {
                    for axis in 0..3 {
                        cell[axis] += f64::from(source.rgba[(sy * source.width + sx) * 4 + axis]);
                    }
                    count += 1;
                }
            }
            *cell = cell.map(|value| value / f64::from(count));
        }
    }
    let size = 1024;
    let mut maps = Maps::new(size, size);
    for row in 0..size {
        for column in 0..size {
            let x = (column as f64 + 0.5) / size as f64 * 2.;
            let y = (row as f64 + 0.5) / size as f64 * 2.;
            let film = oxidation(x, y);
            let crust = smooth_step(
                0.68,
                0.78,
                periodic(x, y, 24., 811.) * 0.55
                    + periodic(x, y, 51., 812.) * 0.3
                    + periodic(x, y, 93., 813.) * 0.15,
            ) * 0.18;
            let grime = smooth_step(0.56, 0.82, periodic(x, y, 2.5, 91.)) * 0.13;
            let broad = (periodic(x, y, 1.1, 11.) - 0.5) * 12.;
            let [u, v] =
                [x, y].map(|coordinate| (1. - (coordinate * std::f64::consts::PI).cos()) * 15.5);
            let (ix, iy) = (u as usize, v as usize);
            let (right, bottom) = ((ix + 1).min(FIELD - 1), (iy + 1).min(FIELD - 1));
            maps.pixel(
                0,
                std::array::from_fn(|axis| {
                    let at = |x: usize, y: usize| pigment[y * FIELD + x][axis];
                    let source = mix(
                        mix(at(ix, iy), at(right, iy), u.fract()),
                        mix(at(ix, bottom), at(right, bottom), u.fract()),
                        v.fract(),
                    );
                    let zinc = [151., 175., 174.][axis] + (source - 163.) * 2.2 + broad;
                    let filmed = mix(zinc, [132., 116., 85.][axis], film);
                    let dirty = mix(filmed, [122., 102., 78.][axis], grime);
                    mix(dirty, [121., 76., 39.][axis], crust) / 255.
                }),
            );
            maps.pixel(
                2,
                [
                    1.,
                    (173. + film * 45. + crust * 35.) / 255.,
                    112. * (1. - film * 0.7 - crust) / 255.,
                ],
            );
            let nx = (periodic(x + 0.015, y, 1.8, 37.) - periodic(x - 0.015, y, 1.8, 37.)) * 180.;
            let ny = (periodic(x, y + 0.015, 1.8, 37.) - periodic(x, y - 0.015, 1.8, 37.)) * 180.;
            maps.pixel(1, [(128. + nx) / 255., (128. + ny) / 255., 1.]);
        }
    }
    Ok(maps)
}

pub fn restaurant_tiles(source: &Image) -> Result<Maps, String> {
    let (width, height, density) = (1152, 576, 320.);
    let mut maps = Maps::new(width, height);
    let mut heights = Vec::with_capacity(width * height);
    let field = Tile::new(&Settings {
        seed: 317,
        share: 1.,
        wear: 0.42,
        grime: 0.95,
        size: Some(0.18),
        aspect: Some(1.),
    })?;
    let cream = srgb("#d8cfb9");
    let bed = srgb("#696453");
    let grime = srgb("#685039");
    for y in 0..height {
        for x in 0..width {
            let u = (x as f64 + 0.5) / density;
            let v = (height as f64 - y as f64 - 0.5) / density;
            let tile = field.channels(u, v, 1. / density)?;
            let tide = 0.11 + 0.07 * value_noise(u * 4., 0., 813.);
            let lower = 1. - smooth_step(tide, tide + 0.13, v);
            let mottling = smooth_step(0.46, 0.72, fractal_noise(u * 7., v * 9., 814., 4));
            let runs = smooth_step(0.58, 0.74, value_noise(u * 18., v * 0.8, 815.)) * 0.23;
            let stain = (lower * (0.3 + 0.28 * value_noise(u * 21., v * 18., 816.))
                + mottling * 0.22
                + runs)
                .min(0.78);
            let tone = 0.93 + 0.09 * value_noise((u / 0.18).floor(), (v / 0.18).floor(), 817.);
            let sx = ((u * 2.5).fract() * source.width as f64) as usize;
            let sy = ((1. - (v * 2.5).fract()) * source.height as f64) as usize % source.height;
            let offset = (sy * source.width + sx) * 4;
            maps.pixel(
                0,
                std::array::from_fn(|axis| {
                    let q = source.rgba[offset + axis] as f64 / 255.;
                    let linear = if q <= 0.04045 {
                        q / 12.92
                    } else {
                        ((q + 0.055) / 1.055).powf(2.4)
                    };
                    let ceramic =
                        cream[axis] * tone * (0.8 + 0.2 * linear) * tile.appearance.tint[axis];
                    let coated = mix(
                        ceramic,
                        grime[axis],
                        (tile.appearance.deposit + stain).min(0.85),
                    );
                    let plaster = bed[axis] * 0.82 * tile.bed.tone * (1. - tile.seam * 0.35);
                    let value = mix(plaster, coated, tile.coverage);
                    if value <= 0.0031308 {
                        value * 12.92
                    } else {
                        1.055 * value.powf(1. / 2.4) - 0.055
                    }
                }),
            );
            let roughness = 0.66 + 0.34 * 0.95 * 0.45;
            maps.pixel(
                2,
                [
                    1.,
                    mix(
                        0.96,
                        roughness + (1. - roughness) * tile.appearance.fade * 0.2,
                        tile.coverage,
                    ),
                    0.,
                ],
            );
            heights.push(mix(tile.bed.bump, tile.relief, tile.coverage));
        }
    }
    maps.normals(&heights, density, false);
    Ok(maps)
}
