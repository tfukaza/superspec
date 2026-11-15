use super::{Files, Inputs};
use super::{
    Image,
    atlas::resize_to,
    clean_stock::{crop, paste, pixel, put, sample, solid},
    image,
};
use serde_json::{Value, json};
use std::path::Path;

const SIZE: usize = 248;
type Face = Vec<[f64; 11]>;
const SUFFIXES: [&str; 3] = ["", "-normal", "-properties"];

fn rect(x: usize, y: usize, w: usize, h: usize) -> [f64; 4] {
    [
        x as f64 / 2560.,
        1. - (y + h) as f64 / 2048.,
        (x + w) as f64 / 2560.,
        1. - y as f64 / 2048.,
    ]
}

fn recolor(image: &mut Image, index: usize, name: &str) {
    let warm = matches!(name, "seasoning-jar" | "drink-bottle");
    let hues = if warm {
        [0.008, 0.975, 0.045, 0.085, 0.13, 0.37, 0.61, 0.49]
    } else {
        [0.38, 0.61, 0.13, 0.075, 0.97, 0.48, 0.08, 0.60]
    };
    let hue: f64 = hues[index] * 6.;
    let fraction = hue.fract();
    for pixel in image.rgba.chunks_exact_mut(4) {
        let low = f64::from(*pixel[..3].iter().min().unwrap()) / 255.;
        let high = f64::from(*pixel[..3].iter().max().unwrap()) / 255.;
        if high == 0. {
            continue;
        }
        let saturation = (high - low) / high;
        if saturation <= 0.22 {
            continue;
        }
        let mut saturation = if warm {
            (saturation * 0.98).min(0.88)
        } else if matches!(index, 3 | 7) {
            saturation * 0.12
        } else {
            (saturation * 0.82).min(0.72)
        };
        let mut value = if warm {
            (high * [1.10, 0.72, 0.85, 1.02, 1.08, 0.88, 0.86, 0.88][index]).min(1.)
        } else {
            (high * if index == 3 { 1.16 } else { 1.02 }).min(1.)
        };
        if !warm && index == 4 {
            value *= 0.7;
        }
        if !warm && index == 7 {
            value *= 0.5;
        }
        if name == "bottle-crate" {
            saturation *= 0.6;
            value *= 0.76;
        }
        let (p, q, t) = (
            value * (1. - saturation),
            value * (1. - saturation * fraction),
            value * (1. - saturation * (1. - fraction)),
        );
        let rgb = match hue as usize {
            0 => [value, t, p],
            1 => [q, value, p],
            2 => [p, value, t],
            3 => [p, q, value],
            4 => [t, p, value],
            _ => [value, p, q],
        };
        pixel[..3].copy_from_slice(&rgb.map(|v| (v * 255.).round_ties_even() as u8));
    }
}

fn projection(
    faces: &[Face],
    source: [f64; 4],
    size: [f64; 3],
    band: [f64; 2],
) -> Vec<Option<[usize; 2]>> {
    let mut result = vec![None; SIZE * SIZE];
    for face in faces {
        if face.len() != 3 || face.iter().map(|v| v[4]).sum::<f64>() > -0.6 {
            continue;
        }
        let uv = |v: &[f64; 11]| {
            [
                (v[6] - source[0]) / (source[2] - source[0]) * (SIZE - 1) as f64,
                (1. - (v[7] - source[1]) / (source[3] - source[1])) * (SIZE - 1) as f64,
            ]
        };
        let [[ax, ay], [bx, by], [cx, cy]] = [uv(&face[0]), uv(&face[1]), uv(&face[2])];
        let denominator = (by - cy) * (ax - cx) + (cx - bx) * (ay - cy);
        if denominator.abs() < 1e-8 {
            continue;
        }
        let pixels = |a: f64, b: f64, c: f64| {
            let low = a.min(b).min(c).floor() as usize;
            let high = (a.max(b).max(c).ceil() as usize)
                .saturating_add(1)
                .min(SIZE);
            low..high
        };
        for y in pixels(ay, by, cy) {
            for x in pixels(ax, bx, cx) {
                let (dx, dy) = (x as f64 - cx, y as f64 - cy);
                let a = ((by - cy) * dx + (cx - bx) * dy) / denominator;
                let b = ((cy - ay) * dx + (ax - cx) * dy) / denominator;
                let c = 1. - a - b;
                if a.min(b).min(c) < -1e-6 {
                    continue;
                }
                let p = |axis| a * face[0][axis] + b * face[1][axis] + c * face[2][axis];
                let u = 0.5 + p(0) / (size[0] * 0.86);
                let v = (p(2) / size[2] - band[0]) / (band[1] - band[0]);
                if (0. ..=1.).contains(&u) && (0. ..=1.).contains(&v) {
                    result[y * SIZE + x] = Some([
                        (u * 255.).round_ties_even() as usize,
                        ((1. - v) * 255.).round_ties_even() as usize,
                    ]);
                }
            }
        }
    }
    result
}

