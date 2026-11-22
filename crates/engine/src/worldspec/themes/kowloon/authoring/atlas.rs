//! Packs source maps and fallback vertex colors from offline mesh bakes.
use super::{Files, Image, Inputs, image};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

const CHANNELS: [&str; 3] = ["color", "normal", "properties"];
const SUFFIXES: [&str; 3] = ["", "-normal", "-properties"];
const CELL: usize = 512;

struct Atlas {
    maps: [Image; 3],
    columns: usize,
    rows: usize,
    gutter: usize,
}

fn linear(value: u8) -> f64 {
    let value = f64::from(value) / 255.;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn finish_roast_atlas(maps: &mut [Image; 3]) {
    let [color, relief, properties] = maps;
    let width = color.width;
    for (index, ((pigment, normal), property)) in color
        .rgba
        .chunks_exact_mut(4)
        .zip(relief.rgba.chunks_exact_mut(4))
        .zip(properties.rgba.chunks_exact_mut(4))
        .enumerate()
    {
        let [red, green, blue] = [pigment[0], pigment[1], pigment[2]].map(u32::from);
        let breast_skin = index % width < CELL || index / width < CELL;
        let polished = if breast_skin {
            (red.saturating_sub(118) * green.saturating_sub(75) / 93).min(43)
        } else {
            0
        };
        pigment[0] = (red * 109 / 100 + (255 - red) * green / 1800 + polished / 5).min(255) as u8;
        pigment[1] = (green * 110 / 100 + red / 13 + polished / 6).min(255) as u8;
        pigment[2] = (blue * 106 / 100 + red / 10).min(255) as u8;
        // Reduce scan noise while letting the upper ridges in the caramelized
        // skin produce narrower reflections. Dry sliced meat stays satin.
        for channel in &mut normal[..2] {
            *channel = (128 + (i32::from(*channel) - 128) * 61 / 100) as u8;
        }
        normal[2] = ((u32::from(normal[2]) * 61 + 255 * 39) / 100) as u8;
        property[1] = (u32::from(property[1]) + 36 - red / 25)
            .saturating_sub(polished)
            .min(255) as u8;
    }
}

impl Atlas {
    fn new(columns: usize, rows: usize, gutter: usize, fill: [[u8; 4]; 3]) -> Self {
        let (width, height) = (columns * CELL, rows * CELL);
        Self {
            maps: fill.map(|pixel| Image {
                width,
                height,
                rgba: pixel.repeat(width * height),
                mime: "image/png",
            }),
            columns,
            rows,
            gutter,
        }
    }

    fn map(
        &mut self,
        channel: usize,
        tile: usize,
        source: &Image,
        transform: impl Fn([u8; 4]) -> [u8; 4],
    ) -> Result<(), String> {
        if tile >= self.columns * self.rows {
            return Err("Atlas tile is outside its layout.".into());
        }
        let inner = CELL - 2 * self.gutter;
        if source.height.saturating_mul(inner) > 16 * 1024 * 1024 {
            return Err("Atlas resizing exceeds its intermediate pixel budget.".into());
        }
        let resized = resize(source, inner);
        let map = &mut self.maps[channel];
        let (left, top) = (tile % self.columns * CELL, tile / self.columns * CELL);
        for y in 0..CELL {
            for x in 0..CELL {
                let sx = x.saturating_sub(self.gutter).min(inner - 1);
                let sy = y.saturating_sub(self.gutter).min(inner - 1);
                let offset = (sy * inner + sx) * 4;
                let pixel = transform(resized[offset..offset + 4].try_into().unwrap());
                let offset = ((top + y) * map.width + left + x) * 4;
                map.rgba[offset..offset + 4].copy_from_slice(&pixel);
            }
        }
        Ok(())
    }

    fn encode(self, name: &str, files: &mut Vec<(String, Vec<u8>)>) -> Result<(), String> {
        for (suffix, map) in SUFFIXES.into_iter().zip(self.maps) {
            files.push((format!("{name}{suffix}.png"), image::encode(&map)?));
        }
        Ok(())
    }
}

pub(super) use crate::materialspec::authoring::image_ops::resize_to;

fn resize(source: &Image, size: usize) -> Vec<u8> {
    resize_to(source, size, size)
}

fn color_at(image: &Image, [u, v]: [f64; 2]) -> [f64; 3] {
    let x = ((u * (image.width - 1) as f64).round_ties_even() as usize).min(image.width - 1);
    let y =
        (((1. - v) * (image.height - 1) as f64).round_ties_even() as usize).min(image.height - 1);
    std::array::from_fn(|axis| linear(image.rgba[(y * image.width + x) * 4 + axis]))
}

fn vertices(asset: &mut Value) -> Result<impl Iterator<Item = &mut Value>, String> {
    let faces = asset["faces"]
        .as_array_mut()
        .ok_or("Atlas asset has no face array.")?;
    if faces.is_empty()
        || faces
            .iter()
            .any(|face| face.as_array().is_none_or(|v| v.len() < 3))
    {
        return Err("Atlas asset must contain complete geometry.".into());
    }
    Ok(faces
        .iter_mut()
        .flat_map(|face| face.as_array_mut().unwrap()))
}

fn image_name(source: &Path, stem: &str, channel: &str) -> Result<std::path::PathBuf, String> {
    Ok(source.join(crate::itemspec::embedded_images::image_name(stem, channel)?))
}

fn remap_food(asset: &mut Value, image: &Image, tile: usize) -> Result<(), String> {
    for vertex in vertices(asset)? {
        let values: [f64; 8] = serde_json::from_value(vertex.clone()).map_err(|e| e.to_string())?;
        let [u, v] = [
            (tile % 2 * CELL + 4) as f64 + values[6] * 504.,
            (tile / 2 * CELL + 4) as f64 + (1. - values[7]) * 504.,
        ];
        let uv = [u / 1024., 1. - v / 1024.];
        let round = |v: f64| (v * 1e7).round_ties_even() / 1e7;
        *vertex = json!(
            values[..6]
                .iter()
                .copied()
                .chain(uv)
                .chain(color_at(image, uv))
                .map(round)
                .collect::<Vec<_>>()
        );
    }
    Ok(())
}

fn prepare_balcony_mesh(name: &str, asset: &mut Value) -> Result<(), String> {
    use superspec_geometry::math::unit;
    let mut faces: Vec<[[f64; 8]; 3]> =
        serde_json::from_value(asset["faces"].take()).map_err(|e| e.to_string())?;
    if faces.is_empty() {
        return Err(format!("Balcony mesh {name} has no triangles."));
    }
    let pot_height = match name {
        "rubber-plant" => Some(0.17),
        "trailing-pothos" => Some(0.20),
        "snake-plant" => Some(0.19),
        _ => None,
    };
    if let Some(top) = pot_height {
        for vertex in faces.iter_mut().flatten().filter(|v| v[2] > top) {
            for axis in 0..3 {
                vertex[axis] += 0.0007 * vertex[axis + 3];
            }
        }
        let base = faces.iter().flatten().filter(|v| v[2] < 0.025);
        let mut origin = [0.; 3];
        for axis in 0..2 {
            let lo = base.clone().map(|v| v[axis]).fold(f64::INFINITY, f64::min);
            let hi = base
                .clone()
                .map(|v| v[axis])
                .fold(f64::NEG_INFINITY, f64::max);
            if !lo.is_finite() {
                return Err(format!("Balcony plant {name} has no pot base."));
            }
            origin[axis] = (lo + hi) / 2.;
        }
        origin[2] = faces
            .iter()
            .flatten()
            .map(|v| v[2])
            .fold(f64::INFINITY, f64::min);
        for vertex in faces.iter_mut().flatten() {
            for (value, offset) in vertex.iter_mut().zip(origin) {
                *value -= offset;
            }
        }
    } else {
        let depth = asset["sourceDepth"]
            .as_f64()
            .filter(|&depth| depth > 0.)
            .ok_or("Missing source fabric depth.")?;
        let factor = (0.048 / depth).min(1.);
        for vertex in faces.iter_mut().flatten() {
            vertex[1] *= factor;
            let normal = unit([vertex[3], vertex[4] / factor, vertex[5]])?;
            vertex[3..6].copy_from_slice(&normal);
        }
    }
    asset["faces"] = json!(faces);
    Ok(())
}

pub fn balcony(inputs: &Inputs<'_>, source: &Path, cotton: &Image) -> Result<Files, String> {
    use superspec_geometry::math::{cross, dot, sub, unit};
    let mut catalog: BTreeMap<String, Value> =
        serde_json::from_slice(&(inputs.read)(&source.join("catalog.json"))?)
            .map_err(|e| e.to_string())?;
    let groups = [
        ("plants", ["rubber-plant", "trailing-pothos", "snake-plant"]),
        (
            "cloth",
            ["hanging-shirt", "hanging-trousers", "hanging-bedsheet"],
        ),
    ];
    if catalog.len() != 6
        || groups
            .iter()
            .flat_map(|(_, names)| names)
            .any(|name| !catalog.contains_key(*name))
    {
        return Err("Balcony atlas requires all six source meshes.".into());
    }
    for (name, asset) in &mut catalog {
        if asset["sourceDepth"].is_number() {
            prepare_balcony_mesh(name, asset)?;
            asset.as_object_mut().unwrap().remove("sourceDepth");
        }
    }
    let map_uv = |tile, [u, v]: [f64; 2]| {
        [
            ((tile % 2 * CELL) as f64 + 8.5 + u * 495.) / 1024.,
            1. - ((tile / 2 * CELL) as f64 + 8.5 + (1. - v) * 495.) / 1024.,
        ]
    };
    let mut files = Vec::new();
    for (group, names) in groups {
        let cloth = group == "cloth";
        let mut atlas = Atlas::new(
            2,
            2,
            8,
            [
                [160, 150, 135, 255],
                [128, 128, 255, 255],
                [255, 200, 0, 255],
            ],
        );
        for (tile, name) in names.into_iter().enumerate() {
            for (channel, kind) in CHANNELS.into_iter().enumerate() {
                let original = inputs.decode(&image_name(source, name, kind)?)?;
                let input = if channel == 0 && matches!(name, "hanging-shirt" | "hanging-bedsheet")
                {
                    cotton
                } else {
                    &original
                };
                atlas.map(channel, tile, input, |[r, g, b, _]| match (channel, name) {
                    (1, "hanging-bedsheet") => [128, 128, 255, 255],
                    (2, _) => [255, g.max(if cloth { 185 } else { 115 }), 0, 255],
                    _ => [r, g, b, 255],
                })?;
            }
        }
        let solid = |rgba: [u8; 4]| Image {
            width: 1,
            height: 1,
            rgba: rgba.to_vec(),
            mime: "image/png",
        };
        if cloth {
            atlas.map(0, 3, cotton, |pixel| pixel)?;
        } else {
            let mut clay = Vec::with_capacity(496 * 496 * 4);
            for y in 0..496 {
                for x in 0..496 {
                    let noise = super::value_noise(x as f64 / 13., y as f64 / 18., 43.);
                    let chalk = (noise - 0.55).max(0.) * ((1. - y as f64 / 496.) * 0.25 + 0.08);
                    clay.extend(
                        [152., 91., 61.]
                            .map(|c| (c + (215. - c) * chalk + (noise - 0.5) * 10.).round() as u8),
                    );
                    clay.push(255);
                }
            }
            atlas.map(
                0,
                3,
                &Image {
                    width: 496,
                    height: 496,
                    rgba: clay,
                    mime: "image/png",
                },
                |pixel| pixel,
            )?;
        }
        atlas.map(1, 3, &solid([128, 128, 255, 255]), |pixel| pixel)?;
        atlas.map(
            2,
            3,
            &solid([255, if cloth { 240 } else { 238 }, 0, 255]),
            |pixel| pixel,
        )?;
        for (tile, name) in names.into_iter().enumerate() {
            let asset = catalog.get_mut(name).unwrap();
            let mut faces: Vec<[[f64; 8]; 3]> =
                serde_json::from_value(asset["faces"].clone()).map_err(|e| e.to_string())?;
            if faces.is_empty() {
                return Err(format!("Empty geometry for {name}."));
            }
            let mut chart_tiles = vec![tile; faces.len()];
            if cloth {
                let width = asset["size"][0]
                    .as_f64()
                    .filter(|w| *w > 0.)
                    .ok_or("Invalid garment width.")?;
                let mut anchors = Vec::new();
                for sign in [-1., 1.] {
                    let vertex = faces
                        .iter()
                        .flatten()
                        .filter(|v| {
                            let x = sign * v[0] / width;
                            if name == "hanging-bedsheet" {
                                x > 0.25
                            } else {
                                x > 0.26 && x < 0.40
                            }
                        })
                        .max_by(|a, b| a[2].total_cmp(&b[2]))
                        .ok_or("Missing garment suspension vertex.")?;
                    anchors.push([vertex[0], vertex[1], vertex[2]]);
                }
                let [left, right] = [anchors[0], anchors[1]];
                let dx = right[0] - left[0];
                let ys = (right[1] - left[1]) / dx;
                let zs = (right[2] - left[2]) / dx;
                for v in faces.iter_mut().flatten() {
                    v[1] -= left[1] + ys * (v[0] - left[0]);
                    v[2] -= left[2] + zs * (v[0] - left[0]);
                    let normal = unit([v[3] + ys * v[4] + zs * v[5], v[4], v[5]])?;
                    v[3..6].copy_from_slice(&normal);
                }
                asset["anchors"] = json!([[left[0], 0., 0.], [right[0], 0., 0.]]);
            }
            if name == "rubber-plant" {
                for segment in 0..12 {
                    let mut corners = [[0.; 8]; 4];
                    for (corner, (step, ring)) in corners.iter_mut().zip([
                        (segment, 0),
                        (segment + 1, 0),
                        (segment + 1, 1),
                        (segment, 1),
                    ]) {
                        let angle = step as f64 / 12. * std::f64::consts::TAU;
                        let (sin, cos) = angle.sin_cos();
                        let r = [0.062, 0.083][ring];
                        let n = unit([cos, sin, -0.021 / 0.13])?;
                        *corner = [
                            r * cos,
                            r * sin,
                            [0.014, 0.144][ring],
                            n[0],
                            n[1],
                            n[2],
                            step as f64 / 12.,
                            ring as f64,
                        ];
                    }
                    faces.extend([
                        [corners[0], corners[1], corners[2]],
                        [corners[0], corners[2], corners[3]],
                    ]);
                    chart_tiles.extend([3, 3]);
                }
            }
            let min: [f64; 3] = std::array::from_fn(|i| {
                faces
                    .iter()
                    .flatten()
                    .map(|v| v[i])
                    .fold(f64::INFINITY, f64::min)
            });
            let max: [f64; 3] = std::array::from_fn(|i| {
                faces
                    .iter()
                    .flatten()
                    .map(|v| v[i])
                    .fold(f64::NEG_INFINITY, f64::max)
            });
            let mut output = Vec::with_capacity(faces.len());
            for (face, chart) in faces.into_iter().zip(chart_tiles) {
                let [a, b, c] = face.map(|v| [v[0], v[1], v[2]]);
                let normal = unit(cross(sub(b, a), sub(c, a)))?;
                output.push(face.map(|mut v| {
                    if dot(normal, [v[3], v[4], v[5]]) < 0.05 {
                        v[3..6].copy_from_slice(&normal);
                    }
                    let uv = map_uv(chart, [v[6], v[7]]);
                    let fallback = color_at(&atlas.maps[0], uv);
                    v[..6]
                        .iter()
                        .copied()
                        .chain(uv)
                        .chain(fallback)
                        .map(|v| (v * 1e7).round_ties_even() / 1e7)
                        .collect::<Vec<_>>()
                }));
            }
            asset["size"] = json!(sub(max, min));
            asset["min"] = json!(min);
            asset["max"] = json!(max);
            asset["width"] = json!(2. * min[0].abs().max(max[0].abs()));
            asset["drop"] = json!((-min[2]).max(0.));
            asset["triangles"] = json!(output.len());
            asset["faces"] = json!(output);
            asset["tile"] = json!(tile);
            asset["slot"] = json!(format!("balcony-{group}"));
        }
        atlas.encode(&format!("balcony-{group}"), &mut files)?;
    }
    files.push((
        "balcony-props.json".into(),
        serde_json::to_vec(&catalog).map_err(|e| e.to_string())?,
    ));
    Ok(files)
}

pub fn pack(inputs: &Inputs<'_>, source: &Path, food: bool) -> Result<Files, String> {
    let mut catalog: BTreeMap<String, Value> =
        serde_json::from_slice(&(inputs.read)(&source.join("catalog.json"))?)
            .map_err(|e| e.to_string())?;
    let mut files = Vec::new();
    let name = if food {
        "restaurant-food"
    } else {
        "grocer-props"
    };
    if food {
        if catalog.len() != 2 || !catalog.contains_key("chicken") || !catalog.contains_key("slices")
        {
            return Err("Food atlas requires chicken and sliced-meat geometry.".into());
        }
        let mut atlas = Atlas::new(
            2,
            2,
            4,
            [
                [150, 110, 60, 255],
                [128, 128, 255, 255],
                [255, 150, 0, 255],
            ],
        );
        for (tile, name) in ["chicken", "chicken", "chicken", "slices"]
            .into_iter()
            .enumerate()
        {
            for (channel, kind) in CHANNELS.into_iter().enumerate() {
                let path = image_name(source, name, kind)?;
                if !(inputs.exists)(&path) {
                    continue;
                }
                let image = inputs.decode(&path)?;
                atlas.map(channel, tile, &image, |[r, g, b, _]| {
                    let [r, g, b] = [r, g, b].map(f64::from);
                    let values = match (channel, tile) {
                        (2, _) => [255., g.max(125.), 0.],
                        (0, 1) => [r * 0.94 + 14., g * 1.17 + 10., b * 1.08 + 5.],
                        (0, 2) => [r * 0.84, g * 0.70, b * 0.64],
                        (0, 3) => {
                            let crust = (((r - g) / r.max(1.) - 0.16) / 0.30).clamp(0., 1.);
                            [
                                r * (1. - 0.45 * crust),
                                g * (1. - 0.40 * crust),
                                b * (0.85 - 0.45 * crust),
                            ]
                        }
                        _ => [r, g, b],
                    };
                    let [r, g, b] = values.map(|v| v.round_ties_even().clamp(0., 255.) as u8);
                    [r, g, b, 255]
                })?;
            }
        }
        for (name, tile) in [("chicken", 0), ("slices", 3)] {
            remap_food(catalog.get_mut(name).unwrap(), &atlas.maps[0], tile)?;
        }
        finish_roast_atlas(&mut atlas.maps);
        atlas.encode(name, &mut files)?;
    } else {
        if catalog.is_empty() || catalog.len() > 128 {
            return Err("Expected a bounded, nonempty liquor catalog.".into());
        }
        let mut atlas = Atlas::new(
            5,
            4,
            4,
            [
                [160, 150, 130, 255],
                [128, 128, 255, 255],
                [255, 220, 0, 255],
            ],
        );
        for (name, asset) in &mut catalog {
            let fields = asset.as_object_mut().ok_or("Invalid liquor asset.")?;
            let stem = fields
                .remove("sourceTexture")
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_else(|| name.clone());
            let tile = fields
                .remove("tile")
                .and_then(|v| v.as_u64())
                .filter(|&tile| tile < 20)
                .ok_or("Invalid liquor tile.")? as usize;
            let color = inputs.decode(&image_name(source, &stem, "color")?)?;
            for (channel, kind) in CHANNELS.into_iter().enumerate() {
                let path = image_name(source, &stem, kind)?;
                if channel == 0 {
                    atlas.map(channel, tile, &color, |[r, g, b, _]| [r, g, b, 255])?;
                } else if (inputs.exists)(&path) {
                    atlas.map(channel, tile, &inputs.decode(&path)?, |[r, g, b, _]| {
                        [r, g, b, 255]
                    })?;
                }
            }
            for vertex in vertices(asset)? {
                let values: Vec<f64> =
                    serde_json::from_value(vertex.clone()).map_err(|e| e.to_string())?;
                if values.len() != 10 {
                    return Err("Expected liquor vertex with both atlas and source UVs.".into());
                }
                let fallback = color_at(&color, [values[8], values[9]]);
                *vertex = json!(
                    values[..8]
                        .iter()
                        .copied()
                        .chain(fallback)
                        .map(|v| (v * 1e6).round_ties_even() / 1e6)
                        .collect::<Vec<_>>()
                );
            }
        }
        atlas.encode(name, &mut files)?;
    }
    files.push((
        format!("{name}.json"),
        serde_json::to_vec(&catalog).map_err(|e| e.to_string())?,
    ));
    Ok(files)
}
