use super::{Maps, smooth_step, value_noise};
use superspec_geometry::image::Image;

fn periodic<const N: usize>(field: &mut [[f64; N]], width: usize, height: usize) {
    for vertical in [false, true] {
        let (length, lines) = if vertical {
            (height, width)
        } else {
            (width, height)
        };
        for along in 0..lines {
            let index = |across| {
                if vertical {
                    across * width + along
                } else {
                    along * width + across
                }
            };
            let first = field[index(0)];
            let last = field[index(length - 1)];
            for offset in 0..24 {
                let weight = (1. - smooth_step(0., 23., offset as f64)) * 0.5;
                for channel in 0..N {
                    let delta = (last[channel] - first[channel]) * weight;
                    field[index(offset)][channel] += delta;
                    field[index(length - 1 - offset)][channel] -= delta;
                }
            }
        }
    }
}

pub fn bake(source: &Image) -> Result<Maps, String> {
    if (source.width, source.height) != (1024, 1536) {
        return Err("Timber source must contain four 256 × 1536 scans.".into());
    }
    let mut maps = Maps::new(1024, 1792);
    for map in &mut maps.pixels {
        map.resize(1024 * 1792 * 4, 255);
    }
    for variant in 0..8 {
        let end = variant >= 4;
        let tile_height = if end { 256 } else { 1536 };
        let (width, height) = (248, tile_height - 8);
        let (left, top) = (variant % 4 * 256, if end { 1536 } else { 0 });
        let seed = 731. + variant as f64 * 193.;
        let mut field = Vec::with_capacity(width * height);
        for y in 0..height {
            for x in 0..width {
                let (u, v) = (x as f64 / 494., y as f64 / 494.);
                let pores = value_noise(u * 210., v * 210., seed + 32.);
                if end {
                    let warp = value_noise(u * 10., v * 10., seed) * 0.03;
                    let radius =
                        (u + 0.2 + (variant % 2) as f64 * 0.35).hypot((v - 0.25) * 0.8) + warp;
                    let ring = (0.5 + 0.5 * (radius * 240.).sin()).powi(9);
                    let [r, g, b] =
                        [0.37, 0.25, 0.14].map(|c| c * (1. - ring * 0.18 + (pores - 0.5) * 0.15));
                    field.push([
                        r,
                        g,
                        b,
                        ring * -0.00015 + pores * 0.00004,
                        0.87 + ring * 0.05,
                    ]);
                } else {
                    let offset = ((y + 4) * source.width + left + x + 4) * 4;
                    let pigment = |axis| f64::from(source.rgba[offset + axis]) / 255.;
                    let relief = (pores - 0.5) * 0.000025
                        + (value_noise(u * 170., v * 11., seed + 119.) - 0.5) * 0.00004;
                    field.push([
                        pigment(0),
                        pigment(1),
                        pigment(2),
                        relief,
                        0.77 + value_noise(u * 5., v * 4., seed + 71.) * 0.13,
                    ]);
                }
            }
        }
        periodic(&mut field, width, height);
        let neighbors = |index, extent| {
            if index == 0 || index == extent - 1 {
                (extent - 2, 1)
            } else {
                (index - 1, index + 1)
            }
        };
        for y in 0..tile_height {
            for x in 0_usize..256 {
                let sx = x.saturating_sub(4).min(width - 1);
                let sy = y.saturating_sub(4).min(height - 1);
                let [r, g, b, _, roughness] = field[sy * width + sx];
                let (before_x, after_x) = neighbors(sx, width);
                let (before_y, after_y) = neighbors(sy, height);
                let dx = (field[sy * width + after_x][3] - field[sy * width + before_x][3]) * 247.;
                let dy = (field[after_y * width + sx][3] - field[before_y * width + sx][3]) * 247.;
                let length = dx.hypot(dy).hypot(1.);
                let color = [r, g, b].map(|value| {
                    let linear = 2.
                        * if value <= 0.04045 {
                            value / 12.92
                        } else {
                            ((value + 0.055) / 1.055).powf(2.4)
                        };
                    if linear <= 0.0031308 {
                        linear * 12.92
                    } else {
                        1.055 * linear.powf(1. / 2.4) - 0.055
                    }
                });
                let offset = ((top + y) * maps.width + left + x) * 4;
                for (map, rgb) in maps.pixels.iter_mut().zip([
                    color,
                    [
                        0.5 - dx / length * 0.5,
                        0.5 + dy / length * 0.5,
                        0.5 + 1. / length * 0.5,
                    ],
                    [1., roughness, 0.],
                ]) {
                    map[offset..offset + 3]
                        .copy_from_slice(&rgb.map(|v| (v.clamp(0., 1.) * 255.).round() as u8));
                }
            }
        }
    }
    Ok(maps)
}
