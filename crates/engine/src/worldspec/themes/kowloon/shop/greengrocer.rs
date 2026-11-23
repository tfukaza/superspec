//! Tiered produce displays with independently seeded crate contents and overhead goods.
use super::super::{
    fixture::Fixture,
    palette::{shade, srgb},
    pen::{Pen, Tint},
};
use super::{
    fittings, forms, furniture, produce,
    room::{Interior, Room},
    stock_mesh,
};
use std::f64::consts::PI;
use superspec_geometry::V3;
use superspec_types::rng::SeededRng;
const SMALL: V3 = [0.34, 0.26, 0.10];
const CRATE: V3 = [0.50, 0.36, 0.15];
const LARGE: V3 = [0.66, 0.41, 0.18];
const TIER_DEPTH: f64 = 0.47;
fn on<'a, 'c>(d: &'a mut Fixture<'_, 'c>, pen: Pen) -> Fixture<'a, 'c> {
    Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    }
}
fn selection(rng: &mut SeededRng) -> &'static str {
    let choices = [
        ("orange", 3.),
        ("apple", 2.5),
        ("greenApple", 1.2),
        ("pear", 1.5),
        ("tomato", 1.5),
        ("lemon", 1.),
        ("pomelo", 1.2),
        ("plum", 0.8),
        ("potato", 1.),
        ("onion", 1.),
        ("cabbages", 1.4),
        ("greens", 1.6),
        ("radishes", 1.),
        ("carrots", 1.),
        ("pineapples", 0.9),
    ];
    let mut roll = rng.next_f64() * choices.iter().map(|c| c.1).sum::<f64>();
    for (name, weight) in choices {
        roll -= weight;
        if roll <= 0. {
            return name;
        }
    }
    "pineapples"
}
/// A baked tray, or at a reduced level a crate heaped from a stream of its own: the baked
/// produce has no colours of its own to mass, and no other stream may notice the level.
fn tray(
    d: &mut Fixture<'_, '_>,
    room: &Room<'_>,
    asset: &super::market_trays::Tray,
    key: &str,
    size: V3,
    wood: V3,
) -> Result<(), String> {
    if !d.mesh.graphics_detail().reduced() {
        return asset.draw(d, size);
    }
    let mut own = room.stream(&format!("reduced-tray/{key}"))?;
    furniture::crate_box(
        d,
        size,
        shade(wood, own.float(0.85, 1.25)),
        &mut own,
        &room.kit.settings().timber,
    )?;
    load(d, room, selection(&mut own), &mut own, key, size)
}
fn load(
    d: &mut Fixture<'_, '_>,
    room: &Room<'_>,
    what: &str,
    rng: &mut SeededRng,
    key: &str,
    size: V3,
) -> Result<(), String> {
    let [w, depth, h] = size;
    if what == "apple" && d.mesh.graphics_detail().reduced() {
        // The baked pile's place is taken by a heap of the same apples from the pile's own
        // stream; the shared stream still makes the pile's one draw.
        let mut pose = SeededRng::from_text(&format!("apple-crate/{key}"));
        rng.pick(&produce::fruit_kind(what)?.colors);
        let kind = produce::fruit_kind(what)?;
        produce::heap(
            d,
            [w / 2., depth / 2., 0.012_f64.max(h - kind.radius * 2.3)],
            [w / 2. - 0.01, depth / 2. - 0.01],
            kind.radius,
            2,
            &mut pose,
            |d, at, rng| produce::fruit(d, at, &kind, rng),
        )?;
    } else if what == "apple" {
        let mut pose = SeededRng::from_text(&format!("apple-crate/{key}"));
        let at = [
            w / 2. + pose.float(-0.006, 0.006),
            depth / 2. + pose.float(-0.006, 0.006),
            h - 0.052 * (w / CRATE[0]).min(depth / CRATE[1]),
        ];
        let pen = forms::rotated(
            &d.pen,
            at,
            2,
            pose.pick(&[0., PI]) + pose.float(-0.035, 0.035),
        )?;
        stock_mesh::apple_pile(
            &mut on(d, pen),
            [0.; 3],
            [
                w - 0.02,
                depth - 0.02,
                0.112 * (w / CRATE[0]).min(depth / CRATE[1]),
            ],
            rng.pick(&produce::fruit_kind(what)?.colors),
            room.assets.market_apple,
        )?;
    } else if let Ok(kind) = produce::fruit_kind(what) {
        let courses = if kind.radius < 0.03 { 3 } else { 2 };
        let base = 0.012_f64.max(h - kind.radius * if courses == 3 { 3.4 } else { 2.3 });
        produce::heap(
            d,
            [w / 2., depth / 2., base],
            [w / 2. - 0.01, depth / 2. - 0.01],
            kind.radius,
            courses,
            rng,
            |d, at, rng| produce::fruit(d, at, &kind, rng),
        )?;
    } else {
        match what {
            "cabbages" => {
                let scale = (w / CRATE[0]).min(depth / CRATE[1]);
                for x in [w * 0.26, w * 0.74] {
                    for y in [depth * 0.27, depth * 0.73] {
                        let at = [x + rng.float(-0.01, 0.01), y, h - 0.12 * scale];
                        let r = rng.float(0.085, 0.1) * scale;
                        produce::cabbage(d, at, r, rng)?;
                    }
                }
            }
            "greens" => {
                let mut x = 0.07;
                while x < w - 0.05 {
                    let at = [x, 0.03, h - 0.05 + rng.float(0., 0.02)];
                    let length = rng.float(0.26, 0.32) * depth / CRATE[1];
                    produce::greens(d, at, length, rng)?;
                    x += 0.075;
                }
            }
            "radishes" | "carrots" => {
                let orange = what == "carrots";
                for layer in 0..2 {
                    let mut y = 0.05;
                    while y < depth - 0.04 {
                        let at = [
                            0.08 + rng.float(0., 0.04),
                            y,
                            h - 0.07 + layer as f64 * if orange { 0.03 } else { 0.05 },
                        ];
                        let length = rng.float(0.28, 0.36) * w / CRATE[0];
                        produce::root(d, at, length, orange, rng)?;
                        y += if orange { 0.036 } else { 0.066 };
                    }
                }
            }
            "pineapples" => {
                let columns = (w / 0.16).floor() as usize;
                let rows = (depth / 0.16).floor() as usize;
                for column in 0..columns {
                    let x = w * (column as f64 + 0.5) / columns as f64;
                    for row in 0..rows {
                        let y = depth * (row as f64 + 0.5) / rows as f64;
                        produce::pineapple(d, [x, y, (h - 0.13).max(0.02)], rng)?;
                    }
                }
            }
            _ => return Err("Unknown display load.".into()),
        }
    }
    fittings::price_card(d, [rng.float(0.1, w - 0.1), depth - 0.05, h - 0.04], rng)
}
fn bag_bundle(d: &mut Fixture<'_, '_>, at: V3, rng: &mut SeededRng) -> Result<(), String> {
    let tint = rng.pick(&[srgb("#e8486a"), srgb("#e8e4dc"), srgb("#d8281e")]);
    forms::limb(
        d,
        "paint",
        &[
            at,
            [at[0], at[1], at[2] - 0.06],
            [at[0] + 0.01, at[1], at[2] - 0.22],
            [at[0], at[1], at[2] - 0.4],
        ],
        &[0.008, 0.03, 0.055, 0.02],
        &Tint::Color(tint),
        6,
        0.6,
    )
}
/// Decorative room stock; navigation and collision remain owned by architectural stages.
pub fn furnish(d: &mut Fixture<'_, '_>, room: &Room<'_>) -> Result<(), String> {
    room.validate()?;
    d.mesh.check_work(0)?;
    let (w, depth, h) = (room.width, room.depth, room.height);
    if w >= 3.6 && depth >= 4.6 {
        return walk_in(d, room);
    }
    let mut rng = room.stream("fittings")?;
    let wood = shade(srgb("#9a7a52"), rng.float(0.75, 1.1));
    let aisle = w >= 2.4;
    let tiers = if depth >= 2.9 { 4 } else { 3 };
    let rack_back = 0.06 + f64::from(tiers) * TIER_DEPTH;
    let aisle_width = if aisle { 0.95 } else { 0.70 };
    let front_checkout = depth - rack_back < 1.75;
    let rack_end = w - aisle_width - if front_checkout { 0.65 } else { 0. };
    let rack_width = rack_end - 0.06;
    let slot_width = CRATE[0] + 0.035;
    let full_slots = (rack_width / slot_width).floor() as usize;
    let columns = ((rack_width + CRATE[0] - SMALL[0]) / slot_width).floor() as usize;
    let mut stock = room.stream("crates")?;
    let mut trays = room
        .assets
        .market_trays
        .then(|| {
            room.stream("tray-assortment").map(|mut rng| {
                ["small", "medium", "large"]
                    .map(|kind| super::market_trays::Assortment::new(kind, &mut rng))
            })
        })
        .transpose()?;
    for tier in 0..tiers {
        let y = 0.06 + f64::from(tier) * TIER_DEPTH;
        let z = 0.6 + 0.65 * f64::from(tier) / f64::from(tiers - 1);
        if columns == 0 {
            continue;
        }
        if tier > 0 {
            d.cuboid(
                "timber",
                [0.04, y, z - 0.045],
                [rack_end, y + TIER_DEPTH, z],
                wood,
                "",
            )?;
            let spans = ((rack_end - 0.09) / 0.75).ceil() as usize;
            for index in 0..=spans {
                let x = 0.04 + (rack_end - 0.09) * index as f64 / spans as f64;
                for leg_y in [y + 0.035, y + TIER_DEPTH - 0.095] {
                    d.cuboid(
                        "timber",
                        [x, leg_y, 0.],
                        [x + 0.05, leg_y + 0.055, z - 0.045],
                        shade(wood, 0.7),
                        "du",
                    )?;
                }
            }
        }
        let mut layout = room.stream(&format!("crate-layout/{tier}"))?;
        let mut sizes = vec![CRATE; full_slots];
        for pair in sizes.chunks_exact_mut(2) {
            if layout.chance(0.8) {
                pair.copy_from_slice(&[SMALL, LARGE]);
            }
        }
        if columns > full_slots {
            sizes.push(SMALL);
        }
        let mut occupied: f64 = sizes.iter().map(|size| size[0]).sum();
        for size in &mut sizes {
            let wider = if *size == SMALL { CRATE } else { LARGE };
            let extra = wider[0] - size[0];
            if extra > 0. && occupied + extra + (columns + 1) as f64 * 0.026 <= rack_width {
                *size = wider;
                occupied += extra;
            }
        }
        for i in (1..sizes.len()).rev() {
            sizes.swap(i, layout.integer(0, i));
        }
        let gap = (rack_width - occupied) / (columns + 1) as f64;
        let mut x = 0.06 + gap;
        for (column, size) in sizes.into_iter().enumerate() {
            let key = format!("{tier}/{column}");
            let kind = if size == SMALL {
                0
            } else if size == LARGE {
                2
            } else {
                1
            };
            let tray = match &mut trays {
                Some(trays) => trays[kind].pick(&mut room.stream(&format!("loaded-tray/{key}"))?),
                None => None,
            };
            let crate_x = x + layout.float(-0.15, 0.15) * gap;
            x += size[0] + gap;
            let front = (size[2] + if tray.is_some() { 0.035 } else { 0. }) * 0.2_f64.sin() + 0.01;
            let back = (front + 0.065)
                .min(TIER_DEPTH - (size[1] + 0.008) * 0.2_f64.cos())
                .max(front);
            let crate_y = y + layout.float(front, back);
            if tier == 0 {
                let deck = d.pen.moved([crate_x, crate_y, 0.])?;
                front_stand(
                    &mut on(d, deck),
                    room,
                    size,
                    wood,
                    &mut room.stream(&format!("crate-stand/{column}"))?,
                    column % 2 == 1,
                )?;
            }
            let pen = forms::rotated(&d.pen, [crate_x, crate_y, z], 0, 0.2)?;
            let tipped = &mut on(d, pen);
            if let Some(asset) = tray {
                self::tray(tipped, room, asset, &key, size, wood)?;
                fittings::price_card(
                    tipped,
                    [size[0] * 0.8, 0.035, size[2]],
                    &mut room.stream(&format!("tray-card/{key}"))?,
                )?;
                continue;
            }
            furniture::crate_box(
                tipped,
                size,
                shade(wood, stock.float(0.85, 1.25)),
                &mut stock,
                &room.kit.settings().timber,
            )?;
            load(tipped, room, selection(&mut stock), &mut stock, &key, size)?;
        }
    }
    let mut overhead = room.stream("overhead")?;
    let lamps = (rack_end / 0.95).round().max(2.) as usize;
    let lamp_shade = overhead.pick(&[srgb("#d8281e"), srgb("#d8281e"), srgb("#e85a20")]);
    for i in 0..lamps {
        fittings::pendant(
            d,
            [
                0.06 + (i as f64 + 0.5) * (rack_end - 0.06) / lamps as f64,
                0.34,
                h,
            ],
            h - 1.72,
            Some(lamp_shade),
        )?;
    }
    let steel = shade(srgb("#a4a9aa"), 0.75);
    let rail_z = h - 0.2;
    fittings::rail(
        d,
        [0.1, 0.98, rail_z],
        [rack_end - 0.04, 0.98, rail_z],
        h,
        steel,
    )?;
    let mut x = 0.3;
    while x < rack_end - 0.25 {
        let held = fittings::hook(d, [x, 0.98, rail_z], 0.07, steel)?;
        let pick = overhead.next_f64();
        if pick < 0.6 {
            produce::bananas(d, held, &mut overhead)?;
        } else if pick < 0.8 {
            let length = overhead.float(0.3, 0.45);
            produce::garlic_string(d, held, length, &mut overhead)?;
        } else {
            bag_bundle(d, held, &mut overhead)?;
        }
        x += overhead.float(0.26, 0.4);
    }
    if aisle {
        let mut melons = room.stream("melons")?;
        let x = rack_end - 0.52;
        let pallet_y = rack_back + 0.84;
        if depth - pallet_y >= 1.3 && w >= 3. {
            d.cuboid(
                "timber",
                [x - 0.33, pallet_y, 0.],
                [x + 0.33, pallet_y + 0.77, 0.1],
                shade(wood, 0.8),
                "df",
            )?;
            for [dx, y, z] in [
                [-0.14, 0.3, 0.1],
                [0.15, 0.36, 0.1],
                [-0.1, 0.68, 0.1],
                [0.14, 0.72, 0.1],
            ] {
                let pen = forms::rotated(
                    &d.pen,
                    [x + dx, pallet_y + y - 0.1, z],
                    2,
                    melons.float(-0.5, 0.5),
                )?;
                let length = melons.float(0.3, 0.36);
                produce::melon(&mut on(d, pen), [0.; 3], length, &mut melons)?;
            }
            produce::melon_half(d, [x + 0.02, pallet_y + 0.4, 0.345], 0.14)?;
        }
        fittings::hanging_scale(
            d,
            [rack_end - 0.21, 0.43, h],
            0.7_f64.max(h - 1.25),
            shade(srgb("#3f7f9a"), melons.float(0.8, 1.1)),
        )?;
        if room.extra("stool", 0.7)? {
            let x = if front_checkout {
                rack_end + 0.28
            } else {
                w - 0.42
            };
            fittings::stool(d, [x, depth - 1.18, 0.], shade(wood, 0.8))?;
        }
    }
    backstock(d, room, rack_back)?;
    let counter_width = if front_checkout || !aisle { 0.60 } else { 0.88 };
    let checkout = if front_checkout {
        [w - aisle_width - 0.625, 0.03]
    } else {
        [w - counter_width - 0.04, depth - 1.75]
    };
    payment(
        d,
        room,
        checkout,
        counter_width,
        if front_checkout { 0.45 } else { 0.5 },
    )?;
    fittings::tube_light(d, [w * 0.2, depth * 0.72, h], [w * 0.8, depth * 0.72, h])
}

