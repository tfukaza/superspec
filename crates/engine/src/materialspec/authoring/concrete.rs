use super::{Maps, mix, smooth_step};
use crate::materialspec::surface::image_sample;
use superspec_geometry::image::Image;

fn hash([x, y]: [f64; 2]) -> [f64; 3] {
    [[127.1, 311.7], [269.5, 183.3], [419.2, 371.9]].map(|[a, b]| {
        let value = (x * a + y * b).sin() * 43758.5453;
        value - value.floor()
    })
}

fn noise([x, y]: [f64; 2]) -> f64 {
    let [ix, iy] = [x.floor(), y.floor()];
    let [fx, fy] = [x - ix, y - iy].map(|t| t * t * t * (t * (t * 6. - 15.) + 10.));
    let at = |x, y| hash([ix + x, iy + y])[0];
    mix(
        mix(at(0., 0.), at(1., 0.), fx),
        mix(at(0., 1.), at(1., 1.), fx),
        fy,
    )
}

fn fractal(mut p: [f64; 2]) -> f64 {
    let (mut result, mut total, mut weight) = (0., 0., 0.5);
    for _ in 0..3 {
        result += noise(p) * weight;
        total += weight;
        p = [
            (0.8 * p[0] + 0.6 * p[1]) * 2.03 + 13.7,
            (-0.6 * p[0] + 0.8 * p[1]) * 2.03 + 9.2,
        ];
        weight *= 0.28;
    }
    result / total
}

fn transform(p: [f64; 2], scale: f64, offset: [f64; 2]) -> [f64; 2] {
    [p[0] * scale + offset[0], p[1] * scale + offset[1]]
}

fn fracture(p: [f64; 2]) -> f64 {
    let warp =
        [[7.1, 31.8], [53.2, 9.4]].map(|offset| (fractal(transform(p, 2.3, offset)) - 0.5) * 0.144);
    let broad = fractal([p[0] + warp[0], p[1] + warp[1]]);
    let detail: f64 = [
        (13., 0., 0.20),
        (31., 17.3, 0.11),
        (73., 39.1, 0.06),
        (151., 81.7, 0.025),
    ]
    .into_iter()
    .map(|(scale, offset, weight)| (noise(transform(p, scale, [offset; 2])) - 0.5) * weight)
    .sum();
    broad + detail * 0.8 * (1. - smooth_step(0.045, 0.18, (broad - 0.5).abs()))
}

fn patches(image: &Image, mean: [f64; 3], uv: [f64; 2]) -> Result<[f64; 3], String> {
    let [x, y] = [uv[0] - uv[1] / 3_f64.sqrt(), uv[1] * 2. / 3_f64.sqrt()];
    let [ix, iy] = [x.floor(), y.floor()];
    let [fx, fy] = [x - ix, y - iy];
    let (cells, weights) = if fx + fy < 1. {
        (
            [[ix, iy], [ix + 1., iy], [ix, iy + 1.]],
            [1. - fx - fy, fx, fy],
        )
    } else {
        (
            [[ix + 1., iy + 1.], [ix, iy + 1.], [ix + 1., iy]],
            [fx + fy - 1., 1. - fx, 1. - fy],
        )
    };
    let weights = weights.map(|v| v.max(0.).powi(3));
    let sum: f64 = weights.iter().sum();
    let weights = weights.map(|v| v / sum);
    let deviation = weights.iter().map(|v| v * v).sum::<f64>().sqrt();
    let mut blended = [0.; 3];
    for ([x, y], weight) in cells.into_iter().zip(weights) {
        let [u, v, turn] = hash([x + 19., y + 47.]);
        let (sin, cos) = ((turn - 0.5) * 0.32).sin_cos();
        let color = image_sample(
            image,
            [
                cos * uv[0] + sin * uv[1] + u,
                -sin * uv[0] + cos * uv[1] + v,
            ],
            true,
        )?;
        for axis in 0..3 {
            blended[axis] += color[axis] * weight;
        }
    }
    Ok(std::array::from_fn(|axis| {
        (mean[axis] + (blended[axis] - mean[axis]) / deviation).clamp(0., 1.)
    }))
}

fn linear(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

pub fn bake(a: &Image, b: &Image) -> Result<Maps, String> {
    let size = 2048;
    let mut maps = Maps::new(size, size);
    let mean = |image: &Image| -> [f64; 3] {
        let mut total = [0.; 3];
        for pixel in image.rgba.chunks_exact(4) {
            for axis in 0..3 {
                total[axis] += linear(f64::from(pixel[axis]) / 255.);
            }
        }
        total.map(|v| v / (image.width * image.height) as f64)
    };
    let (mean_a, mean_b) = (mean(a), mean(b));
    let luma = |c: [f64; 3]| c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722;
    let hue = [0., -120., 120.].map(|h: f64| (h + 60.).to_radians().cos());
    let hue = hue.map(|h| h - luma(hue));
    let mut heights = Vec::with_capacity(size * size);
    for row in 0..size {
        for column in 0..size {
            let u = 4. * (column as f64 + 0.5) / size as f64;
            let v = 4. - 4. * (row as f64 + 0.5) / size as f64;
            let (wu, wv) = (smooth_step(0., 4., u), smooth_step(0., 4., v));
            let mut color = [0.; 3];
            let mut height = 0.;
            for (uv, weight) in [
                ([u, v], (1. - wu) * (1. - wv)),
                ([u - 4., v], wu * (1. - wv)),
                ([u, v - 4.], (1. - wu) * wv),
                ([u - 4., v - 4.], wu * wv),
            ] {
                let a = patches(a, mean_a, uv)?;
                let b = patches(b, mean_b, transform(uv, 1.13, [3.7, 11.2]))?;
                let p = [uv[0] * 0.85, uv[1] * 0.68];
                let edge = |p, width| smooth_step(0.5 - width, 0.5 + width, fracture(p));
                let dark_a = edge(transform(p, 5.4, [5.3, 17.1]), 0.035);
                let dark_b = edge(transform(p, 8.28, [73.4, 29.6]), 0.035);
                let blend = edge(transform(p, 9., [113.2, 67.8]), 0.045);
                let base = std::array::from_fn(|i| {
                    mix(
                        a[i] * (1. - 0.08 * dark_a),
                        b[i] * mix(1., mean_a[i] / mean_b[i].max(1e-9), 0.9) * (1. - 0.10 * dark_b),
                        blend,
                    )
                });
                let pigment =
                    (fractal(transform(p, 4.68, [41.2, 3.7])) * 2. - 0.7) * 0.05 * luma(base);
                for i in 0..3 {
                    color[i] += (base[i] + hue[i] * pigment).max(0.) * weight;
                }
                height += (fractal(transform(uv, 65., [0.; 2])) * 0.0006
                    + noise(transform(uv, 420., [0.; 2])) * 0.00018)
                    * weight;
            }
            maps.pixel(
                0,
                color.map(|v| {
                    if v <= 0.0031308 {
                        v * 12.92
                    } else {
                        1.055 * v.powf(1. / 2.4) - 0.055
                    }
                }),
            );
            maps.pixel(2, [1., 0.96, 0.]);
            heights.push(height);
        }
    }
    maps.normals(&heights, size as f64 / 8., true);
    Ok(maps)
}