fn colors(faces: &[Face], atlas: &Image, source: [f64; 4], destination: [f64; 4]) -> Value {
    json!(
        faces
            .iter()
            .map(|f| f
                .iter()
                .map(|v| sample(
                    atlas,
                    destination,
                    (v[6] - source[0]) / (source[2] - source[0]),
                    (v[7] - source[1]) / (source[3] - source[1])
                ))
                .collect::<Vec<_>>())
            .collect::<Vec<_>>()
    )
}

fn sized(image: &Image, w: usize, h: usize) -> Image {
    Image {
        width: w,
        height: h,
        rgba: resize_to(image, w, h),
        mime: "image/png",
    }
}

pub fn bake(
    inputs: &Inputs<'_>,
    root: &Path,
    textures: &Path,
    pouches_only: bool,
) -> Result<Files, String> {
    let materials = root.join("projects/kowloon-liquor-props/materials");
    let art = inputs.decode(&materials.join("packaging-labels.png"))?;
    let labels: Vec<_> = (0..8)
        .map(|i| {
            sized(
                &crop(
                    &art,
                    i % 4 * art.width / 4,
                    i / 4 * art.height / 2,
                    art.width / 4,
                    art.height / 2,
                ),
                256,
                256,
            )
        })
        .collect();
    let base = sized(
        &inputs.decode(&materials.join("snack-packet-base.png"))?,
        512,
        512,
    );
    let mut pouches = solid(1024, 1024, [160, 150, 130]);
    for (index, design) in [2, 4, 0, 7].into_iter().enumerate() {
        let mut variant = sized(&base, 512, 512);
        recolor(&mut variant, index, "");
        let label = sized(&labels[design], 350, 350);
        for y in 0..350 {
            for x in 0..350 {
                put(&mut variant, x + 81, y + 81, pixel(&label, x, y));
            }
        }
        let (x, y) = (index % 2 * 512 + 4, index / 2 * 512 + 4);
        paste(
            &mut pouches,
            &variant,
            [
                x as f64 / 1024.,
                1. - (y + 504) as f64 / 1024.,
                (x + 504) as f64 / 1024.,
                1. - y as f64 / 1024.,
            ],
        )?;
    }
    let mut files = vec![("snack-packet.png".to_owned(), image::encode(&pouches)?)];
    if !pouches_only {
        let mut catalog: Value = serde_json::from_slice(&(inputs.catalog)("grocer-props.json")?)
            .map_err(|e| e.to_string())?;
        let mut metadata: Value = json!({});
        let mut atlases = [
            inputs.decode(&textures.join("grocer-props.png"))?,
            inputs.decode(&textures.join("grocer-props-normal.png"))?,
            inputs.decode(&textures.join("grocer-props-properties.png"))?,
        ];
        if atlases.iter().any(|a| (a.width, a.height) != (2560, 2048)) {
            return Err("Expected three 2560 × 2048 stock atlases.".into());
        }
        let specs: [(&str, usize, usize, Option<[f64; 2]>, &[usize]); 6] = [
            (
                "seasoning-jar",
                12,
                13,
                Some([0.18, 0.68]),
                &[7, 5, 3, 2, 6, 0, 1, 4],
            ),
            ("food-tin", 6, 14, Some([0.1, 0.88]), &[6, 1, 2, 7]),
            ("tea-carton", 7, 15, Some([0.06, 0.94]), &[0, 4, 2, 5]),
            (
                "drink-bottle",
                5,
                16,
                Some([0.16, 0.55]),
                &[7, 5, 3, 2, 6, 0, 1, 4],
            ),
            ("rice-sack", 0, 17, Some([0.22, 0.65]), &[6, 4, 2, 5]),
            ("bottle-crate", 4, 18, None, &[0, 1, 2, 5]),
        ];
        for (name, tile, dest, band, designs) in specs {
            let asset = &catalog[name];
            let faces: Vec<Face> =
                serde_json::from_value(asset["faces"].clone()).map_err(|e| e.to_string())?;
            let size: [f64; 3] =
                serde_json::from_value(asset["size"].clone()).map_err(|e| e.to_string())?;
            let (x, y) = (tile % 5 * 512 + 4, tile / 5 * 512 + 4);
            let source = rect(x, y, 504, 504);
            let originals = atlases
                .each_ref()
                .map(|a| sized(&crop(a, x, y, 504, 504), SIZE, SIZE));
            let projection = band.map(|band| projection(&faces, source, size, band));
            let mut variants = Vec::new();
            let mut regions = Vec::new();
            for (index, &design) in designs.iter().enumerate() {
                let mut color = sized(&originals[0], SIZE, SIZE);
                if name != "rice-sack" {
                    recolor(&mut color, index, name);
                }
                let mut label = sized(&labels[design], 256, 256);
                if matches!(name, "seasoning-jar" | "drink-bottle") {
                    recolor(&mut label, index, name);
                    let ink = [
                        [173, 42, 29],
                        [105, 24, 43],
                        [156, 65, 29],
                        [179, 104, 29],
                        [182, 153, 35],
                        [36, 105, 62],
                        [37, 64, 123],
                        [31, 111, 111],
                    ][index];
                    for y in 0..256 {
                        for x in 0..256 {
                            if !(6..250).contains(&x) || !(18..238).contains(&y) {
                                put(&mut label, x, y, ink);
                            }
                        }
                    }
                }
                if let Some(project) = &projection {
                    for (point, sample) in project.iter().enumerate() {
                        if let Some([u, v]) = sample {
                            let (x, y) = (point % SIZE, point / SIZE);
                            let brightness =
                                f64::from(*pixel(&originals[0], x, y).iter().max().unwrap()) / 255.;
                            put(
                                &mut color,
                                x,
                                y,
                                pixel(&label, *u, *v).map(|v| {
                                    (f64::from(v) * (0.82 + 0.18 * brightness)).round_ties_even()
                                        as u8
                                }),
                            );
                        }
                    }
                }
                let columns = if designs.len() == 8 { 4 } else { 2 };
                let width = 512 / columns;
                let uv = rect(
                    dest % 5 * 512 + index % columns * width + 4,
                    dest / 5 * 512 + index / columns * 256 + 4,
                    width - 8,
                    SIZE,
                );
                for (atlas, image) in atlases
                    .iter_mut()
                    .zip([&color, &originals[1], &originals[2]])
                {
                    paste(atlas, image, uv)?;
                }
                variants.push(json!({"uv":uv,"colors":colors(&faces,&atlases[0],source,uv)}));
                regions.push(uv);
            }
            metadata[name] = json!({"source":source,"variants":variants});
            let alias = match name {
                "rice-sack" => Some("dry-goods-bag"),
                "drink-bottle" => Some("bottle-load"),
                _ => None,
            };
            if let Some(alias) = alias.filter(|a| catalog.get(*a).is_some()) {
                let faces: Vec<Face> = serde_json::from_value(catalog[alias]["faces"].clone())
                    .map_err(|e| e.to_string())?;
                metadata[alias] = json!({"source":source,"variants":regions.into_iter().map(|uv|json!({"uv":uv,"colors":colors(&faces,&atlases[0],source,uv)})).collect::<Vec<_>>()});
            }
        }
        let source = rect(2052, 1540, SIZE, SIZE);
        let box_faces = box_shell(source);
        let mut variants = Vec::new();
        for index in 0..4 {
            let uv = rect(2052 + index % 2 * 256, 1540 + index / 2 * 256, SIZE, SIZE);
            variants.push(json!({"uv":uv,"colors":colors(&box_faces,&atlases[0],source,uv)}));
        }
        catalog["stock-box"] = json!({"size":[0.3,0.2,0.18],"faces":box_faces});
        metadata["stock-box"] = json!({"source":source,"variants":variants});
        for (atlas, suffix) in atlases.into_iter().zip(SUFFIXES) {
            files.push((format!("grocer-props{suffix}.png"), image::encode(&atlas)?));
        }
        files.push((
            "grocer-props.json".into(),
            serde_json::to_vec(&catalog).map_err(|e| e.to_string())?,
        ));
        files.push((
            "stock-appearances.json".into(),
            serde_json::to_vec(&metadata).map_err(|e| e.to_string())?,
        ));
    }
    Ok(files
        .into_iter()
        .map(|(name, bytes)| (name.to_owned(), bytes))
        .collect())
}

