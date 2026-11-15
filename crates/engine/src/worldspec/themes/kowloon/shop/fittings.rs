//! Lights, suspended fittings and small shop equipment in room-local meters.
use super::super::{
    fixture::Fixture,
    palette::{shade, srgb},
    pen::{Pen, Tint},
    posters,
};
use super::forms::{self, Ball, Tints, front};
use std::f64::consts::PI;
use superspec_geometry::{
    V3,
    math::{cos, sin},
};
use superspec_types::rng::SeededRng;
fn soot() -> V3 {
    srgb("#2a2927")
}
fn steel() -> V3 {
    srgb("#a4a9aa")
}
fn enamel() -> V3 {
    srgb("#e8e6dc")
}
fn check(d: &Fixture<'_, '_>, at: V3, values: &[f64]) -> Result<(), String> {
    if at
        .iter()
        .chain(values)
        .any(|v| !v.is_finite() || v.abs() > 1000.)
    {
        return Err("Shop fittings require finite bounded coordinates and settings.".into());
    }
    d.mesh.check_work(0)
}
fn on<'a, 'c>(d: &'a mut Fixture<'_, 'c>, pen: Pen) -> Fixture<'a, 'c> {
    Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    }
}
fn lathe(
    d: &mut Fixture<'_, '_>,
    slot: &str,
    base: V3,
    profile: &[[f64; 2]],
    tint: V3,
    sides: usize,
) -> Result<(), String> {
    forms::lathe(
        d,
        slot,
        base,
        profile,
        &Tints::Uniform(&Tint::Color(tint)),
        sides,
        None,
    )
}
fn ball(
    d: &mut Fixture<'_, '_>,
    slot: &str,
    center: V3,
    radius: f64,
    tint: V3,
    stretch: V3,
) -> Result<(), String> {
    forms::ball(
        d,
        slot,
        &Ball {
            center,
            radius,
            stretch,
            sides: 6,
            bands: 3,
        },
        &Tint::Color(tint),
    )
}
pub fn bottle_crate(
    d: &mut Fixture<'_, '_>,
    tint: V3,
    glass: V3,
    cap: V3,
    loaded: bool,
) -> Result<(), String> {
    check(d, tint, &[glass, cap].concat())?;
    let (w, h, depth, wall) = (0.42, 0.27, 0.3, 0.014);
    d.cuboid("paint", [0.; 3], [w, depth, h], tint, "du")?;
    for (low, high, omit) in [
        ([0., 0., h - 0.001], [w, wall, h], "dbf"),
        ([0., depth - wall, h - 0.001], [w, depth, h], "dbf"),
        ([0., wall, h - 0.001], [wall, depth - wall, h], "dlr"),
        ([w - wall, wall, h - 0.001], [w, depth - wall, h], "dlr"),
    ] {
        d.cuboid("paint", low, high, shade(tint, 1.1), omit)?;
    }
    d.face(
        "paint",
        &[
            [wall, depth - wall, h * 0.45],
            [w - wall, depth - wall, h * 0.45],
            [w - wall, depth - wall, h],
            [wall, depth - wall, h],
        ],
        [0., -1., 0.],
        shade(tint, 0.6),
    )?;
    d.face(
        "paint",
        &[
            [wall, wall, h * 0.45],
            [wall, depth - wall, h * 0.45],
            [wall, depth - wall, h],
            [wall, wall, h],
        ],
        [1., 0., 0.],
        shade(tint, 0.7),
    )?;
    d.face(
        "paint",
        &[
            [w - wall, wall, h * 0.45],
            [w - wall, depth - wall, h * 0.45],
            [w - wall, depth - wall, h],
            [w - wall, wall, h],
        ],
        [-1., 0., 0.],
        shade(tint, 0.7),
    )?;
    d.face(
        "paint",
        &[
            [wall, wall, h * 0.45],
            [w - wall, wall, h * 0.45],
            [w - wall, depth - wall, h * 0.45],
            [wall, depth - wall, h * 0.45],
        ],
        [0., 0., 1.],
        shade(tint, 0.35),
    )?;
    front(
        d,
        "paint",
        [w * 0.36, w * 0.64, h * 0.66, h * 0.82],
        -0.001,
        shade(tint, 0.25),
    )?;
    front(
        d,
        "paint",
        [0.03, w - 0.03, h * 0.12, h * 0.4],
        -0.001,
        shade(tint, 0.82),
    )?;
    if loaded {
        for i in 0..6 {
            for j in 0..4 {
                let at = [
                    wall + 0.033 + i as f64 * ((w - 2. * wall - 0.066) / 5.),
                    wall + 0.033 + j as f64 * ((depth - 2. * wall - 0.066) / 3.),
                    h * 0.45,
                ];
                lathe(
                    d,
                    "pane",
                    at,
                    &[[0.028, 0.], [0.028, 0.05], [0.012, 0.13], [0.011, 0.17]],
                    glass,
                    5,
                )?;
                lathe(
                    d,
                    "metal",
                    at,
                    &[[0.014, 0.17], [0.014, 0.182], [0., 0.182]],
                    cap,
                    5,
                )?;
            }
        }
    }
    Ok(())
}
pub fn rail(d: &mut Fixture<'_, '_>, a: V3, b: V3, ceiling: f64, tint: V3) -> Result<(), String> {
    check(d, a, &[b.as_slice(), tint.as_slice(), &[ceiling]].concat())?;
    if ceiling <= a[2] + 0.01 || ceiling <= b[2] + 0.01 {
        return Err("Rail hangers must reach a higher ceiling.".into());
    }
    d.tube("metal", &[a, b], 0.012, tint, 6)?;
    for t in [0.06, 0.94] {
        let p: V3 = std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t);
        d.bar(
            "metal",
            [p[0], p[1], p[2] + 0.01],
            [p[0], p[1], ceiling],
            0.012,
            tint,
        )?;
    }
    Ok(())
}
pub fn hook(d: &mut Fixture<'_, '_>, at: V3, drop: f64, tint: V3) -> Result<V3, String> {
    check(d, at, &[drop])?;
    if !(0.02..=2.).contains(&drop) {
        return Err("Shop hook drop must be 0.02–2 meters.".into());
    }
    let [x, y, z] = at;
    d.tube(
        "metal",
        &[
            [x, y + 0.016, z - 0.004],
            [x, y, z + 0.016],
            [x, y - 0.014, z - 0.006],
            [x, y, z - drop * 0.6],
            [x, y + 0.012, z - drop - 0.012],
            [x, y, z - drop + 0.004],
        ],
        0.003,
        tint,
        4,
    )?;
    Ok([x, y, z - drop])
}
pub fn pendant(
    d: &mut Fixture<'_, '_>,
    at: V3,
    drop: f64,
    shade_color: Option<V3>,
) -> Result<(), String> {
    check(d, at, &[drop])?;
    if !(0.08..=20.).contains(&drop) {
        return Err("Pendant drop must leave room for its socket.".into());
    }
    let [x, y, _] = at;
    let z = at[2] - drop;
    d.bar("paint", at, [x, y, z + 0.07], 0.006, soot())?;
    lathe(
        d,
        "paint",
        [x, y, z + 0.03],
        &[[0.016, 0.], [0.016, 0.045], [0., 0.045]],
        shade(soot(), 1.5),
        6,
    )?;
    ball(d, "pane", [x, y, z], 0.03, [1., 0.94, 0.78], [1., 1., 1.2])?;
    if let Some(color) = shade_color {
        lathe(
            d,
            "paint",
            [x, y, z - 0.03],
            &[[0.17, 0.], [0.06, 0.1], [0.028, 0.125]],
            color,
            10,
        )?;
        lathe(
            d,
            "paint",
            [x, y, z - 0.03],
            &[[0.028, 0.123], [0.06, 0.098], [0.168, -0.001]],
            shade(enamel(), 1.05),
            10,
        )?;
    }
    Ok(())
}
pub fn tube_light(d: &mut Fixture<'_, '_>, a: V3, b: V3) -> Result<(), String> {
    check(d, a, &b)?;
    if b[0] - a[0] < 0.1 || a[1] != b[1] || a[2] != b[2] {
        return Err("Batten endpoints must span at least 0.1 meters along X.".into());
    }
    d.cuboid(
        "paint",
        [a[0], a[1] - 0.03, a[2] - 0.045],
        [b[0], a[1] + 0.03, a[2]],
        shade(enamel(), 0.95),
        "u",
    )?;
    d.tube(
        "pane",
        &[
            [a[0] + 0.03, a[1], a[2] - 0.066],
            [b[0] - 0.03, a[1], a[2] - 0.066],
        ],
        0.016,
        [1., 1., 0.96],
        6,
    )?;
    for x in [a[0] + 0.012, b[0] - 0.042] {
        d.cuboid(
            "paint",
            [x, a[1] - 0.02, a[2] - 0.085],
            [x + 0.03, a[1] + 0.02, a[2] - 0.045],
            shade(enamel(), 0.8),
            "u",
        )?;
    }
    Ok(())
}
pub fn ceiling_fan(
    d: &mut Fixture<'_, '_>,
    at: V3,
    drop: f64,
    span: f64,
    rng: &mut SeededRng,
) -> Result<(), String> {
    check(d, at, &[drop, span])?;
    if !(0.12..=10.).contains(&drop) || !(0.4..=10.).contains(&span) {
        return Err("Fan requires bounded drop and blade span.".into());
    }
    let hub = [at[0], at[1], at[2] - drop];
    let paint = shade(enamel(), rng.float(0.75, 1.));
    d.bar("metal", at, hub, 0.02, shade(steel(), 0.7))?;
    lathe(
        d,
        "metal",
        [hub[0], hub[1], hub[2] - 0.07],
        &[
            [0., 0.],
            [0.07, 0.01],
            [0.09, 0.05],
            [0.05, 0.1],
            [0.02, 0.11],
        ],
        paint,
        10,
    )?;
    let start = rng.float(0., PI * 2.);
    for blade in 0..3 {
        let pen = forms::rotated(
            &d.pen,
            [hub[0], hub[1], hub[2] - 0.02],
            2,
            start + blade as f64 * PI * 2. / 3.,
        )?;
        let arm = &mut on(d, pen);
        arm.cuboid(
            "metal",
            [0.07, -0.012, -0.004],
            [0.18, 0.012, 0.004],
            shade(steel(), 0.6),
            "",
        )?;
        arm.cuboid(
            "paint",
            [0.16, -0.06, -0.005],
            [span / 2., 0.06, 0.005],
            paint,
            "",
        )?;
    }
    Ok(())
}
pub fn stool(d: &mut Fixture<'_, '_>, at: V3, tint: V3) -> Result<(), String> {
    check(d, at, &tint)?;
    let [x, y, z] = at;
    lathe(
        d,
        "timber",
        [x, y, z + 0.42],
        &[[0.15, 0.], [0.155, 0.03], [0., 0.03]],
        tint,
        10,
    )?;
    for leg in 0..4 {
        let angle = PI / 4. + leg as f64 * PI / 2.;
        let (c, s) = (cos(angle), sin(angle));
        d.bar(
            "timber",
            [x + c * 0.15, y + s * 0.15, z + 0.004],
            [x + c * 0.1, y + s * 0.1, z + 0.42],
            0.028,
            shade(tint, 0.85),
        )?;
        let next = angle + PI / 2.;
        d.bar(
            "timber",
            [x + c * 0.135, y + s * 0.135, z + 0.14],
            [x + cos(next) * 0.135, y + sin(next) * 0.135, z + 0.14],
            0.018,
            shade(tint, 0.8),
        )?;
    }
    Ok(())
}
pub fn price_card(d: &mut Fixture<'_, '_>, at: V3, rng: &mut SeededRng) -> Result<(), String> {
    check(d, at, &[])?;
    let [x, y, z] = at;
    let stick = rng.float(0.1, 0.17);
    let w = rng.float(0.09, 0.13);
    let h = w * 0.72;
    d.bar("timber", at, [x, y, z + stick], 0.005, srgb("#c8b888"))?;
    let paper = srgb(rng.pick(&["#f4e04a", "#f0ece0", "#e8483a", "#f4a030"]));
    let ink = srgb(if paper[1] > 0.5 { "#b01e1a" } else { "#f4ecd8" });
    let card = [
        [x - w / 2., y - 0.004, z + stick - h * 0.15],
        [x + w / 2., y - 0.004, z + stick - h * 0.15],
        [x + w / 2., y - 0.004, z + stick + h * 0.85],
        [x - w / 2., y - 0.004, z + stick + h * 0.85],
    ];
    d.pen.sheet(
        d.mesh,
        "paint",
        &card,
        &Tint::Color(paper),
        &Tint::Color(shade(paper, 0.7)),
        d.repeat("paint"),
    )?;
    front(
        d,
        "paint",
        [
            x - w * 0.34,
            x + w * 0.05,
            z + stick + h * 0.18,
            z + stick + h * 0.66,
        ],
        y - 0.005,
        ink,
    )?;
    front(
        d,
        "paint",
        [
            x + w * 0.12,
            x + w * 0.36,
            z + stick + h * 0.18,
            z + stick + h * 0.42,
        ],
        y - 0.005,
        ink,
    )?;
    front(
        d,
        "paint",
        [x - w * 0.34, x + w * 0.36, z + stick, z + stick + h * 0.08],
        y - 0.005,
        shade(ink, 0.8),
    )
}
pub fn counter_scale(d: &mut Fixture<'_, '_>, at: V3, tint: V3) -> Result<(), String> {
    check(d, at, &tint)?;
    let [x, y, z] = at;
    d.cuboid(
        "metal",
        [x - 0.13, y - 0.1, z],
        [x + 0.13, y + 0.1, z + 0.06],
        tint,
        "d",
    )?;
    d.cuboid(
        "metal",
        [x - 0.11, y - 0.07, z + 0.06],
        [x + 0.11, y + 0.07, z + 0.3],
        shade(tint, 0.92),
        "d",
    )?;
    d.disc(
        "paint",
        [x, y - 0.071, z + 0.18],
        0.092,
        [0., -1., 0.],
        shade(enamel(), 1.05),
        14,
    )?;
    d.bar(
        "paint",
        [x, y - 0.073, z + 0.18],
        [x + 0.045, y - 0.073, z + 0.235],
        0.006,
        srgb("#b01e1a"),
    )?;
    for tick in 0..8 {
        let angle = tick as f64 / 8. * PI * 2.;
        d.bar(
            "paint",
            [
                x + cos(angle) * 0.07,
                y - 0.072,
                z + 0.18 + sin(angle) * 0.07,
            ],
            [
                x + cos(angle) * 0.086,
                y - 0.072,
                z + 0.18 + sin(angle) * 0.086,
            ],
            0.005,
            soot(),
        )?;
    }
    d.bar(
        "metal",
        [x, y, z + 0.3],
        [x, y, z + 0.34],
        0.03,
        shade(tint, 0.7),
    )?;
    lathe(
        d,
        "metal",
        [x, y, z + 0.34],
        &[
            [0.03, 0.],
            [0.15, 0.03],
            [0.16, 0.045],
            [0.15, 0.04],
            [0.03, 0.012],
            [0., 0.012],
        ],
        shade(steel(), 1.1),
        12,
    )
}
pub fn hanging_scale(d: &mut Fixture<'_, '_>, at: V3, drop: f64, tint: V3) -> Result<(), String> {
    check(d, at, &[drop])?;
    if !(0.5..=20.).contains(&drop) {
        return Err("Hanging scale requires room for its dial and chains.".into());
    }
    let [x, y, _] = at;
    let dial = at[2] - (drop * 0.45).min(0.5);
    d.bar("metal", at, [x, y, dial + 0.11], 0.008, shade(steel(), 0.6))?;
    lathe(
        d,
        "metal",
        [x, y, dial - 0.11],
        &[[0., 0.], [0.1, 0.02], [0.12, 0.11], [0.1, 0.2], [0., 0.22]],
        tint,
        10,
    )?;
    d.disc(
        "paint",
        [x, y - 0.118, dial],
        0.085,
        [0., -1., 0.],
        shade(enamel(), 1.05),
        12,
    )?;
    d.bar(
        "paint",
        [x, y - 0.12, dial],
        [x - 0.03, y - 0.12, dial + 0.06],
        0.006,
        srgb("#b01e1a"),
    )?;
    let pan = at[2] - drop;
    d.bar(
        "metal",
        [x, y, dial - 0.11],
        [x, y, dial - 0.16],
        0.006,
        shade(steel(), 0.6),
    )?;
    for chain in 0..3 {
        let angle = PI / 2. + chain as f64 * PI * 2. / 3.;
        d.bar(
            "metal",
            [x, y, dial - 0.16],
            [x + cos(angle) * 0.15, y + sin(angle) * 0.15, pan],
            0.004,
            shade(steel(), 0.7),
        )?;
    }
    lathe(
        d,
        "metal",
        [x, y, pan - 0.05],
        &[
            [0.04, 0.],
            [0.14, 0.03],
            [0.158, 0.05],
            [0.15, 0.048],
            [0.04, 0.01],
            [0., 0.01],
        ],
        shade(steel(), 1.05),
        12,
    )
}
pub fn calendar(
    d: &mut Fixture<'_, '_>,
    at: V3,
    rng: &mut SeededRng,
    textured: bool,
) -> Result<(), String> {
    check(d, at, &[])?;
    if !textured {
        return Ok(());
    }
    let pen = d
        .pen
        .facing([at[0], at[1], at[2] - 0.225], [1., 0., 0.], [0., -1., 0.])?;
    posters::draw(
        &mut on(d, pen),
        &posters::Options {
            design: "calendar".into(),
            width: 0.3,
            rotation: rng.float(-2., 2.),
            seed: Some("shop-calendar".into()),
            layer: 0,
            wear: 0.3,
        },
        true,
    )
}
pub fn wall_clock(d: &mut Fixture<'_, '_>, at: V3, rng: &mut SeededRng) -> Result<(), String> {
    check(d, at, &[])?;
    let [x, y, z] = at;
    d.disc(
        "paint",
        [x, y - 0.006, z],
        0.15,
        [0., -1., 0.],
        shade(soot(), 1.6),
        14,
    )?;
    d.disc(
        "paint",
        [x, y - 0.012, z],
        0.132,
        [0., -1., 0.],
        shade(enamel(), 1.05),
        14,
    )?;
    let hour = rng.float(0., PI * 2.);
    let minute = rng.float(0., PI * 2.);
    d.bar(
        "paint",
        [x, y - 0.016, z],
        [x + cos(hour) * 0.07, y - 0.016, z + sin(hour) * 0.07],
        0.012,
        soot(),
    )?;
    d.bar(
        "paint",
        [x, y - 0.018, z],
        [x + cos(minute) * 0.11, y - 0.018, z + sin(minute) * 0.11],
        0.008,
        soot(),
    )
}
