//! Cookware and service pieces sized for the roast shop's existing worktops.
use super::super::super::{
    palette::{shade, srgb},
    pen::Tint,
};
use super::forms::Tints;
use super::{Fixture, V3, forms, on};
use std::f64::consts::FRAC_PI_2;
use superspec_geometry::math::{cos, sin};
use superspec_types::rng::SeededRng;

pub(super) fn rounded_rect(at: V3, half: [f64; 2], corner: f64, height: f64) -> [V3; 16] {
    let [w, d] = half;
    let centers = [
        [w - corner, d - corner],
        [-w + corner, d - corner],
        [-w + corner, -d + corner],
        [w - corner, -d + corner],
    ];
    std::array::from_fn(|i| {
        let corner_id = i / 4;
        let angle = (corner_id as f64 + (i % 4) as f64 / 3.) * FRAC_PI_2;
        [
            at[0] + centers[corner_id][0] + corner * cos(angle),
            at[1] + centers[corner_id][1] + corner * sin(angle),
            at[2] + height,
        ]
    })
}

pub(super) fn join_rects(
    d: &mut Fixture<'_, '_>,
    slot: &str,
    at: V3,
    from: &[V3; 16],
    to: &[V3; 16],
    inward: bool,
    tint: &Tint<'_>,
) -> Result<(), String> {
    for i in 0..16 {
        let j = (i + 1) % 16;
        let center = [
            (from[i][0] + from[j][0]) / 2. - at[0],
            (from[i][1] + from[j][1]) / 2. - at[1],
        ];
        let sign = if inward { -1. } else { 1. };
        let hint = if from[i][2] == to[i][2] {
            [0., 0., sign]
        } else {
            [sign * center[0], sign * center[1], 0.]
        };
        d.pen.face(
            d.mesh,
            slot,
            &[from[i], from[j], to[j], to[i]],
            hint,
            tint,
            d.repeat(slot),
            None,
            None,
        )?;
    }
    Ok(())
}

pub(super) fn meat_tray(d: &mut Fixture<'_, '_>, repeat: f64) -> Result<f64, String> {
    let pen = d.pen.with_kitchen_metal(repeat)?;
    let d = &mut on(d, pen);
    let at = [0.; 3];
    let body = rounded_rect(at, [0.153, 0.102], 0.021, 0.002);
    let shoulder = rounded_rect(at, [0.165, 0.115], 0.025, 0.025);
    let lip = rounded_rect(at, [0.173, 0.123], 0.027, 0.031);
    let rim = rounded_rect(at, [0.168, 0.118], 0.026, 0.035);
    let inner = rounded_rect(at, [0.161, 0.111], 0.025, 0.031);
    let floor = rounded_rect(at, [0.151, 0.101], 0.019, 0.009);
    d.face("paint", &body, [0., 0., -1.], srgb("#8c8a79"))?;
    for (a, b, inward, color) in [
        (&body, &shoulder, false, "#d2cab1"),
        (&shoulder, &lip, false, "#dcd3bc"),
        (&lip, &rim, false, "#547780"),
        (&rim, &inner, true, "#68878e"),
        (&inner, &floor, true, "#c6bea9"),
    ] {
        let color = srgb(color);
        let wiping = |p: V3| {
            let grain = sin(p[0] * 12. + 1.9 * sin(p[1] * 46.0)) * 0.045;
            Ok(shade(color, 0.97 + grain))
        };
        join_rects(d, "pane", at, a, b, inward, &Tint::Field(&wiping))?;
    }
    d.face("pane", &floor, [0., 0., 1.], srgb("#d8d0b9"))?;
    Ok(0.009)
}

fn turn(
    d: &mut Fixture<'_, '_>,
    slot: &str,
    at: V3,
    profile: &[[f64; 2]],
    colors: &[V3],
    sides: usize,
    repeat: Option<f64>,
) -> Result<(), String> {
    let bands: Vec<_> = colors.iter().copied().map(Tint::Color).collect();
    forms::lathe(d, slot, at, profile, &Tints::Bands(&bands), sides, repeat)
}