fn walk_in(d: &mut Fixture<'_, '_>, room: &Room<'_>) -> Result<(), String> {
    let (width, depth, height) = (room.width, room.depth, room.height);
    let bank_depth = 1.71_f64.min(width / 2. - 0.69);
    let bank_front = 1.55;
    let bank_back = depth - 1.45;
    let span = bank_back - bank_front;
    let margin = 0.08 + bank_depth;
    let tiers = (bank_depth / 0.47).floor().max(1.) as usize;
    let row_pitch = bank_depth / tiers as f64;
    let mut assortment = if room.assets.market_trays {
        Some(super::market_trays::Assortment::new(
            "medium",
            &mut room.stream("walk-in/tray-assortment")?,
        ))
    } else {
        None
    };
    for (side, origin, tangent, outward) in [
        (
            "left",
            [margin, bank_front, 0.],
            [0., 1., 0.],
            [-1., 0., 0.],
        ),
        (
            "right",
            [width - margin, bank_back, 0.],
            [0., -1., 0.],
            [1., 0., 0.],
        ),
    ] {
        let pen = d
            .pen
            .into_part(&format!("{}/produce-bank-{side}", d.pen.part))?
            .facing(origin, tangent, outward)?;
        let columns = (span / 0.535).floor() as usize;
        let gap = (span - columns as f64 * CRATE[0]) / (columns + 1) as f64;
        let wood = srgb("#796045");
        let mut stock = room.stream(&format!("walk-in/{side}/stock"))?;
        for tier in 0..tiers {
            let y = tier as f64 * row_pitch;
            let z = 0.3 + 0.6 * tier as f64 / tiers.saturating_sub(1).max(1) as f64;
            on(d, pen.clone()).cuboid(
                "timber",
                [0., y, z - 0.045],
                [span, y + row_pitch, z],
                wood,
                "",
            )?;
            for column in 0..=columns {
                let x = (column as f64 * span / columns.max(1) as f64).clamp(0.025, span - 0.025);
                for leg_y in [y + 0.025, y + row_pitch - 0.025] {
                    on(d, pen.clone()).cuboid(
                        "timber",
                        [x - 0.02, leg_y - 0.02, 0.],
                        [x + 0.02, leg_y + 0.02, z - 0.045],
                        wood,
                        "",
                    )?;
                }
            }
            for column in 0..columns {
                let key = format!("walk-in/{side}/{tier}/{column}");
                let x = gap + column as f64 * (CRATE[0] + gap);
                let tray_pen = forms::rotated(&pen, [x, y + 0.045, z + 0.012], 0, 0.2)?;
                let tray = &mut on(d, tray_pen);
                if let Some(assortment) = &mut assortment
                    && let Some(asset) = assortment.pick(&mut room.stream(&format!("{key}/tray"))?)
                {
                    self::tray(tray, room, asset, &key, CRATE, wood)?;
                } else {
                    furniture::crate_box(
                        tray,
                        CRATE,
                        wood,
                        &mut stock,
                        &room.kit.settings().timber,
                    )?;
                    let selection = selection(&mut stock);
                    load(tray, room, selection, &mut stock, &key, CRATE)?;
                }
            }
        }
    }
    if height >= 2.4 {
        upper_storage(d, room, bank_front, span)?;
    }
    payment(d, room, [0.08, 0.08], 0.88, 0.5)?;
    for y in [1.05, depth * 0.5, depth - 0.8] {
        fittings::tube_light(d, [width * 0.22, y, height], [width * 0.72, y, height])?;
    }
    let pen = d.pen.clone();
    let aisle_l = width / 2. - 0.55;
    let aisle_r = width / 2. + 0.55;
    for (side, x, span) in [
        ("left", 1.06, aisle_l - 1.12),
        ("right", aisle_r + 0.06, width - aisle_r - 0.14),
    ] {
        if span < 0.535 {
            continue;
        }
        let island = pen
            .into_part(&format!("produce-island-{side}"))?
            .moved([x, 0.08, 0.])?;
        let mut local = on(d, island);
        let wood = srgb("#796045");
        let count = (span / 0.535).floor() as usize;
        let gap = (span - count as f64 * CRATE[0]) / (count + 1) as f64;
        for column in 0..count {
            let key = format!("walk-in/front-{side}/{column}");
            let mut rng = room.stream(&key)?;
            let x = gap + column as f64 * (CRATE[0] + gap);
            let deck = local.pen.moved([x, 0.045, 0.])?;
            front_stand(
                &mut on(&mut local, deck.clone()),
                room,
                CRATE,
                wood,
                &mut rng,
                column % 2 == 0,
            )?;
            let tray_pen = forms::rotated(&deck, [0., 0., 0.6], 0, 0.2)?;
            let tray = &mut on(&mut local, tray_pen);
            if let Some(asset) = assortment.as_mut().and_then(|pool| pool.pick(&mut rng)) {
                self::tray(tray, room, asset, &key, CRATE, wood)?;
            } else {
                furniture::crate_box(tray, CRATE, wood, &mut rng, &room.kit.settings().timber)?;
                load(tray, room, selection(&mut rng), &mut rng, &key, CRATE)?;
            }
        }
    }
    for [x, y] in [
        [0.6, 2.15],
        [width - 0.6, 2.15],
        [width - 0.6, depth - 1.85],
    ] {
        fittings::pendant(d, [x, y, height], height - 1.87, Some(srgb("#b42b21")))?;
    }
    let rail_y = (depth - 1.5).min(3.7);
    let rail_z = height - 0.18;
    let steel = srgb("#7f827d");
    fittings::rail(
        d,
        [0.45, 1.65, rail_z],
        [0.45, rail_y, rail_z],
        height,
        steel,
    )?;
    let mut y = 1.8;
    let mut rng = room.stream("walk-in/overhead")?;
    while y < rail_y {
        let held = fittings::hook(d, [0.45, y, rail_z], 0.07, steel)?;
        if rng.chance(0.75) {
            produce::bananas(d, held, &mut rng)?;
        } else {
            produce::garlic_string(d, held, 0.3, &mut rng)?;
        }
        y += 0.42;
    }
    fittings::hanging_scale(d, [0.63, 0.34, height], height - 1.45, srgb("#3f7f9a"))?;
    backstock(d, room, depth - 1.4)
}

