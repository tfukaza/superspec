//! Rounded end-grain work block with continuous side charts and a supported cleaver.
use super::super::{
    fixture::Fixture,
    palette::{shade, srgb},
    pen::Tint,
};
use super::{
    forms::{self, Tints},
    restaurant_surfaces::{self, Field, Settings},
};
use std::f64::consts::PI;
use superspec_geometry::{
    V3,
    math::{cos, hypot, sin},
};

fn body(d: &mut Fixture<'_, '_>, timber: bool, fields: bool) -> Result<(), String> {
    let profile: [[f64; 2]; 7] = [
        [0.188, 0.],
        [0.195, 0.004],
        [0.199, 0.015],
        [0.2, 0.035],
        [0.199, 0.103],
        [0.195, 0.114],
        [0.186, 0.12],
    ];
    let radius = 0.2;
    let field_width = 0.5;
    let circumference = 2. * PI * radius;
    let mut distances: Vec<_> = (0..=40).map(|i| i as f64 / 40. * circumference).collect();
    let mut seam = field_width;
    while seam < circumference {
        distances.push(seam);
        seam += field_width;
    }
    distances.sort_by(f64::total_cmp);
    let slopes: Vec<_> = profile
        .windows(2)
        .map(|p| {
            let n = hypot(p[1][0] - p[0][0], p[1][1] - p[0][1]);
            [(p[1][1] - p[0][1]) / n, (p[0][0] - p[1][0]) / n]
        })
        .collect();
    let normals: Vec<_> = (0..profile.len())
        .map(|i| {
            let a = slopes[i.saturating_sub(1)];
            let b = slopes[i.min(slopes.len() - 1)];
            let radial = a[0] + b[0];
            let up = a[1] + b[1];
            let n = hypot(radial, up);
            [radial / n, up / n]
        })
        .collect();
    let mut length = 0.;
    let heights: Vec<_> = profile
        .iter()
        .enumerate()
        .map(|(i, p)| {
            if i > 0 {
                length += hypot(p[0] - profile[i - 1][0], p[1] - profile[i - 1][1]);
            }
            length
        })
        .collect();
    let rings: Vec<Vec<V3>> = profile
        .iter()
        .map(|[r, z]| {
            distances
                .iter()
                .map(|distance| {
                    let angle = distance / radius + PI / 2.;
                    [cos(angle) * r, sin(angle) * r, *z]
                })
                .collect()
        })
        .collect();
    for ring in 0..profile.len() - 1 {
        for side in 0..distances.len() - 1 {
            let corners = [
                [ring, side],
                [ring, side + 1],
                [ring + 1, side + 1],
                [ring + 1, side],
            ];
            let crop =
                ((distances[side] + distances[side + 1]) / 2. / field_width).floor() * field_width;
            let uv = corners.map(|[j, i]| {
                [
                    (4.5 + (distances[i] - crop) * 494.) / 1024.,
                    1. - (1536. - 4.5 - (0.37 + heights[j]) * 494.) / 1792.,
                ]
            });
            let shading = corners.map(|[j, i]| {
                let angle = distances[i] / radius + PI / 2.;
                [
                    cos(angle) * normals[j][0],
                    sin(angle) * normals[j][0],
                    normals[j][1],
                ]
            });
            let tint = |p: V3| {
                let edge = ((p[2] - 0.10) / 0.02).max(0.);
                let foot = (1. - p[2] / 0.025).max(0.);
                let brightness = 1.04 + edge * 0.32 - foot * 0.2;
                Ok(if timber {
                    [brightness / 2.; 3]
                } else {
                    shade(srgb("#9c754c"), brightness)
                })
            };
            let slot = if timber { "shop-timber" } else { "paint" };
            d.pen.face(
                d.mesh,
                slot,
                &corners.map(|[j, i]| rings[j][i]),
                shading[0],
                &Tint::Field(&tint),
                d.repeat(slot),
                Some(&uv),
                Some(&shading),
            )?;
        }
    }
    for side in 0..distances.len() - 1 {
        d.face(
            "paint",
            &[[0.; 3], rings[0][side], rings[0][side + 1]],
            [0., 0., -1.],
            srgb("#67462e"),
        )?;
        let points = [[0., 0., 0.12], rings[6][side], rings[6][side + 1]];
        restaurant_surfaces::face(
            d,
            &points,
            [0., 0., 1.],
            &Field {
                tile: 2,
                coordinates: &points
                    .map(|[x, y, _]| [0.5 + x / 0.186 * 0.48, 0.5 + y / 0.186 * 0.48]),
                brightness: 1.,
                normals: None,
                textured: fields,
            },
        )?;
    }
    Ok(())
}
pub fn draw(
    d: &mut Fixture<'_, '_>,
    at: V3,
    timber: bool,
    settings: Settings,
) -> Result<(), String> {
    if at.iter().any(|v| !v.is_finite() || v.abs() > 1000.) {
        return Err("Chopping block requires a bounded finite position.".into());
    }
    d.mesh.check_work(0)?;
    let root = d.pen.clone();
    let pen = root
        .into_part(&format!("{}/chopping-board", root.part))?
        .moved(at)?;
    body(
        &mut Fixture {
            pen,
            mesh: d.mesh,
            repeats: d.repeats,
        },
        timber,
        settings.fields,
    )?;
    let pen = root
        .into_part(&format!("{}/board-scoring", root.part))?
        .moved(at)?;
    let scoring = &mut Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    };
    for i in 0..9 {
        let y = -0.115 + i as f64 * 0.026;
        let x = -0.065 + sin(i as f64 * 2.1) * 0.035;
        scoring.face(
            "paint",
            &[
                [x, y, 0.12005],
                [x + 0.07, y + 0.013, 0.12005],
                [x + 0.018, y + 0.002, 0.12005],
            ],
            [0., 0., 1.],
            srgb(if i % 3 == 0 { "#c49d6d" } else { "#805c39" }),
        )?;
    }
    let pen = root
        .into_part(&format!("{}/cleaver", root.part))?
        .moved(at)?;
    let metal = pen.with_kitchen_metal(settings.metal_repeat)?;
    let steel = &mut Fixture {
        pen: metal.clone(),
        mesh: d.mesh,
        repeats: d.repeats,
    };
    let galvanized = srgb("#a4a9aa");
    steel.cuboid(
        "metal",
        [-0.1, -0.05, 0.12],
        [0.08, 0.04, 0.126],
        shade(galvanized, 1.2),
        "",
    )?;
    steel.cuboid(
        "metal",
        [0.075, 0.009, 0.123],
        [0.18, 0.031, 0.128],
        galvanized,
        "",
    )?;
    forms::limb(
        &mut Fixture {
            pen,
            mesh: d.mesh,
            repeats: d.repeats,
        },
        "timber",
        &[
            [0.078, 0.02, 0.133],
            [0.089, 0.02, 0.133],
            [0.184, 0.02, 0.133],
            [0.2, 0.02, 0.133],
        ],
        &[0., 0.013, 0.012, 0.],
        &Tint::Color(srgb("#4a2f1c")),
        8,
        0.65,
    )?;
    let steel = &mut Fixture {
        pen: metal,
        mesh: d.mesh,
        repeats: d.repeats,
    };
    for x in [0.109, 0.164] {
        let top = 0.133 + 0.013 - (x - 0.089) / 0.095 * 0.001;
        forms::lathe(
            steel,
            "metal",
            [x, 0.02, top - 0.0003],
            &[[0.0025, 0.], [0.0025, 0.001], [0., 0.001]],
            &Tints::Uniform(&Tint::Color(galvanized)),
            8,
            None,
        )?;
    }
    Ok(())
}
