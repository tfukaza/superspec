//! Rooftop cisterns with structural supports, access hardware and procedural plumbing.
use super::{
    fixture::Fixture,
    metal::{self, Binding, Kind},
    palette::{shade, srgb},
    pen::Tint,
    pipes::{Options, PipeSystem},
    steel::{self, Finish, Plate, Steel},
    wall::finish_seed,
    weathered,
};
use std::f64::consts::PI;
use superspec_geometry::{
    V3,
    math::{cos, hypot, sin},
};
use superspec_types::rng::SeededRng;

#[derive(Clone, serde::Deserialize)]
pub struct Settings {
    pub weathering: weathered::Settings,
    pub brushed: Binding,
}
fn validate(center: V3, settings: &Settings) -> Result<(), String> {
    if center.iter().any(|v| !v.is_finite() || v.abs() > 10000.)
        || !settings.brushed.repeat.is_finite()
        || settings.brushed.repeat <= 0.
    {
        return Err("Tank needs a bounded finite center and positive brushed-metal repeat.".into());
    }
    PipeSystem::new(pipe_options(settings, [1.; 3], "tank-feed"))?;
    Ok(())
}
fn pipe_options(settings: &Settings, color: V3, variant: &str) -> Options {
    Options {
        radius: 0.03,
        color,
        sides: 16,
        variant: variant.into(),
        weathering: Some(settings.weathering.clone()),
    }
}
fn vessel(
    drawing: &mut Fixture<'_, '_>,
    radius: f64,
    height: f64,
    paint: V3,
    settings: &Settings,
    tint: V3,
) -> Result<(), String> {
    let profile = [
        [radius * 0.58, 0.],
        [radius * 0.84, 0.035],
        [radius * 0.96, 0.085],
        [radius, 0.14],
        [radius, height - 0.07],
        [radius * 0.98, height - 0.035],
        [radius * 0.94, height - 0.012],
        [radius * 0.72, height],
        [radius * 0.38, height + 0.008],
        [radius * 0.24, height + 0.01],
    ];
    let mut distances = [0.; 10];
    for i in 1..10 {
        distances[i] = distances[i - 1]
            + hypot(
                profile[i][0] - profile[i - 1][0],
                profile[i][1] - profile[i - 1][1],
            );
    }
    let circumference = 2. * PI * radius;
    let first = drawing.mesh.triangle_count();
    let active = settings.weathering.texture_density > 0. && !drawing.mesh.surveying();
    let point = |row: usize, side: usize| {
        let angle = side as f64 / 48. * 2. * PI;
        let previous = profile[row.saturating_sub(1)];
        let next = profile[(row + 1).min(9)];
        let dr = next[0] - previous[0];
        let dz = next[1] - previous[1];
        let length = hypot(dr, dz);
        (
            [
                cos(angle) * profile[row][0],
                sin(angle) * profile[row][0],
                profile[row][1],
            ],
            [
                cos(angle) * dz / length,
                sin(angle) * dz / length,
                -dr / length,
            ],
            [side as f64 / 48. * circumference, distances[row]],
        )
    };
    let repeat = if active {
        1.
    } else {
        drawing.repeat("brushed-metal")
    };
    for row in 0..9 {
        for side in 0..48 {
            let corners = [
                point(row, side),
                point(row, side + 1),
                point(row + 1, side + 1),
                point(row + 1, side),
            ];
            drawing.pen.face_raw(
                drawing.mesh,
                "brushed-metal",
                &corners.map(|v| v.0),
                corners[0].1,
                &Tint::Color(tint),
                repeat,
                Some(&corners.map(|v| v.2.map(|u| u * repeat))),
                Some(&corners.map(|v| v.1)),
            )?;
        }
    }
    if active {
        let finish = &settings.weathering;
        let material = metal::Settings {
            kind: Kind::Sheet,
            bounds: [0., 0., circumference, distances[9]],
            wrap_u: Some(circumference),
            paint,
            seed: finish_seed("tank-galvanized") as f64,
            paint_coverage: Some(0.),
            wear: finish.wear,
            grime: finish.grime,
            rust: finish.rust * 0.65,
            edge_axes: Some([false, true]),
            metric: Some(metal::Metric {
                scale: [1., 1.],
                gravity: [0., -1.],
            }),
            contacts: None,
            folds: [0.25, 0.55, 0.82]
                .map(|t| metal::Fold {
                    axis: 1,
                    at: distances[3] + (height - 0.3) * t,
                    concave: true,
                })
                .to_vec(),
            substrate: settings.brushed.clone(),
            coating: finish.paint.clone(),
        };
        weathered::receive(
            drawing.mesh,
            first,
            drawing.mesh.triangle_count() - first,
            material,
            finish.texture_density.max(128.),
            "tank/v2".into(),
            None,
        )?;
    }
    let pen = drawing
        .pen
        .with_brushed_metal(drawing.repeat("brushed-metal"))?;
    let mut polished = Fixture {
        pen,
        mesh: drawing.mesh,
        repeats: drawing.repeats,
    };
    polished.disc("metal", [0.; 3], radius * 0.58, [0., 0., -1.], tint, 48)?;
    polished.disc(
        "metal",
        [0., 0., height + 0.01],
        radius * 0.24,
        [0., 0., 1.],
        tint,
        48,
    )
}
fn tinted(value: f64, tint: V3) -> V3 {
    tint.map(|t| value * t)
}
/// What a caller fixes of a round tank instead of leaving it to the seed. The default fixes
/// nothing and draws the seeded tank; the seed's draws are made either way, so a fixed size
/// leaves the rest of the tank the seed's.
#[derive(Clone, Copy, Debug, Default)]
pub struct RoundShape {
    /// Vessel radius in meters (the seed's 0.55–0.67).
    pub radius: Option<f64>,
    /// Vessel height in meters, foot to shoulder (the seed's 1.05–1.3).
    pub height: Option<f64>,
    /// Height of the stand under the vessel in meters (the seed's 0.42–0.53).
    pub stand: Option<f64>,
    /// Evenly spaced rolled ribs round the shell (three hoops when `None`).
    pub ribs: Option<usize>,
    /// Rise of a conical lid over the shoulder in meters; 0 keeps the flat top.
    pub lid: f64,
    /// A tint over the stainless shell, its lid and ribs (none: the plain metal).
    pub shell: Option<V3>,
}
impl RoundShape {
    fn validate(&self) -> Result<(), String> {
        let within = |value: Option<f64>, range: std::ops::RangeInclusive<f64>| {
            value.is_none_or(|v| range.contains(&v))
        };
        if !within(self.radius, 0.3..=1.5)
            || !within(self.height, 0.6..=3.)
            || !within(self.stand, 0.2..=3.)
            || self.ribs.is_some_and(|n| n > 24)
            || !(0. ..=1.).contains(&self.lid)
            || self.shell.is_some_and(|tint| {
                tint.iter()
                    .any(|v| !v.is_finite() || !(0. ..=2.).contains(v))
            })
        {
            return Err("Round tank needs radius 0.3–1.5 m, height 0.6–3 m, stand 0.2–3 m, at most 24 ribs and a lid of 0–1 m.".into());
        }
        Ok(())
    }
}
/// Draws within the source's two-meter rooftop footprint; caller owns the detached host and RNG stream.
pub fn round_tank(
    drawing: &mut Fixture<'_, '_>,
    center: V3,
    rng: &mut SeededRng,
    settings: &Settings,
) -> Result<(), String> {
    shaped_round_tank(drawing, center, rng, settings, &RoundShape::default())
}
/// A conical lid from an eave just proud of the shell at `z` up to a collar of `top` radius.
fn cone_lid(
    drawing: &mut Fixture<'_, '_>,
    radius: f64,
    z: f64,
    rise: f64,
    top: f64,
    tint: V3,
) -> Result<(), String> {
    let eave = radius + 0.025;
    // The skirt's foot, its eave, then the cone to its collar.
    let profile = [[eave, z - 0.05], [eave, z], [top, z + rise]];
    let slant = hypot(eave - top, rise);
    let normals = [[1., 0.], [1., 0.], [rise / slant, (eave - top) / slant]];
    let sides = 48;
    let repeat = drawing.repeat("brushed-metal");
    for row in 0..2 {
        // The skirt is vertical; the cone takes its own slant on both its rows.
        let normal = if row == 0 { normals[0] } else { normals[2] };
        for side in 0..sides {
            let corner = |r: usize, s: usize| {
                let angle = s as f64 / sides as f64 * 2. * PI;
                let [radius, z] = profile[r];
                (
                    [cos(angle) * radius, sin(angle) * radius, z],
                    [cos(angle) * normal[0], sin(angle) * normal[0], normal[1]],
                    [s as f64 / sides as f64 * 2. * PI * eave, z],
                )
            };
            let corners = [
                corner(row, side),
                corner(row, side + 1),
                corner(row + 1, side + 1),
                corner(row + 1, side),
            ];
            drawing.pen.face_raw(
                drawing.mesh,
                "brushed-metal",
                &corners.map(|v| v.0),
                corners[0].1,
                &Tint::Color(tinted(0.92, tint)),
                repeat,
                Some(&corners.map(|v| v.2.map(|u| u * repeat))),
                Some(&corners.map(|v| v.1)),
            )?;
        }
    }
    // The eave's underside, a ring facing down between the skirt and the shell.
    let inner = radius - 0.01;
    for side in 0..sides {
        let at = |r: f64, s: usize| {
            let angle = s as f64 / sides as f64 * 2. * PI;
            [cos(angle) * r, sin(angle) * r, z - 0.05]
        };
        drawing.face(
            "metal",
            &[
                at(eave, side + 1),
                at(eave, side),
                at(inner, side),
                at(inner, side + 1),
            ],
            [0., 0., -1.],
            tinted(0.55, tint),
        )?;
    }
    // The collar the manhole stands on.
    drawing.disc(
        "metal",
        [0., 0., z + rise],
        top,
        [0., 0., 1.],
        tinted(0.88, tint),
        48,
    )
}
/// [`round_tank`] with whatever `shape` fixes of it.
pub fn shaped_round_tank(
    drawing: &mut Fixture<'_, '_>,
    center: V3,
    rng: &mut SeededRng,
    settings: &Settings,
    shape: &RoundShape,
) -> Result<(), String> {
    validate(center, settings)?;
    shape.validate()?;
    let pen = drawing.pen.moved(center)?;
    let mut drawing = Fixture {
        pen,
        mesh: drawing.mesh,
        repeats: drawing.repeats,
    };
    let radius = rng.float(0.55, 0.67);
    let height = rng.float(1.05, 1.3);
    let base = rng.float(0.42, 0.53);
    let paint = srgb(rng.pick(&["#b6b8ae", "#a5ada5", "#9da9a4"]));
    let radius = shape.radius.unwrap_or(radius);
    let height = shape.height.unwrap_or(height);
    let base = shape.stand.unwrap_or(base);
    // What stands on the top (the manhole, its bolts and handle, the ladder's head) rises
    // with a conical lid.
    let lift = shape.lid;
    let tint = shape.shell.unwrap_or([1.; 3]);
    let steel = Steel {
        slot: "metal",
        tint: srgb("#626d64"),
        finish: Finish::Metal(Some(&settings.weathering)),
    };
    let hardware = drawing.pen.with_weathering(
        drawing.mesh,
        settings.weathering.clone(),
        Kind::Frame,
        None,
        vec!["paint".into(), "metal".into()],
    )?;
    let leg = radius * 0.63;
    for x in [-leg, leg] {
        for y in [-leg, leg] {
            drawing.cuboid(
                "concrete",
                [x - 0.12, y - 0.12, 0.],
                [x + 0.12, y + 0.12, 0.1],
                shade(srgb("#b4b0a4"), 0.7),
                "d",
            )?;
            let mut hd = Fixture {
                pen: hardware.clone(),
                mesh: drawing.mesh,
                repeats: drawing.repeats,
            };
            steel::fixing_plate(
                &mut hd,
                steel,
                &Plate {
                    center: [x, y, 0.1],
                    across: [1., 0., 0.],
                    up: [0., 1., 0.],
                    out: [0., 0., 1.],
                    size: [0.18, 0.18],
                    bolts: 4,
                },
            )?;
            steel::member(
                &mut hd,
                steel,
                [[x, y, 0.11], [x, y, base + 0.08]],
                [1., 0., 0.],
                [0.065, 0.065],
            )?;
        }
    }
    {
        let mut hd = Fixture {
            pen: hardware.clone(),
            mesh: drawing.mesh,
            repeats: drawing.repeats,
        };
        for y in [-leg, leg] {
            steel::member(
                &mut hd,
                steel,
                [[-leg - 0.08, y, base - 0.04], [leg + 0.08, y, base - 0.04]],
                [0., 1., 0.],
                [0.1, 0.08],
            )?;
            steel::member(
                &mut hd,
                steel,
                [[-leg, y, 0.14], [leg, y, base - 0.07]],
                [0., 1., 0.],
                [0.025, 0.025],
            )?;
        }
        for x in [-leg, leg] {
            steel::member(
                &mut hd,
                steel,
                [[x, -leg, 0.14], [x, leg, base - 0.07]],
                [1., 0., 0.],
                [0.025, 0.025],
            )?;
        }
    }
    vessel(
        &mut Fixture {
            pen: drawing.pen.moved([0., 0., base])?,
            mesh: drawing.mesh,
            repeats: drawing.repeats,
        },
        radius,
        height,
        paint,
        settings,
        tint,
    )?;
    let polished = drawing
        .pen
        .with_brushed_metal(drawing.repeat("brushed-metal"))?;
    // Three hoops, or the caller's evenly spaced rolled ribs, a little fuller and prouder.
    let (hoops, rib, proud): (Vec<f64>, f64, f64) = match shape.ribs {
        None => (
            [0.25, 0.55, 0.82]
                .map(|t| base + 0.14 + (height - 0.3) * t)
                .to_vec(),
            0.009,
            0.004,
        ),
        Some(n) => (
            (0..n)
                .map(|i| base + 0.1 + (height - 0.2) * (i as f64 + 0.5) / n as f64)
                .collect(),
            0.013,
            0.006,
        ),
    };
    for z in hoops {
        let path: Vec<_> = (0..49)
            .map(|i| {
                [
                    cos(i as f64 / 48. * PI * 2.) * (radius + proud),
                    sin(i as f64 / 48. * PI * 2.) * (radius + proud),
                    z,
                ]
            })
            .collect();
        Fixture {
            pen: polished.clone(),
            mesh: drawing.mesh,
            repeats: drawing.repeats,
        }
        .tube("metal", &path, rib, tinted(0.85, tint), 6)?;
    }
    if lift > 0. {
        cone_lid(
            &mut Fixture {
                pen: polished.clone(),
                mesh: drawing.mesh,
                repeats: drawing.repeats,
            },
            radius,
            base + height - 0.01,
            lift + 0.01,
            0.23,
            tint,
        )?;
    }
    let crown = base + height + lift;
    Fixture {
        pen: polished,
        mesh: drawing.mesh,
        repeats: drawing.repeats,
    }
    .drum(
        "metal",
        [0., 0., crown + 0.01],
        0.21,
        0.045,
        tinted(0.9, tint),
        32,
    )?;
    {
        let mut hd = Fixture {
            pen: hardware.clone(),
            mesh: drawing.mesh,
            repeats: drawing.repeats,
        };
        for i in 0..6 {
            let angle = i as f64 * PI / 3.;
            steel::bolt(
                &mut hd,
                steel,
                [0.18 * cos(angle), 0.18 * sin(angle), crown + 0.057],
                [0., 0., 1.],
                0.009,
            )?;
        }
        hd.tube(
            "metal",
            &[
                [-0.07, 0., crown + 0.057],
                [-0.07, 0., crown + 0.112],
                [0.07, 0., crown + 0.112],
                [0.07, 0., crown + 0.057],
            ],
            0.009,
            steel.tint,
            8,
        )?;
        let ladder_x = -radius - 0.13;
        let top = crown + 0.16;
        for y in [-0.16, 0.16] {
            steel::member(
                &mut hd,
                steel,
                [[ladder_x, y, 0.04], [ladder_x, y, top]],
                [0., 1., 0.],
                [0.028, 0.026],
            )?;
            for z in [base + 0.25, base + height - 0.22] {
                steel::member(
                    &mut hd,
                    steel,
                    [[ladder_x, y, z], [-radius + 0.04, y, z]],
                    [0., 1., 0.],
                    [0.022, 0.025],
                )?;
            }
        }
        let mut z = 0.22;
        while z < top - 0.13 {
            hd.tube(
                "metal",
                &[[ladder_x, -0.16, z], [ladder_x, 0.16, z]],
                0.012,
                steel.tint,
                8,
            )?;
            z += 0.25;
        }
    }
    let pipes = PipeSystem::new(pipe_options(settings, srgb("#7b8176"), "tank-feed"))?;
    pipes.run(
        &mut drawing,
        &[
            [radius - 0.025, 0., base + height - 0.25],
            [radius + 0.16, 0., base + height - 0.25],
            [radius + 0.16, 0., 0.06],
        ],
        None,
    )?;
    pipes.coupling(
        &mut drawing,
        [radius + 0.03, 0., base + height - 0.25],
        [1., 0., 0.],
    )?;
    pipes.run(
        &mut drawing,
        &[
            [0., 0., base + 0.01],
            [0., 0., 0.25],
            [0., radius + 0.14, 0.25],
            [0., radius + 0.14, 0.06],
        ],
        None,
    )?;
    Fixture {
        pen: hardware,
        mesh: drawing.mesh,
        repeats: drawing.repeats,
    }
    .drum(
        "metal",
        [0., radius * 0.55, 0.25],
        0.045,
        0.08,
        steel.tint,
        12,
    )?;
    drawing.disc(
        "paint",
        [0., radius * 0.55, 0.34],
        0.07,
        [0., 0., 1.],
        srgb("#734338"),
        12,
    )
}
pub fn box_tank(
    drawing: &mut Fixture<'_, '_>,
    center: V3,
    rng: &mut SeededRng,
    settings: &Settings,
) -> Result<(), String> {
    validate(center, settings)?;
    let pen = drawing.pen.moved(center)?;
    let mut drawing = Fixture {
        pen,
        mesh: drawing.mesh,
        repeats: drawing.repeats,
    };
    let w = rng.float(1.3, 1.6);
    let d = rng.float(1., 1.3);
    let h = rng.float(0.85, 1.1);
    let base = 0.24;
    let paint = srgb("#b5b39b");
    let steel = Steel {
        slot: "metal",
        tint: srgb("#757b70"),
        finish: Finish::Metal(Some(&settings.weathering)),
    };
    let hardware = drawing.pen.with_weathering(
        drawing.mesh,
        settings.weathering.clone(),
        Kind::Sheet,
        None,
        vec!["paint".into(), "metal".into()],
    )?;
    for x in [-w * 0.3, w * 0.3] {
        drawing.cuboid(
            "concrete",
            [x - 0.1, -d / 2., 0.],
            [x + 0.1, d / 2., base],
            shade(srgb("#b4b0a4"), 0.72),
            "d",
        )?;
    }
    for (origin, tangent, normal, width) in [
        ([-w / 2., -d / 2., base], [1., 0., 0.], [0., -1., 0.], w),
        ([w / 2., -d / 2., base], [0., 1., 0.], [1., 0., 0.], d),
        ([w / 2., d / 2., base], [-1., 0., 0.], [0., 1., 0.], w),
        ([-w / 2., d / 2., base], [0., -1., 0.], [-1., 0., 0.], d),
    ] {
        let mut face = Fixture {
            pen: hardware.facing(origin, tangent, normal)?,
            mesh: drawing.mesh,
            repeats: drawing.repeats,
        };
        let count = super::mesh::rounded(width / 0.7, 1.).max(1.) as usize;
        for i in 0..count {
            let x0 = i as f64 * width / count as f64;
            let x1 = (i + 1) as f64 * width / count as f64;
            let rim = [[x0, 0., 0.], [x1, 0., 0.], [x1, 0., h], [x0, 0., h]];
            let raised = [
                [x0 + 0.09, 0.045, 0.09],
                [x1 - 0.09, 0.045, 0.09],
                [x1 - 0.09, 0.045, h - 0.09],
                [x0 + 0.09, 0.045, h - 0.09],
            ];
            for k in 0..4 {
                face.face(
                    "metal",
                    &[rim[k], rim[(k + 1) % 4], raised[(k + 1) % 4], raised[k]],
                    [0., 1., 0.],
                    paint,
                )?;
            }
            face.face("metal", &raised, [0., 1., 0.], paint)?;
            for x in [x0 + 0.025, x1 - 0.025] {
                face.cuboid(
                    "metal",
                    [x - 0.012, 0.002, 0.02],
                    [x + 0.012, 0.016, h - 0.02],
                    steel.tint,
                    "b",
                )?;
                let mut z = 0.07;
                while z < h {
                    steel::bolt(&mut face, steel, [x, 0.017, z], [0., 1., 0.], 0.007)?;
                    z += 0.2;
                }
            }
        }
    }
    {
        let mut hd = Fixture {
            pen: hardware,
            mesh: drawing.mesh,
            repeats: drawing.repeats,
        };
        hd.cuboid(
            "metal",
            [-w / 2. - 0.035, -d / 2. - 0.035, base + h],
            [w / 2. + 0.035, d / 2. + 0.035, base + h + 0.055],
            paint,
            "",
        )?;
        hd.cuboid(
            "metal",
            [-0.25, -0.22, base + h + 0.055],
            [0.25, 0.22, base + h + 0.105],
            shade(paint, 0.9),
            "d",
        )?;
    }
    drawing.face(
        "metal",
        &[
            [-w / 2., -d / 2., base],
            [w / 2., -d / 2., base],
            [w / 2., d / 2., base],
            [-w / 2., d / 2., base],
        ],
        [0., 0., -1.],
        paint,
    )?;
    PipeSystem::new(pipe_options(settings, steel.tint, "pipe-standard"))?.run(
        &mut drawing,
        &[
            [w / 2. - 0.01, 0., base + h * 0.75],
            [w / 2. + 0.13, 0., base + h * 0.75],
            [w / 2. + 0.13, 0., 0.05],
        ],
        None,
    )
}
