use super::{Files, Inputs};
use super::{Image, atlas::resize_to, image};
use serde_json::{Value, json};
use std::path::Path;
use superspec_geometry::{V3, math::unit};
use superspec_types::rng::SeededRng;

type Face = [[f64; 8]; 3];
const PALETTE: [[u8; 3]; 8] = [
    [173, 42, 29],
    [105, 24, 43],
    [156, 65, 29],
    [179, 104, 29],
    [182, 153, 35],
    [36, 105, 62],
    [37, 64, 123],
    [31, 111, 111],
];
const SUFFIXES: [&str; 3] = ["", "-normal", "-properties"];

fn container(name: &str, size: V3) -> Result<Vec<Face>, String> {
    let (sides, profile): (usize, &[[f64; 2]]) = match name {
        "food-tin" => (12, &[[0., 0.5], [1., 0.5]]),
        "seasoning-jar" => (
            8,
            &[
                [0., 0.49],
                [0.72, 0.5],
                [0.78, 0.43],
                [0.80, 0.5],
                [1., 0.5],
            ],
        ),
        "drink-bottle" => (
            8,
            &[
                [0., 0.5],
                [0.58, 0.5],
                [0.64, 0.45],
                [0.71, 0.23],
                [0.76, 0.17],
                [0.95, 0.17],
                [1., 0.21],
            ],
        ),
        _ => (
            8,
            &[
                [0., 0.5],
                [0.60, 0.5],
                [0.73, 0.18],
                [0.95, 0.17],
                [1., 0.21],
            ],
        ),
    };
    let [width, depth, height] = size;
    let angle = |side| -std::f64::consts::PI + side as f64 * std::f64::consts::TAU / sides as f64;
    let point = |side, ring: usize| {
        let [z, r] = profile[ring];
        let (sin, cos) = angle(side).sin_cos();
        [width * r * sin, -depth * r * cos, height * z]
    };
    let mut faces = Vec::new();
    for ring in 0..profile.len() - 1 {
        let mut corners = [[0.; 8]; 4];
        for side in 0..sides {
            for (v, (i, row)) in corners.iter_mut().zip([
                (side, ring),
                (side + 1, ring),
                (side + 1, ring + 1),
                (side, ring + 1),
            ]) {
                let (sin, cos) = angle(i).sin_cos();
                let slope = if name == "drink-bottle" && ring < profile.len() - 2 {
                    if matches!(row, 2 | 3) {
                        (profile[row + 1][1] - profile[row - 1][1])
                            / (profile[row + 1][0] - profile[row - 1][0])
                    } else {
                        0.
                    }
                } else {
                    (profile[ring + 1][1] - profile[ring][1])
                        / (profile[ring + 1][0] - profile[ring][0])
                };
                v[..3].copy_from_slice(&point(i, row));
                v[3..6].copy_from_slice(&unit([sin / width, -cos / depth, -slope / height])?);
                v[6..].copy_from_slice(&[
                    0.02 + 0.96 * i as f64 / sides as f64,
                    0.30 + 0.68 * profile[row][0],
                ]);
            }
            faces.extend([
                [corners[0], corners[1], corners[2]],
                [corners[0], corners[2], corners[3]],
            ]);
        }
    }
    for (ring, center, sign) in [(0, 0.75, -1.), (profile.len() - 1, 0.25, 1.)] {
        let make = |side| {
            let [x, y, z] = point(side, ring);
            let (sin, cos) = angle(side).sin_cos();
            [
                x,
                y,
                z,
                0.,
                0.,
                sign,
                center + 0.11 * sin,
                0.15 - 0.11 * cos,
            ]
        };
        for i in 1..sides - 1 {
            faces.push(if sign > 0. {
                [make(0), make(i), make(i + 1)]
            } else {
                [make(0), make(i + 1), make(i)]
            });
        }
    }
    Ok(faces)
}

pub(super) use crate::materialspec::authoring::image_ops::{crop, pixel, put, solid};

