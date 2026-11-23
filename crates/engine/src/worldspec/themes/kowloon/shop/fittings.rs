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
    // A hook is 6 mm of bent wire: a reduced level hangs its goods without drawing it.
    if d.mesh.graphics_detail().reduced() {
        return Ok([x, y, z - drop]);
    }
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
/// Twin fluorescent reflector. Section coordinates are [cross-fixture Y, elevation Z].
pub fn twin_strip_housing(
    d: &mut Fixture<'_, '_>,
    length: f64,
    enclosed: bool,
) -> Result<(), String> {
    if !(0.4..=2.4).contains(&length) {
        return Err("Twin-strip length must be between 0.4 and 2.4 metres.".into());
    }
    let half = length * 0.5;
    let span = [-half + 0.003, half - 0.003];
    let enamel = if enclosed {
        srgb("#9ea99e")
    } else {
        [0.94, 0.96, 0.92]
    };
    let aluminum = [0.91, 0.94, 0.95];

    // A surface-mounted folded channel, with the narrower ballast cover opening down.
    d.molding(
        "case",
        span,
        &[
            [-0.064, 0.],
            [0.064, 0.],
            [0.073, -0.012],
            [0.073, -0.033],
            [-0.073, -0.033],
            [-0.073, -0.012],
        ],
        enamel,
    )?;
    if enclosed {
        return enclosed_batten(d, length, enamel);
    }
    d.molding(
        "case",
        [-half + 0.023, half - 0.023],
        &[
            [-0.035, -0.031],
            [0.035, -0.031],
            [0.027, -0.064],
            [0.021, -0.071],
            [-0.021, -0.071],
            [-0.027, -0.064],
        ],
        enamel,
    )?;

    for sign in [-1., 1.] {
        // The underside has a shallow crease above each tube and a turned-down outer visor.
        let folds = [
            ([0.031, -0.044], [0.069, -0.031], [0.88, 0.90, 0.85]),
            ([0.069, -0.031], [0.093, -0.031], [0.91, 0.93, 0.88]),
            ([0.093, -0.031], [0.139, -0.061], [0.95, 0.96, 0.91]),
            ([0.139, -0.061], [0.143, -0.066], [0.85, 0.88, 0.83]),
        ];
        for ([a, za], [b, zb], tint) in folds {
            d.molding(
                "reflector",
                span,
                &[
                    [sign * a, za],
                    [sign * b, zb],
                    [sign * b, zb - 0.002],
                    [sign * a, za - 0.002],
                ],
                tint,
            )?;
        }
        d.molding(
            "case",
            span,
            &[
                [sign * 0.143, -0.066],
                [sign * 0.145, -0.066],
                [sign * 0.145, -0.081],
                [sign * 0.140, -0.081],
                [sign * 0.140, -0.078],
            ],
            enamel,
        )?;
        for end in [-1., 1.] {
            open_batten_socket(d, half, sign * 0.068, end, enamel, aluminum)?;
        }
    }
    // Stamped end returns hold the wings and protect the cut edge of the reflector.
    for end in [-1., 1.] {
        let x = end * (half - 0.003);
        d.molding(
            "case",
            [x - 0.003, x + 0.003],
            &[
                [-0.143, -0.080],
                [-0.143, -0.064],
                [-0.091, -0.030],
                [-0.070, -0.024],
                [0.070, -0.024],
                [0.091, -0.030],
                [0.143, -0.064],
                [0.143, -0.080],
            ],
            enamel,
        )?;
        let screw = [end * (half - 0.085), 0., -0.073];
        d.drum("hardware", screw, 0.007, 0.0025, aluminum, 20)?;
        d.disc("hardware", screw, 0.007, [0., 0., -1.], aluminum, 20)?;
        // Shallow pressed louvers admit air to the ballast. Their open fronts face the end.
        for louver in 0..3 {
            let x = end * (half - 0.112 - 0.014 * f64::from(louver));
            let rise = x - 0.007 * end;
            d.sheet(
                "case",
                &[
                    [x, -0.012, -0.071],
                    [x, 0.012, -0.071],
                    [rise, 0.010, -0.076],
                    [rise, -0.010, -0.076],
                ],
                enamel,
            )?;
            for side in [-1., 1.] {
                d.sheet(
                    "case",
                    &[
                        [x, side * 0.012, -0.071],
                        [rise, side * 0.012, -0.071],
                        [rise, side * 0.010, -0.076],
                    ],
                    enamel,
                )?;
            }
        }
        d.cuboid(
            "seal",
            [screw[0] - 0.004, -0.0007, -0.0732],
            [screw[0] + 0.004, 0.0007, -0.073],
            [0.78, 0.81, 0.76],
            "",
        )?;
    }
    Ok(())
}

