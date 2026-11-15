//! Standing cookware and hanging household wares, in room-local meters.
use super::super::{
    fixture::Fixture,
    palette::{shade, srgb},
    pen::Tint,
};
use super::forms::{self, Tints};
use std::f64::consts::PI;
use superspec_geometry::{
    V3,
    math::{add, cos, hypot, sin},
};
use superspec_types::rng::SeededRng;

fn check(d: &Fixture<'_, '_>, at: V3, dimensions: &[f64]) -> Result<(), String> {
    if at.iter().any(|v| !v.is_finite() || v.abs() > 1000.)
        || dimensions
            .iter()
            .any(|v| !v.is_finite() || !(0.001..=20.).contains(v))
    {
        return Err("Shop wares require finite positions and bounded positive dimensions.".into());
    }
    d.mesh.check_work(0)
}
fn lathe(
    d: &mut Fixture<'_, '_>,
    slot: &str,
    at: V3,
    profile: &[[f64; 2]],
    tints: &[V3],
    sides: usize,
) -> Result<(), String> {
    let tints: Vec<_> = tints.iter().map(|c| Tint::Color(*c)).collect();
    forms::lathe(d, slot, at, profile, &Tints::Bands(&tints), sides, None)
}
fn limb(
    d: &mut Fixture<'_, '_>,
    slot: &str,
    path: &[V3],
    radii: &[f64],
    tint: V3,
    sides: usize,
) -> Result<(), String> {
    forms::limb(d, slot, path, radii, &Tint::Color(tint), sides, 1.)
}
fn steel() -> V3 {
    srgb("#a4a9aa")
}

