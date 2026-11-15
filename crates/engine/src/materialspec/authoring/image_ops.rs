//! Image resampling and channel buffer operations for offline authoring.
use superspec_geometry::image::Image;

fn lanczos(x: f64) -> f64 {
    if x.abs() < 1e-10 {
        return 1.;
    }
    if x.abs() >= 3. {
        return 0.;
    }
    let p = x * std::f64::consts::PI;
    (p.sin() / p) * ((p / 3.).sin() / (p / 3.))
}

fn weights(input: usize, output: usize) -> Vec<Vec<(usize, f64)>> {
    let ratio = input as f64 / output as f64;
    let support = ratio.max(1.);
    (0..output)
        .map(|index| {
            let center = (index as f64 + 0.5) * ratio - 0.5;
            let low = ((center - 3. * support).ceil() as usize).min(input - 1);
            let high = ((center + 3. * support).floor() as usize).min(input - 1);
            let mut row: Vec<_> = (low..=high)
                .map(|i| (i, lanczos((i as f64 - center) / support)))
                .collect();
            let total = row.iter().map(|(_, weight)| weight).sum::<f64>();
            for (_, weight) in &mut row {
                *weight /= total;
            }
            row
        })
        .collect()
}

pub fn resize_to(source: &Image, width: usize, height: usize) -> Vec<u8> {
    let horizontal = weights(source.width, width);
    let vertical = weights(source.height, height);
    let mut intermediate = Vec::<[u8; 3]>::with_capacity(source.height * width);
    let byte = |value: f64| value.round().clamp(0., 255.) as u8;
    for y in 0..source.height {
        for weights in &horizontal {
            intermediate.push(std::array::from_fn(|axis| {
                byte(
                    weights
                        .iter()
                        .map(|&(x, w)| {
                            f64::from(source.rgba[(y * source.width + x) * 4 + axis]) * w
                        })
                        .sum(),
                )
            }));
        }
    }
    let mut output = Vec::with_capacity(width * height * 4);
    for weights in &vertical {
        for x in 0..width {
            let color: [u8; 3] = std::array::from_fn(|axis| {
                byte(
                    weights
                        .iter()
                        .map(|&(y, w)| f64::from(intermediate[y * width + x][axis]) * w)
                        .sum(),
                )
            });
            output.extend([color[0], color[1], color[2], 255]);
        }
    }
    output
}

pub fn solid(width: usize, height: usize, rgb: [u8; 3]) -> Image {
    Image {
        width,
        height,
        rgba: [rgb[0], rgb[1], rgb[2], 255].repeat(width * height),
        mime: "image/png",
    }
}

pub fn put(image: &mut Image, x: usize, y: usize, rgb: [u8; 3]) {
    let i = (y * image.width + x) * 4;
    image.rgba[i..i + 3].copy_from_slice(&rgb);
}

pub fn pixel(image: &Image, x: usize, y: usize) -> [u8; 3] {
    let i = (y * image.width + x) * 4;
    image.rgba[i..i + 3].try_into().unwrap()
}

pub fn crop(image: &Image, x: usize, y: usize, w: usize, h: usize) -> Image {
    let mut result = solid(w, h, [0; 3]);
    for row in 0..h {
        let start = ((y + row) * image.width + x) * 4;
        result.rgba[row * w * 4..(row + 1) * w * 4]
            .copy_from_slice(&image.rgba[start..start + w * 4]);
    }
    result
}

pub fn resize_max(source: &Image, edge: usize) -> Result<Image, String> {
    if !(1..=4096).contains(&edge) {
        return Err("Texture edge must be between 1 and 4096 pixels.".into());
    }
    let scale = edge as f64 / source.width.max(source.height) as f64;
    let width = (source.width as f64 * scale).round().max(1.) as usize;
    let height = (source.height as f64 * scale).round().max(1.) as usize;
    let output_image = Image {
        width,
        height,
        rgba: resize_to(&source, width, height),
        mime: "image/png",
    };
    Ok(output_image)
}
