//! Fixed shelf banks and street stock for the reference corner provisions shop.
use super::{
    fittings, furniture, goods, grocer_props,
    room::{Interior, Room},
    snack_string,
    stock::{self, Grocery, Settings},
};
use crate::worldspec::themes::kowloon::{
    fixture::Fixture, kit::Kit, laundry, palette::srgb, pen::Pen,
};

fn on<'a, 'c>(d: &'a mut Fixture<'_, 'c>, pen: Pen) -> Fixture<'a, 'c> {
    Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    }
}

pub(super) fn interior() -> Interior {
    Interior {
        floor: srgb("#747168"),
        ceiling: srgb("#b3ac99"),
        wall: srgb("#b3aa91"),
        dado: srgb("#4c655b"),
        dado_height: 1.1,
    }
}

fn rows(
    d: &mut Fixture<'_, '_>,
    shelves: &[furniture::Shelf],
    goods: &[Grocery],
    kit: &Kit,
    key: &str,
    settings: Settings,
    textured: bool,
) -> Result<(), String> {
    let factories: Vec<_> = goods
        .iter()
        .map(|&kind| move |rng: &mut _| stock::grocery(kind, rng, textured))
        .collect();
    let makers: Vec<_> = factories.iter().map(|f| f as stock::Maker<'_>).collect();
    for (index, level) in shelves.iter().enumerate() {
        stock::shelf(
            d,
            level,
            &makers,
            &mut kit.rng(&format!("{key}/{index}"))?,
            settings,
        )?;
    }
    Ok(())
}

pub(super) fn furnish(d: &mut Fixture<'_, '_>, room: &Room<'_>) -> Result<(), String> {
    use Grocery::*;
    let wood = srgb("#78644b");
    let timber = &room.kit.settings().timber;
    let root = d.pen.clone();
    let textured = room.assets.grocer_props;
    for (i, [x, width, height]) in [[0.08, 2.86, 2.22], [3.06, 2.84, 2.46]]
        .into_iter()
        .enumerate()
    {
        let mut bank = on(
            d,
            root.into_part(&format!("back-stock-{i}"))?
                .moved([x, room.depth - 0.39, 0.])?,
        );
        let shelves = furniture::shelving(
            &mut bank,
            [width, 0.35, height],
            6,
            "timber",
            wood,
            0.2,
            timber,
        )?;
        rows(
            &mut bank,
            &shelves,
            &[Boxes, TallBoxes, Tins, SmallJars, Bottles, DryBags],
            room.kit,
            &format!("back-stock-{i}"),
            Settings {
                fill: 0.96,
                maximum_rows: 2,
                packed: true,
            },
            textured,
        )?;
    }
    let mut bank = on(
        d,
        root.into_part("side-stock")?
            .facing([0.25, 0.8, 0.], [0., 1., 0.], [-1., 0., 0.])?,
    );
    let shelves =
        furniture::shelving(&mut bank, [1.8, 0.32, 2.16], 5, "timber", wood, 0.2, timber)?;
    rows(
        &mut bank,
        &shelves,
        &[Cartons, SmallBottles, SmallJars],
        room.kit,
        "side-stock",
        Settings {
            fill: 0.95,
            maximum_rows: 1,
            packed: true,
        },
        textured,
    )?;
    let mut counter = on(d, root.into_part("counter")?.moved([0.1, 0.39, 0.])?);
    let top = furniture::counter(
        &mut counter,
        [1.3, 0.56, 0.93],
        furniture::Finish::Timber,
        wood,
        &mut room.stream("counter")?,
        timber,
    )?;
    rows(
        &mut counter,
        &[top],
        &[SmallJars, Cartons],
        room.kit,
        "counter-stock",
        Settings {
            fill: 0.75,
            maximum_rows: 1,
            packed: false,
        },
        textured,
    )?;
    snack_string::row(
        d,
        [0.22, 2.25, 0.55, 2.53],
        &mut room.stream("hung-packets")?,
        room.assets.snack_packets,
    )?;
    let cloth = d.mesh.has_source_material("balcony-cloth");
    laundry::washing(
        &mut on(d, root.moved([0.36, 0.25, 0.])?),
        [0., 1.45],
        2.48,
        &mut room.stream("shop-cloths")?,
        &laundry::WashingOptions {
            fill: 1.,
            drop: 1.08,
            support: laundry::Support::Pole,
        },
        cloth,
    )?;
    fittings::tube_light(d, [0.38, 1.22, 2.69], [2.3, 1.22, 2.69])?;
    for (i, [x, y, size, height]) in [
        [0.7, 1.42, 0.48, 0.72],
        [1.24, 1.7, 0.58, 1.02],
        [4.65, 2.2, 0.56, 0.8],
        [5.36, 1.69, 0.43, 0.63],
    ]
    .into_iter()
    .enumerate()
    {
        goods::carton_stack(
            d,
            [x, y, 0.025],
            size,
            height,
            &mut room.stream(&format!("inside-cartons-{i}"))?,
            textured,
        )?;
    }
    Ok(())
}

/// A provisions shop fitted to any room: packed shelf banks along the back (one per 3 m),
/// a side bank and a counter where there is room, hung packets and cloths, a tube light per
/// bank and seeded carton stacks on the floor.
pub(super) fn furnish_fitted(d: &mut Fixture<'_, '_>, room: &Room<'_>) -> Result<(), String> {
    use Grocery::*;
    room.validate()?;
    let wood = srgb("#78644b");
    let timber = &room.kit.settings().timber;
    let root = d.pen.clone();
    let textured = room.assets.grocer_props;
    let (w, depth, h) = (room.width, room.depth, room.height);
    let banks = ((w / 3.).round() as usize).max(1);
    let pitch = (w - 0.16) / banks as f64;
    for i in 0..banks {
        let height = (if i % 2 == 0 { 2.22_f64 } else { 2.46 }).min(h - 0.3);
        let mut bank = on(
            d,
            root.into_part(&format!("back-stock-{i}"))?.moved([
                0.08 + i as f64 * pitch,
                depth - 0.39,
                0.,
            ])?,
        );
        let shelves = furniture::shelving(
            &mut bank,
            [pitch - 0.1, 0.35, height],
            6,
            "timber",
            wood,
            0.2,
            timber,
        )?;
        rows(
            &mut bank,
            &shelves,
            &[Boxes, TallBoxes, Tins, SmallJars, Bottles, DryBags],
            room.kit,
            &format!("{}/back-stock-{i}", room.key),
            Settings {
                fill: 0.96,
                maximum_rows: 2,
                packed: true,
            },
            textured,
        )?;
    }
    let side = (depth - 1.3).min(1.8);
    if side >= 0.9 && w >= 2.6 {
        let mut bank = on(
            d,
            root.into_part("side-stock")?
                .facing([0.25, 0.8, 0.], [0., 1., 0.], [-1., 0., 0.])?,
        );
        let shelves = furniture::shelving(
            &mut bank,
            [side, 0.32, 2.16_f64.min(h - 0.3)],
            5,
            "timber",
            wood,
            0.2,
            timber,
        )?;
        rows(
            &mut bank,
            &shelves,
            &[Cartons, SmallBottles, SmallJars],
            room.kit,
            &format!("{}/side-stock", room.key),
            Settings {
                fill: 0.95,
                maximum_rows: 1,
                packed: true,
            },
            textured,
        )?;
    }
    if w >= 3.6 {
        let mut counter = on(d, root.into_part("counter")?.moved([w - 1.5, 0.45, 0.])?);
        let top = furniture::counter(
            &mut counter,
            [1.3, 0.56, 0.93],
            furniture::Finish::Timber,
            wood,
            &mut room.stream("counter")?,
            timber,
        )?;
        rows(
            &mut counter,
            &[top],
            &[SmallJars, Cartons],
            room.kit,
            &format!("{}/counter-stock", room.key),
            Settings {
                fill: 0.75,
                maximum_rows: 1,
                packed: false,
            },
            textured,
        )?;
    }
    if w >= 2.2 && h >= 2.5 {
        snack_string::row(
            d,
            [0.45, (w - 0.3).min(0.45 + 2.2), 0.55, (h - 0.2).min(2.53)],
            &mut room.stream("hung-packets")?,
            room.assets.snack_packets,
        )?;
    }
    if w >= 4.6 && h >= 2.6 {
        let cloth = d.mesh.has_source_material("balcony-cloth");
        laundry::washing(
            &mut on(d, root.moved([w - 1.9, 0.25, 0.])?),
            [0., 1.45],
            (h - 0.25).min(2.48),
            &mut room.stream("shop-cloths")?,
            &laundry::WashingOptions {
                fill: 1.,
                drop: 1.08,
                support: laundry::Support::Pole,
            },
            cloth,
        )?;
    }
    for i in 0..banks {
        let x = 0.08 + i as f64 * pitch;
        fittings::tube_light(
            d,
            [x + pitch * 0.2, depth * 0.45, h - 0.04],
            [x + pitch * 0.8, depth * 0.45, h - 0.04],
        )?;
    }
    let mut rng = room.stream("inside-cartons")?;
    for i in 0..(w / 1.4) as usize {
        let size = rng.float(0.42, 0.58);
        let x = rng.float(0.75, (w - 0.6).max(0.8));
        let y = rng.float(1.3, (depth - 0.95).max(1.35));
        goods::carton_stack(
            d,
            [x, y, 0.025],
            size,
            rng.float(0.6, 1.05),
            &mut room.stream(&format!("inside-cartons-{i}"))?,
            textured,
        )?;
    }
    Ok(())
}

/// Street stock along a shop front of `width`, in the wall's frame (x along it from 0, y out
/// from the wall, the pavement at z = 0): seeded stacks of slatted crates loaded with bottles or
/// cartons, a stocked wire rack by the right jamb and a sack truck by the left.
pub(crate) fn street_fitted(
    d: &mut Fixture<'_, '_>,
    kit: &Kit,
    key: &str,
    width: f64,
    textured: bool,
) -> Result<(), String> {
    if !width.is_finite() || !(1.2..=40.).contains(&width) || key.len() > 1024 {
        return Err("Street stock requires a frontage of 1.2 to 40 meters.".into());
    }
    let root = d.pen.into_part("street-stock")?;
    let mut rng = kit.rng(&format!("{key}/street-stock"))?;
    let rack = width >= 3.;
    let dolly = width >= 4.2;
    let low = if dolly { 0.75 } else { 0.1 };
    let high = width - if rack { 0.95 } else { 0.1 };
    let stacks = ((high - low) / 1.15) as usize;
    let slot = (high - low) / stacks.max(1) as f64;
    for i in 0..stacks {
        let color = srgb(rng.pick(&["#a46752", "#586c5d", "#a87359", "#717451", "#866b4f"]));
        let size = rng.float(0.46, 0.57);
        let rows = rng.integer(1, 3);
        let x = low + i as f64 * slot + rng.float(0.02, (slot - size - 0.04).max(0.03));
        let y = rng.float(0.26, 0.56);
        let cartons = rng.chance(0.3);
        for row in 0..rows {
            let mut crate_fixture = on(
                d,
                root.moved([x + (row % 2) as f64 * 0.026, y, 0.015 + row as f64 * 0.29])?,
            );
            let floor = furniture::crate_box(
                &mut crate_fixture,
                [size, 0.4, 0.29],
                color,
                &mut kit.rng(&format!("{key}/crate-{i}-{row}"))?,
                &kit.settings().timber,
            )?;
            if row + 1 != rows {
                continue;
            }
            let center = (floor.x0 + floor.x1) / 2.;
            if cartons {
                let rim = floor.z + floor.clear;
                crate_fixture.cuboid(
                    "timber",
                    [-0.008, -0.008, rim],
                    [size + 0.008, 0.408, rim + 0.018],
                    color,
                    "",
                )?;
                goods::carton_stack(
                    &mut crate_fixture,
                    [center, 0.2, rim + 0.018],
                    0.36,
                    0.52,
                    &mut kit.rng(&format!("{key}/street-cartons-{i}"))?,
                    textured,
                )?;
            } else {
                let load = grocer_props::size(
                    "bottle-load",
                    [
                        floor.x1 - floor.x0 - 0.012,
                        floor.depth - 0.012,
                        floor.clear + 0.085,
                    ],
                )?;
                grocer_props::draw(
                    &mut crate_fixture,
                    "bottle-load",
                    [center, floor.y + floor.depth / 2., floor.z],
                    load,
                    0.,
                    &grocer_props::Options {
                        variant: i % 4 + 1,
                        textured,
                        ..Default::default()
                    },
                )?;
            }
        }
    }
    if rack {
        let mut fixture = on(
            d,
            root.into_part("threshold-rack")?.facing(
                [width - 0.8, 0.26, 0.03],
                [1., 0., 0.],
                [0., -1., 0.],
            )?,
        );
        let shelves = furniture::shop_shelving(
            &mut fixture,
            furniture::Family::Wire,
            [0.66, 0.34, 2.22],
            6,
            srgb("#808579"),
            0.,
            kit,
        )?;
        rows(
            &mut fixture,
            &shelves,
            &[
                Grocery::Tins,
                Grocery::SmallJars,
                Grocery::SmallBottles,
                Grocery::Cartons,
            ],
            kit,
            &format!("{key}/threshold-rack"),
            Settings {
                fill: 0.97,
                maximum_rows: 1,
                packed: true,
            },
            textured,
        )?;
    }
    if dolly {
        let mut truck = on(d, root.into_part("dolly")?.moved([0.12, 0.24, 0.])?);
        let ivory = srgb("#dfcfaa");
        for x in [0., 0.4] {
            truck.tube(
                "metal",
                &[[x, 0., 0.15], [x, 0., 1.58], [x, 0.2, 1.64]],
                0.021,
                ivory,
                8,
            )?;
            truck.disc(
                "paint",
                [x, 0.14, 0.14],
                0.13,
                [1., 0., 0.],
                srgb("#343632"),
                20,
            )?;
        }
        for z in [0.32, 0.76, 1.24] {
            truck.tube("metal", &[[0., 0., z], [0.4, 0., z]], 0.018, ivory, 8)?;
        }
        truck.cuboid(
            "metal",
            [-0.02, 0., 0.06],
            [0.42, 0.33, 0.095],
            srgb("#3f554b"),
            "",
        )?;
    }
    Ok(())
}

pub(crate) fn street(d: &mut Fixture<'_, '_>, kit: &Kit, textured: bool) -> Result<(), String> {
    let root = d.pen.into_part("street-stock")?;
    for (i, (x, y, rows, width, pigment)) in [
        (3.3, 0.48, 3, 0.48, "#a46752"),
        (3.94, 0.39, 2, 0.55, "#586c5d"),
        (4.54, 0.55, 3, 0.49, "#a87359"),
        (7.66, 0.28, 2, 0.56, "#717451"),
        (8.4, 0.53, 1, 0.49, "#866b4f"),
    ]
    .into_iter()
    .enumerate()
    {
        let color = srgb(pigment);
        for row in 0..rows {
            let mut crate_fixture = on(
                d,
                root.moved([
                    x + f64::from(row % 2) * 0.026,
                    y,
                    0.015 + f64::from(row) * 0.29,
                ])?,
            );
            let floor = furniture::crate_box(
                &mut crate_fixture,
                [width, 0.4, 0.29],
                color,
                &mut kit.rng(&format!("crate-{i}-{row}"))?,
                &kit.settings().timber,
            )?;
            if row + 1 != rows {
                continue;
            }
            let center = (floor.x0 + floor.x1) / 2.;
            if i == 1 {
                let rim = floor.z + floor.clear;
                crate_fixture.cuboid(
                    "timber",
                    [-0.008, -0.008, rim],
                    [width + 0.008, 0.408, rim + 0.018],
                    color,
                    "",
                )?;
                goods::carton_stack(
                    &mut crate_fixture,
                    [center, 0.2, rim + 0.018],
                    0.36,
                    0.52,
                    &mut kit.rng("street-cartons")?,
                    textured,
                )?;
            } else {
                let size = grocer_props::size(
                    "bottle-load",
                    [
                        floor.x1 - floor.x0 - 0.012,
                        floor.depth - 0.012,
                        floor.clear + 0.085,
                    ],
                )?;
                grocer_props::draw(
                    &mut crate_fixture,
                    "bottle-load",
                    [center, floor.y + floor.depth / 2., floor.z],
                    size,
                    0.,
                    &grocer_props::Options {
                        variant: i + 1,
                        textured,
                        ..Default::default()
                    },
                )?;
            }
        }
    }
    let mut rack = on(
        d,
        root.into_part("threshold-rack")?.facing(
            [8.29, 0.26, 0.03],
            [1., 0., 0.],
            [0., -1., 0.],
        )?,
    );
    let shelves = furniture::shop_shelving(
        &mut rack,
        furniture::Family::Wire,
        [0.66, 0.34, 2.22],
        6,
        srgb("#808579"),
        0.,
        kit,
    )?;
    rows(
        &mut rack,
        &shelves,
        &[
            Grocery::Tins,
            Grocery::SmallJars,
            Grocery::SmallBottles,
            Grocery::Cartons,
        ],
        kit,
        "threshold-rack",
        Settings {
            fill: 0.97,
            maximum_rows: 1,
            packed: true,
        },
        textured,
    )?;
    let mut dolly = on(d, root.into_part("dolly")?.moved([2.83, 0.24, 0.])?);
    let ivory = srgb("#dfcfaa");
    for x in [0., 0.4] {
        dolly.tube(
            "metal",
            &[[x, 0., 0.15], [x, 0., 1.58], [x, 0.2, 1.64]],
            0.021,
            ivory,
            8,
        )?;
        dolly.disc(
            "paint",
            [x, 0.14, 0.14],
            0.13,
            [1., 0., 0.],
            srgb("#343632"),
            20,
        )?;
    }
    for z in [0.32, 0.76, 1.24] {
        dolly.tube("metal", &[[0., 0., z], [0.4, 0., z]], 0.018, ivory, 8)?;
    }
    dolly.cuboid(
        "metal",
        [-0.02, 0., 0.06],
        [0.42, 0.33, 0.095],
        srgb("#3f554b"),
        "",
    )
}