fn upper_storage(
    d: &mut Fixture<'_, '_>,
    room: &Room<'_>,
    origin: f64,
    span: f64,
) -> Result<(), String> {
    let pen = d.pen.into_part("upper-storage-left")?.facing(
        [0.36, origin + 0.10, 0.],
        [0., 1., 0.],
        [-1., 0., 0.],
    )?;
    let width = span - 0.15;
    let wood = srgb("#745d41");
    let steel = srgb("#626662");
    let d = &mut on(d, pen);
    d.cuboid("timber", [0., 0., 1.945], [width, 0.36, 1.98], wood, "f")?;
    let mut x = 0.08;
    while x + 0.04 < width {
        d.cuboid(
            "timber",
            [x, 0.31, 1.8],
            [x + 0.04, 0.36, 1.945],
            shade(wood, 0.65),
            "f",
        )?;
        d.bar(
            "metal",
            [x + 0.02, 0.34, 1.8],
            [x + 0.02, 0.025, 1.945],
            0.018,
            steel,
        )?;
        x += 0.65;
    }
    use super::stock::{self, Grocery as G};
    let textured = room.assets.grocer_props;
    let jars = |rng: &mut SeededRng| stock::grocery(G::Jars, rng, textured);
    let cartons = |rng: &mut SeededRng| stock::grocery(G::Cartons, rng, textured);
    let tins = |rng: &mut SeededRng| stock::grocery(G::LargeTins, rng, textured);
    stock::shelf(
        d,
        &furniture::Shelf {
            x0: 0.03,
            x1: width - 0.03,
            y: 0.015,
            depth: 0.29,
            z: 1.98,
            clear: (room.height - 2.04).min(0.34),
        },
        &[&jars, &cartons, &tins],
        &mut room.stream("walk-in/upper-storage")?,
        stock::Settings {
            fill: 1.,
            maximum_rows: 1,
            packed: true,
        },
    )
}

