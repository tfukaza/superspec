//! Derives appearance channels from a source photograph.
use super::image_ops::{put, resize_to, solid};
use superspec_geometry::image::Image;

pub fn maps(source: &Image, roughness: f64) -> Result<[Image; 3], String> {
    let color = Image {
        width: 504,
        height: 504,
        rgba: resize_to(&source, 504, 504),
        mime: "image/png",
    };
    let mut normal = solid(504, 504, [128, 128, 255]);
    let mut properties = solid(504, 504, [255, 220, 0]);
    let luminance: Vec<_> = color
        .rgba
        .chunks_exact(4)
        .map(|p| {
            (0.299 * f64::from(p[0]) + 0.587 * f64::from(p[1]) + 0.114 * f64::from(p[2])).round()
        })
        .collect();
    let weights = [
        (-2, 0.003866),
        (-1, 0.249352),
        (0, 1.),
        (1, 0.249352),
        (2, 0.003866),
    ];
    let blur = |samples: &[f64], horizontal| {
        (0..samples.len())
            .map(|index| {
                let x = (index % 504) as i32;
                let y = (index / 504) as i32;
                weights
                    .iter()
                    .map(|&(offset, weight)| {
                        let (sx, sy) = if horizontal {
                            ((x + offset).clamp(0, 503), y)
                        } else {
                            (x, (y + offset).clamp(0, 503))
                        };
                        samples[(sy * 504 + sx) as usize] * weight
                    })
                    .sum::<f64>()
                    / 1.506436
            })
            .collect::<Vec<_>>()
    };
    let height = blur(&blur(&luminance, true), false);
    let at = |x: usize, y: usize| height[y * 504 + x];
    for y in 0..504 {
        for x in 0..504 {
            let dx = (at((x + 1) % 504, y) - at((x + 503) % 504, y)) * 0.002;
            let dy = (at(x, (y + 1) % 504) - at(x, (y + 503) % 504)) * 0.002;
            let length = dx.hypot(dy).hypot(1.);
            put(
                &mut normal,
                x,
                y,
                [
                    127.5 - 127.5 * dx / length,
                    127.5 + 127.5 * dy / length,
                    127.5 + 127.5 / length,
                ]
                .map(|v| v.round_ties_even() as u8),
            );
            put(
                &mut properties,
                x,
                y,
                [
                    255,
                    (roughness + (at(x, y) - 110.) * 0.18)
                        .round_ties_even()
                        .clamp(0., 255.) as u8,
                    0,
                ],
            );
        }
    }
    Ok([color, normal, properties])
}