fn recolor(image: &Image, color: [u8; 3], red: bool) -> Image {
    let (width, height) = if red { (256, 128) } else { (128, 128) };
    let mut result = Image {
        width,
        height,
        rgba: resize_to(image, width, height),
        mime: "image/png",
    };
    for pixel in result.rgba.chunks_exact_mut(4) {
        let (r, g, b) = (
            f64::from(pixel[0]),
            f64::from(pixel[1]),
            f64::from(pixel[2]),
        );
        let high = r.max(g).max(b);
        let low = r.min(g).min(b);
        if (red && r > g * 1.35 && r > b * 1.35)
            || (!red && high > 0. && (high - low) / high > 0.22 && low < 180.)
        {
            pixel[..3].copy_from_slice(&color.map(|c| {
                (f64::from(c) * (0.65 + 0.5 * high / 255.))
                    .round()
                    .min(255.) as u8
            }));
        }
    }
    result
}

fn rectangle(image: &mut Image, [left, top, right, bottom]: [usize; 4], color: [u8; 3]) {
    for y in top..bottom {
        for x in left..right {
            put(image, x, y, color);
        }
    }
}

fn maps(name: &str, variant: usize, labels: &Image, tin: &Image) -> [Image; 3] {
    let color = PALETTE[variant];
    let jar = name == "seasoning-jar";
    let canned = name == "food-tin";
    let base = if jar { [86, 47, 22] } else { [65, 33, 15] };
    let mut albedo = solid(256, 256, base);
    let mut props = solid(256, 256, [255, 100, 0]);
    let mut rng = SeededRng::from_u32(410);
    for p in albedo.rgba.chunks_exact_mut(4) {
        for c in &mut p[..3] {
            *c = i32::from(*c)
                .saturating_add(rng.integer(0, 6) as i32 - 3)
                .clamp(0, 255) as u8;
        }
    }
    let (art, x, y, w, h) = if canned {
        (recolor(tin, color, true), 5, 5, 246, 174)
    } else {
        let design = if jar {
            [7, 5, 3, 2, 6, 0, 1, 4][variant]
        } else {
            [5, 2, 0, 1, 7, 3, 4, 6][variant]
        };
        let mut image = recolor(
            &crop(
                labels,
                design % 4 * labels.width / 4,
                design / 4 * labels.height / 2,
                labels.width / 4,
                labels.height / 2,
            ),
            color,
            false,
        );
        rectangle(&mut image, [0, 0, 128, 9], color);
        rectangle(&mut image, [0, 119, 128, 128], color);
        rectangle(&mut image, [0, 0, 3, 128], color);
        rectangle(&mut image, [125, 0, 128, 128], color);
        let (z0, z1) = if jar { (0.12, 0.66) } else { (0.16, 0.56) };
        let row = |z: f64| ((1. - (0.30 + 0.68 * z)) * 256.).round() as usize;
        (image, 68, row(z1), 120, row(z0) - row(z1))
    };
    let resized = resize_to(&art, w, h);
    for j in 0..h {
        let target = ((y + j) * 256 + x) * 4;
        albedo.rgba[target..target + w * 4].copy_from_slice(&resized[j * w * 4..(j + 1) * w * 4]);
    }
    rectangle(&mut props, [x, y, x + w, y + h], [255, 210, 0]);
    if !canned {
        let cap = ((1_f64 - (0.30 + 0.68 * if jar { 0.80 } else { 0.95 })) * 256.).round() as usize;
        rectangle(&mut albedo, [5, 5, 251, cap + 1], color);
        rectangle(&mut props, [5, 5, 251, cap + 1], [255, 150, 180]);
    }
    for cx in [64_i32, 192] {
        let metal = if jar && cx == 64 {
            color
        } else {
            [135, 128, 106]
        };
        for row in 187..=248 {
            for x in cx - 30..=cx + 30 {
                let distance = ((x - cx) as f64).hypot((row - 218) as f64);
                if distance > 30. {
                    continue;
                }
                let (ink, material) = if cx == 192 && !canned {
                    (base, [255, 100, 0])
                } else {
                    let inset = [2., 5., 11.]
                        .into_iter()
                        .find(|v| (distance - (28. - v)).abs() <= 0.55);
                    (
                        inset.map_or(metal, |i| metal.map(|c| c.saturating_sub(18 - i as u8))),
                        [255, 165, 210],
                    )
                };
                put(&mut albedo, x as usize, row as usize, ink);
                put(&mut props, x as usize, row as usize, material);
            }
        }
    }
    for p in albedo.rgba.chunks_exact_mut(4) {
        let gray = f64::from(p[0]) * 0.299 + f64::from(p[1]) * 0.587 + f64::from(p[2]) * 0.114;
        for c in &mut p[..3] {
            *c = (gray * 0.42 + f64::from(*c) * 0.58).round() as u8;
        }
    }
    [albedo, solid(256, 256, [128, 128, 255]), props]
}