fn front_stand(
    d: &mut Fixture<'_, '_>,
    room: &Room<'_>,
    [width, depth, _]: V3,
    wood: V3,
    rng: &mut SeededRng,
    plastic: bool,
) -> Result<(), String> {
    let board = 0.025;
    let stack = (0.6 - board) / 2.;
    let pen = d.pen.clone();
    for level in 0..2 {
        let z = f64::from(level) * stack;
        if plastic {
            let count = if width >= 0.6 { 2 } else { 1 };
            let pitch = width / f64::from(count);
            for slot in 0..count {
                super::grocer_props::fitted_crate(
                    d,
                    [(f64::from(slot) + 0.5) * pitch, depth / 2., z],
                    [pitch - 0.008, depth - 0.008, stack],
                    &super::grocer_props::Options {
                        variant: rng.integer(1, 4),
                        supported: true,
                        textured: room.assets.grocer_props,
                        ..Default::default()
                    },
                )?;
            }
        } else {
            furniture::crate_box(
                &mut on(d, pen.moved([0., 0., z])?),
                [width, depth, stack],
                shade(wood, 0.68 + f64::from(level) * 0.12),
                rng,
                &room.kit.settings().timber,
            )?;
        }
    }
    d.cuboid(
        "timber",
        [0., 0., 0.6 - board],
        [width, depth, 0.6],
        wood,
        "d",
    )
}

