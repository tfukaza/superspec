use super::{Files, Inputs};
use super::{image, lattice_hash, mix, smooth_step, value_noise};
use serde_json::{Value, json};
use std::path::Path;
use superspec_geometry::image::Image;

const ATLAS: [&str; 3] = [
    "grocer-props.png",
    "grocer-props-normal.png",
    "grocer-props-properties.png",
];

fn byte(value: f64) -> u8 {
    value.clamp(0., 255.).round() as u8
}

fn linear(value: u8) -> f64 {
    let x = f64::from(value) / 255.;
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

#[derive(Clone, Copy)]
struct Sample {
    color: [u8; 3],
    relief: f64,
    roughness: u8,
}

#[derive(Clone, Copy)]
struct Chart {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
}

impl Chart {
    fn from_uv([u0, v0, u1, v1]: [f64; 4], image: &Image) -> Self {
        let x = (u0 * image.width as f64).round() as usize;
        let y = ((1. - v1) * image.height as f64).round() as usize;
        Self {
            x,
            y,
            w: ((u1 * image.width as f64).round() as usize).saturating_sub(x),
            h: (((1. - v0) * image.height as f64).round() as usize).saturating_sub(y),
        }
    }

    fn render(
        self,
        maps: &mut [Image; 3],
        meters: [f64; 2],
        sample: impl Fn(usize, usize) -> Sample,
    ) -> Result<(), String> {
        let Self { x, y, w, h } = self;
        if x < 4
            || y < 4
            || w < 2
            || h < 2
            || x + w + 4 > maps[0].width
            || y + h + 4 > maps[0].height
        {
            return Err("Stock atlas chart exceeds its padded bounds.".into());
        }
        let samples: Vec<_> = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| sample(x, y))
            .collect();
        let at = |x: usize, y: usize| samples[y * w + x];
        for ty in y - 4..y + h + 4 {
            for tx in x - 4..x + w + 4 {
                let sx = tx.saturating_sub(x).min(w - 1);
                let sy = ty.saturating_sub(y).min(h - 1);
                let sample = at(sx, sy);
                let dx = (at((sx + 1).min(w - 1), sy).relief - at(sx.saturating_sub(1), sy).relief)
                    * meters[0];
                let dy = (at(sx, (sy + 1).min(h - 1)).relief - at(sx, sy.saturating_sub(1)).relief)
                    * meters[1];
                let length = dx.hypot(dy).hypot(1.);
                let normal = [
                    byte(127.5 - 127.5 * dx / length),
                    byte(127.5 + 127.5 * dy / length),
                    byte(127.5 + 127.5 / length),
                ];
                let offset = (ty * maps[0].width + tx) * 4;
                for (map, channels) in
                    maps.iter_mut()
                        .zip([sample.color, normal, [255, sample.roughness, 0]])
                {
                    map.rgba[offset..offset + 3].copy_from_slice(&channels);
                }
            }
        }
        Ok(())
    }

    fn uv(self, image: &Image) -> [f64; 4] {
        [
            (self.x as f64 + 0.5) / image.width as f64,
            1. - (self.y as f64 + self.h as f64 - 0.5) / image.height as f64,
            (self.x as f64 + self.w as f64 - 0.5) / image.width as f64,
            1. - (self.y as f64 + 0.5) / image.height as f64,
        ]
    }
}

fn scratches(w: usize, h: usize, seed: f64) -> Vec<f32> {
    let mut field = vec![0_f32; w * h];
    for index in 0..112 {
        let roll = |channel| lattice_hash(index as f64, channel, seed);
        let x = roll(0.) * w as f64;
        let y = roll(1.) * h as f64;
        let angle = (roll(2.) - 0.5) * 1.6;
        let length = w as f64 * (0.014 + roll(3.).powi(2) * 0.15);
        let (dx, dy) = (angle.cos() * length, angle.sin() * length);
        let radius = 0.55 + roll(4.).powi(3) * w as f64 * 0.008;
        let strength = 0.33 + roll(5.) * 0.67;
        for row in (y.min(y + dy) - radius).floor() as usize
            ..((y.max(y + dy) + radius).ceil() as usize).min(h)
        {
            for column in (x - radius).floor() as usize..((x + dx + radius).ceil() as usize).min(w)
            {
                let (a, b) = (column as f64 - x, row as f64 - y);
                let along = ((a * dx + b * dy) / length.powi(2)).clamp(0., 1.);
                let distance = (a - along * dx).hypot(b - along * dy);
                let taper = 0.38 + 0.62 * (along * std::f64::consts::PI).sin();
                let mark = (1. - smooth_step(0., radius, distance)) * taper * strength;
                let pixel = &mut field[row * w + column];
                *pixel = pixel.max(mark as f32);
            }
        }
    }
    field
}