fn open_batten_socket(
    d: &mut Fixture<'_, '_>,
    half: f64,
    y: f64,
    end: f64,
    enamel: V3,
    aluminum: V3,
) -> Result<(), String> {
    let span = |outboard: f64, inboard: f64| {
        if end < 0. {
            [-half + outboard, -half + inboard]
        } else {
            [half - inboard, half - outboard]
        }
    };
    let carrier = [
        [-0.028, -0.032],
        [0.028, -0.032],
        [0.030, -0.039],
        [0.024, -0.062],
        [0.019, -0.068],
        [-0.019, -0.068],
        [-0.024, -0.062],
        [-0.030, -0.039],
    ];
    d.molding(
        "case",
        span(0.002, 0.026),
        &carrier.map(|[u, z]| [y + u, z]),
        enamel,
    )?;

    // The molded G13 body snaps into its steel carrier. Its lid has a recessed seam.
    let body = [
        [-0.017, -0.057],
        [0.017, -0.057],
        [0.020, -0.061],
        [0.020, -0.102],
        [0.017, -0.109],
        [0.011, -0.115],
        [-0.011, -0.115],
        [-0.017, -0.109],
        [-0.020, -0.102],
        [-0.020, -0.061],
    ];
    let shell = body.map(|[u, z]| [y + u, z]);
    let seam = body.map(|[u, z]| [y + 0.91 * u, -0.086 + 0.91 * (z + 0.086)]);
    d.molding("insulator", span(0.004, 0.013), &shell, [0.88, 0.88, 0.83])?;
    d.molding("insulator", span(0.0145, 0.020), &shell, [0.94, 0.93, 0.87])?;
    d.molding("seal", span(0.0128, 0.0147), &seam, [0.84, 0.87, 0.81])?;

    let center = [end * (half - 0.0198), y, -0.099];
    // The rotor surrounds the aluminum cap on the lamp-facing side of the tombstone.
    d.tube(
        "seal",
        &[center, [end * (half - 0.0216), y, -0.099]],
        0.0184,
        [0.94; 3],
        24,
    )?;
    d.disc(
        "seal",
        [end * (half - 0.0216), y, -0.099],
        0.0184,
        [-end, 0., 0.],
        [0.94; 3],
        24,
    )?;

    // Turn in from the socket. A rolled lip closes onto the glass without changing its span.
    let pen = forms::rotated(&d.pen, center, 1, -end * PI / 2.)?;
    lathe(
        &mut on(d, pen),
        "hardware",
        [0.; 3],
        &[
            [0.0163, 0.0015],
            [0.017, 0.0024],
            [0.017, 0.0079],
            [0.0174, 0.0089],
            [0.0174, 0.0106],
            [0.0161, 0.0119],
            [0.0158, 0.0119],
        ],
        aluminum,
        24,
    )
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
    // Its 5 mm stick is left out at a reduced level; the card still stands in the goods.
    if !d.mesh.graphics_detail().reduced() {
        d.bar("timber", at, [x, y, z + stick], 0.005, srgb("#c8b888"))?;
    }
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
/// Enclosed weatherproof cover fitted to the same surface-mounted folded channel.
fn enclosed_batten(d: &mut Fixture<'_, '_>, length: f64, enamel: V3) -> Result<(), String> {
    let half = length / 2.;
    let seal = srgb("#585957");
    let hardware = srgb("#c5c8bf");
    d.molding(
        "case",
        [-half + 0.006, half - 0.006],
        &[
            [-0.092, -0.014],
            [0.092, -0.014],
            [0.125, -0.034],
            [0.131, -0.044],
            [0.130, -0.054],
            [-0.130, -0.054],
            [-0.131, -0.044],
            [-0.125, -0.034],
        ],
        enamel,
    )?;
    d.molding(
        "seal",
        [-half + 0.025, half - 0.025],
        &[
            [-0.123, -0.053],
            [0.123, -0.053],
            [0.120, -0.060],
            [-0.120, -0.060],
        ],
        seal,
    )?;

    // Develop the half-ellipse by physical arclength so each of the eighteen
    // extruded optical prisms coincides with the corresponding texture crest.
    let steps = 108;
    let mut profile = Vec::with_capacity(steps + 1);
    let mut distance = 0.;
    let mut previous: Option<[f64; 2]> = None;
    for i in 0..=steps {
        let theta = PI * (i as f64 / steps as f64 - 0.5);
        let (s, c) = (sin(theta), cos(theta));
        let (y, z) = (0.120 * s, -0.057 - 0.047 * c);
        let n = (0.047 * s).hypot(0.120 * c);
        if let Some([py, pz]) = previous {
            distance += (y - py).hypot(z - pz);
        }
        profile.push((y, z, 0.047 * s / n, -0.120 * c / n, distance));
        previous = Some([y, z]);
    }
    let tip = half - 0.030;
    for pair in profile.windows(2) {
        let vertices = [pair[0], pair[1]].map(|(y, z, ny, nz, arc)| {
            let u = arc / distance;
            let phase = 36. * PI * u;
            let relief = 0.00055 * cos(phase);
            let slope = -0.00055 * 36. * PI * sin(phase) / distance;
            let norm = 1_f64.hypot(slope);
            (
                y + ny * relief,
                z + nz * relief,
                [0., (ny + nz * slope) / norm, (nz - ny * slope) / norm],
                u,
            )
        });
        let (ya, za, na, ua) = vertices[0];
        let (yb, zb, nb, ub) = vertices[1];
        d.pen.face(
            d.mesh,
            "cover",
            &[[-tip, ya, za], [-tip, yb, zb], [tip, yb, zb], [tip, ya, za]],
            na,
            &Tint::Color([1.; 3]),
            1.,
            Some(&[[ua, 0.], [ub, 0.], [ub, 1.], [ua, 1.]]),
            Some(&[na, nb, nb, na]),
        )?;
    }
    // Stamped end closures follow the optic shoulder. A dark narrow gasket is
    // visible immediately inboard of each cap.
    for side in [-1., 1.] {
        for (outer, inner, width, drop, slot, tint) in [
            (half, half - 0.031, 0.133, 0.049, "end", [1.; 3]),
            (half - 0.031, half - 0.035, 0.125, 0.048, "seal", seal),
        ] {
            let mut section = vec![
                [-width + 0.012, -0.024],
                [width - 0.012, -0.024],
                [width, -0.040],
            ];
            for k in 0..=18 {
                let t = PI * k as f64 / 18.;
                section.push([width * cos(t), -0.057 - drop * sin(t)]);
            }
            section.push([-width, -0.040]);
            let span = if side < 0. {
                [-outer, -inner]
            } else {
                [inner, outer]
            };
            d.molding(slot, span, &section, tint)?;
        }
    }
    for side in [-1., 1.] {
        let rolled_edge = [
            [0.129, -0.048],
            [0.132, -0.048],
            [0.132, -0.052],
            [0.129, -0.053],
        ]
        .map(|[y, z]| [side * y, z]);
        d.molding(
            "end",
            [-half + 0.031, half - 0.031],
            &rolled_edge,
            srgb("#c3bdab"),
        )?;
    }
    let stations = if length > 1.6 { 3 } else { 2 };
    for i in 0..stations {
        let x = (i as f64 / (stations - 1) as f64 - 0.5) * (length - 0.19);
        for side in [-1., 1.] {
            let tongue = [
                [0.124, -0.032],
                [0.132, -0.034],
                [0.136, -0.041],
                [0.135, -0.056],
                [0.129, -0.063],
                [0.117, -0.066],
                [0.113, -0.064],
                [0.127, -0.059],
                [0.131, -0.054],
                [0.132, -0.041],
                [0.129, -0.037],
                [0.124, -0.036],
            ];
            let tongue = tongue.map(|[y, z]| [side * y, z]);
            for k in 0..5 {
                let section = [tongue[k], tongue[k + 1], tongue[10 - k], tongue[11 - k]];
                d.molding("clip", [x - 0.010, x + 0.010], &section, hardware)?;
            }
        }
    }
    Ok(())
}