fn payment(
    d: &mut Fixture<'_, '_>,
    room: &Room<'_>,
    [x, y]: [f64; 2],
    width: f64,
    depth: f64,
) -> Result<(), String> {
    let checkout = d
        .pen
        .into_part(&format!("{}/checkout", d.pen.part))?
        .moved([x, y, 0.])?;
    let checkout = &mut on(d, checkout);
    super::grocer_props::draw(
        checkout,
        "shop-counter",
        [width / 2., depth / 2., 0.],
        [width, depth, 0.9],
        0.,
        &super::grocer_props::Options {
            textured: room.assets.grocer_props,
            ..Default::default()
        },
    )?;
    let register = checkout
        .pen
        .facing([0.325, 0.40, 0.9], [-1., 0., 0.], [0., -1., 0.])?;
    super::checkout::draw(
        &mut on(checkout, register),
        [0.; 3],
        1.,
        room.assets.shop_register,
    )?;
    if width >= 0.8 {
        super::grocer_props::draw(
            checkout,
            "counter-scale",
            [width - 0.14, depth / 2., 0.9],
            [0.22, 0.24, 0.28],
            0.,
            &super::grocer_props::Options {
                textured: room.assets.grocer_props,
                ..Default::default()
            },
        )?;
    }
    Ok(())
}