fn crate_chart(
    maps: &mut [Image; 3],
    uv: [f64; 4],
    pigment: [f64; 3],
    seed: f64,
) -> Result<(), String> {
    let chart = Chart::from_uv(uv, &maps[0]);
    if chart.w > maps[0].width || chart.h > maps[0].height {
        return Err("Invalid crate atlas dimensions.".into());
    }
    let marks = scratches(chart.w, chart.h, seed);
    chart.render(maps, [0.5; 2], |x, y| {
        let (u, v) = (
            x as f64 / (chart.w - 1) as f64,
            y as f64 / (chart.h - 1) as f64,
        );
        let patch = value_noise(u * 8., v * 9., seed + 1.);
        let cluster = value_noise(u * 23., v * 32., seed + 2.);
        let grain = value_noise(u * 142., v * 168., seed + 3.);
        let rim = 1. - smooth_step(0.035, 0.19, v + (cluster - 0.5) * 0.12);
        let bottom = smooth_step(0.64, 0.94, v + (patch - 0.5) * 0.21);
        let rubbing = smooth_step(0.39, 0.79, patch * 0.65 + cluster * 0.35);
        let scratch = f64::from(marks[y * chart.w + x]);
        let fade = (rubbing * (0.12 + 0.56 * rim) + scratch * 0.72).min(0.83);
        let dirt = (bottom * (0.24 + patch * 0.43)
            + smooth_step(0.55, 0.86, cluster * 0.62 + patch * 0.38) * (0.19 + grain * 0.26))
            .min(0.79);
        Sample {
            color: std::array::from_fn(|axis| {
                let rubbed = mix(pigment[axis], [174., 168., 150.][axis], fade);
                byte(
                    mix(rubbed, [56., 45., 33.][axis], dirt)
                        + (grain - 0.5) * 13.
                        + (patch - 0.5) * 18.,
                )
            }),
            relief: f64::from(((grain - 0.5) * 0.085 + dirt * 0.19 - scratch * 0.53) as f32),
            roughness: byte(158. + fade * 57. + dirt * 49. + (grain - 0.5) * 19.),
        }
    })
}

