//! Closed visual roof-access bulkhead with a door reveal, louvers, cap and separately aged hardware.
use super::{
    facade_material::{self, Facade},
    fixture::Fixture,
    massing::Rect,
    metal::{Binding, Kind},
    palette::{shade, srgb},
    pipes::{Options, PipeSystem},
    skin,
    steel::{self, Finish, Plate, Steel},
    wall, weathered,
};
use serde::Deserialize;
use superspec_geometry::V3;
use superspec_types::rng::SeededRng;
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct Plot {
    pub x0: f64,
    pub x1: f64,
    pub y0: f64,
    pub y1: f64,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Masonry {
    pub grime: f64,
    pub wear: f64,
    pub density: f64,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Settings {
    pub weathering: weathered::Settings,
    pub wall: Binding,
    pub concrete: Binding,
    pub wall_tint: V3,
    pub lot_key: String,
    pub masonry: Option<Masonry>,
}
pub fn stair_hut(
    drawing: &mut Fixture<'_, '_>,
    plot: Plot,
    height: f64,
    settings: &Settings,
) -> Result<(), String> {
    let width = plot.x1 - plot.x0;
    let depth = plot.y1 - plot.y0;
    if [plot.x0, plot.x1, plot.y0, plot.y1]
        .iter()
        .any(|v| !v.is_finite() || v.abs() > 1000.)
        || !height.is_finite()
        || !(2.3..=10.).contains(&height)
        || !(1.6..=20.).contains(&width)
        || !(1.5..=20.).contains(&depth)
        || settings.lot_key.len() > 1024
        || settings.weathering.seed.len() > 2048
        || drawing.pen.part.len() > 2048
    {
        return Err(
            "Roof access requires bounded identity and a plot large enough for its door and vents."
                .into(),
        );
    }
    let aging = settings.masonry.clone().unwrap_or(Masonry {
        grime: settings.weathering.grime,
        wear: settings.weathering.wear,
        density: settings.weathering.texture_density,
    });
    let mut rng = SeededRng::from_text(&format!(
        "{}/{}/cladding",
        settings.weathering.seed, settings.lot_key
    ));
    let concrete = rng.chance(0.3);
    rng.chance(0.45);
    let variant = rng.integer(1, 2147483647) as u32 % 2;
    let coat = if concrete {
        &settings.concrete
    } else {
        &settings.wall
    };
    let finish = facade_material::Settings {
        concrete,
        coat: facade_material::Coat {
            material: coat.material.clone(),
            repeat: coat.repeat,
            tint: shade(settings.wall_tint, 0.96),
        },
        backing: settings.concrete.clone(),
        wear: aging.wear,
        density: aging.density,
        variant,
    };
    if !drawing.mesh.surveying() {
        settings.weathering.validate()?;
        Facade::new(&finish)?;
        if !aging.grime.is_finite()
            || !(0. ..=1.).contains(&aging.grime)
            || aging.density <= 0.
            || settings
                .wall_tint
                .iter()
                .any(|v| !v.is_finite() || !(0. ..=4.).contains(v))
        {
            return Err(
                "Roof masonry requires normalized aging, finite tint and positive density.".into(),
            );
        }
    }
    let faces = [
        drawing
            .pen
            .facing([plot.x0, plot.y0, 0.], [1., 0., 0.], [0., -1., 0.])?,
        drawing
            .pen
            .facing([plot.x1, plot.y0, 0.], [0., 1., 0.], [1., 0., 0.])?,
        drawing
            .pen
            .facing([plot.x1, plot.y1, 0.], [-1., 0., 0.], [0., 1., 0.])?,
        drawing
            .pen
            .facing([plot.x0, plot.y1, 0.], [0., -1., 0.], [-1., 0., 0.])?,
    ];
    let door = Rect {
        u0: 0.28,
        u1: 1.2,
        v0: 0.12,
        v1: 2.15,
    };
    let cement = srgb("#b4b0a4");
    for (i, pen) in faces.iter().enumerate() {
        let span = if i % 2 == 0 { width } else { depth };
        let seed = SeededRng::from_text(&format!(
            "{}/{}/roof-access/{i}",
            settings.weathering.seed, settings.lot_key
        ))
        .integer(1, 2147483647) as u32;
        let openings = if i == 0 { vec![door] } else { vec![] };
        let condition = wall::Settings {
            base: Some(0.),
            ledges: vec![wall::Ledge {
                u0: 0.,
                u1: span,
                z: height,
                flow: 0.85,
            }],
            openings: openings
                .iter()
                .map(|r| wall::Opening {
                    u0: r.u0,
                    u1: r.u1,
                    z0: r.v0,
                    z1: r.v1,
                })
                .collect(),
            grime: aging.grime,
            wear: aging.wear,
            seed: f64::from(seed),
        };
        let mut face = Fixture {
            pen: pen.clone(),
            mesh: drawing.mesh,
            repeats: drawing.repeats,
        };
        skin::draw(
            &mut face,
            if concrete { "concrete" } else { "wall" },
            &skin::Settings {
                rects: vec![Rect {
                    u0: 0.,
                    u1: span,
                    v0: 0.,
                    v1: height,
                }],
                openings,
                u_marks: vec![door.u0, door.u1],
                z_marks: vec![door.v0, door.v1],
                finish: finish.clone(),
                condition,
                washes: vec![],
                tiles: None,
            },
            &format!("{}/roof-skin/{i}", pen.part),
        )?;
        let strips = if i == 0 {
            vec![[0., door.u0], [door.u1, span]]
        } else {
            vec![[0., span]]
        };
        for span in strips {
            face.molding(
                "concrete",
                span,
                &[[0.002, 0.], [0.035, 0.], [0.035, 0.17], [0.004, 0.21]],
                shade(cement, 0.55),
            )?;
        }
    }
    let front = &faces[0];
    let lintel = shade(cement, 0.9);
    {
        let mut face = Fixture {
            pen: front.clone(),
            mesh: drawing.mesh,
            repeats: drawing.repeats,
        };
        for x in [door.u0, door.u1 - 0.07] {
            face.cuboid(
                "concrete",
                [x, -0.16, 0.12],
                [x + 0.07, 0.035, door.v1],
                lintel,
                "b",
            )?;
        }
        face.cuboid(
            "concrete",
            [door.u0, -0.16, door.v1 - 0.07],
            [door.u1, 0.045, door.v1 + 0.08],
            lintel,
            "b",
        )?;
    }
    let hardware = front.with_weathering(
        drawing.mesh,
        settings.weathering.clone(),
        Kind::Frame,
        None,
        vec!["paint".into(), "metal".into()],
    )?;
    let steel = Steel {
        slot: "metal",
        tint: srgb("#59635e"),
        finish: Finish::Metal(Some(&settings.weathering)),
    };
    let left = door.u0 + 0.075;
    let right = door.u1 - 0.075;
    let top = door.v1 - 0.075;
    let vent_left = left + 0.1;
    let vent_right = right - 0.1;
    let vent_bottom = 1.28;
    let vent_top = top - 0.12;
    {
        let mut face = Fixture {
            pen: hardware.clone(),
            mesh: drawing.mesh,
            repeats: drawing.repeats,
        };
        face.cuboid(
            "paint",
            [left, -0.14, 0.13],
            [right, -0.105, vent_bottom],
            steel.tint,
            "",
        )?;
        face.cuboid(
            "paint",
            [left, -0.14, vent_top],
            [right, -0.105, top],
            steel.tint,
            "",
        )?;
        for [l, r] in [[left, vent_left], [vent_right, right]] {
            face.cuboid(
                "paint",
                [l, -0.14, vent_bottom],
                [r, -0.105, vent_top],
                steel.tint,
                "",
            )?;
        }
    }
    Fixture {
        pen: front.clone(),
        mesh: drawing.mesh,
        repeats: drawing.repeats,
    }
    .panel(
        "pane",
        [vent_left, vent_right, vent_bottom, vent_top],
        -0.145,
        srgb("#171b18"),
    )?;
    {
        let mut face = Fixture {
            pen: hardware.clone(),
            mesh: drawing.mesh,
            repeats: drawing.repeats,
        };
        let slats = ((vent_top - vent_bottom) / 0.075).ceil() as usize;
        let pitch = (vent_top - vent_bottom) / slats as f64;
        for i in 0..slats {
            let z = vent_bottom + i as f64 * pitch;
            face.molding(
                "paint",
                [vent_left, vent_right],
                &[
                    [-0.135, z + pitch],
                    [-0.105, z + 0.008],
                    [-0.105, z],
                    [-0.135, z + pitch - 0.008],
                ],
                steel.tint,
            )?;
        }
        for z in [0.43, 1.14, 1.88] {
            steel::fixing_plate(
                &mut face,
                steel,
                &Plate {
                    center: [door.u0 + 0.08, -0.1, z],
                    across: [1., 0., 0.],
                    up: [0., 0., 1.],
                    out: [0., 1., 0.],
                    size: [0.065, 0.11],
                    bolts: 2,
                },
            )?;
            face.tube(
                "metal",
                &[
                    [door.u0 + 0.075, -0.07, z - 0.06],
                    [door.u0 + 0.075, -0.07, z + 0.06],
                ],
                0.012,
                steel.tint,
                10,
            )?;
        }
        face.cuboid(
            "metal",
            [door.u1 - 0.19, -0.105, 0.94],
            [door.u1 - 0.12, -0.085, 1.1],
            shade(steel.tint, 1.2),
            "b",
        )?;
        face.tube(
            "metal",
            &[
                [door.u1 - 0.155, -0.08, 1.035],
                [door.u1 - 0.155, -0.035, 1.035],
                [door.u1 - 0.27, -0.035, 1.035],
            ],
            0.012,
            steel.tint,
            10,
        )?;
    }
    Fixture {
        pen: front.clone(),
        mesh: drawing.mesh,
        repeats: drawing.repeats,
    }
    .cuboid(
        "concrete",
        [door.u0 - 0.04, -0.18, 0.],
        [door.u1 + 0.04, 0.42, 0.12],
        shade(cement, 0.85),
        "d",
    )?;
    for z in [1.65, 1.72, 1.79, 1.86] {
        Fixture {
            pen: faces[1].clone(),
            mesh: drawing.mesh,
            repeats: drawing.repeats,
        }
        .molding(
            "metal",
            [0.42, (depth - 0.3).min(1.1)],
            &[
                [0.006, z + 0.045],
                [0.05, z],
                [0.05, z - 0.008],
                [0.006, z + 0.037],
            ],
            shade(steel.tint, 0.9),
        )?;
    }
    drawing.cuboid(
        "concrete",
        [plot.x0 - 0.1, plot.y0 - 0.12, height],
        [plot.x1 + 0.1, plot.y1 + 0.1, height + 0.13],
        lintel,
        "",
    )?;
    drawing.cuboid(
        "concrete",
        [plot.x0 - 0.065, plot.y0 - 0.085, height + 0.13],
        [plot.x1 + 0.065, plot.y1 + 0.065, height + 0.145],
        shade(cement, 0.55),
        "d",
    )?;
    PipeSystem::new(Options {
        radius: 0.012,
        color: srgb("#747b70"),
        sides: 16,
        variant: "pipe-standard".into(),
        weathering: Some(settings.weathering.clone()),
    })?
    .run(
        &mut Fixture {
            pen: front.clone(),
            mesh: drawing.mesh,
            repeats: drawing.repeats,
        },
        &[
            [width - 0.18, 0.035, 0.23],
            [width - 0.18, 0.035, height - 0.16],
            [width * 0.65, 0.035, height - 0.16],
        ],
        Some(0.),
    )?;
    let mut face = Fixture {
        pen: hardware,
        mesh: drawing.mesh,
        repeats: drawing.repeats,
    };
    face.cuboid(
        "metal",
        [width - 0.24, 0.012, 1.05],
        [width - 0.12, 0.07, 1.22],
        steel.tint,
        "b",
    )?;
    steel::bolt(
        &mut face,
        steel,
        [width - 0.18, 0.071, 1.17],
        [0., 1., 0.],
        0.006,
    )
}
