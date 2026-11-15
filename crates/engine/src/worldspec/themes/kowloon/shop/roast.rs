//! A complete compact kitchen, with independently seeded work and display zones.
use super::super::{
    fixture::Fixture,
    palette::{shade, srgb},
    pen::{Pen, Tint},
    pipes::{Options as PipeOptions, PipeSystem},
    posters,
};
use super::{
    chopping_block, fittings,
    forms::{self, Tints},
    furniture::{self, Shelf},
    goods,
    grocer_props::{self, Options},
    restaurant_fittings as kitchen, restaurant_food as food, restaurant_oven,
    restaurant_surfaces::{self, Settings},
    restaurant_work_details as details,
    room::{self, Interior, Room},
    stock::{self, Grocery as G},
    wares,
};
use superspec_geometry::{
    V3,
    math::{cos, sin},
};
use superspec_types::rng::SeededRng;
#[derive(Clone, Copy)]
pub struct Assets {
    pub food: bool,
    pub metal: bool,
    pub surfaces: bool,
    pub tiles: bool,
}
fn on<'a, 'c>(d: &'a mut Fixture<'_, 'c>, pen: Pen) -> Fixture<'a, 'c> {
    Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    }
}
fn galvanized() -> V3 {
    srgb("#a4a9aa")
}
fn prop(
    d: &mut Fixture<'_, '_>,
    room: &Room<'_>,
    name: &str,
    at: V3,
    size: V3,
    yaw: f64,
) -> Result<(), String> {
    grocer_props::draw(
        d,
        name,
        at,
        size,
        yaw,
        &Options {
            textured: room.assets.grocer_props,
            ..Default::default()
        },
    )
    .map(|_| ())
}
fn pantry(
    d: &mut Fixture<'_, '_>,
    shelf: &Shelf,
    rng: &mut SeededRng,
    textured: bool,
    side: bool,
) -> Result<(), String> {
    let tins = |r: &mut SeededRng| stock::grocery(G::Tins, r, textured);
    let small_jars = |r: &mut SeededRng| stock::grocery(G::SmallJars, r, textured);
    let jars = |r: &mut SeededRng| stock::grocery(G::Jars, r, textured);
    let bottles = |r: &mut SeededRng| stock::grocery(G::Bottles, r, textured);
    let small_tins = |r: &mut SeededRng| stock::grocery(G::SmallTins, r, textured);
    let bowls = |r: &mut SeededRng| goods::line(goods::Kind::Bowls, r, textured);
    let boxes = |r: &mut SeededRng| stock::grocery(G::Boxes, r, textured);
    let makers: &[stock::Maker<'_>] = if side {
        &[&bowls, &jars, &bottles, &small_tins, &boxes]
    } else {
        &[&tins, &small_jars, &bowls, &boxes]
    };
    stock::shelf(
        d,
        shelf,
        makers,
        rng,
        stock::Settings {
            fill: if side { 0.99 } else { 0.98 },
            maximum_rows: 1,
            packed: true,
        },
    )
}
pub fn furnish(d: &mut Fixture<'_, '_>, room: &Room<'_>, assets: Assets) -> Result<(), String> {
    room.validate()?;
    d.mesh.check_work(0)?;
    let (width, depth, h) = (room.width, room.depth, room.height);
    let dining = width >= 4.4 && depth >= 5.2;
    let w = if dining {
        (width - 2.05).min(2.45)
    } else {
        width
    };
    let mut layout = room.stream("layout")?;
    let mut pen = d.pen.clone();
    if layout.chance(0.5) && !dining {
        pen = pen.facing([w, 0., 0.], [-1., 0., 0.], [0., 1., 0.])?;
        pen.mirrored = !pen.mirrored;
    }
    let d = &mut on(d, pen);
    let root = d.pen.clone();
    let repeat = d.repeat("kitchen-metal");
    let settings = Settings {
        fields: assets.surfaces,
        tiles: assets.tiles,
        metal_repeat: repeat,
    };
    for (side, origin, tangent, outward) in [
        ("left", [0.004, 0., 0.], [0., 1., 0.], [-1., 0., 0.]),
        (
            "right",
            [width - 0.004, depth, 0.],
            [0., -1., 0.],
            [1., 0., 0.],
        ),
    ] {
        let pen = root
            .into_part(&format!("{}/room-wall-{side}", root.part))?
            .facing(origin, tangent, outward)?;
        let wall = &mut on(d, pen);
        wall.panel("concrete", [0., depth, 0., 0.9], 0., srgb("#426855"))?;
        let pen = wall.pen.moved([0., 0., 0.9])?;
        restaurant_surfaces::tiles(
            &mut on(wall, pen),
            [depth, h - 0.96],
            0.,
            super::super::wall::finish_seed(&format!("{}/{side}-wall", room.key)),
            true,
            settings,
        )?;
    }
    let mut rng = room.stream("fittings")?;
    let counter_y = if depth >= 2.7 && layout.chance(0.5) {
        0.26
    } else {
        0.08
    };
    let end = w - if w < 2.3 { 0.56 } else { 0.72 };
    let bench_width = end - 0.06;
    let bench = root.moved([0.06, counter_y, 0.])?;
    let top = kitchen::counter(
        &mut on(d, bench.clone()),
        [bench_width, 0.55, 0.92],
        rng.integer(1, 100000) as u32,
        settings,
    )?;
    let chop_x = bench_width - if bench_width >= 1.7 { 0.78 } else { 0.26 };
    chopping_block::draw(
        &mut on(d, bench.clone()),
        [chop_x, 0.3, top],
        room.kit.settings().timber.textured,
        settings,
    )?;
    let (register_x, register_y, scale) = if bench_width >= 1.7 {
        (bench_width - 0.02, 0.50, 1.)
    } else {
        (0.225, 0.39, 0.65)
    };
    let register = bench
        .into_part(&format!("{}/checkout/register", root.part))?
        .facing([register_x, register_y, top], [-1., 0., 0.], [0., -1., 0.])?;
    super::checkout::draw(
        &mut on(d, register),
        [0.; 3],
        scale,
        room.assets.shop_register,
    )?;
    let mut stock = room.stream("counter-stock")?;
    if bench_width >= 1.7 {
        kitchen::condiment_group(
            &mut on(d, bench.clone()),
            [0.19, 0.37, top],
            &mut stock,
            room.assets.grocer_props,
        )?;
        kitchen::takeaway_stack(&mut on(d, bench.clone()), [0.53, 0.42, top], &mut stock)?;
        if bench_width >= 2.3 {
            prop(
                &mut on(d, bench.clone()),
                room,
                "counter-scale",
                [0.92, 0.4, top],
                [0.28, 0.26, 0.32],
                -0.1,
            )?;
        }
    }
    let mut x = if bench_width >= 1.7 { 0.48 } else { 0.20 };
    let mut index = 0;
    while x < bench_width - 0.66 {
        let tray = forms::rotated(
            &bench,
            [x, 0.14 + stock.float(-0.006, 0.006), top],
            2,
            stock.float(-0.05, 0.05),
        )?;
        let tray = &mut on(d, tray);
        if index % 3 == 2 {
            kitchen::sauce_tray(tray, [0.; 3], repeat)?;
        } else {
            let floor = kitchen::meat_tray(tray, [0.; 3], [0.34, 0.24], repeat)?;
            let variation = stock.float(0.9, 1.);
            food::sliced_meat(
                tray,
                [0., 0., floor],
                [0.30 * variation, 0.20 * variation, 0.075],
                assets.food,
            )?;
        }
        x += 0.38;
        index += 1;
    }
    if bench_width < 1.7 {
        kitchen::bowl_stack(
            &mut on(d, bench.clone()),
            [0.16, 0.41, top],
            3,
            0.075,
            srgb("#385c7a"),
        )?;
    }
    let rail_z = (h - 0.22).min(1.98);
    let rail_y = counter_y + 0.30;
    let display_width = bench_width - if bench_width >= 1.7 { 0.72 } else { 0.62 };
    if bench_width >= 1.7 {
        details::display_cabinet(
            &mut on(d, bench.clone()),
            display_width,
            top,
            rail_z,
            repeat,
        )?;
        details::counter_towel(&mut on(d, bench), bench_width - 0.62, top)?;
    }
    let steel = shade(galvanized(), 0.95);
    fittings::rail(
        d,
        [0.06 + 0.10, rail_y, rail_z],
        [0.06 + display_width - 0.06, rail_y, rail_z],
        h,
        steel,
    )?;
    let mut meats = room.stream("meats")?;
    let food_pen = root.into_part(&format!("{}/display-food", root.part))?;
    let duck = food::duck_size();
    let bird_scale = ((rail_z - top - 0.34) / duck[2]).min(1.25);
    let pitch = (duck[0] + duck[1] * 0.1) * bird_scale + 0.035;
    let mut x = 0.08 + pitch / 2.;
    let mut index = 0;
    while bird_scale > 0. && x < display_width - pitch / 2. - 0.02 {
        if rail_z - 0.15 - duck[2] * bird_scale < top + 0.49
            && (x - 0.06 - chop_x).abs() < 0.2 + pitch / 2.
        {
            x += pitch;
            index += 1;
            continue;
        }
        let hanging = forms::rotated(&food_pen, [x, rail_y, rail_z], 2, meats.float(-0.08, 0.08))?;
        let hanging = &mut on(d, hanging);
        let held = fittings::hook(
            hanging,
            [0.; 3],
            if index % 3 == 1 { 0.12 } else { 0.065 } + meats.float(0., 0.025),
            steel,
        )?;
        food::hanging_duck(
            hanging,
            held,
            bird_scale * meats.float(0.90, 1.),
            meats.integer(0, 2),
            assets.food,
        )?;
        x += pitch;
        index += 1;
    }
    let lamps = (display_width / 1.05).round().max(1.) as usize;
    for i in 0..lamps {
        let pen = root.into_part(&format!("{}/task-lamp-{i}", root.part))?;
        fittings::pendant(
            &mut on(d, pen),
            [
                0.06 + (i as f64 + 0.5) * display_width / lamps as f64,
                counter_y + 0.43,
                h,
            ],
            h - rail_z - 0.04,
            Some(srgb("#b83e28")),
        )?;
    }
    restaurant_oven::roaster(
        &mut on(d, root.with_kitchen_metal(repeat)?),
        [0.46, depth - 0.46, 0.],
        &mut room.stream("oven")?,
        assets.metal,
        repeat,
    )?;
    PipeSystem::new(PipeOptions {
        radius: 0.06,
        color: shade(galvanized(), 0.8),
        sides: 16,
        variant: "pipe-standard".into(),
        weathering: Some(room.kit.settings().weathering.clone()),
    })?
    .run(
        d,
        &[[0.62, depth - 0.34, 1.3], [0.62, depth - 0.34, h]],
        None,
    )?;
    let stove_width = w - 0.98;
    if stove_width >= 0.8 {
        let stove = root.moved([0.92, depth - 0.64, 0.])?;
        let metal = stove.with_kitchen_metal(repeat)?;
        let hob = kitchen::workbench(&mut on(d, stove.clone()), [stove_width, 0.56, 0.78], repeat)?;
        let steel = &mut on(d, metal.clone());
        steel.cuboid(
            "metal",
            [0.015, 0.54, hob.z],
            [stove_width - 0.015, 0.56, hob.z + 0.075],
            [0.67, 0.70, 0.70],
            "d",
        )?;
        steel.bar(
            "metal",
            [0.015, 0.55, hob.z + 0.075],
            [stove_width - 0.015, 0.55, hob.z + 0.075],
            0.008,
            [0.88, 0.90, 0.91],
        )?;
        wares::pot(
            steel,
            [0.29, 0.3, hob.z],
            0.18,
            0.28,
            galvanized(),
            Some(srgb("#795231")),
        )?;
        if stove_width >= 1.25 {
            forms::lathe(
                steel,
                "metal",
                [0.82, 0.3, hob.z],
                &[[0.15, 0.], [0.15, 0.05], [0.11, 0.05]],
                &Tints::Uniform(&Tint::Color(srgb("#2a2927"))),
                10,
                None,
            )?;
            wares::wok(
                &mut on(d, stove.clone()),
                [0.82, 0.3, hob.z + 0.05],
                0.22,
                srgb("#353430"),
            )?;
        }
        if stove_width >= 1.95 {
            wares::pot(
                &mut on(d, metal.clone()),
                [stove_width - 0.28, 0.27, hob.z],
                0.15,
                0.18,
                galvanized(),
                None,
            )?;
            kitchen::bowl_stack(
                &mut on(d, stove.clone()),
                [stove_width - 0.70, 0.28, hob.z],
                5,
                0.075,
                srgb("#385c7a"),
            )?;
        }
        details::order_slips(
            d,
            [1.02, depth - 0.075, 1.59_f64.min(h - 0.5)],
            0.72_f64.min(stove_width - 0.16),
            &mut room.stream("orders")?,
            repeat,
        )?;
        details::chopstick_cup(
            &mut on(d, stove),
            [(stove_width - 0.10).min(0.57), 0.43, hob.z],
            &mut room.stream("chopsticks")?,
        )?;
        wares::ladles(
            &mut on(d, metal),
            [stove_width - 0.14, 0.50, 1.26],
            &mut rng,
        )?;
        kitchen::extractor(
            d,
            [0.95, depth - 0.63, 2.02_f64.min(h - 0.3)],
            (stove_width - 0.06).min(1.35),
            h,
            repeat,
        )?;
        let high = root.moved([0.92, depth - 0.22, h - 0.32])?;
        let board = &mut on(d, high.with_timber(room.kit.settings().timber.clone())?);
        board.cuboid(
            "timber",
            [0.; 3],
            [stove_width, 0.22, 0.022],
            srgb("#7a5a3c"),
            "f",
        )?;
        board.cuboid(
            "timber",
            [0.03, 0.18, -0.045],
            [stove_width - 0.03, 0.22, 0.],
            srgb("#6e5136"),
            "f",
        )?;
        pantry(
            &mut on(d, high),
            &Shelf {
                x0: 0.03,
                x1: stove_width - 0.03,
                y: 0.01,
                depth: 0.2,
                z: 0.022,
                clear: 0.26,
            },
            &mut room.stream("high-pantry")?,
            room.assets.grocer_props,
            false,
        )?;
    }
    if w >= 2.3 && depth >= 2.7 {
        let side = room
            .left_wall(&root, 0.91, 0.03)?
            .moved([0., -0.25, 1.26])?;
        let d = &mut on(d, side);
        let shelves = furniture::shelving(
            d,
            [depth - 1.83, 0.23, 0.65_f64.min(h - 1.51)],
            2,
            "timber",
            srgb("#7a5a3c"),
            0.25,
            &room.kit.settings().timber,
        )?;
        let mut rng = room.stream("side-pantry")?;
        for shelf in shelves {
            pantry(d, &shelf, &mut rng, room.assets.grocer_props, true)?;
        }
    }
    let mut supplies = room.stream("supplies")?;
    if w >= 2.3 {
        prop(
            d,
            room,
            "stock-box",
            [w - 0.28, depth - 1.25, 0.],
            [0.42, 0.34, 0.28],
            0.045,
        )?;
        prop(
            d,
            room,
            "stock-box",
            [w - 0.29, depth - 1.25, 0.28],
            [0.36, 0.30, 0.22],
            -0.07,
        )?;
        let yaw = supplies.float(-0.2, 0.2);
        let size = grocer_props::size("shop-stool", [0.36, 0.36, 0.46])?;
        let stool_depth = sin(yaw).abs() * size[0] + cos(yaw) * size[1];
        let box_depth = 0.42 * sin(0.045) + 0.34 * cos(0.045);
        if depth - 1.25 - 1.12 > (stool_depth + box_depth) / 2. + 0.04 {
            prop(
                d,
                room,
                "shop-stool",
                [w - 0.28, 1.12, 0.],
                [0.36, 0.36, 0.46],
                yaw,
            )?;
        }
    }
    wares::bucket(d, [w - 0.2, 0.3, 0.], 0.13, 0.23, srgb("#2d7885"))?;
    if depth >= 2.7 {
        restaurant_oven::gas_cylinder(d, [w - 0.25, depth - 0.89, 0.], &mut rng)?;
    }
    restaurant_surfaces::tiles(d, [w, 1.5], depth - 0.003, 317, true, settings)?;
    restaurant_surfaces::tiles(
        &mut on(d, room.left_wall(&root, 0., 0.)?),
        [depth, 1.5],
        -0.003,
        318,
        false,
        settings,
    )?;
    restaurant_surfaces::tiles(
        &mut on(d, room.right_wall(&root, depth, 0.)?),
        [depth, 1.5],
        -0.003,
        319,
        false,
        settings,
    )?;
    if room.assets.posters && h >= 2.35 {
        let mut rng = room.stream("posters")?;
        let wall = root.into_part(&format!("{}/posters", root.part))?.facing(
            [w - 0.006, 0., 0.],
            [0., -1., 0.],
            [-1., 0., 0.],
        )?;
        for (design, width, position, turn, seed) in [
            ("roast-menu", 0.34, [-0.94, 0., 1.91], 2., "menu"),
            ("calendar", 0.25, [-1.55, 0., 1.92], 3., "calendar"),
        ] {
            posters::draw(
                &mut on(d, wall.moved(position)?),
                &posters::Options {
                    design: design.into(),
                    width,
                    rotation: rng.float(-turn, turn),
                    layer: 0,
                    wear: 0.3,
                    seed: Some(format!("{}/{seed}", room.key)),
                },
                true,
            )?;
        }
        let left = root.into_part(&format!("{}/posters", root.part))?.facing(
            [0.006, 0.43, 1.71],
            [0., 1., 0.],
            [1., 0., 0.],
        )?;
        posters::draw(
            &mut on(d, left),
            &posters::Options {
                design: "soda-ad".into(),
                width: 0.23,
                rotation: rng.float(-3., 3.),
                layer: 0,
                wear: 0.3,
                seed: Some(format!("{}/soda", room.key)),
            },
            true,
        )?;
    }
    if dining {
        dining_tables(d, room)?;
    }
    let kitchen = Room { width: w, ..*room };
    room::amenities(
        d,
        &kitchen,
        0.9_f64.min(depth - 1.9),
        (depth >= 3.).then_some(1.2),
    )
}