pub fn wok(d: &mut Fixture<'_, '_>, at: V3, r: f64, tint: V3) -> Result<(), String> {
    check(d, at, &[r])?;
    lathe(
        d,
        "metal",
        at,
        &[
            [r * 0.22, 0.],
            [r * 0.7, r * 0.13],
            [r, r * 0.33],
            [r * 1.03, r * 0.35],
            [r * 0.98, r * 0.335],
            [r * 0.68, r * 0.145],
            [r * 0.2, r * 0.02],
            [0., r * 0.02],
        ],
        &[
            tint,
            tint,
            tint,
            shade(tint, 1.2),
            shade(tint, 0.7),
            shade(tint, 0.6),
            shade(tint, 0.6),
        ],
        12,
    )?;
    for side in [-1., 1.] {
        limb(
            d,
            "metal",
            &[
                add(at, [side * r, -r * 0.22, r * 0.33]),
                add(at, [side * r * 1.2, -r * 0.12, r * 0.4]),
                add(at, [side * r * 1.2, r * 0.12, r * 0.4]),
                add(at, [side * r, r * 0.22, r * 0.33]),
            ],
            &[0.006; 4],
            tint,
            4,
        )?;
    }
    Ok(())
}
pub fn hanging_wok(
    d: &mut Fixture<'_, '_>,
    at: V3,
    radius: f64,
    tint: V3,
    bowl_out: bool,
) -> Result<(), String> {
    check(d, at, &[radius])?;
    let center = [
        at[0],
        at[1]
            + if bowl_out {
                radius * 0.2
            } else {
                -radius * 0.2
            },
        at[2] - radius * 1.2,
    ];
    let pen = forms::rotated(&d.pen, center, 0, if bowl_out { PI / 2. } else { -PI / 2. })?;
    let pen = forms::rotated(&pen, [0.; 3], 2, PI / 2.)?;
    wok(
        &mut Fixture {
            pen,
            mesh: d.mesh,
            repeats: d.repeats,
        },
        [0.; 3],
        radius,
        tint,
    )
}
pub fn pot(
    d: &mut Fixture<'_, '_>,
    at: V3,
    r: f64,
    h: f64,
    tint: V3,
    content: Option<V3>,
) -> Result<(), String> {
    check(d, at, &[r, h])?;
    if let Some(content) = content {
        lathe(
            d,
            "metal",
            at,
            &[
                [r * 0.94, 0.],
                [r, 0.015],
                [r, h],
                [r * 1.05, h + 0.008],
                [r * 0.97, h + 0.004],
                [r * 0.96, h * 0.78],
            ],
            &[
                tint,
                tint,
                shade(tint, 1.15),
                shade(tint, 0.8),
                shade(tint, 0.6),
            ],
            12,
        )?;
        lathe(
            d,
            "pane",
            at,
            &[[r * 0.96, h * 0.78], [0., h * 0.78]],
            &[content],
            12,
        )?;
    } else {
        lathe(
            d,
            "metal",
            at,
            &[
                [r * 0.94, 0.],
                [r, 0.015],
                [r, h],
                [r * 1.05, h + 0.008],
                [r * 0.6, h + 0.05],
                [r * 0.14, h + 0.065],
                [0.016, h + 0.07],
                [0.028, h + 0.1],
                [0., h + 0.1],
            ],
            &[
                tint,
                tint,
                shade(tint, 1.15),
                shade(tint, 1.05),
                shade(tint, 1.05),
                shade(tint, 0.5),
                shade(tint, 0.5),
                shade(tint, 0.5),
            ],
            12,
        )?;
    }
    for side in [-1., 1.] {
        let inner = at[0] + side * (r - 0.012);
        let outer = at[0] + side * (r + 0.052);
        d.cuboid(
            "metal",
            [inner.min(outer), at[1] - 0.04, at[2] + h * 0.72],
            [inner.max(outer), at[1] + 0.04, at[2] + h * 0.72 + 0.014],
            shade(tint, 0.6),
            "",
        )?;
    }
    Ok(())
}
pub fn kettle(d: &mut Fixture<'_, '_>, at: V3, r: f64, tint: V3) -> Result<(), String> {
    check(d, at, &[r])?;
    lathe(
        d,
        "metal",
        at,
        &[
            [r * 0.7, 0.],
            [r, r * 0.3],
            [r * 0.96, r * 0.95],
            [r * 0.5, r * 1.35],
            [r * 0.3, r * 1.4],
            [0.014, r * 1.42],
            [0.024, r * 1.55],
            [0., r * 1.55],
        ],
        &[
            tint,
            tint,
            tint,
            shade(tint, 1.1),
            shade(tint, 0.5),
            shade(tint, 0.5),
            shade(tint, 0.5),
        ],
        10,
    )?;
    limb(
        d,
        "metal",
        &[
            add(at, [r * 0.8, 0., r * 0.5]),
            add(at, [r * 1.35, 0., r * 0.95]),
            add(at, [r * 1.6, 0., r * 1.4]),
        ],
        &[r * 0.2, r * 0.13, r * 0.09],
        tint,
        6,
    )?;
    limb(
        d,
        "metal",
        &[
            add(at, [-r * 0.75, 0., r * 1.1]),
            add(at, [-r * 0.7, 0., r * 1.9]),
            add(at, [0., 0., r * 2.15]),
            add(at, [r * 0.7, 0., r * 1.9]),
            add(at, [r * 0.75, 0., r * 1.1]),
        ],
        &[0.006, 0.007, 0.01, 0.007, 0.006],
        shade(tint, 0.6),
        4,
    )
}
pub fn bucket(d: &mut Fixture<'_, '_>, at: V3, r: f64, h: f64, tint: V3) -> Result<(), String> {
    check(d, at, &[r, h])?;
    lathe(
        d,
        "paint",
        at,
        &[
            [r * 0.72, 0.],
            [r, h],
            [r * 1.05, h + 0.012],
            [r * 0.96, h + 0.004],
            [r * 0.7, 0.02],
            [0., 0.02],
        ],
        &[
            tint,
            shade(tint, 1.15),
            shade(tint, 0.75),
            shade(tint, 0.6),
            shade(tint, 0.5),
        ],
        12,
    )?;
    limb(
        d,
        "metal",
        &[
            add(at, [-r * 1.02, 0., h - 0.01]),
            add(at, [-r * 0.8, -r * 0.8, h - 0.06]),
            add(at, [r * 0.8, -r * 0.8, h - 0.06]),
            add(at, [r * 1.02, 0., h - 0.01]),
        ],
        &[0.004; 4],
        shade(steel(), 0.8),
        4,
    )
}
pub fn basin(d: &mut Fixture<'_, '_>, at: V3, r: f64, tint: V3) -> Result<(), String> {
    check(d, at, &[r])?;
    lathe(
        d,
        "pane",
        at,
        &[
            [r * 0.6, 0.],
            [r * 0.95, r * 0.36],
            [r * 1.04, r * 0.38],
            [r * 0.93, r * 0.355],
            [r * 0.58, 0.016],
            [0., 0.016],
        ],
        &[
            tint,
            shade(tint, 1.15),
            shade(tint, 0.8),
            shade(tint, 0.7),
            shade(tint, 0.6),
        ],
        12,
    )
}
pub fn basket(d: &mut Fixture<'_, '_>, at: V3, r: f64, h: f64, tint: V3) -> Result<(), String> {
    check(d, at, &[r, h])?;
    lathe(
        d,
        "timber",
        at,
        &[
            [r * 0.7, 0.],
            [r * 0.98, h * 0.5],
            [r, h],
            [r * 1.05, h + 0.012],
            [r * 0.95, h],
            [r * 0.68, 0.015],
            [0., 0.015],
        ],
        &[
            tint,
            shade(tint, 0.92),
            shade(tint, 1.15),
            shade(tint, 0.7),
            shade(tint, 0.6),
            shade(tint, 0.5),
        ],
        10,
    )
}
pub fn clay_pot(
    d: &mut Fixture<'_, '_>,
    at: V3,
    r: f64,
    rng: &mut SeededRng,
) -> Result<(), String> {
    check(d, at, &[r])?;
    let clay = shade(srgb("#b0703a"), rng.float(0.8, 1.15));
    lathe(
        d,
        "paint",
        at,
        &[
            [r * 0.6, 0.],
            [r, r * 0.5],
            [r * 0.92, r * 0.9],
            [r * 0.98, r * 0.95],
            [r * 0.5, r * 1.2],
            [0.02, r * 1.26],
            [0.03, r * 1.36],
            [0., r * 1.36],
        ],
        &[
            clay,
            clay,
            shade(clay, 1.1),
            shade(clay, 0.75),
            shade(clay, 0.75),
            shade(clay, 0.6),
            shade(clay, 0.6),
        ],
        9,
    )?;
    limb(
        d,
        "paint",
        &[
            add(at, [r * 0.9, 0., r * 0.75]),
            add(at, [r * 1.5, 0., r * 0.85]),
        ],
        &[r * 0.14, r * 0.12],
        clay,
        5,
    )
}
pub fn stool_stack(
    d: &mut Fixture<'_, '_>,
    at: V3,
    count: usize,
    tints: &[V3],
) -> Result<(), String> {
    check(d, at, &[])?;
    if count > 64 || tints.is_empty() || tints.len() > 64 {
        return Err("Stool stacks require at most 64 stools and 1–64 tints.".into());
    }
    for i in 0..count {
        let tint = tints[i % tints.len()];
        let [x, y, _] = at;
        let z = at[2] + i as f64 * 0.085;
        d.cuboid(
            "paint",
            [x - 0.14, y - 0.14, z + 0.4],
            [x + 0.14, y + 0.14, z + 0.43],
            tint,
            "",
        )?;
        let corners = [[1., 1.], [-1., 1.], [-1., -1.], [1., -1.]];
        for (k, [sx, sy]) in corners.iter().enumerate() {
            let [nx, ny] = corners[(k + 1) % 4];
            d.bar(
                "paint",
                [x + sx * 0.17, y + sy * 0.17, z + 0.004],
                [x + sx * 0.125, y + sy * 0.125, z + 0.4],
                0.035,
                shade(tint, 0.85),
            )?;
            d.bar(
                "paint",
                [x + sx * 0.147, y + sy * 0.147, z + 0.2],
                [x + nx * 0.147, y + ny * 0.147, z + 0.2],
                0.014,
                shade(tint, 0.75),
            )?;
        }
    }
    Ok(())
}
pub fn broom(
    d: &mut Fixture<'_, '_>,
    foot: V3,
    top: V3,
    rng: &mut SeededRng,
) -> Result<(), String> {
    check(d, foot, &[])?;
    check(d, top, &[])?;
    limb(
        d,
        "timber",
        &[foot, top],
        &[0.012; 2],
        shade(srgb("#c8b888"), rng.float(0.8, 1.1)),
        5,
    )?;
    let tint = if rng.chance(0.6) {
        srgb("#b89a5a")
    } else {
        rng.pick(&[srgb("#3f8a5a"), srgb("#c8281e"), srgb("#2f5f9a")])
    };
    let bristle = shade(tint, rng.float(0.85, 1.1));
    let crown = [
        top[0] + (top[0] - foot[0]) * 0.18,
        top[1] + (top[1] - foot[1]) * 0.18,
        top[2] + 0.3,
    ];
    forms::limb(
        d,
        "paint",
        &[top, [top[0], top[1], top[2] + 0.06], crown],
        &[0.02, 0.05, 0.11],
        &Tint::Color(bristle),
        7,
        0.28,
    )
}
pub fn mop(d: &mut Fixture<'_, '_>, foot: V3, top: V3, rng: &mut SeededRng) -> Result<(), String> {
    check(d, foot, &[])?;
    check(d, top, &[])?;
    limb(
        d,
        "timber",
        &[foot, top],
        &[0.012; 2],
        shade(srgb("#c8b888"), rng.float(0.8, 1.1)),
        5,
    )?;
    let yarn = shade(srgb("#b8b4a4"), rng.float(0.75, 1.));
    forms::ball(
        d,
        "paint",
        &forms::Ball {
            center: add(top, [0., 0., 0.02]),
            radius: 0.085,
            stretch: [1., 0.7, 0.85],
            sides: 7,
            bands: 3,
        },
        &Tint::Color(yarn),
    )?;
    for strand in 0..7 {
        let angle = strand as f64 / 7. * PI * 2.;
        let out = [cos(angle) * 0.09, sin(angle) * 0.06];
        let path = [
            add(top, [out[0] * 0.7, out[1] * 0.7, 0.03]),
            add(top, [out[0] * 1.3, out[1] * 1.3, -0.08]),
            [
                top[0] + out[0] * 1.2,
                top[1] + out[1] * 1.2,
                top[2] - 0.2 - rng.float(0., 0.06),
            ],
        ];
        limb(
            d,
            "paint",
            &path,
            &[0.016, 0.015, 0.008],
            shade(yarn, rng.float(0.85, 1.05)),
            4,
        )?;
    }
    Ok(())
}
pub fn duster(d: &mut Fixture<'_, '_>, at: V3, rng: &mut SeededRng) -> Result<(), String> {
    check(d, at, &[])?;
    let [x, y, z] = at;
    limb(
        d,
        "timber",
        &[at, [x, y, z - 0.3]],
        &[0.008, 0.009],
        shade(srgb("#c8b888"), 0.9),
        5,
    )?;
    let feather = rng.pick(&[
        srgb("#7a4a2a"),
        srgb("#3a2a22"),
        srgb("#c8502a"),
        srgb("#d8c8a0"),
    ]);
    for ring in 0..4 {
        for k in 0..6 {
            let angle = k as f64 / 6. * PI * 2. + ring as f64 * 0.5;
            let top = z - 0.26 - ring as f64 * 0.07;
            let c = cos(angle);
            let s = sin(angle);
            d.sheet(
                "paint",
                &[
                    [x - s * 0.012, y + c * 0.012, top],
                    [x + s * 0.012, y - c * 0.012, top],
                    [x + c * 0.07 + s * 0.02, y + s * 0.07 - c * 0.02, top - 0.13],
                    [x + c * 0.07 - s * 0.02, y + s * 0.07 + c * 0.02, top - 0.13],
                ],
                shade(feather, rng.float(0.75, 1.2)),
            )?;
        }
    }
    Ok(())
}
pub fn coil(
    d: &mut Fixture<'_, '_>,
    at: V3,
    radius: f64,
    section: f64,
    turns: usize,
    tint: V3,
) -> Result<(), String> {
    check(d, at, &[radius, section])?;
    if turns > 64 || section >= radius {
        return Err("Coils require at most 64 turns and a section smaller than the radius.".into());
    }
    let center = [at[0], at[1], at[2] - radius];
    for turn in 0..turns {
        let r = radius - (turn % 2) as f64 * section * 0.9;
        let y = center[1] + (turn as f64 - (turns as f64 - 1.) / 2.) * section * 1.7;
        let path: Vec<_> = (0..=14)
            .map(|k| {
                let angle = k as f64 / 14. * PI * 2. + turn as f64;
                [center[0] + cos(angle) * r, y, center[2] + sin(angle) * r]
            })
            .collect();
        limb(
            d,
            "paint",
            &path,
            &[section; 15],
            shade(tint, 0.9 + (turn % 3) as f64 * 0.08),
            5,
        )?;
    }
    Ok(())
}
pub fn ladles(d: &mut Fixture<'_, '_>, at: V3, rng: &mut SeededRng) -> Result<(), String> {
    check(d, at, &[])?;
    let steel = shade(steel(), 1.05);
    for i in 0..3 {
        let x = at[0] + (i as f64 - 1.) * 0.05;
        let y = at[1] + rng.float(-0.02, 0.02);
        let length = rng.float(0.3, 0.42);
        d.bar("metal", [x, y, at[2]], [x, y, at[2] - length], 0.008, steel)?;
        lathe(
            d,
            "metal",
            [x, y - 0.03, at[2] - length - 0.04],
            &[
                [0., 0.],
                [0.04, 0.015],
                [0.05, 0.04],
                [0.045, 0.038],
                [0., 0.02],
            ],
            &[steel, steel, shade(steel, 0.7), shade(steel, 0.6)],
            8,
        )?;
    }
    Ok(())
}
pub fn ladder(
    d: &mut Fixture<'_, '_>,
    foot: V3,
    top: V3,
    width: f64,
    rng: &mut SeededRng,
) -> Result<(), String> {
    check(d, foot, &[width])?;
    check(d, top, &[])?;
    let length = hypot(top[0] - foot[0], top[2] - foot[2]);
    if !(0.1..=20.).contains(&length) {
        return Err("Ladder length must be 0.1–20 meters.".into());
    }
    let cane = shade(srgb("#c8b888"), rng.float(0.8, 1.05));
    for side in [-1., 1.] {
        limb(
            d,
            "timber",
            &[
                [foot[0], foot[1] + side * width / 2., foot[2]],
                [top[0], top[1] + side * width / 2., top[2]],
            ],
            &[0.022, 0.019],
            cane,
            6,
        )?;
    }
    let mut s = 0.28;
    while s < length - 0.1 {
        let t = s / length;
        let x = foot[0] + (top[0] - foot[0]) * t;
        let z = foot[2] + (top[2] - foot[2]) * t;
        limb(
            d,
            "timber",
            &[[x, foot[1] - width / 2., z], [x, foot[1] + width / 2., z]],
            &[0.013; 2],
            shade(cane, 0.9),
            5,
        )?;
        s += 0.3;
    }
    Ok(())
}