pub(super) fn region(value: &Value) -> Result<[f64; 4], String> {
    let rect: [f64; 4] = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
    if rect.iter().any(|x| !(0. ..=1.).contains(x)) || rect[0] >= rect[2] || rect[1] >= rect[3] {
        return Err("Invalid stock chart UVs.".into());
    }
    Ok(rect)
}

pub(super) fn paste(
    atlas: &mut Image,
    source: &Image,
    [u0, v0, u1, v1]: [f64; 4],
) -> Result<(), String> {
    let x = (u0 * atlas.width as f64).round_ties_even() as usize;
    let y = ((1. - v1) * atlas.height as f64).round_ties_even() as usize;
    let w = ((u1 - u0) * atlas.width as f64).round_ties_even() as usize;
    let h = ((v1 - v0) * atlas.height as f64).round_ties_even() as usize;
    if x < 4 || y < 4 || w == 0 || h == 0 || x + w + 4 > atlas.width || y + h + 4 > atlas.height {
        return Err("Clean stock chart exceeds padded atlas bounds.".into());
    }
    let pixels = resize_to(source, w, h);
    for ty in y - 4..y + h + 4 {
        for tx in x - 4..x + w + 4 {
            let (sx, sy) = (
                tx.saturating_sub(x).min(w - 1),
                ty.saturating_sub(y).min(h - 1),
            );
            let src = (sy * w + sx) * 4;
            let dst = (ty * atlas.width + tx) * 4;
            atlas.rgba[dst..dst + 4].copy_from_slice(&pixels[src..src + 4]);
        }
    }
    Ok(())
}

pub(super) fn sample(atlas: &Image, rect: [f64; 4], u: f64, v: f64) -> V3 {
    let [u0, v0, u1, v1] = rect;
    let x = (u0 * atlas.width as f64).round_ties_even() as usize;
    let y = ((1. - v1) * atlas.height as f64).round_ties_even() as usize;
    let w = ((u1 - u0) * atlas.width as f64).round_ties_even() as usize;
    let h = ((v1 - v0) * atlas.height as f64).round_ties_even() as usize;
    let dx = (u.clamp(0., 1.) * (w - 1) as f64).round_ties_even() as usize;
    let dy = ((1. - v.clamp(0., 1.)) * (h - 1) as f64).round_ties_even() as usize;
    pixel(atlas, x + dx, y + dy).map(|c| {
        let v = f64::from(c) / 255.;
        let value = if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        };
        (value * 1e6).round_ties_even() / 1e6
    })
}