fn paper(art: &Image, u: f64, v: f64, variant: usize, face: usize) -> Sample {
    let seed = 901. + variant as f64 * 173.;
    let broad = value_noise(u * 7., v * 6., seed);
    let fine = value_noise(u * 127., v * 93., seed + 1.);
    let edge = u.min(1. - u).min(v).min(1. - v);
    let abrasion =
        (1. - smooth_step(
            0.002,
            0.018 + value_noise(u * 31., v * 37., seed + 3.) * 0.016,
            edge,
        )) * (0.45 + fine * 0.55);
    let top = face == 2;
    let ripple = (value_noise(u * 17., v * 15., seed + 5.) - 0.5) * 0.027
        + (value_noise(u * 41., v * 39., seed + 25.) - 0.5) * 0.009;
    let waterline = 0.75 + (value_noise(u * 5., 0., seed + 15.) - 0.5) * 0.2 + ripple * 2.;
    let bottom = if top {
        0.
    } else {
        smooth_step(waterline - 0.009, waterline + 0.012, v)
    };
    let rim = if top {
        0.
    } else {
        (-((v - waterline) / 0.007).powi(2)).exp()
    };
    let center = if top { 0.38 } else { 0.48 };
    let radius = ((u - 0.22 - variant as f64 * 0.13
        + (value_noise(v * 7., 0., seed + 30.) - 0.5) * 0.1)
        / 0.26)
        .hypot((v - center) / 0.2);
    let contour = 1. + (value_noise(u * 13., v * 11., seed + 8.) - 0.5) * 0.4 + ripple * 8.;
    let pool = 1. - smooth_step(contour - 0.025, contour + 0.035, radius);
    let pool_rim = (-((radius - contour) / 0.035).powi(2)).exp();
    let liquid = (bottom * (0.21 + broad * 0.1)
        + rim * (0.1 + fine * 0.2)
        + pool * (0.13 + broad * 0.1)
        + pool_rim * (0.08 + value_noise(u * 27., v * 23., seed + 29.) * 0.25))
        .min(0.68);
    let dirt = smooth_step(0.48, 0.85, broad) * 0.08;
    let scratch = value_noise(u * 180., v * 13., seed + 11.).powi(10) * 0.16;
    let fiber = (fine - 0.5) * 0.045 + (broad - 0.5) * 0.05;
    let mut ink = 0.;
    if face == 0 && (0.15..=0.85).contains(&u) && (0.23..=0.77).contains(&v) {
        let design = [4, 0, 2, 5][variant];
        let x = (((design % 4) as f64 + (u - 0.15) / 0.7) * art.width as f64 / 4.) as usize;
        let y = (((design / 4) as f64 + (v - 0.23) / 0.54) * art.height as f64 / 2.) as usize;
        let offset = (y.min(art.height - 1) * art.width + x.min(art.width - 1)) * 4;
        let luminance = (f64::from(art.rgba[offset]) * 0.3
            + f64::from(art.rgba[offset + 1]) * 0.59
            + f64::from(art.rgba[offset + 2]) * 0.11)
            / 255.;
        ink = (1. - smooth_step(0.18, 0.77, luminance))
            * (0.4 + 0.1 * broad)
            * (1. - abrasion)
            * (0.7 + 0.3 * fine);
    }
    let seam = if top {
        1. - smooth_step(0.002, 0.007, (u - 0.5).abs())
    } else {
        0.
    };
    let tape = if top {
        1. - smooth_step(0.047, 0.052 + fine * 0.004, (u - 0.5).abs())
    } else {
        0.
    };
    let tape_edge = if top {
        (-(((u - 0.5).abs() - 0.05) / 0.005).powi(2)).exp()
    } else {
        0.
    };
    let crease_center = 0.12 + u * 0.11 + value_noise(u * 6., 0., seed + 18.) * 0.035;
    let crease = (-((v - crease_center) / 0.007).powi(2)).exp() * smooth_step(0.15, 0.6, u);
    Sample {
        color: std::array::from_fn(|axis| {
            let base = [177., 146., 105.][axis] + variant as f64 * [4., 3., 3.][axis];
            let stained = mix(base, [116., 65., 28.][axis], liquid);
            byte(
                stained
                    * (1. - dirt + fiber
                        - ink * [1., 1., 0.86][axis]
                        - seam * 0.28
                        - tape_edge * 0.09)
                    + abrasion * [23., 25., 25.][axis]
                    + scratch * 20.
                    + tape * [4., 0., -7.][axis],
            )
        }),
        relief: (fine - 0.5) * 0.00008 + crease * 0.0002 + abrasion * 0.00012 - seam * 0.0004
            + tape * 0.00015,
        roughness: byte(235. + broad * 12. - abrasion * 8. - tape * 35.),
    }
}

fn uv(value: &Value) -> Result<[f64; 4], String> {
    let uv: [f64; 4] = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
    if uv.iter().any(|v| !(0. ..=1.).contains(v)) || uv[0] >= uv[2] || uv[1] >= uv[3] {
        return Err("Invalid stock atlas UV range.".into());
    }
    Ok(uv)
}

fn colors(
    faces: &[Vec<[f64; 11]>],
    image: &Image,
    source: [f64; 4],
    regions: [[f64; 4]; 3],
    carton: bool,
) -> Vec<Vec<[f64; 3]>> {
    faces
        .iter()
        .map(|face| {
            face.iter()
                .map(|vertex| {
                    let region = if carton && vertex[5] > 0.9 {
                        regions[2]
                    } else if carton && vertex[4] > -0.9 {
                        regions[1]
                    } else {
                        regions[0]
                    };
                    let [u, v] = std::array::from_fn(|axis| {
                        mix(
                            region[axis],
                            region[axis + 2],
                            (vertex[axis + 6] - source[axis]) / (source[axis + 2] - source[axis]),
                        )
                    });
                    let x = ((u * image.width as f64) as usize).min(image.width - 1);
                    let y = (((1. - v) * image.height as f64) as usize).min(image.height - 1);
                    std::array::from_fn(|axis| linear(image.rgba[(y * image.width + x) * 4 + axis]))
                })
                .collect()
        })
        .collect()
}

