//! Drawer cabinets, specimen jars and dried provisions in a traditional dispensary.
use super::super::{
    fixture::Fixture,
    palette::{shade, srgb},
    pen::{Pen, Tint},
};
use super::{
    fittings,
    forms::{self, Ball, Tints},
    furniture::{self, Finish, Shelf},
    goods::{self, Kind as G},
    produce,
    room::{self, Interior, Room},
    stock::Settings,
    wares,
};
use std::f64::consts::PI;
use superspec_geometry::{
    V3,
    math::{cos, sin},
};
use superspec_types::rng::SeededRng;
const DRIED: &str = "#6a1e1a #3a2a1e #e8dcc0 #a85a1e #c8a050 #5a4a2a #d8862c #8a6a3a";
fn rosewood() -> V3 {
    srgb("#5a3222")
}
fn brass() -> V3 {
    srgb("#c8a04a")
}
fn on<'a, 'c>(d: &'a mut Fixture<'_, 'c>, pen: Pen) -> Fixture<'a, 'c> {
    Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    }
}
fn ball(
    d: &mut Fixture<'_, '_>,
    slot: &str,
    center: V3,
    radius: f64,
    tint: V3,
    stretch: V3,
    bands: usize,
) -> Result<(), String> {
    forms::ball(
        d,
        slot,
        &Ball {
            center,
            radius,
            stretch,
            sides: 5,
            bands,
        },
        &Tint::Color(tint),
    )
}
fn lathe(
    d: &mut Fixture<'_, '_>,
    at: V3,
    profile: &[[f64; 2]],
    tints: &[V3],
) -> Result<(), String> {
    let tints: Vec<_> = tints.iter().map(|c| Tint::Color(*c)).collect();
    forms::lathe(d, "paint", at, profile, &Tints::Bands(&tints), 9, None)
}
fn cabinet(
    d: &mut Fixture<'_, '_>,
    width: f64,
    height: f64,
    tint: V3,
    rng: &mut SeededRng,
) -> Result<(), String> {
    let base = 0.72;
    d.cuboid(
        "timber",
        [0., 0.02, 0.],
        [width, 0.34, height],
        shade(tint, 0.75),
        "df",
    )?;
    let doors = (width / 0.55).round().max(2.) as usize;
    for i in 0..doors {
        let x0 = i as f64 * width / doors as f64 + 0.012;
        let x1 = (i + 1) as f64 * width / doors as f64 - 0.012;
        d.cuboid(
            "timber",
            [x0, 0., 0.07],
            [x1, 0.02, base - 0.03],
            shade(tint, rng.float(0.9, 1.05)),
            "df",
        )?;
        d.cuboid(
            "timber",
            [x0 + 0.05, -0.006, 0.12],
            [x1 - 0.05, 0., base - 0.08],
            shade(tint, 0.82),
            "df",
        )?;
        ball(
            d,
            "metal",
            [
                if i % 2 == 1 { x0 + 0.03 } else { x1 - 0.03 },
                -0.012,
                base * 0.55,
            ],
            0.012,
            brass(),
            [1.; 3],
            3,
        )?;
    }
    let columns = (width / 0.19).round().max(3.) as usize;
    let rows = ((height - base - 0.04) / 0.15).round().max(3.) as usize;
    let w = width / columns as f64;
    let h = (height - base - 0.04) / rows as f64;
    for column in 0..columns {
        for row in 0..rows {
            let x = column as f64 * w;
            let z = base + row as f64 * h;
            d.cuboid(
                "timber",
                [x + 0.006, 0., z + 0.006],
                [x + w - 0.006, 0.02, z + h - 0.006],
                shade(tint, rng.float(0.88, 1.12)),
                "df",
            )?;
            ball(
                d,
                "metal",
                [x + w / 2., -0.008, z + h * 0.34],
                0.009,
                brass(),
                [1.; 3],
                2,
            )?;
            forms::front(
                d,
                "paint",
                [x + w * 0.2, x + w * 0.8, z + h * 0.56, z + h * 0.84],
                -0.001,
                shade(srgb("#e8dcc0"), rng.float(0.8, 1.)),
            )?;
            forms::front(
                d,
                "paint",
                [x + w * 0.3, x + w * 0.7, z + h * 0.64, z + h * 0.76],
                -0.002,
                srgb("#3a1a14"),
            )?;
        }
    }
    Ok(())
}
fn abacus(d: &mut Fixture<'_, '_>, at: V3) -> Result<(), String> {
    let [x, y, z] = at;
    let w = 0.32;
    let depth = 0.15;
    for edge in [y, y + depth - 0.012] {
        d.cuboid(
            "timber",
            [x, edge, z],
            [x + w, edge + 0.012, z + 0.022],
            rosewood(),
            "d",
        )?;
    }
    for edge in [x, x + w - 0.012] {
        d.cuboid(
            "timber",
            [edge, y, z],
            [edge + 0.012, y + depth, z + 0.022],
            rosewood(),
            "d",
        )?;
    }
    d.cuboid(
        "timber",
        [x, y + depth * 0.66, z],
        [x + w, y + depth * 0.66 + 0.01, z + 0.022],
        rosewood(),
        "d",
    )?;
    for rod in 0..9 {
        let rx = x + 0.03 + rod as f64 * ((w - 0.06) / 8.);
        d.bar(
            "timber",
            [rx, y + 0.01, z + 0.012],
            [rx, y + depth - 0.01, z + 0.012],
            0.004,
            srgb("#c8b888"),
        )?;
        for bead in [0.022, 0.038, 0.054, 0.082, 0.118, 0.131] {
            ball(
                d,
                "pane",
                [rx, y + bead, z + 0.012],
                0.012,
                shade(rosewood(), 0.7),
                [1., 0.62, 1.],
                2,
            )?;
        }
    }
    Ok(())
}
fn mortar(d: &mut Fixture<'_, '_>, at: V3) -> Result<(), String> {
    let stone = srgb("#8a8a84");
    lathe(
        d,
        at,
        &[
            [0.05, 0.],
            [0.075, 0.02],
            [0.085, 0.09],
            [0.078, 0.092],
            [0.05, 0.03],
            [0., 0.03],
        ],
        &[
            stone,
            stone,
            shade(stone, 1.15),
            shade(stone, 0.7),
            shade(stone, 0.55),
        ],
    )?;
    forms::limb(
        d,
        "paint",
        &[
            [at[0] + 0.01, at[1], at[2] + 0.035],
            [at[0] + 0.05, at[1] + 0.02, at[2] + 0.17],
        ],
        &[0.022, 0.014],
        &Tint::Color(shade(stone, 1.1)),
        6,
        1.,
    )
}
fn herb_bundle(d: &mut Fixture<'_, '_>, at: V3, rng: &mut SeededRng) -> Result<(), String> {
    let tint = goods::color(rng, DRIED);
    for _ in 0..7 {
        let angle = rng.float(0., PI * 2.);
        let spread = rng.float(0.02, 0.06);
        let length = rng.float(0.22, 0.34);
        forms::limb(
            d,
            "paint",
            &[
                at,
                [
                    at[0] + cos(angle) * spread * 0.3,
                    at[1] + sin(angle) * spread * 0.3,
                    at[2] - 0.08,
                ],
                [
                    at[0] + cos(angle) * spread,
                    at[1] + sin(angle) * spread,
                    at[2] - length,
                ],
            ],
            &[0.006, 0.01, 0.006],
            &Tint::Color(shade(tint, rng.float(0.8, 1.25))),
            4,
            1.,
        )?;
    }
    Ok(())
}
fn stock(
    d: &mut Fixture<'_, '_>,
    room: &Room<'_>,
    shelf: &Shelf,
    kinds: &[G],
    rng: &mut SeededRng,
    fill: f64,
) -> Result<(), String> {
    goods::shelf(
        d,
        shelf,
        kinds,
        rng,
        Settings {
            fill,
            maximum_rows: 2,
            packed: false,
        },
        room.assets.snack_packets,
    )
}
pub fn furnish(d: &mut Fixture<'_, '_>, room: &Room<'_>) -> Result<(), String> {
    room.validate()?;
    d.mesh.check_work(0)?;
    let (w, depth, h) = (room.width, room.depth, room.height);
    let mut rng = room.stream("fittings")?;
    let wood = shade(rosewood(), rng.float(0.85, 1.2));
    let cabinet_height = (h - 0.42).min(1.9);
    let pen = d.pen.moved([0.05, depth - 0.36, 0.])?;
    let drawers = &mut on(d, pen);
    cabinet(
        drawers,
        w - 0.1,
        cabinet_height,
        wood,
        &mut room.stream("drawers")?,
    )?;
    stock(
        drawers,
        room,
        &Shelf {
            x0: 0.04,
            x1: w - 0.14,
            y: 0.05,
            depth: 0.26,
            z: cabinet_height,
            clear: h - cabinet_height - 0.03,
        },
        &[G::CeramicJars, G::CeramicJars, G::BigJars],
        &mut room.stream("jars")?,
        0.6,
    )?;
    let side_length = depth - 0.36 - 0.05 - 1.;
    if side_length >= 0.8 {
        let unit = (h - 0.3).min(1.9);
        let pen = room.left_wall(&d.pen, 1., 0.285)?;
        let left = &mut on(d, pen);
        let mut side = room.stream("side")?;
        for shelf in furniture::shelving(
            left,
            [side_length, 0.28, unit],
            4,
            "timber",
            wood,
            h - unit - 0.03,
            &room.kit.settings().timber,
        )? {
            let kinds: &[G] = if shelf.z > unit - 0.01 {
                &[G::Cartons]
            } else {
                &[G::BigJars, G::BigJars, G::Jars, G::CeramicJars, G::Cartons]
            };
            stock(left, room, &shelf, kinds, &mut side, 0.9)?;
        }
    }
    let width = 1.9_f64.min(w - 0.85);
    let counter_at = w - 0.06 - width;
    let pen = d.pen.moved([counter_at, 0.95, 0.])?;
    let bench = &mut on(d, pen);
    let top = furniture::counter(
        bench,
        [width, 0.5, 0.92],
        Finish::Timber,
        wood,
        &mut rng,
        &room.kit.settings().timber,
    )?;
    let mut wares = room.stream("counter")?;
    fittings::counter_scale(
        bench,
        [width - 0.22, 0.27, top.z],
        shade(brass(), wares.float(0.8, 1.05)),
    )?;
    abacus(bench, [0.08, 0.06, top.z])?;
    if width >= 1.2 {
        mortar(bench, [0.56, 0.3, top.z])?;
    }
    let mut x = 0.75;
    while x < width - 0.45 {
        goods::jar(
            bench,
            [x, 0.14, top.z],
            0.08,
            0.22,
            goods::color(&mut wares, goods::CONTENTS),
            srgb("#2a2a2c"),
        )?;
        x += 0.2;
    }
    fittings::stool(d, [counter_at + width * 0.5, 1.8, 0.], shade(wood, 1.1))?;
    let mut floor = room.stream("floor")?;
    for row in 0..2 {
        let mut x = 0.28 + row as f64 * 0.2;
        while x < w - 0.24 {
            let r = floor.float(0.17, 0.2);
            let at = [x, 0.27 + row as f64 * 0.38, 0.];
            let content = goods::color(&mut floor, DRIED);
            if floor.chance(0.6) {
                let height = floor.float(0.32, 0.46) + row as f64 * 0.12;
                goods::sack(
                    d,
                    at,
                    r,
                    height,
                    shade(srgb("#c8b48a"), floor.float(0.8, 1.1)),
                    content,
                )?;
                fittings::price_card(
                    d,
                    [x + r * 0.25, at[1] + r * 0.3, height - 0.03],
                    &mut floor,
                )?;
            } else {
                let height = 0.2 + row as f64 * 0.14;
                wares::basket(
                    d,
                    at,
                    r,
                    height,
                    shade(srgb("#b89a62"), floor.float(0.8, 1.15)),
                )?;
                lathe(
                    d,
                    at,
                    &[
                        [r * 0.93, height - 0.02],
                        [r * 0.5, height + 0.03],
                        [0., height + 0.05],
                    ],
                    &[content],
                )?;
                fittings::price_card(d, [x + r * 0.25, at[1] + r * 0.3, height], &mut floor)?;
            }
            x += 0.43;
        }
    }
    let mut overhead = room.stream("overhead")?;
    let steel = shade(srgb("#a4a9aa"), 0.7);
    let rail_z = h - 0.14;
    fittings::rail(d, [0.12, 0.3, rail_z], [w - 0.12, 0.3, rail_z], h, steel)?;
    let mut x = 0.3;
    while x < w - 0.25 {
        let held = fittings::hook(d, [x, 0.3, rail_z], 0.06, steel)?;
        let pick = overhead.next_f64();
        if pick < 0.4 {
            produce::sausages(d, held, &mut overhead)?;
        } else if pick < 0.65 {
            let length = overhead.float(0.28, 0.4);
            produce::salted_fish(d, held, length, &mut overhead)?;
        } else if pick < 0.85 {
            herb_bundle(d, held, &mut overhead)?;
        } else {
            let length = overhead.float(0.3, 0.42);
            produce::garlic_string(d, held, length, &mut overhead)?;
        }
        x += overhead.float(0.17, 0.26);
    }
    fittings::pendant(
        d,
        [counter_at + width * 0.5, 0.78, h],
        0.3_f64.max(h - 1.8),
        None,
    )?;
    room::amenities(d, room, depth * 0.45, (depth >= 3.2).then_some(1.6))
}
pub fn interior(rng: &mut SeededRng) -> Interior {
    Interior {
        floor: shade(srgb("#7a6a58"), rng.float(0.85, 1.05)),
        ceiling: srgb("#d0c8b0"),
        wall: srgb("#e0d0a8"),
        dado: rng.pick(&[srgb("#8a5a3c"), srgb("#7a4a34"), srgb("#96684a")]),
        dado_height: 1.1,
    }
}
