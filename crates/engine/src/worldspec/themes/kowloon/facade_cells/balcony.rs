use super::{Cell, Opening, Role, on, runoff, window_kind};
use crate::worldspec::themes::kowloon::{
    balconies,
    fixture::Fixture,
    household, laundry,
    metal::{Binding, Kind},
    palette::{mix, shade, srgb},
    pen::{Pen, Tint},
    signs::{
        frontage,
        mounts::{Proposal, WallRect},
    },
    windows,
};
use serde::Serialize;
use superspec_geometry::V3;
#[derive(Serialize)]
pub struct Fit {
    pub door: Door,
    pub splay: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<Opening>,
}
#[derive(Serialize)]
pub struct Door {
    pub x: f64,
    pub top: f64,
}
pub fn fit(c: &Cell<'_>) -> Result<Fit, String> {
    let splay = if c.story == 0
        && c.kit
            .rng(&format!("{}/splayed-balcony", c.lot_key))?
            .chance(0.7)
    {
        0.22_f64.min(c.width * 0.08)
    } else {
        0.
    };
    let left = 0.12 + splay;
    let right = c.width - 0.12 - splay;
    let x = left + 0.1 + c.rng("")?.float(0., (right - left - 2.4).max(0.));
    let width = 1.25_f64.min(right - x - 1.1);
    Ok(Fit {
        door: Door {
            x,
            top: 2.05_f64.min(c.height - 0.35),
        },
        splay,
        window: if width > 0.6 && c.height > 2.6 {
            Some(Opening {
                x: x + 0.95,
                z: 0.95,
                w: width,
                h: 1.2,
                depth: 0.,
            })
        } else {
            None
        },
    })
}
fn beveled(
    d: &mut Fixture<'_, '_>,
    a: V3,
    b: V3,
    side: V3,
    size: [f64; 2],
    bevel: f64,
    color: V3,
) -> Result<(), String> {
    d.pen.beveled_bar(
        d.mesh,
        "paint",
        a,
        b,
        side,
        size[0],
        size[1],
        bevel,
        &Tint::Color(color),
        (d.repeats)("paint"),
    )
}
fn strips(
    d: &mut Fixture<'_, '_>,
    pen: &Pen,
    outline: &balconies::Outline,
    z0: f64,
    z1: f64,
    color: V3,
) -> Result<(), String> {
    for edge in balconies::edges(pen, outline)? {
        let count = (edge.length / 0.13).ceil().max(1.) as usize;
        let d = &mut on(d, edge.pen);
        for i in 1..count {
            let x = i as f64 * edge.length / count as f64;
            beveled(
                d,
                [x, -0.025, z0],
                [x, -0.025, z1],
                [1., 0., 0.],
                [0.014, 0.014],
                0.001,
                color,
            )?;
        }
    }
    Ok(())
}
fn ring(
    d: &mut Fixture<'_, '_>,
    pen: &Pen,
    outline: &balconies::Outline,
    z: f64,
    color: V3,
) -> Result<(), String> {
    for edge in balconies::edges(pen, outline)? {
        on(d, edge.pen).tube(
            "paint",
            &[[0., -0.025, z], [edge.length, -0.025, z]],
            0.02,
            color,
            10,
        )?;
    }
    Ok(())
}
fn glazed(
    d: &mut Fixture<'_, '_>,
    span: [f64; 4],
    y: f64,
    rng: &mut superspec_types::rng::SeededRng,
) -> Result<(), String> {
    d.panel(
        if rng.chance(0.55) {
            "interior-lit"
        } else {
            "pane"
        },
        span,
        y,
        srgb("#101418"),
    )
}
pub fn draw(d: &mut Fixture<'_, '_>, c: &Cell<'_>) -> Result<Vec<Proposal>, String> {
    let kit = c.kit;
    let mut rng = c.rng("")?;
    let mut proposals = vec![];
    let depth = (c.reach - 0.02).min(c.balcony);
    let x0 = 0.12;
    let x1 = c.width - 0.12;
    let rail = 1.;
    let trim = c.style.trim;
    let accent = c.style.accent;
    let frame = c.style.frame;
    let Fit {
        door: Door {
            x: door_x,
            top: door_top,
        },
        splay,
        window,
    } = fit(c)?;
    let outline = [
        [x0 + splay, 0., 0.],
        [x1 - splay, 0., 0.],
        [x1, depth, 0.],
        [x0, depth, 0.],
    ];
    let fixture_pen = d.pen.into_part("fixtures")?;
    let opening_pen = d.pen.into_part("openings")?;
    balconies::slab(
        &mut on(d, fixture_pen.clone()),
        &outline,
        -0.18,
        0.02,
        trim,
        "concrete",
    )?;
    let edges = balconies::edges(&fixture_pen, &outline)?;
    let railing = fixture_pen.with_weathering(
        d.mesh,
        kit.settings().weathering.clone(),
        Kind::Cage,
        None,
        vec!["paint".into(), "metal".into()],
    )?;
    rng.next_f64();
    {
        let d = &mut on(d, opening_pen);
        d.cuboid(
            "paint",
            [door_x - 0.05, 0., 0.02],
            [door_x + 0.8, 0.04, door_top + 0.05],
            shade(frame, 0.7),
            "b",
        )?;
        d.panel(
            "timber",
            [door_x, door_x + 0.75, 0.03, door_top],
            0.045,
            frame,
        )?;
        glazed(
            d,
            [door_x + 0.1, door_x + 0.65, 0.95, door_top - 0.15],
            0.05,
            &mut rng,
        )?;
        for x in [door_x + 0.08, door_x + 0.65] {
            d.cuboid(
                "paint",
                [x, 0.045, 0.93],
                [x + 0.02, 0.058, door_top - 0.13],
                shade(frame, 0.8),
                "bdu",
            )?;
        }
        for z in [0.93, door_top - 0.15] {
            d.cuboid(
                "paint",
                [door_x + 0.08, 0.045, z],
                [door_x + 0.67, 0.058, z + 0.02],
                shade(frame, 0.8),
                "blr",
            )?;
        }
        d.cuboid(
            "metal",
            [door_x + 0.66, 0.045, 0.98],
            [door_x + 0.69, 0.085, 1.1],
            srgb("#a4a9aa"),
            "b",
        )?;
        if let Some(w) = window {
            let mut look = c.look(window_kind(c.style.window)?, &mut rng)?;
            look.hood = false;
            look.reach = 0.15;
            look.recess = 0.;
            windows::window_sized(d, [w.x, w.z, w.w, w.h], &look, &mut rng)?;
        }
    }
    let d = &mut on(d, fixture_pen);
    let solid = match c.style.balustrade {
        "solid" => rail,
        "half" => 0.55,
        "rail" => 0.,
        _ => return Err("Unknown balcony balustrade.".into()),
    };
    let steel = mix(shade(c.style.cage, 0.9), srgb("#7a4a2a"), 0.2);
    if solid > 0. {
        balconies::parapet(
            d,
            &outline,
            solid,
            accent,
            trim,
            &balconies::Settings {
                weathering: kit.settings().weathering.clone(),
                wall: Binding {
                    material: "wall".into(),
                    repeat: (d.repeats)("wall"),
                },
                concrete: Binding {
                    material: "concrete".into(),
                    repeat: (d.repeats)("concrete"),
                },
            },
            c.key,
        )?;
    }
    if solid < rail {
        strips(d, &railing, &outline, solid.max(0.02), rail, steel)?;
        ring(d, &railing, &outline, rail, steel)?;
    }
    let enclosure = if c.flat.vacant {
        "open"
    } else {
        c.flat.enclosure
    };
    let head = c.height - 0.14;
    if enclosure == "caged" {
        strips(d, &railing, &outline, rail, head, steel)?;
        ring(d, &railing, &outline, head, steel)?;
        ring(d, &railing, &outline, (rail + head) / 2., steel)?;
    } else if enclosure == "glazed" {
        for edge in edges {
            let lights = (edge.length / 0.6).round().max(1.) as usize;
            let pitch = edge.length / lights as f64;
            let d = &mut on(d, edge.pen);
            for i in 0..lights {
                let a = i as f64 * pitch;
                glazed(
                    d,
                    [a + 0.025, a + pitch - 0.025, rail + 0.02, head],
                    -0.025,
                    &mut rng,
                )?;
                beveled(
                    d,
                    [a + 0.012, -0.025, rail],
                    [a + 0.012, -0.025, head],
                    [1., 0., 0.],
                    [0.024, 0.035],
                    0.0015,
                    frame,
                )?;
            }
            beveled(
                d,
                [edge.length - 0.012, -0.025, rail],
                [edge.length - 0.012, -0.025, head],
                [1., 0., 0.],
                [0.024, 0.035],
                0.0015,
                frame,
            )?;
            beveled(
                d,
                [0., -0.025, head],
                [edge.length, -0.025, head],
                [0., 1., 0.],
                [0.04, 0.055],
                0.0015,
                frame,
            )?;
        }
    } else if enclosure != "open" {
        return Err("Unknown balcony enclosure.".into());
    }
    if solid < rail || enclosure == "caged" {
        let height = if enclosure == "caged" { head } else { rail };
        let d = &mut on(d, railing);
        for p in outline {
            beveled(
                d,
                [p[0], p[1], 0.02],
                [p[0], p[1], height],
                [1., 0., 0.],
                [0.035, 0.035],
                0.0015,
                steel,
            )?;
        }
    }
    if c.top {
        balconies::slab(
            d,
            &outline,
            c.height - 0.1,
            c.height - 0.02,
            trim,
            "concrete",
        )?;
    }
    if enclosure == "open" && head - rail > 0.8 && kit.has(&format!("{}/anti-climb", c.key), 0.5)? {
        balconies::anti_climb(
            d,
            x0 + splay + 0.05,
            (rail + head) / 2.,
            (depth * 0.8).min((head - rail) * 0.43),
            steel,
            &kit.settings().weathering,
        )?;
    }
    let board_key = format!("{}/board", c.key);
    let board = kit.option(&board_key, 0.2)?;
    if solid >= 0.5
        && c.role == Role::Front
        && c.story < 3
        && x1 - x0 > 1.8
        && c.reach >= depth + 0.08
        && board.planned
    {
        let height = 0.42_f64.min(solid - 0.14);
        let width = height * kit.rng(&format!("{board_key}/size"))?.float(3., 4.);
        let start = x0
            + 0.1
            + kit
                .rng(&format!("{board_key}/place"))?
                .float(0., x1 - x0 - 0.2 - width);
        proposals.push(frontage::balcony_board(
            &d.pen.moved([0., depth, 0.])?,
            WallRect {
                x0: start,
                z0: 0.1,
                width,
                height,
            },
            c.ground + c.story as f64 * c.story_height,
            kit,
            &board_key,
            board.shown,
        )?);
    }
    if c.flat.vacant {
        return Ok(proposals);
    }
    let busy = c.flat.clutter;
    let weather = Some(&kit.settings().weathering);
    if kit.detail() > 0 && kit.has(&format!("{}/pots", c.key), 0.5 * busy)? {
        let mut rng = c.rng("/pots")?;
        if solid >= rail && c.reach >= depth + 0.4 {
            household::plants(
                d,
                [x0 + 0.1, x1 - 0.1],
                depth - 0.035,
                rail + 0.04,
                &mut rng,
                c.plants,
            )?;
        } else if depth >= 0.7 {
            household::plants(
                d,
                [x0 + splay + 0.4, x1 - splay - 0.4],
                depth - 0.5,
                0.03,
                &mut rng,
                c.plants,
            )?;
        }
    }
    if enclosure != "glazed" && kit.has(&format!("{}/wash", c.key), 0.5 * busy)? {
        d.bar(
            "paint",
            [x0 + splay * 0.45, depth * 0.55, head - 0.2],
            [x1 - splay * 0.45, depth * 0.55, head - 0.2],
            0.01,
            srgb("#2a2927"),
        )?;
        let pen = d.pen.moved([0., depth * 0.55, 0.])?;
        laundry::washing(
            &mut on(d, pen),
            [x0 + splay + 0.15, x1 - splay - 0.15],
            head - 0.2,
            &mut c.rng("/wash")?,
            &laundry::WashingOptions {
                fill: 0.8,
                drop: 0.85,
                support: laundry::Support::Line,
            },
            c.cloth,
        )?;
    }
    if c.reach >= depth + 0.5 && kit.has(&format!("{}/pole", c.key), 0.3 * busy)? {
        let mut rng = c.rng("/pole")?;
        let x = x0 + (x1 - x0) * rng.float(0.2, 0.8);
        let pen = d.pen.moved([0., depth, 0.])?;
        household::drying_pole(
            &mut on(d, pen),
            x,
            rail + 0.03,
            (c.reach - depth - 0.05).min(0.8),
            &mut rng,
            weather,
            c.cloth,
        )?;
    }
    if c.height >= 2.75 && kit.has(&format!("{}/ac", c.key), 0.55 * busy)? {
        let bounds = crate::worldspec::themes::kowloon::aircon::air_conditioner(
            d,
            door_x + 0.38,
            door_top + 0.14,
            0.1,
            &mut c.rng("/ac")?,
            weather,
        )?;
        runoff(d, bounds, c, c.wash)?;
    }
    if kit.detail() > 0
        && enclosure != "glazed"
        && kit.has(&format!("{}/boxes", c.key), 0.3 * busy)?
    {
        let mut rng = c.rng("/boxes")?;
        let mut x = x1 - splay - 0.2;
        while x > x1 - 1. && x > door_x + 1. {
            d.mesh.spend_work(1)?;
            let size = rng.float(0.25, 0.36);
            let high = 0.02 + size * rng.float(0.8, 1.6);
            let color = shade(srgb("#a8845a"), rng.float(0.7, 1.2));
            d.cuboid(
                "paint",
                [x - size, 0.1, 0.02],
                [x, 0.1 + size, high],
                color,
                "d",
            )?;
            x -= 0.4;
        }
    }
    Ok(proposals)
}
