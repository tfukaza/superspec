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
