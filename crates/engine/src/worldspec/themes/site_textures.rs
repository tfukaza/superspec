//! Authored site-style texture collection.
use std::f64::consts::TAU;
use superspec_geometry::image::{self, Image};

fn seam(coordinate: f64) -> f64 {
    let coordinate = coordinate.rem_euclid(1.);
    coordinate.min(1. - coordinate)
}

fn pigment(style: &str, kind: &str, column: usize, row: usize) -> [u8; 3] {
    let (x, y) = (column as f64 / 256., row as f64 / 256.);
    let grain = (((column as u64 * 73856093) ^ (row as u64 * 19349663)) % 101) as f64;
    let grain = (grain / 100. - 0.5) * 3.;
    let chalk = style == "chalk";
    let value = match kind {
        "travertine" | "limestone" | "basalt" => {
            let height = y * if kind == "limestone" { 3. } else { 5. };
            let row = height.floor();
            let mut value =
                if seam(x * 3. + row.rem_euclid(2.) * 0.5) < 0.009 || seam(height) < 0.018 {
                    161.
                } else {
                    227. + 5. * (y * TAU * 33. + 2. * (x * TAU * 2.).sin()).sin() + grain
                };
            if kind == "basalt" {
                value += -15. + 9. * ((x * 3.).floor() * 9. + row * 15.).sin();
            }
            value
        }
        "terracotta" => {
            let height = y * if chalk { 6. } else { 4. };
            let row = height.floor();
            if seam(x * 4. + 0.5 * row.rem_euclid(2.)).min(seam(height)) < 0.014 {
                169.
            } else {
                230. + 10. * (row * 5. + (x * 4.).floor() * 12.).sin() + grain
            }
        }
        "cedar" | "oak" | "charred" => {
            let mut value = if seam(x * if kind == "oak" { 3. } else { 5. }) < 0.008 {
                145.
            } else {
                229. + 6. * (x * TAU * 51. + 2. * (y * TAU).sin()).sin() + grain
            };
            if kind == "charred" {
                value += -25. + 8. * (y * TAU * 3.).sin();
            }
            value
        }
        "brass" | "copper" => {
            let patina = (x * TAU * 3.).sin() * (y * TAU * 4.).cos() + 0.2 * (y * TAU * 9.).sin();
            if kind == "copper" && patina > 0.48 {
                return [145, 190, 174];
            }
            235. + 5. * (y * TAU * 90.).sin() + grain
        }
        "azulejo" | "zellige" | "mosaic" => {
            let (dx, dy) = ((x * 4.).fract() - 0.5, (y * 4.).fract() - 0.5);
            let (dx, dy) = (dx.abs(), dy.abs());
            if seam(x * 4.).min(seam(y * 4.)) < 0.014 {
                161.
            } else if kind == "azulejo" {
                let mark =
                    (dx + dy > 0.18 && dx + dy < 0.3) || (dx.min(dy) < 0.047 && dx.max(dy) > 0.32);
                return if mark {
                    [60, 103, 170]
                } else {
                    [247, 244, 228]
                };
            } else if kind == "mosaic" {
                return if dx + dy < 0.32 {
                    [196, 218, 183]
                } else {
                    [237, 241, 217]
                };
            } else {
                228. + 14. * ((x * 4.).floor() * 12. + (y * 4.).floor() * 5.).sin() + grain
            }
        }
        "concrete" => {
            let (u, v) = ((x * 2.).fract(), (y * 2.).fract());
            if (u - 0.1)
                .abs()
                .min((u - 0.9).abs())
                .hypot((v - 0.1).abs().min((v - 0.9).abs()))
                < 0.019
            {
                133.
            } else if seam(u).min(seam(v)) < 0.007 {
                183.
            } else {
                232. + 5. * (x * TAU * 3.).sin() * (y * TAU * 2.).sin() + grain
            }
        }
        "steel" => {
            let ribs = seam((x + y) * 8.).min(seam((x - y) * 8.)) < 0.05;
            229. + if ribs { 14. } else { 0. } + grain
        }
        "grid" => {
            let block = [(x * 7.).fract(), (y * 7.).fract()]
                .into_iter()
                .all(|n| n > 0.12 && n < 0.88);
            if block { 161. } else { 235. }
        }
        _ => {
            let (base, frequency) = if chalk { (244., 3.) } else { (234., 5.) };
            base + 3. * (x * TAU * frequency).sin() * (y * TAU * 4.).sin() + grain
        }
    };
    [value.round_ties_even().clamp(0., 255.) as u8; 3]
}

pub const COLLECTIONS: [(&str, [&str; 6]); 3] = [
    (
        "clay",
        [
            "limewash",
            "travertine",
            "terracotta",
            "zellige",
            "cedar",
            "brass",
        ],
    ),
    (
        "chalk",
        [
            "limewash",
            "limestone",
            "terracotta",
            "azulejo",
            "oak",
            "mosaic",
        ],
    ),
    (
        "night",
        ["concrete", "basalt", "steel", "grid", "charred", "copper"],
    ),
];

pub fn texture(style: &str, kind: &str) -> Result<Vec<u8>, String> {
    let mut rgba = Vec::with_capacity(256 * 256 * 4);
    for y in 0..256 {
        for x in 0..256 {
            rgba.extend(pigment(style, kind, x, y));
            rgba.push(255);
        }
    }
    let bytes = image::encode(&Image {
        width: 256,
        height: 256,
        rgba,
        mime: "image/png",
    })?;
    Ok(bytes)
}