pub fn bake_from(
    inputs: &Inputs<'_>,
    carton: bool,
    root: &Path,
    textures: &Path,
) -> Result<Files, String> {
    let read_json = |name: &str| -> Result<Value, String> {
        serde_json::from_slice(&(inputs.catalog)(name)?).map_err(|e| e.to_string())
    };
    let mut maps = [
        inputs.decode(&textures.join(ATLAS[0]))?,
        inputs.decode(&textures.join(ATLAS[1]))?,
        inputs.decode(&textures.join(ATLAS[2]))?,
    ];
    if maps
        .iter()
        .any(|image| image.width != maps[0].width || image.height != maps[0].height)
    {
        return Err("Stock atlas images differ in dimensions.".into());
    }
    let asset = if carton { "stock-box" } else { "bottle-crate" };
    let mut catalog = read_json("grocer-props.json")?;
    let mut appearances = read_json("stock-appearances.json")?;
    let mut faces: Vec<Vec<[f64; 11]>> =
        serde_json::from_value(catalog[asset]["faces"].clone()).map_err(|e| e.to_string())?;
    let source = uv(&appearances[asset]["source"])?;
    let mut base_regions = [source; 3];
    let mut variants = Vec::new();
    if carton {
        let art = inputs
            .decode(&root.join("projects/kowloon-liquor-props/materials/packaging-labels.png"))?;
        for variant in 0..4 {
            let mut regions = [source; 3];
            for (face, (x, y, w)) in [(4, 4, 248), (4, 132, 120), (132, 132, 120)]
                .into_iter()
                .enumerate()
            {
                let chart = Chart {
                    x: 2048 + variant % 2 * 256 + x,
                    y: 1536 + variant / 2 * 256 + y,
                    w,
                    h: 120,
                };
                chart.render(&mut maps, [w as f64 / 0.96, 120. / 0.58], |x, y| {
                    paper(
                        &art,
                        x as f64 / (w - 1) as f64,
                        y as f64 / 119.,
                        variant,
                        face,
                    )
                })?;
                regions[face] = chart.uv(&maps[0]);
            }
            if variant == 0 {
                base_regions = regions;
            }
            variants.push(json!({"uv":regions[0], "plainUv":regions[1], "topUv":regions[2], "colors":colors(&faces,&maps[0],source,regions,true)}));
        }
    } else {
        let pigments = [
            [158., 50., 38.],
            [83., 128., 88.],
            [76., 100., 142.],
            [151., 129., 66.],
            [149., 141., 128.],
        ];
        crate_chart(&mut maps, source, pigments[0], 519.)?;
        let previous = appearances[asset]["variants"]
            .as_array()
            .ok_or("Missing crate variants.")?;
        if previous.len() != 4 {
            return Err("Expected four crate variants.".into());
        }
        for (index, variant) in previous.iter().enumerate() {
            let chart = uv(&variant["uv"])?;
            crate_chart(
                &mut maps,
                chart,
                pigments[index + 1],
                683. + index as f64 * 173.,
            )?;
            variants
                .push(json!({"uv":chart, "colors":colors(&faces,&maps[0],source,[chart;3],false)}));
        }
    }
    let fallback = colors(&faces, &maps[0], source, base_regions, carton);
    for (face, colors) in faces.iter_mut().zip(fallback) {
        for (vertex, rgb) in face.iter_mut().zip(colors) {
            vertex[8..11].copy_from_slice(&rgb);
        }
    }
    catalog[asset]["faces"] = json!(faces);
    appearances[asset]["variants"] = json!(variants);
    let mut files = Vec::new();
    for (name, map) in ATLAS.into_iter().zip(maps) {
        files.push((name, image::encode(&map)?));
    }
    files.push((
        "grocer-props.json",
        serde_json::to_vec(&catalog).map_err(|e| e.to_string())?,
    ));
    files.push((
        "stock-appearances.json",
        serde_json::to_vec(&appearances).map_err(|e| e.to_string())?,
    ));
    Ok(files
        .into_iter()
        .map(|(name, bytes)| (name.to_owned(), bytes))
        .collect())
}