pub fn bake(inputs: &Inputs<'_>, root: &Path, textures: &Path) -> Result<Files, String> {
    let materials = root.join("projects/kowloon-liquor-props/materials");
    let labels = inputs.decode(&materials.join("packaging-labels.png"))?;
    let tin = inputs.decode(&materials.join("clean-stock/tin-label-source.png"))?;
    if labels.width < 4 || labels.height < 2 {
        return Err("Packaging labels must contain a four-by-two sheet.".into());
    }
    let read = |name: &str| -> Result<Value, String> {
        serde_json::from_slice(&(inputs.catalog)(name)?).map_err(|e| e.to_string())
    };
    let mut catalog = read("grocer-props.json")?;
    let mut appear = read("stock-appearances.json")?;
    let mut atlases = [
        inputs.decode(&textures.join("grocer-props.png"))?,
        inputs.decode(&textures.join("grocer-props-normal.png"))?,
        inputs.decode(&textures.join("grocer-props-properties.png"))?,
    ];
    if atlases.iter().any(|a| (a.width, a.height) != (2560, 2048)) {
        return Err("Expected three 2560 × 2048 stock atlases.".into());
    }
    let mut files = Vec::new();
    for (name, size) in [
        ("food-tin", [0.075633, 0.07539, 0.115]),
        ("seasoning-jar", [0.14, 0.139378, 0.15346]),
        ("drink-bottle", [0.07, 0.068556, 0.268747]),
        ("bottle-load", [0.291842, 0.216794, 0.27]),
    ] {
        let source = region(&appear[name]["source"])?;
        let variants = appear[name]["variants"]
            .as_array_mut()
            .ok_or("Missing clean stock appearance variants.")?;
        let faces = if name == "bottle-load" {
            let body = container(name, [size[0] - 0.222, size[1] - 0.148, 0.27])?;
            let mut load = Vec::new();
            for row in 0..3 {
                for column in 0..4 {
                    for face in &body {
                        if face.iter().all(|v| v[2] == 0.) {
                            continue;
                        }
                        load.push(face.map(|mut v| {
                            v[0] += (column as f64 - 1.5) * 0.074;
                            v[1] += (row as f64 - 1.) * 0.074;
                            v
                        }));
                    }
                }
            }
            load
        } else {
            container(name, size)?
        };
        if name != "bottle-load" {
            for (index, rect) in std::iter::once(Ok(source))
                .chain(variants.iter().map(|v| region(&v["uv"])))
                .enumerate()
            {
                let rect = rect?;
                let palette = if index == 0 {
                    0
                } else if name == "food-tin" {
                    *[0, 4, 5, 6]
                        .get(index - 1)
                        .ok_or("Too many tin variants.")?
                } else {
                    index - 1
                };
                if palette >= PALETTE.len() {
                    return Err("Too many clean stock variants.".into());
                }
                for (i, map) in maps(name, palette, &labels, &tin).into_iter().enumerate() {
                    paste(&mut atlases[i], &map, rect)?;
                    if index == 0 {
                        files.push((
                            format!("{name}-{}-256.png", ["color", "normal", "properties"][i]),
                            image::encode(&map)?,
                        ));
                    }
                }
            }
        }
        let atlas = &atlases[0];
        for variant in variants {
            let rect = region(&variant["uv"])?;
            variant["colors"] = json!(
                faces
                    .iter()
                    .map(|face| face.map(|v| sample(atlas, rect, v[6], v[7])))
                    .collect::<Vec<_>>()
            );
        }
        let faces = faces
            .into_iter()
            .map(|face| {
                face.map(|v| {
                    let uv = [
                        source[0] + v[6] * (source[2] - source[0]),
                        source[1] + v[7] * (source[3] - source[1]),
                    ];
                    v[..6]
                        .iter()
                        .copied()
                        .chain(uv)
                        .map(|v| (v * 1e8).round_ties_even() / 1e8)
                        .chain(sample(atlas, source, v[6], v[7]))
                        .collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>();
        catalog[name] = json!({"size":size,"faces":faces});
    }
    for (suffix, atlas) in SUFFIXES.into_iter().zip(atlases) {
        files.push((format!("grocer-props{suffix}.png"), image::encode(&atlas)?));
    }
    files.push((
        "grocer-props.json".into(),
        serde_json::to_vec(&catalog).map_err(|e| e.to_string())?,
    ));
    files.push((
        "stock-appearances.json".into(),
        serde_json::to_vec(&appear).map_err(|e| e.to_string())?,
    ));
    Ok(files
        .into_iter()
        .map(|(name, bytes)| (name.to_owned(), bytes))
        .collect())
}
