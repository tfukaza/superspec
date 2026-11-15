//! Fresh and hanging provisions; visible courses retain their own local support.
use super::super::{
    fixture::Fixture,
    palette::{mix, shade, srgb},
    pen::Tint,
};
use super::forms::{self, Ball, Tints};
use std::f64::consts::PI;
use superspec_geometry::{
    V3,
    math::{atan2, cos, sin},
};
use superspec_types::rng::SeededRng;
#[derive(Clone)]
pub struct Fruit {
    pub radius: f64,
    pub stretch: V3,
    pub glossy: bool,
    pub colors: Vec<V3>,
}
pub fn fruit_kind(name: &str) -> Result<Fruit, String> {
    let (radius, stretch, glossy, colors) = match name {
        "orange" => (0.038, [1., 1., 0.94], false, "#e8821e #f09428 #e07818"),
        "apple" => (
            0.036,
            [1., 1., 0.95],
            true,
            "#b81e22 #c8322a #d8483a #a81a2a",
        ),
        "greenApple" => (0.036, [1., 1., 0.95], true, "#8ab83a #9cc44a #a4c84a"),
        "pear" => (0.034, [1., 1., 1.3], false, "#c8c05a #d4c868 #b8b450"),
        "tomato" => (0.032, [1., 1., 0.85], true, "#d8281e #e03a24 #c82a1a"),
        "lemon" => (0.028, [1.3, 1., 1.], false, "#f0d030 #e8c828"),
        "pomelo" => (0.075, [1., 1., 1.08], false, "#c8c860 #b8c058 #d8d070"),
        "plum" => (0.026, [1., 1., 1.], true, "#5a2a5a #6a2a4a #4a2a60"),
        "potato" => (0.036, [1.35, 1., 0.8], false, "#b89a6a #a88a5a #c4a878"),
        "onion" => (0.034, [1., 1., 0.92], true, "#c8904a #b87a3a #8a3a5a"),
        "egg" => (
            0.024,
            [1., 1., 1.3],
            false,
            "#e8d8c0 #d8b890 #f0e8dc #c8a078",
        ),
        _ => return Err("Unknown produce variety.".into()),
    };
    Ok(Fruit {
        radius,
        stretch,
        glossy,
        colors: colors.split(' ').map(srgb).collect(),
    })
}
fn check(d: &Fixture<'_, '_>, at: V3, size: &[f64]) -> Result<(), String> {
    if at.iter().any(|v| !v.is_finite() || v.abs() > 1000.)
        || size
            .iter()
            .any(|v| !v.is_finite() || !(0.001..=20.).contains(v))
    {
        return Err("Produce requires bounded finite positions and positive dimensions.".into());
    }
    d.mesh.check_work(0)
}
fn ball(
    d: &mut Fixture<'_, '_>,
    slot: &str,
    center: V3,
    radius: f64,
    tint: V3,
    sides: usize,
    stretch: V3,
) -> Result<(), String> {
    forms::ball(
        d,
        slot,
        &Ball {
            center,
            radius,
            stretch,
            sides,
            bands: 3,
        },
        &Tint::Color(tint),
    )
}
fn lathe(
    d: &mut Fixture<'_, '_>,
    slot: &str,
    at: V3,
    profile: &[[f64; 2]],
    tints: &[V3],
    sides: usize,
) -> Result<(), String> {
    let tints: Vec<_> = tints.iter().map(|v| Tint::Color(*v)).collect();
    forms::lathe(d, slot, at, profile, &Tints::Bands(&tints), sides, None)
}
pub fn fruit(
    d: &mut Fixture<'_, '_>,
    at: V3,
    kind: &Fruit,
    rng: &mut SeededRng,
) -> Result<(), String> {
    check(d, at, &[kind.radius])?;
    if kind.colors.is_empty()
        || kind.colors.len() > 64
        || kind
            .stretch
            .iter()
            .any(|v| !v.is_finite() || !(0.01..=10.).contains(v))
    {
        return Err("Fruit requires finite proportions and 1–64 ripening colors.".into());
    }
    let center = [at[0], at[1], at[2] + kind.radius * kind.stretch[2]];
    ball(
        d,
        if kind.glossy { "pane" } else { "paint" },
        center,
        kind.radius,
        shade(rng.pick(&kind.colors), rng.float(0.9, 1.08)),
        5,
        kind.stretch,
    )
}
/// Calls the item emitter for visible points in shrinking elliptical courses.
pub fn heap(
    d: &mut Fixture<'_, '_>,
    center: V3,
    axes: [f64; 2],
    size: f64,
    courses: usize,
    rng: &mut SeededRng,
    mut draw: impl FnMut(&mut Fixture<'_, '_>, V3, &mut SeededRng) -> Result<(), String>,
) -> Result<(), String> {
    check(d, center, &[axes[0], axes[1], size])?;
    if courses > 64 {
        return Err("Produce heaps allow at most 64 courses.".into());
    }
    for course in 0..courses {
        let shrink = 1. - course as f64 * 0.34;
        let ax = axes[0] * shrink - size * 0.6;
        let ay = axes[1] * shrink - size * 0.6;
        if ax < size * 0.3 || ay < size * 0.3 {
            break;
        }
        let pitch = size * 2.05;
        let rows = (ay / pitch).ceil() as i32;
        let columns = (ax / pitch).ceil() as i32;
        for row in -rows..=rows {
            for column in -columns..=columns {
                d.mesh.spend_work(1)?;
                let x = (column as f64 + if row % 2 != 0 { 0.5 } else { 0. } + course as f64 * 0.5)
                    * pitch
                    + rng.float(-0.12, 0.12) * size;
                let y = (row as f64 * 0.87 + course as f64 * 0.3) * pitch
                    + rng.float(-0.12, 0.12) * size;
                if (x / ax).powi(2) + (y / ay).powi(2) > 1. {
                    continue;
                }
                draw(
                    d,
                    [
                        center[0] + x,
                        center[1] + y,
                        center[2] + course as f64 * size * 1.55,
                    ],
                    rng,
                )?;
            }
        }
    }
    Ok(())
}
pub fn melon(
    d: &mut Fixture<'_, '_>,
    at: V3,
    length: f64,
    rng: &mut SeededRng,
) -> Result<(), String> {
    check(d, at, &[length])?;
    let radius = length * 0.4;
    let center = [at[0], at[1], at[2] + radius];
    let dark = srgb("#1f4a22");
    let light = srgb("#5a9a48");
    let phase = rng.float(0., PI);
    let tint = |p: V3| {
        Ok(
            if sin(atan2(p[2] - center[2], p[1] - center[1]) * 6. + phase) > 0. {
                dark
            } else {
                light
            },
        )
    };
    forms::ball(
        d,
        "pane",
        &Ball {
            center,
            radius,
            stretch: [1.25, 1., 1.],
            sides: 12,
            bands: 5,
        },
        &Tint::Field(&tint),
    )
}
pub fn melon_half(d: &mut Fixture<'_, '_>, at: V3, r: f64) -> Result<(), String> {
    check(d, at, &[r])?;
    lathe(
        d,
        "pane",
        at,
        &[[0., 0.], [r * 0.7, r * 0.25], [r, r * 0.75]],
        &[srgb("#2a5a28")],
        10,
    )?;
    lathe(
        d,
        "paint",
        at,
        &[[r, r * 0.75], [r * 0.92, r * 0.76], [0., r * 0.76]],
        &[srgb("#dfe8c0"), srgb("#d8323a")],
        10,
    )
}
pub fn bananas(d: &mut Fixture<'_, '_>, at: V3, rng: &mut SeededRng) -> Result<(), String> {
    check(d, at, &[])?;
    let ripe = mix(srgb("#e8c838"), srgb("#9ab040"), rng.float(0., 0.5));
    let count = rng.integer(5, 8);
    forms::limb(
        d,
        "paint",
        &[at, [at[0], at[1], at[2] - 0.04]],
        &[0.012, 0.016],
        &Tint::Color(srgb("#6a5a2a")),
        5,
        1.,
    )?;
    for i in 0..count {
        let azimuth =
            -PI * 0.85 + i as f64 / (count - 1) as f64 * PI * 0.7 + rng.float(-0.08, 0.08);
        let reach = rng.float(0.85, 1.1);
        let path: Vec<_> = (0..=4)
            .map(|s| {
                let t = s as f64 / 4.;
                let out = 0.015 + 0.075 * sin(t * PI * 0.85) * reach;
                [
                    at[0] + cos(azimuth) * out,
                    at[1] + sin(azimuth) * out,
                    at[2] - 0.04 - 0.17 * t * reach,
                ]
            })
            .collect();
        let tip = path[4][2];
        let tint = |p: V3| {
            Ok(if p[2] < tip + 0.02 {
                shade(ripe, 0.45)
            } else {
                ripe
            })
        };
        forms::limb(
            d,
            "paint",
            &path,
            &[0.008, 0.017, 0.019, 0.015, 0.005],
            &Tint::Field(&tint),
            5,
            1.,
        )?;
    }
    Ok(())
}
pub fn pineapple(d: &mut Fixture<'_, '_>, at: V3, rng: &mut SeededRng) -> Result<(), String> {
    check(d, at, &[])?;
    lathe(
        d,
        "paint",
        at,
        &[
            [0.03, 0.],
            [0.058, 0.035],
            [0.062, 0.1],
            [0.05, 0.17],
            [0.022, 0.2],
        ],
        &[shade(srgb("#b8862a"), rng.float(0.85, 1.1))],
        7,
    )?;
    let green = srgb("#4a7a3a");
    for leaf in 0..7 {
        let angle = leaf as f64 / 7. * PI * 2. + rng.float(-0.2, 0.2);
        let out = rng.float(0.05, 0.09);
        let c = cos(angle);
        let s = sin(angle);
        let points = [
            [at[0] - s * 0.015, at[1] + c * 0.015, at[2] + 0.195],
            [at[0] + s * 0.015, at[1] - c * 0.015, at[2] + 0.195],
            [
                at[0] + c * out,
                at[1] + s * out,
                at[2] + 0.2 + rng.float(0.08, 0.14),
            ],
        ];
        d.sheet("paint", &points, shade(green, rng.float(0.8, 1.2)))?;
    }
    Ok(())
}
pub fn cabbage(d: &mut Fixture<'_, '_>, at: V3, r: f64, rng: &mut SeededRng) -> Result<(), String> {
    check(d, at, &[r])?;
    let green = shade(srgb("#a8c878"), rng.float(0.85, 1.1));
    let center = [at[0], at[1], at[2] + r * 0.85];
    ball(d, "paint", center, r, green, 7, [1., 1., 0.85])?;
    for _ in 0..4 {
        let angle = rng.float(0., PI * 2.);
        let c = cos(angle);
        let s = sin(angle);
        d.sheet(
            "paint",
            &[
                [
                    center[0] + c * r * 0.5 - s * r * 0.6,
                    center[1] + s * r * 0.5 + c * r * 0.6,
                    at[2] + 0.004,
                ],
                [
                    center[0] + c * r * 0.5 + s * r * 0.6,
                    center[1] + s * r * 0.5 - c * r * 0.6,
                    at[2] + 0.004,
                ],
                [
                    center[0] + c * r * 1.12 + s * r * 0.4,
                    center[1] + s * r * 1.12 - c * r * 0.4,
                    center[2] + r * 0.2,
                ],
                [
                    center[0] + c * r * 1.12 - s * r * 0.4,
                    center[1] + s * r * 1.12 + c * r * 0.4,
                    center[2] + r * 0.2,
                ],
            ],
            shade(srgb("#5a8a3a"), rng.float(0.85, 1.15)),
        )?;
    }
    Ok(())
}
pub fn greens(
    d: &mut Fixture<'_, '_>,
    at: V3,
    length: f64,
    rng: &mut SeededRng,
) -> Result<(), String> {
    check(d, at, &[length])?;
    let [x, y, z] = at;
    let leafy = srgb("#3f7a34");
    let stalk = srgb("#dfe8c8");
    for i in 0..6 {
        let offset = (i as f64 - 2.5) * 0.018 + rng.float(-0.005, 0.005);
        let lift = 0.012 + (i as f64 - 2.5).abs() * 0.004 + rng.float(0., 0.012);
        forms::limb(
            d,
            "paint",
            &[
                [x + offset * 0.5, y, z + lift],
                [x + offset, y + length * 0.45, z + lift + 0.01],
            ],
            &[0.009, 0.007],
            &Tint::Color(stalk),
            4,
            1.,
        )?;
        d.sheet(
            "paint",
            &[
                [x + offset - 0.006, y + length * 0.4, z + lift + 0.008],
                [x + offset * 1.6 - 0.035, y + length * 0.75, z + lift + 0.02],
                [x + offset * 1.9, y + length, z + lift + 0.004],
                [x + offset * 1.6 + 0.035, y + length * 0.75, z + lift + 0.02],
                [x + offset + 0.006, y + length * 0.4, z + lift + 0.008],
            ],
            shade(leafy, rng.float(0.8, 1.2)),
        )?;
    }
    Ok(())
}
pub fn root(
    d: &mut Fixture<'_, '_>,
    at: V3,
    length: f64,
    orange: bool,
    rng: &mut SeededRng,
) -> Result<(), String> {
    check(d, at, &[length])?;
    let [x, y, z] = at;
    let thick = if orange { 0.016 } else { 0.032 };
    let tint = if orange {
        shade(srgb("#e8782a"), rng.float(0.9, 1.1))
    } else {
        shade(srgb("#ece8dc"), rng.float(0.92, 1.04))
    };
    forms::limb(
        d,
        "paint",
        &[
            [x, y, z + thick],
            [x + length * 0.12, y, z + thick],
            [x + length * 0.55, y, z + thick],
            [x + length, y, z + thick * 0.5],
        ],
        &[thick * 0.5, thick, thick * 0.9, 0.003],
        &Tint::Color(tint),
        6,
        1.,
    )?;
    for leaf in 0..3 {
        let points = [
            [x, y - 0.008, z + thick],
            [x, y + 0.008, z + thick],
            [
                x - rng.float(0.06, 0.12),
                y + (leaf as f64 - 1.) * 0.04,
                z + thick + rng.float(0., 0.03),
            ],
        ];
        d.sheet(
            "paint",
            &points,
            shade(srgb("#4a8a3a"), rng.float(0.8, 1.2)),
        )?;
    }
    Ok(())
}
pub fn garlic_string(
    d: &mut Fixture<'_, '_>,
    at: V3,
    length: f64,
    rng: &mut SeededRng,
) -> Result<(), String> {
    check(d, at, &[length])?;
    let bulb = if rng.chance(0.7) {
        srgb("#ece4d0")
    } else {
        srgb("#9a4a5a")
    };
    d.bar(
        "paint",
        at,
        [at[0], at[1], at[2] - length],
        0.006,
        srgb("#c8b888"),
    )?;
    let mut z = 0.08;
    while z < length {
        let angle = rng.float(0., PI * 2.);
        ball(
            d,
            "paint",
            [
                at[0] + cos(angle) * 0.022,
                at[1] + sin(angle) * 0.022,
                at[2] - z,
            ],
            0.024,
            shade(bulb, rng.float(0.88, 1.05)),
            5,
            [1., 1., 1.15],
        )?;
        z += 0.05;
    }
    Ok(())
}
pub fn sausages(d: &mut Fixture<'_, '_>, at: V3, rng: &mut SeededRng) -> Result<(), String> {
    check(d, at, &[])?;
    let cured = shade(srgb("#8a2a1e"), rng.float(0.8, 1.15));
    for side in [-1., 1.] {
        let length = rng.float(0.17, 0.22);
        let mut path = vec![at, [at[0] + side * 0.012, at[1], at[2] - 0.04]];
        for s in 1..=4 {
            path.push([
                at[0] + side * (0.016 + s as f64 * 0.003),
                at[1],
                at[2] - 0.04 - s as f64 / 4. * length,
            ]);
        }
        forms::limb(
            d,
            "pane",
            &path,
            &[0.002, 0.003, 0.014, 0.016, 0.016, 0.004],
            &Tint::Color(cured),
            5,
            1.,
        )?;
    }
    Ok(())
}
pub fn salted_fish(
    d: &mut Fixture<'_, '_>,
    at: V3,
    length: f64,
    rng: &mut SeededRng,
) -> Result<(), String> {
    check(d, at, &[length])?;
    let skin = shade(srgb("#a89a78"), rng.float(0.8, 1.1));
    let [x, y, z] = at;
    d.sheet(
        "paint",
        &[
            [x - length * 0.11, y, z],
            [x + length * 0.11, y, z],
            [x, y, z - length * 0.16],
        ],
        shade(skin, 0.8),
    )?;
    forms::limb(
        d,
        "paint",
        &[
            [x, y, z - length * 0.1],
            [x, y, z - length * 0.3],
            [x, y, z - length * 0.6],
            [x, y, z - length * 0.88],
            [x, y, z - length],
        ],
        &[0.012, length * 0.1, length * 0.15, length * 0.11, 0.004],
        &Tint::Color(skin),
        6,
        0.24,
    )
}