pub(super) fn dial_scale(d: &mut Fixture<'_, '_>, repeat: f64) -> Result<(), String> {
    let origin = [0.; 3];
    let heel = rounded_rect(origin, [0.115, 0.105], 0.017, 0.);
    let foot = rounded_rect(origin, [0.123, 0.111], 0.017, 0.011);
    let housing = rounded_rect(origin, [0.108, 0.096], 0.016, 0.031);
    let shoulder = rounded_rect(origin, [0.077, 0.072], 0.022, 0.175);
    let cap = rounded_rect(origin, [0.064, 0.061], 0.021, 0.195);
    let enamel = srgb("#527b72");
    d.face("paint", &heel, [0., 0., -1.], srgb("#424844"))?;
    for (a, b, tint) in [
        (&heel, &foot, srgb("#4c534c")),
        (&foot, &housing, srgb("#889789")),
        (&housing, &shoulder, enamel),
        (&shoulder, &cap, srgb("#6f9b88")),
    ] {
        let wear = |[x, y, z]: V3| {
            let stroke = 0.05 * sin(x * 63. + 1.6 * sin(y * 81. + z * 11.));
            let rub = ((z - 0.13) / 0.065).clamp(0., 1.) * 0.18;
            Ok(shade(tint, 0.90 + stroke + rub))
        };
        join_rects(d, "paint", origin, a, b, false, &Tint::Field(&wear))?;
    }
    d.face("paint", &cap, [0., 0., 1.], shade(enamel, 1.1))?;
    turn(
        d,
        "paint",
        [0., 0., 0.194],
        &[[0.038, 0.], [0.038, 0.015], [0.025, 0.024]],
        &[srgb("#4b5650"), srgb("#819083")],
        12,
        None,
    )?;
    turn(
        d,
        "pane",
        [0., 0., 0.215],
        &[
            [0.05, 0.],
            [0.073, 0.007],
            [0.114, 0.029],
            [0.120, 0.035],
            [0.115, 0.039],
            [0.107, 0.038],
            [0.097, 0.029],
            [0.049, 0.012],
            [0., 0.010],
        ],
        &[
            srgb("#a6aaa1"),
            srgb("#c2c6ba"),
            srgb("#d4d2c0"),
            srgb("#66888a"),
            srgb("#839797"),
            srgb("#decfb2"),
            srgb("#d3c8af"),
            srgb("#c5bfa9"),
        ],
        24,
        None,
    )?;
    let dial = forms::rotated(&d.pen, [0., -0.082, 0.105], 0, FRAC_PI_2 - 0.15)?;
    let d = &mut on(d, dial);
    let metal = d.pen.with_kitchen_metal(repeat)?;
    turn(
        &mut on(d, metal),
        "metal",
        origin,
        &[[0.077, 0.], [0.080, 0.008], [0.076, 0.014], [0.071, 0.015]],
        &[[0.50, 0.55, 0.54], [0.87, 0.90, 0.89], [0.72, 0.78, 0.75]],
        24,
        None,
    )?;
    turn(
        d,
        "pane",
        origin,
        &[[0.072, 0.014], [0., 0.014]],
        &[srgb("#dcd2ac")],
        24,
        None,
    )?;
    let ink = srgb("#484339");
    for i in 0..=18 {
        let angle = -0.8 + i as f64 / 18. * 4.74;
        let minor = i % 3 != 0;
        let r = if minor { 0.060 } else { 0.051 };
        let point = |r| [r * cos(angle), r * sin(angle), 0.0145];
        d.face(
            "paint",
            &[
                point(0.065),
                point(r),
                [r * cos(angle + 0.031), r * sin(angle + 0.031), 0.0145],
                [
                    0.065 * cos(angle + 0.031),
                    0.065 * sin(angle + 0.031),
                    0.0145,
                ],
            ],
            [0., 0., 1.],
            ink,
        )?;
    }
    d.face(
        "paint",
        &[
            [-0.010, -0.013, 0.015],
            [0.002, -0.011, 0.015],
            [0.037, 0.046, 0.015],
        ],
        [0., 0., 1.],
        srgb("#9a432d"),
    )?;
    turn(
        d,
        "paint",
        origin,
        &[[0.006, 0.015], [0.006, 0.016], [0., 0.016]],
        &[ink],
        10,
        None,
    )
}

