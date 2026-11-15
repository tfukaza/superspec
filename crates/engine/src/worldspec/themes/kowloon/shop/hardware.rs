//! Household store with stocked steel shelves, nested wares and suspended equipment.
use super::super::{
    fixture::Fixture,
    palette::{shade, srgb},
    pen::{Pen, Tint},
};
use super::{
    fittings,
    forms::{self, Tints},
    furniture,
    goods::{self, Kind as G},
    room::{Interior, Room},
    stock::Settings,
    wares,
};
use std::f64::consts::PI;
use superspec_geometry::{
    V3,
    math::{cos, sin},
};
use superspec_types::rng::SeededRng;
const HOSES: &str = "#2f8f4f #d8a82c #c8281e #c8b888 #1f5fa8";
fn steel() -> V3 {
    srgb("#a4a9aa")
}
fn cane() -> V3 {
    srgb("#b89a62")
}
fn on<'a, 'c>(d: &'a mut Fixture<'_, 'c>, pen: Pen) -> Fixture<'a, 'c> {
    Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    }
}
fn colors(rng: &mut SeededRng, count: usize) -> Vec<V3> {
    let mut pool: Vec<_> = goods::PLASTICS.split_whitespace().collect();
    (0..count)
        .map(|_| srgb(pool.remove(rng.integer(0, pool.len() - 1))))
        .collect()
}
fn hang(d: &mut Fixture<'_, '_>, at: V3, rng: &mut SeededRng) -> Result<(), String> {
    let [x, y, z] = at;
    let pick = rng.next_f64();
    if pick < 0.2 {
        let r = rng.float(0.15, 0.2);
        let tint = shade(srgb("#3c3c3a"), rng.float(0.8, 1.4));
        let bowl = rng.chance(0.6);
        wares::hanging_wok(d, at, r, tint, bowl)?;
    } else if pick < 0.34 {
        let r = rng.float(0.1, 0.14);
        d.bar("metal", at, [x, y, z - 0.1], 0.005, shade(steel(), 0.7))?;
        wares::pot(
            d,
            [x, y, z - 0.1 - r * 1.4 - 0.1],
            r,
            r * 1.4,
            shade(steel(), rng.float(1., 1.25)),
            None,
        )?;
    } else if pick < 0.48 {
        let r = rng.float(0.09, 0.12);
        wares::kettle(
            d,
            [x, y, z - r * 2.15],
            r,
            shade(steel(), rng.float(0.95, 1.25)),
        )?;
    } else if pick < 0.62 {
        let r = rng.float(0.12, 0.15);
        let h = r * 1.7;
        let tints = colors(rng, 2);
        d.bar("metal", at, [x, y, z - 0.12], 0.005, shade(steel(), 0.7))?;
        for i in 0..3 {
            wares::bucket(
                d,
                [x, y, z - 0.12 - h - 0.012 - (2 - i) as f64 * 0.05],
                r,
                h,
                tints[i % 2],
            )?;
        }
    } else if pick < 0.76 {
        let r = rng.float(0.14, 0.19);
        let section = rng.float(0.011, 0.016);
        let turns = rng.integer(3, 5);
        let tint = goods::color(rng, HOSES);
        wares::coil(d, at, r, section, turns, tint)?;
    } else if pick < 0.86 {
        wares::duster(d, at, rng)?;
    } else if pick < 0.94 {
        let r = rng.float(0.13, 0.17);
        d.bar("metal", at, [x, y, z - 0.1], 0.005, shade(steel(), 0.7))?;
        wares::basket(
            d,
            [x, y, z - 0.1 - r * 1.1 - 0.012],
            r,
            r * 1.1,
            shade(cane(), rng.float(0.8, 1.15)),
        )?;
    } else {
        wares::ladles(d, at, rng)?;
    }
    Ok(())
}
fn floor_stack(
    d: &mut Fixture<'_, '_>,
    at: V3,
    kind: usize,
    rng: &mut SeededRng,
) -> Result<(), String> {
    let [x, y, z] = at;
    match kind {
        0 => {
            let tints = colors(rng, 3);
            let mut i = 0;
            // The source samples the stopping height at each iteration, including the last.
            while i < rng.integer(4, 6) {
                wares::bucket(d, [x, y, z + i as f64 * 0.065], 0.17, 0.3, tints[i % 3])?;
                i += 1;
            }
        }
        1 => {
            let tints = colors(rng, 2);
            let mut i = 0;
            while i < rng.integer(4, 7) {
                wares::basin(d, [x, y, z + i as f64 * 0.04], 0.23, tints[i % 2])?;
                i += 1;
            }
        }
        2 => {
            let count = rng.integer(3, 6);
            wares::stool_stack(d, at, count, &colors(rng, 2))?;
        }
        3 => {
            for i in 0..3 {
                wares::clay_pot(
                    d,
                    [
                        x + (i as f64 - 1.) * 0.04 * if i % 2 == 1 { 1. } else { -1. },
                        y,
                        z + i as f64 * 0.2,
                    ],
                    0.15 - i as f64 * 0.012,
                    rng,
                )?;
            }
        }
        _ => {
            let mut i = 0;
            while i < rng.integer(3, 5) {
                wares::basket(
                    d,
                    [x, y, z + i as f64 * 0.07],
                    0.2,
                    0.2,
                    shade(cane(), rng.float(0.8, 1.15)),
                )?;
                i += 1;
            }
        }
    }
    Ok(())
}
fn stock(
    d: &mut Fixture<'_, '_>,
    room: &Room<'_>,
    shelf: &furniture::Shelf,
    kinds: &[G],
    rng: &mut SeededRng,
) -> Result<(), String> {
    goods::shelf(
        d,
        shelf,
        kinds,
        rng,
        Settings {
            fill: 0.9,
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
    let steel = shade(srgb("#6f7f78"), rng.float(0.8, 1.15));
    let unit = (h - 0.3).min(1.95);
    let pen = d.pen.moved([0.05, depth - 0.36, 0.])?;
    let back = &mut on(d, pen);
    let mut rng = room.stream("back")?;
    for shelf in furniture::shelving(
        back,
        [w - 0.1, 0.35, unit],
        5,
        "metal",
        steel,
        h - unit - 0.03,
        &room.kit.settings().timber,
    )? {
        let kinds: &[G] = if shelf.z > unit - 0.01 {
            &[G::Cartons]
        } else if shelf.z < 0.5 {
            &[G::PaintTins, G::PaintTins, G::CeramicJars]
        } else {
            &[
                G::Mugs,
                G::Bowls,
                G::Thermoses,
                G::Tins,
                G::Cartons,
                G::PaintTins,
                G::Rolls,
            ]
        };
        stock(back, room, &shelf, kinds, &mut rng)?;
    }
    let side_length = depth - 0.36 - 0.05 - 0.95;
    if side_length >= 0.8 {
        let pen = room.left_wall(&d.pen, 0.95, 0.285)?;
        let left = &mut on(d, pen);
        let mut rng = room.stream("side")?;
        for shelf in furniture::shelving(
            left,
            [side_length, 0.28, unit],
            5,
            "metal",
            steel,
            h - unit - 0.03,
            &room.kit.settings().timber,
        )? {
            let kinds: &[G] = if shelf.z > unit - 0.01 {
                &[G::Cartons]
            } else {
                &[G::Cartons, G::Tins, G::Bowls, G::Mugs, G::Thermoses]
            };
            stock(left, room, &shelf, kinds, &mut rng)?;
        }
    }
    let bin_at = w - 0.34;
    let mut overhead = room.stream("overhead")?;
    let rail_steel = shade(srgb("#a4a9aa"), 0.7);
    let mut rails = vec![[0.42, 0.14, bin_at - 0.42]];
    if depth >= 2.9 {
        rails.push([1.2, 0.62, w - 0.5]);
    }
    for [y, from, to] in rails {
        fittings::rail(d, [from, y, h - 0.1], [w - 0.14, y, h - 0.1], h, rail_steel)?;
        let mut x = from + 0.24;
        while x < to {
            let at = fittings::hook(d, [x, y, h - 0.1], 0.07, rail_steel)?;
            hang(d, at, &mut overhead)?;
            x += overhead.float(0.4, 0.5);
        }
    }
    let mut floor = room.stream("floor")?;
    let mut kinds = [0, 1, 2, 3, 4];
    for i in (1..kinds.len()).rev() {
        let j = floor.integer(0, i);
        kinds.swap(i, j);
    }
    let mut slot = 0;
    let mut x = 0.32;
    while x < bin_at - 0.5 {
        floor_stack(d, [x, 0.34, 0.], kinds[slot % kinds.len()], &mut floor)?;
        slot += 1;
        x += 0.52;
    }
    let mut x = 0.6;
    while x < bin_at - 0.3 {
        let height = floor.float(0.5, 1.);
        goods::carton_stack(
            d,
            [x, 0.9, 0.],
            0.42,
            height,
            &mut floor,
            room.assets.grocer_props,
        )?;
        x += 0.55;
    }
    let bin = [bin_at, 0.36, 0.];
    let colors = [
        steel,
        shade(steel, 1.2),
        shade(steel, 0.6),
        shade(steel, 0.5),
        shade(steel, 0.4),
    ]
    .map(Tint::Color);
    forms::lathe(
        d,
        "metal",
        bin,
        &[
            [0.2, 0.],
            [0.22, 0.55],
            [0.23, 0.56],
            [0.21, 0.55],
            [0.19, 0.02],
            [0., 0.02],
        ],
        &Tints::Bands(&colors),
        12,
        None,
    )?;
    for i in 0..6 {
        let angle = i as f64 / 6. * PI * 2. + floor.float(-0.3, 0.3);
        let foot = [bin[0] + cos(angle) * 0.07, bin[1] + sin(angle) * 0.07, 0.02];
        let top = [
            bin[0] + cos(angle) * 0.17,
            bin[1] + sin(angle) * 0.17,
            floor.float(1.15, 1.45_f64.min(h - 0.75)),
        ];
        if i % 3 == 2 {
            wares::mop(d, foot, top, &mut floor)?;
        } else {
            wares::broom(d, foot, top, &mut floor)?;
        }
    }
    let pen = room.left_wall(&d.pen, 0.1, 0.)?;
    let wall = &mut on(d, pen);
    for [x, z] in [[0.22, 1.85_f64.min(h - 0.25)], [0.6, 1.7_f64.min(h - 0.4)]] {
        wall.cuboid(
            "metal",
            [x - 0.015, -0.17, z - 0.015],
            [x + 0.015, 0., z + 0.015],
            rail_steel,
            "f",
        )?;
        let r = floor.float(0.15, 0.2);
        let section = floor.float(0.011, 0.017);
        let turns = floor.integer(3, 5);
        let tint = goods::color(&mut floor, HOSES);
        wares::coil(wall, [x, -0.09, z - 0.01], r, section, turns, tint)?;
    }
    if depth >= 2.7 {
        wares::ladder(
            d,
            [w - 0.45, 1.75, 0.03],
            [w - 0.07, 1.75, (h - 0.12).min(2.25)],
            0.34,
            &mut floor,
        )?;
    }
    fittings::tube_light(d, [w * 0.2, depth * 0.66, h], [w * 0.8, depth * 0.66, h])
}
pub fn interior(rng: &mut SeededRng) -> Interior {
    Interior {
        floor: shade(srgb("#7c7a72"), rng.float(0.85, 1.05)),
        ceiling: srgb("#c8c4b6"),
        wall: srgb("#d8d4c6"),
        dado: rng.pick(&[srgb("#a8a89c"), srgb("#8f9a90"), srgb("#b0a890")]),
        dado_height: 1.1,
    }
}
