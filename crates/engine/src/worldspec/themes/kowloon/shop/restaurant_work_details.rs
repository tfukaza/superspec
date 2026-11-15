//! Supported display glazing, closed cloth shells, order slips and tableware.
use super::super::{
    fixture::Fixture,
    noise::{fractal_noise, smooth_step, value_noise},
    palette::{shade, srgb},
    pen::Tint,
};
use super::forms::{self, Tints};
use std::f64::consts::PI;
use superspec_geometry::{
    V3,
    math::{add, cos, cross, dot, scale, sin, sub},
};
use superspec_types::rng::SeededRng;
fn check(d: &Fixture<'_, '_>, at: V3) -> Result<(), String> {
    if at.iter().any(|v| !v.is_finite() || v.abs() > 1000.) {
        return Err("Kitchen work details require a bounded finite position.".into());
    }
    d.mesh.check_work(0)
}
fn galvanized() -> V3 {
    srgb("#a4a9aa")
}
pub fn display_cabinet(
    d: &mut Fixture<'_, '_>,
    width: f64,
    top: f64,
    rail: f64,
    repeat: f64,
) -> Result<(), String> {
    check(d, [width, top, rail])?;
    if !(0.1..=40.).contains(&width) || rail <= top + 0.1 {
        return Err("Display glazing needs positive width and clearance above the counter.".into());
    }
    let pen = d
        .pen
        .into_part(&format!("{}/display", d.pen.part))?
        .with_kitchen_metal(repeat)?;
    let d = &mut Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    };
    let front = 0.035;
    let back = 0.52;
    let ceiling = rail + 0.10;
    for x in [0.025, width] {
        d.bar(
            "metal",
            [x, front, top],
            [x, front, ceiling],
            0.018,
            galvanized(),
        )?;
        d.bar(
            "metal",
            [x, back, top],
            [x, back, ceiling],
            0.018,
            galvanized(),
        )?;
        d.bar(
            "metal",
            [x, front, ceiling],
            [x, back, ceiling],
            0.018,
            galvanized(),
        )?;
    }
    for y in [front, back] {
        d.bar(
            "metal",
            [0.025, y, ceiling],
            [width, y, ceiling],
            0.018,
            galvanized(),
        )?;
    }
    d.bar(
        "metal",
        [0.025, front, top + 0.045],
        [width, front, top + 0.045],
        0.018,
        galvanized(),
    )?;
    d.face(
        "display-glass",
        &[
            [0.035, front, top + 0.055],
            [width - 0.01, front, top + 0.055],
            [width - 0.01, front, ceiling - 0.01],
            [0.035, front, ceiling - 0.01],
        ],
        [0., -1., 0.],
        [1.; 3],
    )?;
    d.face(
        "display-glass",
        &[
            [0.025, front + 0.01, top + 0.055],
            [0.025, back - 0.01, top + 0.055],
            [0.025, back - 0.01, ceiling - 0.01],
            [0.025, front + 0.01, ceiling - 0.01],
        ],
        [-1., 0., 0.],
        [1.; 3],
    )
}
fn towel_grime(point: V3, center: f64, top: f64, pigment: V3) -> V3 {
    let u = point[0] - center;
    let v = if point[1] > 0.004 {
        point[1]
    } else {
        point[2] - top
    };
    let patches = smooth_step(0.32, 0.72, fractal_noise(u * 24. + 3., v * 19., 731., 3));
    let smear = (-(((u + 0.026) / 0.044).powi(2) + ((v + 0.12) / 0.055).powi(2))).exp();
    let hem = (1. - smooth_step(0., 0.038, (v + 0.232).abs()))
        * (0.35 + 0.65 * value_noise(u * 60., 0., 97.));
    let stain = (0.12 + patches * 0.38 + smear * 0.28 + hem * 0.32).min(0.8);
    let dirt = srgb("#6e5536");
    std::array::from_fn(|i| pigment[i] * (1. - stain) + dirt[i] * stain)
}
// The source cloth uses Vector3.normalize, whose length is sqrt(dot(v,v)).
fn cloth_normal(p: V3) -> V3 {
    let n = dot(p, p).sqrt();
    scale(p, 1. / if n == 0. { 1. } else { n })
}
pub fn counter_towel(d: &mut Fixture<'_, '_>, x: f64, top: f64) -> Result<(), String> {
    check(d, [x, 0., top])?;
    let pen = d.pen.into_part(&format!("{}/work-towel", d.pen.part))?;
    let d = &mut Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    };
    let columns = 16;
    let thickness = 0.0006;
    let mut path: Vec<_> = [0.16, 0.12, 0.08, 0.04, 0.02, 0.004]
        .map(|y| [y, top + thickness])
        .into();
    for step in 1..=4 {
        let angle = step as f64 * PI / 8.;
        path.push([
            0.004 - 0.0048 * sin(angle),
            top - 0.004 + 0.0048 * cos(angle),
        ]);
    }
    for drop in [0.025, 0.055, 0.09, 0.13, 0.18, 0.228, 0.235] {
        path.push([-0.0008, top - drop]);
    }
    let middle: Vec<Vec<V3>> = path
        .iter()
        .map(|&[y, z]| {
            (0..=columns)
                .map(|column| {
                    let u = column as f64 / columns as f64;
                    let hanging = ((top - z - 0.004) / 0.08).clamp(0., 1.);
                    let fold = (1. + sin(u * PI * 5. + hanging * 0.8)) * 0.0055
                        + (1. + sin(u * PI * 9. - hanging)) * 0.0012;
                    let hem = ((top - z - 0.13) / 0.105).max(0.);
                    let wrinkle = if y >= 0.004 {
                        sin(PI * ((y - 0.004) / 0.116).min(1.)).powi(2)
                            * sin(u * PI * 3. + y * 14.).powi(2)
                            * 0.002
                    } else {
                        0.
                    };
                    [
                        x + (u - 0.5) * 0.18 + hanging * sin(u * PI) * 0.001,
                        y - hanging * fold,
                        z + wrinkle + hem * sin(u * 8. + 0.4) * 0.006,
                    ]
                })
                .collect()
        })
        .collect();
    let normals: Vec<Vec<V3>> = middle
        .iter()
        .enumerate()
        .map(|(j, row)| {
            row.iter()
                .enumerate()
                .map(|(i, _)| {
                    let across = sub(row[columns.min(i + 1)], row[i.saturating_sub(1)]);
                    let down = sub(
                        middle[(path.len() - 1).min(j + 1)][i],
                        middle[j.saturating_sub(1)][i],
                    );
                    cloth_normal(cross(down, across))
                })
                .collect()
        })
        .collect();
    let shell = |side: f64| -> Vec<Vec<V3>> {
        middle
            .iter()
            .enumerate()
            .map(|(j, row)| {
                row.iter()
                    .enumerate()
                    .map(|(i, p)| add(*p, scale(normals[j][i], side * thickness)))
                    .collect()
            })
            .collect()
    };
    let outer = shell(1.);
    let inner = shell(-1.);
    for j in 0..path.len() - 1 {
        for i in 0..columns {
            let corners = [[j, i], [j, i + 1], [j + 1, i + 1], [j + 1, i]];
            let stripe = i == 2 || i == 3 || i == 12;
            let hem = j == path.len() - 2 || i == 0 || i == columns - 1;
            let tint = shade(
                srgb(if stripe { "#995347" } else { "#d0c7ad" }),
                if hem {
                    0.82
                } else {
                    0.97 + sin(i as f64 * 1.7 + j as f64 * 0.8) * 0.025
                },
            );
            for side in [1., -1.] {
                let shell = if side == 1. { &outer } else { &inner };
                let shading = corners.map(|[r, c]| scale(normals[r][c], side));
                let tint = |p| Ok(towel_grime(p, x, top, tint));
                d.pen.face(
                    d.mesh,
                    "paint",
                    &corners.map(|[r, c]| shell[r][c]),
                    shading[0],
                    &Tint::Field(&tint),
                    d.repeat("paint"),
                    None,
                    Some(&shading),
                )?;
            }
        }
    }
    let border: Vec<_> = (0..columns)
        .map(|i| [0, i])
        .chain((0..path.len() - 1).map(|j| [j, columns]))
        .chain((0..columns).map(|i| [path.len() - 1, columns - i]))
        .chain((0..path.len() - 1).map(|j| [path.len() - 1 - j, 0]))
        .collect();
    for i in 0..border.len() {
        let [aj, ai] = border[i];
        let [bj, bi] = border[(i + 1) % border.len()];
        let points = [outer[aj][ai], outer[bj][bi], inner[bj][bi], inner[aj][ai]];
        let hint = cloth_normal(cross(sub(points[1], points[0]), sub(points[2], points[0])));
        let tint = |p| Ok(towel_grime(p, x, top, srgb("#aaa18a")));
        d.pen.face(
            d.mesh,
            "paint",
            &points,
            hint,
            &Tint::Field(&tint),
            d.repeat("paint"),
            None,
            None,
        )?;
    }
    Ok(())
}
pub fn order_slips(
    d: &mut Fixture<'_, '_>,
    at: V3,
    width: f64,
    rng: &mut SeededRng,
    repeat: f64,
) -> Result<(), String> {
    check(d, at)?;
    if !width.is_finite() || !(0.1..=40.).contains(&width) {
        return Err("Order rail requires bounded positive width.".into());
    }
    let papers = d
        .pen
        .into_part(&format!("{}/order-slips", d.pen.part))?
        .moved(at)?;
    let steel = papers.with_kitchen_metal(repeat)?;
    let d = &mut Fixture {
        pen: steel.clone(),
        mesh: d.mesh,
        repeats: d.repeats,
    };
    d.bar("metal", [0.; 3], [width, 0., 0.], 0.008, galvanized())?;
    for x in [0., width] {
        d.bar("metal", [x, 0., 0.], [x, 0.072, 0.], 0.01, galvanized())?;
    }
    let count = (width / 0.13).floor().max(2.) as usize;
    for i in 0..count {
        d.pen = papers.clone();
        let x = 0.03 + i as f64 * (width - 0.10) / (count - 1).max(1) as f64;
        let length = rng.float(0.12, 0.19);
        let dx = rng.float(-0.008, 0.008);
        let y = -0.008;
        let mut points = [
            [x, y, -0.012],
            [x + 0.075, y, -0.012],
            [x + 0.075 + dx, y - 0.008, -length],
            [x + dx, y - 0.008, -length],
        ];
        d.face(
            "paint",
            &points,
            [0., -1., 0.],
            srgb(if i % 3 == 0 { "#dbc798" } else { "#ded7be" }),
        )?;
        points.reverse();
        d.face("paint", &points, [0., 1., 0.], srgb("#c7bfa9"))?;
        d.pen = steel.clone();
        d.cuboid(
            "metal",
            [x + 0.027, -0.017, -0.027],
            [x + 0.045, 0.005, 0.009],
            galvanized(),
            "",
        )?;
        d.pen = papers.clone();
        for line in 0..4 {
            let z = -0.05 - line as f64 * 0.021;
            let t = (z + 0.012) / (-length + 0.012);
            let left = x + dx * t + 0.012;
            let depth = y - 0.008 * t - 0.0005;
            let right = left + 0.036 - (line % 2) as f64 * 0.012;
            d.face(
                "paint",
                &[
                    [left, depth, z],
                    [right, depth, z],
                    [right, depth, z - 0.002],
                    [left, depth, z - 0.002],
                ],
                [0., -1., 0.],
                srgb(if line == 0 { "#994b3c" } else { "#777163" }),
            )?;
        }
    }
    Ok(())
}
pub fn chopstick_cup(d: &mut Fixture<'_, '_>, at: V3, rng: &mut SeededRng) -> Result<(), String> {
    check(d, at)?;
    let pen = d
        .pen
        .into_part(&format!("{}/tableware", d.pen.part))?
        .moved(at)?;
    let d = &mut Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    };
    forms::lathe(
        d,
        "pane",
        [0.; 3],
        &[
            [0.035, 0.],
            [0.043, 0.10],
            [0.036, 0.103],
            [0.029, 0.012],
            [0., 0.012],
        ],
        &Tints::Uniform(&Tint::Color(srgb("#d3ceba"))),
        12,
        None,
    )?;
    for i in 0..9 {
        let angle = i as f64 * 2.4;
        let r = 0.012 + (i % 3) as f64 * 0.004;
        let x = cos(angle) * r;
        let y = sin(angle) * r;
        d.bar(
            "timber",
            [x, y, 0.012],
            [
                x + rng.float(-0.014, 0.014),
                y + rng.float(-0.014, 0.014),
                rng.float(0.17, 0.22),
            ],
            0.004,
            srgb("#ac8456"),
        )?;
    }
    Ok(())
}