fn dining_tables(d: &mut Fixture<'_, '_>, room: &Room<'_>) -> Result<(), String> {
    let mut y = 1.2;
    let mut index = 0;
    let wood = srgb("#746149");
    let frame = srgb("#6a7267");
    while y + 1.45 <= room.depth - 1.1 + 1e-8 {
        let table_pen = d
            .pen
            .into_part(&format!("{}/dining/table-{index}", d.pen.part))?
            .moved([room.width - 0.78, y + 0.40, 0.])?;
        let mut boxes = vec![[[0., 0., 0.72], [0.65, 0.62, 0.766]]];
        for x in [0.045, 0.605] {
            for py in [0.045, 0.575] {
                boxes.push([[x - 0.012, py - 0.012, 0.], [x + 0.012, py + 0.012, 0.72]]);
            }
        }
        let table_pen = table_pen.with_shelf_patina(super::patina::Shelf::Furniture {
            inverse: table_pen.matrix.inverse()?,
            boxes: boxes.into(),
        });
        let table = &mut on(d, table_pen);
        for x in [0.045, 0.605] {
            for py in [0.045, 0.575] {
                table.cuboid(
                    "metal",
                    [x - 0.012, py - 0.012, 0.],
                    [x + 0.012, py + 0.012, 0.72],
                    frame,
                    "",
                )?;
            }
        }
        table.cuboid("timber", [0., 0., 0.72], [0.65, 0.62, 0.76], wood, "")?;
        table.cuboid(
            "paint",
            [0.008, 0.008, 0.76],
            [0.642, 0.612, 0.766],
            srgb("#b8b19c"),
            "",
        )?;
        let mut stock = room.stream(&format!("table-setting/{index}"))?;
        kitchen::condiment_group(
            table,
            [0.27, 0.46, 0.766],
            &mut stock,
            room.assets.grocer_props,
        )?;
        for py in [y + 0.16, y + 1.29] {
            prop(
                d,
                room,
                "shop-stool",
                [room.width - 0.455, py, 0.],
                [0.32, 0.32, 0.45],
                0.,
            )?;
        }
        index += 1;
        y += 1.6;
    }
    Ok(())
}
pub fn interior(rng: &mut SeededRng) -> Interior {
    Interior {
        floor: shade(srgb("#8a4a3a"), rng.float(0.8, 1.05)),
        ceiling: srgb("#d4d0c0"),
        wall: srgb("#e8eae4"),
        dado: rng.pick(&[srgb("#e0e4e0"), srgb("#b8d0c8"), srgb("#dfe6ea")]),
        dado_height: 1.5,
    }
}