pub(super) fn prep_tools(d: &mut Fixture<'_, '_>, at: V3, repeat: f64) -> Result<(), String> {
    let plate = rounded_rect(at, [0.106, 0.048], 0.016, 0.006);
    let lip = rounded_rect(at, [0.108, 0.050], 0.017, 0.011);
    let well = rounded_rect(at, [0.098, 0.041], 0.014, 0.009);
    join_rects(
        d,
        "paint",
        at,
        &plate,
        &lip,
        false,
        &Tint::Color(srgb("#b5ad94")),
    )?;
    join_rects(
        d,
        "pane",
        at,
        &lip,
        &well,
        true,
        &Tint::Color(srgb("#839795")),
    )?;
    d.face("pane", &well, [0., 0., 1.], srgb("#c9bfa1"))?;
    let [x, y, z] = at;
    let metal = d.pen.with_kitchen_metal(repeat)?;
    let steel = &mut on(d, metal);
    for sign in [-1., 1.] {
        forms::limb(
            steel,
            "metal",
            &[
                [x - 0.074, y + sign * 0.004, z + 0.02],
                [x - 0.035, y + sign * 0.012, z + 0.026],
                [x + 0.053, y + sign * 0.016, z + 0.024],
                [x + 0.076, y + sign * 0.012, z + 0.014],
            ],
            &[0.004, 0.003, 0.004, 0.006],
            &Tint::Color([0.94, 0.91, 0.83]),
            6,
            0.55,
        )?;
    }
    d.bar(
        "timber",
        [x - 0.083, y + 0.026, z + 0.017],
        [x + 0.074, y + 0.031, z + 0.013],
        0.004,
        srgb("#a8774b"),
    )
}

pub(super) fn sauce_station(d: &mut Fixture<'_, '_>, at: V3) -> Result<(), String> {
    let [x, y, z] = at;
    for (dx, dy, r, h, color, lid) in [
        (-0.082, 0.01, 0.033, 0.110, "#933d28", "#c59062"),
        (0.002, -0.021, 0.037, 0.073, "#825436", "#bdd0b9"),
        (0.081, 0.009, 0.033, 0.13, "#556750", "#d4b776"),
    ] {
        let pigment = srgb(color);
        let lid = srgb(lid);
        turn(
            d,
            "pane",
            [x + dx, y + dy, z],
            &[
                [r * 0.81, 0.],
                [r, 0.004],
                [r, h * 0.24],
                [r, h * 0.69],
                [r * 0.91, h * 0.83],
                [r * 0.89, h * 0.88],
                [r * 0.98, h * 0.90],
                [r * 0.98, h * 0.97],
                [r * 0.87, h],
                [0., h],
            ],
            &[
                shade(pigment, 0.76),
                pigment,
                srgb("#c6b488"),
                pigment,
                shade(pigment, 1.18),
                shade(lid, 0.72),
                lid,
                shade(lid, 1.18),
                lid,
            ],
            16,
            None,
        )?;
    }
    Ok(())
}

pub(super) fn roasted_joint(d: &mut Fixture<'_, '_>, x: f64, floor: f64) -> Result<(), String> {
    let path = [
        [x - 0.035, 0.024, floor + 0.015],
        [x - 0.023, 0.020, floor + 0.024],
        [x + 0.003, 0.010, floor + 0.023],
        [x + 0.028, -0.010, floor + 0.014],
        [x + 0.045, -0.020, floor + 0.010],
    ];
    let glaze = |[x, y, z]: V3| {
        let cast = ((z - floor) / 0.047).clamp(0., 1.);
        let stroke = (sin(x * 80. + y * 24.).max(0.)).powi(2);
        Ok(shade(srgb("#a3582a"), 0.74 + 0.30 * cast + 0.22 * stroke))
    };
    forms::limb(
        d,
        "pane",
        &path,
        &[0.003, 0.024, 0.026, 0.014, 0.005],
        &Tint::Field(&glaze),
        10,
        0.78,
    )?;
    forms::limb(
        d,
        "pane",
        &[path[4], [x + 0.057, -0.022, floor + 0.009]],
        &[0.006, 0.004],
        &Tint::Color(srgb("#bc9c70")),
        8,
        0.85,
    )
}