fn box_shell(source: [f64; 4]) -> Vec<Face> {
    let points = [
        [-0.15, -0.1, 0.],
        [0.15, -0.1, 0.],
        [0.15, 0.1, 0.],
        [-0.15, 0.1, 0.],
        [-0.15, -0.1, 0.18],
        [0.15, -0.1, 0.18],
        [0.15, 0.1, 0.18],
        [-0.15, 0.1, 0.18],
    ];
    let mut faces = Vec::new();
    for (indices, [nx, ny, nz]) in [
        ([0, 4, 7, 3], [-1., 0., 0.]),
        ([1, 2, 6, 5], [1., 0., 0.]),
        ([0, 1, 5, 4], [0., -1., 0.]),
        ([3, 7, 6, 2], [0., 1., 0.]),
        ([0, 3, 2, 1], [0., 0., -1.]),
        ([4, 5, 6, 7], [0., 0., 1.]),
    ] {
        let corners: [[f64; 11]; 4] = std::array::from_fn(|i| {
            let [x, y, z] = points[indices[i]];
            let [mut u, mut v] = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]][i];
            if nz == 0. {
                u = if nx != 0. {
                    (y + 0.1) / 0.2
                } else {
                    (x + 0.15) / 0.3
                };
                if nx < 0. || ny > 0. {
                    u = 1. - u;
                }
                v = z / 0.18;
            }
            [
                x,
                y,
                z,
                nx,
                ny,
                nz,
                source[0] + u * (source[2] - source[0]),
                source[1] + v * (source[3] - source[1]),
                0.5,
                0.4,
                0.25,
            ]
        });
        faces.extend([
            vec![corners[0], corners[1], corners[2]],
            vec![corners[0], corners[2], corners[3]],
        ]);
    }
    faces
}