fn backstock(d: &mut Fixture<'_, '_>, room: &Room<'_>, display_back: f64) -> Result<(), String> {
    let (width, depth, height) = (room.width, room.depth, room.height);
    let split = (width - 0.12) * 0.7;
    let top = (height - 0.24).min(2.35);
    let textured = room.assets.grocer_props;
    use super::stock::{self, Grocery as G};
    let boxes = |rng: &mut SeededRng| stock::grocery(G::Boxes, rng, textured);
    let tins = |rng: &mut SeededRng| stock::grocery(G::LargeTins, rng, textured);
    let jars = |rng: &mut SeededRng| stock::grocery(G::Jars, rng, textured);
    let bottles = |rng: &mut SeededRng| stock::grocery(G::Bottles, rng, textured);
    let root = d.pen.clone();
    let mut banks = vec![
        (
            "rear-timber",
            furniture::Family::Timber,
            root.moved([0.04, depth - 0.51, 0.])?,
            [split, 0.49, top],
        ),
        (
            "rear-steel",
            furniture::Family::Steel,
            root.moved([split + 0.08, depth - 0.44, 0.])?,
            [width - split - 0.12, 0.42, top.min(1.96)],
        ),
    ];
    if depth - display_back > 1.85 {
        banks.push((
            "side",
            furniture::Family::TimberOpen,
            root.facing([0.48, display_back + 0.82, 0.], [0., 1., 0.], [-1., 0., 0.])?,
            [depth - display_back - 1.38, 0.46, top.min(1.8)],
        ));
    }
    for (name, family, frame, size) in banks {
        let pen = frame.into_part(&format!("{}/back-stock/{name}", root.part))?;
        let local = &mut on(d, pen);
        let mut rng = room.stream(&format!("back-stock/{name}"))?;
        let levels = (((size[2] - 0.1) / 0.34).floor() as usize).max(4);
        for shelf in
            furniture::shop_shelving(local, family, size, levels, srgb("#655b43"), 0.15, room.kit)?
        {
            stock::shelf(
                local,
                &shelf,
                &[&boxes, &tins, &jars, &bottles],
                &mut rng,
                stock::Settings {
                    fill: 1.,
                    maximum_rows: 2,
                    packed: true,
                },
            )?;
        }
    }
    Ok(())
}
pub fn interior(rng: &mut SeededRng) -> Interior {
    Interior {
        floor: shade(srgb("#6a6a64"), rng.float(0.85, 1.05)),
        ceiling: srgb("#cfcabb"),
        wall: srgb("#e4decb"),
        dado: rng.pick(&[srgb("#c8c4b4"), srgb("#9ab0a0"), srgb("#b8b09a")]),
        dado_height: 1.,
    }
}