pub(super) fn sliced_fan(d: &mut Fixture<'_, '_>, at: V3, yaw: f64) -> Result<(), String> {
    let pen = forms::rotated(&d.pen, at, 2, yaw)?;
    let d = &mut on(d, pen);
    let shape = [
        [0.030, 0.009],
        [0.023, 0.031],
        [0.009, 0.040],
        [-0.010, 0.039],
        [-0.026, 0.027],
        [-0.031, 0.012],
        [-0.020, 0.],
        [0.018, 0.002],
    ];
    for i in 0..4 {
        let x = -0.031 + i as f64 * 0.021;
        let y = -0.022 + i as f64 * 0.014;
        let ring = |inside: bool, back: f64| {
            shape.map(|[u, v]| {
                let u = if inside { u * 0.88 } else { u };
                let v = if inside { 0.003 + v * 0.87 } else { v };
                [x + u, y + v * 0.55 + back, v + 0.001]
            })
        };
        let rind = ring(false, 0.);
        let middle = ring(true, 0.);
        let rear = ring(false, 0.009);
        let outward = [0., -1., 0.55];
        let flesh = |[u, _, v]: V3| {
            let grain = sin((u - x) * 78. + v * 45.);
            let upward = (v / 0.037).min(1.);
            Ok(shade(srgb("#d9a68e"), 0.83 + upward * 0.19 + grain * 0.045))
        };
        d.pen.face(
            d.mesh,
            "pane",
            &middle,
            outward,
            &Tint::Field(&flesh),
            d.repeat("pane"),
            None,
            None,
        )?;
        d.face("pane", &rear, [0., 1., -0.55], srgb("#a35c38"))?;
        for k in 0..rind.len() {
            let j = (k + 1) % rind.len();
            let seam = if k < 4 { "#a6582c" } else { "#81442c" };
            d.face(
                "pane",
                &[rind[k], rind[j], middle[j], middle[k]],
                outward,
                srgb(seam),
            )?;
            d.face(
                "pane",
                &[rind[k], rear[k], rear[j], rind[j]],
                [
                    rind[k][0] + rind[j][0] - x * 2.,
                    0.,
                    rind[k][2] + rind[j][2] - 0.040,
                ],
                srgb(if k < 4 { "#a46334" } else { "#603522" }),
            )?;
        }
    }
    Ok(())
}

pub(super) fn paper_card(d: &mut Fixture<'_, '_>, width: f64, menu: bool) -> Result<(), String> {
    let height = width * if menu { 1.18 } else { 1.34 };
    let left = -width * 0.5;
    let paper = |[x, _, z]: V3| {
        let tint = srgb(if menu { "#ddca9c" } else { "#d0cbb3" });
        Ok(shade(tint, 0.93 + (x / width + z / height) * 0.10))
    };
    d.pen.panel(
        d.mesh,
        "pane",
        [left, -left, 0., height],
        0.0005,
        &Tint::Field(&paper),
        d.repeat("pane"),
    )?;
    let mut panel = |u0, u1, v0, v1, offset, color| {
        d.panel(
            "pane",
            [
                left + u0 * width,
                left + u1 * width,
                v0 * height,
                v1 * height,
            ],
            offset,
            srgb(color),
        )
    };
    panel(
        0.07,
        0.93,
        0.74,
        0.92,
        0.0009,
        if menu { "#8e4734" } else { "#9b6751" },
    )?;
    if menu {
        for column in 0..4 {
            let x = 0.18 + column as f64 * 0.18;
            panel(x, x + 0.083, 0.83, 0.849, 0.0012, "#dec99d")?;
            panel(x + 0.031, x + 0.047, 0.79, 0.884, 0.0012, "#dec99d")?;
        }
        for i in 0..6 {
            let y = 0.665 - i as f64 * 0.097;
            panel(
                0.13,
                0.42 + (i % 3) as f64 * 0.076,
                y,
                y + 0.023,
                0.0009,
                "#695947",
            )?;
            panel(0.72, 0.86, y, y + 0.021, 0.0009, "#84553b")?;
        }
    } else {
        for row in 0..5 {
            for col in 0..7 {
                let x = 0.122 + col as f64 * 0.111;
                let y = 0.63 - row as f64 * 0.123;
                panel(
                    x,
                    x + 0.041,
                    y,
                    y + 0.032,
                    0.0009,
                    if col == 0 { "#a15b4e" } else { "#6b6454" },
                )?;
            }
        }
    }
    Ok(())
}

pub(super) fn wall_utensils(d: &mut Fixture<'_, '_>, at: V3, repeat: f64) -> Result<(), String> {
    let part = d
        .pen
        .into_part(&format!("{}/utensil-rail", d.pen.part))?
        .moved(at)?;
    let metal = part.with_kitchen_metal(repeat)?;
    let rail = &mut on(d, metal.clone());
    let steel = [0.79, 0.82, 0.77];
    for x in [0., 0.72] {
        rail.cuboid(
            "metal",
            [x - 0.023, 0.032, -0.023],
            [x + 0.023, 0.056, 0.023],
            shade(steel, 0.82),
            "",
        )?;
    }
    rail.bar("metal", [0., 0.017, 0.], [0.72, 0.017, 0.], 0.016, steel)?;
    for (x, radius, drop, tint) in [
        (0.13, 0.082, 0.14, srgb("#627d73")),
        (0.33, 0.068, 0.16, srgb("#9d6550")),
        (0.59, 0.089, 0.19, srgb("#b1a68a")),
    ] {
        forms::limb(
            &mut on(d, metal.clone()),
            "metal",
            &[
                [x, 0.007, 0.008],
                [x, -0.006, -0.010],
                [x, -0.002, -0.043],
                [x, -0.006, 0.03 - drop],
            ],
            &[0.004; 4],
            &Tint::Color(steel),
            6,
            1.,
        )?;
        let pen = forms::rotated(&part, [x, -0.008, -drop], 0, FRAC_PI_2)?;
        let object = &mut on(d, pen);
        turn(
            object,
            "pane",
            [0.; 3],
            &[
                [radius * 0.94, 0.],
                [radius, 0.008],
                [radius * 0.86, 0.015],
                [radius * 0.53, 0.022],
                [radius * 0.22, 0.026],
                [0., 0.027],
            ],
            &[
                shade(tint, 0.71),
                shade(tint, 1.29),
                tint,
                shade(tint, 1.13),
                tint,
            ],
            18,
            None,
        )?;
        turn(
            object,
            "paint",
            [0.; 3],
            &[[0.011, 0.026], [0.011, 0.047], [0.017, 0.049], [0., 0.049]],
            &[srgb("#494641")],
            10,
            None,
        )?;
    }
    Ok(())
}

pub(super) fn ceramic_bowl(
    d: &mut Fixture<'_, '_>,
    at: V3,
    r: f64,
    height: f64,
    rim: V3,
    sauce: Option<V3>,
) -> Result<(), String> {
    let celadon = srgb("#d8d7bd");
    let foot = srgb("#ad9a78");
    let profile = [
        [r * 0.44, 0.],
        [r * 0.44, height * 0.11],
        [r * 0.57, height * 0.21],
        [r * 0.88, height * 0.67],
        [r * 0.97, height * 0.86],
        [r, height * 0.95],
        [r * 0.985, height],
        [r * 0.902, height],
        [r * 0.883, height * 0.84],
        [r * 0.67, height * 0.43],
        [r * 0.38, height * 0.22],
        [0., height * 0.2],
    ];
    let bands = [
        foot,
        celadon,
        celadon,
        shade(celadon, 0.93),
        rim,
        shade(rim, 1.2),
        srgb("#f1ebd3"),
        shade(celadon, 1.04),
        celadon,
        shade(celadon, 0.95),
        celadon,
    ];
    let end = if sauce.is_some() { 10 } else { profile.len() };
    turn(d, "pane", at, &profile[..end], &bands, 16, None)?;
    if let Some(color) = sauce {
        turn(
            d,
            "paint",
            at,
            &[
                [profile[end - 1][0], profile[end - 1][1]],
                [0., profile[end - 1][1]],
            ],
            &[color],
            16,
            None,
        )?;
    }
    Ok(())
}

pub(super) fn bowl_stack(d: &mut Fixture<'_, '_>, at: V3, count: usize) -> Result<(), String> {
    let r = 0.075;
    let rim = srgb("#3d696f");
    for i in 0..count {
        let z = at[2] + i as f64 * 0.022;
        let base = [at[0] + (i % 3) as f64 * 0.0015, at[1], z];
        if i == count - 1 {
            ceramic_bowl(d, base, r, 0.052, rim, None)?;
        } else {
            turn(
                d,
                "pane",
                base,
                &[
                    [r * 0.44, 0.],
                    [r * 0.87, 0.028],
                    [r * 0.98, 0.046],
                    [r, 0.052],
                ],
                &[srgb("#d0c7a6"), srgb("#dfd8bb"), shade(rim, 0.95)],
                12,
                None,
            )?;
        }
    }
    Ok(())
}

pub(super) fn sauce_tray(d: &mut Fixture<'_, '_>, repeat: f64) -> Result<(), String> {
    let z = meat_tray(d, repeat)?;
    for (x, color) in [(-0.072, "#778441"), (0.073, "#8c472c")] {
        let at = [x, 0., z];
        ceramic_bowl(d, at, 0.055, 0.05, srgb("#427877"), Some(srgb(color)))?;
        let pen = d.pen.with_kitchen_metal(repeat)?;
        let steel = &mut on(d, pen);
        forms::limb(
            steel,
            "metal",
            &[
                [x, -0.028, z + 0.027],
                [x + 0.008, 0.005, z + 0.030],
                [x + 0.016, 0.042, z + 0.062],
                [x + 0.021, 0.079, z + 0.077],
            ],
            &[0.008, 0.005, 0.004, 0.003],
            &Tint::Color([0.84, 0.86, 0.82]),
            6,
            0.55,
        )?;
    }
    Ok(())
}

pub(super) fn takeaway_stack(
    d: &mut Fixture<'_, '_>,
    at: V3,
    rng: &mut SeededRng,
) -> Result<(), String> {
    let pen = forms::rotated(&d.pen, at, 2, rng.float(-0.07, 0.07))?;
    let d = &mut on(d, pen);
    let count = rng.integer(4, 6);
    for i in 0..count {
        let base = [i as f64 * 0.002, (i % 2) as f64 * 0.003, i as f64 * 0.037];
        let foot = rounded_rect(base, [0.111, 0.081], 0.014, 0.);
        let fold = rounded_rect(base, [0.121, 0.091], 0.014, 0.025);
        let cream = srgb(if i % 3 == 0 { "#d1c5a2" } else { "#e3dabd" });
        let seam = rounded_rect(base, [0.124, 0.094], 0.014, 0.029);
        let crown = rounded_rect(base, [0.116, 0.087], 0.014, 0.037);
        for (a, b, tone) in [
            (&foot, &fold, 0.89),
            (&fold, &seam, 0.69),
            (&seam, &crown, 1.05),
        ] {
            join_rects(
                d,
                "paint",
                base,
                a,
                b,
                false,
                &Tint::Color(shade(cream, tone)),
            )?;
        }
        if i == count - 1 {
            d.face("paint", &crown, [0., 0., 1.], cream)?;
            let tab = [base[0], base[1] - 0.077, base[2] + 0.0371];
            d.face(
                "paint",
                &rounded_rect(tab, [0.031, 0.010], 0.005, 0.),
                [0., 0., 1.],
                srgb("#a45e49"),
            )?;
        }
    }
    Ok(())
}

pub(super) fn stock_pot(
    d: &mut Fixture<'_, '_>,
    at: V3,
    r: f64,
    h: f64,
    broth: bool,
    repeat: f64,
) -> Result<(), String> {
    let mask = if broth {
        srgb("#507b73")
    } else {
        srgb("#8b5945")
    };
    let steel = [0.82, 0.84, 0.80];
    turn(
        d,
        "paint",
        at,
        &[
            [r * 0.87, 0.],
            [r * 0.94, h * 0.035],
            [r, h * 0.11],
            [r, h * 0.74],
            [r, h * 0.92],
            [r * 0.995, h],
            [r * 0.94, h],
        ],
        &[
            shade(mask, 0.62),
            shade(mask, 0.88),
            mask,
            shade(mask, 1.13),
            shade(mask, 1.06),
            srgb("#c9c1a3"),
        ],
        20,
        None,
    )?;

    let pen = d.pen.with_kitchen_metal(repeat)?;
    let metal = &mut on(d, pen);
    turn(
        metal,
        "metal",
        at,
        &[
            [r * 0.997, h * 0.91],
            [r * 1.024, h * 0.94],
            [r * 1.023, h * 0.98],
            [r * 0.94, h],
        ],
        &[shade(steel, 0.72), steel, shade(steel, 1.1)],
        20,
        Some(repeat),
    )?;
    for sign in [-1., 1.] {
        let [x, y, z] = at;
        let z = z + h * 0.74;
        forms::limb(
            metal,
            "metal",
            &[
                [x + sign * r * 0.94, y - 0.040, z],
                [x + sign * (r + 0.034), y - 0.038, z + 0.012],
                [x + sign * (r + 0.048), y - 0.021, z + 0.012],
                [x + sign * (r + 0.048), y + 0.021, z + 0.012],
                [x + sign * (r + 0.034), y + 0.038, z + 0.012],
                [x + sign * r * 0.94, y + 0.040, z],
            ],
            &[0.007; 6],
            &Tint::Color(shade(steel, 0.88)),
            6,
            0.7,
        )?;
    }
    if broth {
        turn(
            d,
            "pane",
            at,
            &[[r * 0.94, h], [r * 0.88, h * 0.79]],
            &[srgb("#a49a79")],
            20,
            None,
        )?;
        turn(
            d,
            "paint",
            at,
            &[[r * 0.88, h * 0.79], [0., h * 0.79]],
            &[srgb("#8c683f")],
            20,
            None,
        )
    } else {
        turn(
            d,
            "paint",
            at,
            &[
                [r * 0.93, h],
                [r * 0.91, h + 0.012],
                [r * 0.69, h + 0.033],
                [r * 0.25, h + 0.043],
                [0.012, h + 0.046],
                [0.018, h + 0.069],
                [0.024, h + 0.084],
                [0., h + 0.084],
            ],
            &[
                shade(mask, 0.76),
                shade(mask, 1.08),
                shade(mask, 1.14),
                mask,
                srgb("#3d3530"),
                srgb("#393430"),
                srgb("#514944"),
            ],
            20,
            None,
        )
    }
}

pub(super) fn wok(d: &mut Fixture<'_, '_>, at: V3, r: f64, repeat: f64) -> Result<(), String> {
    let [x, y, z] = at;
    let pen = d.pen.with_kitchen_metal(repeat)?;
    let metal = &mut on(d, pen);
    let gun = [0.55, 0.61, 0.59];
    let rim = [0.93, 0.94, 0.85];
    turn(
        metal,
        "metal",
        at,
        &[
            [r * 0.21, 0.],
            [r * 0.43, r * 0.045],
            [r * 0.71, r * 0.145],
            [r * 0.94, r * 0.29],
            [r, r * 0.33],
            [r * 1.02, r * 0.351],
            [r * 0.99, r * 0.364],
            [r * 0.95, r * 0.338],
            [r * 0.81, r * 0.25],
            [r * 0.55, r * 0.12],
            [r * 0.25, r * 0.052],
            [0., r * 0.037],
        ],
        &[
            shade(gun, 0.72),
            gun,
            shade(gun, 0.9),
            gun,
            shade(gun, 1.16),
            rim,
            shade(rim, 0.82),
            gun,
            shade(gun, 0.91),
            shade(gun, 0.81),
            shade(gun, 0.72),
        ],
        24,
        Some(repeat),
    )?;
    for sign in [-1., 1.] {
        forms::limb(
            metal,
            "metal",
            &[
                [x + sign * r, y - r * 0.19, z + r * 0.33],
                [x + sign * r * 1.14, y - r * 0.16, z + r * 0.38],
                [x + sign * r * 1.19, y - r * 0.081, z + r * 0.40],
                [x + sign * r * 1.19, y + r * 0.081, z + r * 0.4],
                [x + sign * r * 1.14, y + r * 0.16, z + r * 0.38],
                [x + sign * r, y + r * 0.19, z + r * 0.33],
            ],
            &[0.006; 6],
            &Tint::Color(shade(rim, 0.7)),
            6,
            1.,
        )?;
    }
    Ok(())
}
