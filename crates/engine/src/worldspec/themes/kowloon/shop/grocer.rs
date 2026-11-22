//! Complete provisions-store furniture, stock, suspended goods and lighting.
use super::super::{
    fixture::Fixture,
    palette::{shade, srgb},
};
use super::{
    fittings, forms,
    furniture::{self, Shelf},
    grocer_layout::{self, Placement},
    grocer_props::{self, Group, Options},
    room::{Interior, Room},
    snack_string,
    stock::{self, Grocery as G, Settings},
};
use superspec_geometry::{
    V3,
    math::{cos, sin},
};
use superspec_types::rng::SeededRng;

fn footprint(size: V3, yaw: f64) -> [f64; 2] {
    let c = cos(yaw).abs();
    let s = sin(yaw).abs();
    [size[0] * c + size[1] * s, size[0] * s + size[1] * c]
}
fn placed<'a, 'c>(d: &'a mut Fixture<'_, 'c>, p: &Placement) -> Result<Fixture<'a, 'c>, String> {
    Ok(Fixture {
        pen: forms::rotated(&d.pen, p.origin, 2, p.turn)?,
        mesh: d.mesh,
        repeats: d.repeats,
    })
}
fn prop(
    d: &mut Fixture<'_, '_>,
    room: &Room<'_>,
    name: &str,
    at: V3,
    size: V3,
    yaw: f64,
    variant: usize,
) -> Result<(), String> {
    grocer_props::draw(
        d,
        name,
        at,
        size,
        yaw,
        &Options {
            variant,
            textured: room.assets.grocer_props,
            ..Default::default()
        },
    )?;
    Ok(())
}
fn stock(
    d: &mut Fixture<'_, '_>,
    room: &Room<'_>,
    shelf: &Shelf,
    kinds: &[G],
    rng: &mut SeededRng,
    rows: usize,
) -> Result<(), String> {
    stock::groceries(
        d,
        shelf,
        kinds,
        rng,
        Settings {
            fill: 1.,
            maximum_rows: rows,
            packed: true,
        },
        room.assets.grocer_props,
    )
}
fn counter(d: &mut Fixture<'_, '_>, room: &Room<'_>, placement: &Placement) -> Result<(), String> {
    let size = grocer_props::size("shop-counter", [placement.size[0], 0.5, 0.9])?;
    let d = &mut placed(d, placement)?;
    prop(
        d,
        room,
        "shop-counter",
        [size[0] / 2., size[1] / 2., 0.],
        size,
        0.,
        0,
    )?;
    let mut rng = room.stream("counter")?;
    let scale = grocer_props::size(
        "counter-scale",
        [0.3_f64.min(size[0] * 0.3), size[1] * 0.64, 0.38],
    )?;
    let yaw = rng.float(-0.28, 0.28);
    let fp = footprint(scale, yaw);
    let left = size[0] - fp[0] - 0.025;
    prop(
        d,
        room,
        "counter-scale",
        [left + fp[0] / 2., size[1] / 2., size[2]],
        scale,
        yaw,
        0,
    )?;
    let pen = d
        .pen
        .into_part(&format!("{}/checkout/register", d.pen.part))?
        .facing([0.325, 0.40, size[2]], [-1., 0., 0.], [0., -1., 0.])?;
    super::checkout::draw(
        &mut Fixture {
            pen,
            mesh: d.mesh,
            repeats: d.repeats,
        },
        [0.; 3],
        1.,
        room.assets.shop_register,
    )?;
    if size[0] >= 1.3 {
        let basket = grocer_props::size("egg-basket", [0.28, 0.16, 0.12])?;
        let yaw = rng.float(-0.3, 0.3);
        let depth = footprint(basket, yaw)[1];
        prop(
            d,
            room,
            "egg-basket",
            [0.55, size[1] - depth / 2. - 0.02, size[2]],
            basket,
            yaw,
            0,
        )?;
    }
    Ok(())
}
fn crates(d: &mut Fixture<'_, '_>, room: &Room<'_>, placement: &Placement) -> Result<(), String> {
    let d = &mut placed(d, placement)?;
    let mut rng = room.stream("crates")?;
    let size = grocer_props::size("bottle-crate", [0.42, 0.3, 0.27])?;
    let stacks = ((placement.size[0] - 0.02) / 0.47).floor().clamp(1., 2.) as usize;
    for stack in 0..stacks {
        let height = rng.integer(2, 3);
        let center = [
            0.23 + stack as f64 * 0.47,
            0.22 + rng.float(-0.012, 0.012),
            0.,
        ];
        let pen = forms::rotated(&d.pen, center, 2, rng.float(-0.09, 0.09))?;
        let local = &mut Fixture {
            pen,
            mesh: d.mesh,
            repeats: d.repeats,
        };
        for level in 0..height {
            let z = level as f64 * size[2];
            prop(
                local,
                room,
                "bottle-crate",
                [0., 0., z],
                size,
                0.,
                stack % 3 + 1,
            )?;
            if level == height - 1 {
                prop(
                    local,
                    room,
                    "bottle-load",
                    [0., 0., z + size[2] * 0.075],
                    [size[0] * 0.9, size[1] * 0.85, size[2] * 1.2],
                    0.,
                    rng.integer(1, 8),
                )?;
            }
        }
    }
    Ok(())
}
fn cooler(d: &mut Fixture<'_, '_>, room: &Room<'_>, placement: &Placement) -> Result<(), String> {
    let size = placement.size;
    let d = &mut placed(d, placement)?;
    d.pen = d.pen.moved([size[0] / 2., size[1] / 2., 0.])?;
    prop(d, room, "drinks-cooler", [0.; 3], size, 0., 0)?;
    let lining = srgb("#d7d9d2");
    d.cuboid(
        "metal",
        [-size[0] * 0.47, size[1] * 0.36, size[2] * 0.20],
        [size[0] * 0.47, size[1] * 0.40, size[2] * 0.87],
        lining,
        "",
    )?;
    for side in [-1., 1.] {
        let a = side * size[0] * 0.42;
        let b = side * size[0] * 0.47;
        d.cuboid(
            "metal",
            [a.min(b), -size[1] * 0.46, size[2] * 0.20],
            [a.max(b), size[1] * 0.4, size[2] * 0.87],
            lining,
            "",
        )?;
    }
    for z in [size[2] * 0.20, size[2] * 0.86] {
        d.cuboid(
            "metal",
            [-size[0] * 0.47, -size[1] * 0.46, z],
            [size[0] * 0.47, size[1] * 0.40, z + 0.012],
            lining,
            "",
        )?;
    }
    let mut rng = room.stream("cooler")?;
    for fraction in [0.24, 0.45, 0.66] {
        let z = size[2] * fraction;
        d.cuboid(
            "metal",
            [-size[0] * 0.42, -size[1] * 0.42, z - 0.012],
            [size[0] * 0.42, size[1] * 0.40, z],
            srgb("#a9aaa2"),
            "d",
        )?;
        stock(
            d,
            room,
            &Shelf {
                x0: -size[0] * 0.4,
                x1: size[0] * 0.4,
                y: -size[1] * 0.37,
                depth: size[1] * 0.68,
                z,
                clear: size[2] * 0.19,
            },
            &[G::SmallBottles, G::Tins, G::LargeTins],
            &mut rng,
            1,
        )?;
    }
    Ok(())
}
fn sacks(d: &mut Fixture<'_, '_>, room: &Room<'_>, placement: &Placement) -> Result<(), String> {
    let mut rng = room.stream("sacks")?;
    let d = &mut placed(d, placement)?;
    let mut left = 0.;
    while left < placement.size[0] - 0.2 {
        d.mesh.check_work(0)?;
        let arrangement = rng.integer(0, 3);
        let mut size = grocer_props::bag_size(
            "rice-sack",
            [rng.float(1.12, 1.26), 0.48, rng.float(0.45, 0.6)],
            d.pen.mirrored,
            arrangement,
        )?;
        let yaw = rng.float(-0.2, 0.2);
        let [width, depth] = footprint(size, yaw);
        let fit = 1_f64
            .min((placement.size[0] - 0.002) / width)
            .min((placement.size[1] - 0.002) / depth);
        size = size.map(|v| v * fit);
        let [width, depth] = footprint(size, yaw);
        if left + width > placement.size[0] {
            break;
        }
        let at = [
            left + width / 2.,
            depth / 2. + rng.float(0., placement.size[1] - depth),
            0.,
        ];
        grocer_props::bag_cluster(
            d,
            "rice-sack",
            at,
            size,
            &Group {
                yaw,
                variant: rng.integer(1, 4),
                arrangement,
                textured: room.assets.grocer_props,
            },
        )?;
        left += width + rng.float(0.003, 0.012);
    }
    Ok(())
}
fn lights(d: &mut Fixture<'_, '_>, room: &Room<'_>, counter: &Placement) -> Result<(), String> {
    let (w, depth, h) = (room.width, room.depth, room.height);
    fittings::pendant(
        &mut placed(d, counter)?,
        [counter.size[0] / 2., 0.16, h],
        0.3_f64.max(h - 1.8),
        Some(srgb("#a52920")),
    )?;
    fittings::tube_light(d, [w * 0.2, depth * 0.5, h], [w * 0.8, depth * 0.5, h])?;
    let fan = (depth - 1.02).min(depth * 0.5 + 0.75);
    if h >= 2.2 && fan - depth * 0.5 >= 0.62 && room.extra("fan", 0.45)? {
        fittings::ceiling_fan(
            d,
            [w / 2., fan, h],
            0.28,
            1.1_f64.min(w - 0.7),
            &mut room.stream("fan")?,
        )?;
    }
    Ok(())
}
/// Furnishes a validated room in its host frame; emitted detail does not imply collision.
pub fn furnish(d: &mut Fixture<'_, '_>, room: &Room<'_>) -> Result<(), String> {
    room.validate()?;
    d.mesh.check_work(0)?;
    let layout = grocer_layout::plan(
        room.width,
        room.depth,
        room.height,
        &mut room.stream("layout")?,
    )?;
    let pen = if layout.mirrored {
        let reflected = d
            .pen
            .facing([room.width, 0., 0.], [-1., 0., 0.], [0., 1., 0.])?;
        let mut pen = d.pen.shape_frame(reflected.matrix, &d.pen.part)?;
        pen.mirrored = !d.pen.mirrored;
        pen
    } else {
        d.pen.clone()
    };
    let d = &mut Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    };
    let wood = shade(srgb("#7a5a3c"), room.stream("fittings")?.float(0.8, 1.15));
    for placement in &layout.shelves {
        let local = &mut placed(d, &placement.placement)?;
        let mut rng = room.stream(placement.placement.id)?;
        for shelf in furniture::shop_shelving(
            local,
            placement.family,
            placement.placement.size,
            placement.levels,
            wood,
            placement.above,
            room.kit,
        )? {
            let kinds: &[G] = if shelf.z > placement.placement.size[2] - 0.01 {
                &[G::Boxes, G::TallBoxes, G::Cartons]
            } else if shelf.z < 0.5 {
                &[G::Boxes, G::Boxes, G::TallBoxes, G::BigJars, G::DryBags]
            } else {
                &[
                    G::Boxes,
                    G::Boxes,
                    G::TallBoxes,
                    G::LargeTins,
                    G::Tins,
                    G::SmallJars,
                    G::Cartons,
                    G::SmallBottles,
                    G::Bottles,
                    G::DryBags,
                ]
            };
            stock(local, room, &shelf, kinds, &mut rng, 1)?;
        }
    }
    if let Some(p) = &layout.cooler {
        cooler(d, room, p)?;
    }
    counter(d, room, &layout.counter)?;
    crates(d, room, &layout.crates)?;
    sacks(d, room, &layout.sacks)?;
    prop(
        &mut placed(d, &layout.stool)?,
        room,
        "shop-stool-detailed",
        [0.2, 0.2, 0.],
        [0.31, 0.31, 0.45],
        room.stream("stool")?.float(-0.25, 0.25),
        0,
    )?;
    snack_string::row(
        d,
        [
            layout.hanging.x0,
            layout.hanging.x1,
            layout.hanging.y,
            room.height,
        ],
        &mut room.stream("hung")?,
        room.assets.snack_packets,
    )?;
    let bench = &mut placed(d, &layout.counter)?;
    let garlic = grocer_props::size("garlic-braid", [0.12, 0.1, 0.45])?;
    let at = [layout.counter.size[0] * 0.1, 0.25, room.height - 0.6];
    prop(bench, room, "garlic-braid", at, garlic, 0., 0)?;
    bench.bar(
        "paint",
        [at[0], at[1], at[2] + garlic[2]],
        [at[0], at[1], room.height],
        0.003,
        srgb("#b29b72"),
    )?;
    lights(d, room, &layout.counter)
}
pub fn interior(rng: &mut SeededRng) -> Interior {
    Interior {
        floor: shade(srgb("#8a8880"), rng.float(0.85, 1.05)),
        ceiling: srgb("#d8d4c4"),
        wall: srgb("#ece4cc"),
        dado: rng.pick(&[srgb("#5f9a80"), srgb("#6f9fc0"), srgb("#8fa870")]),
        dado_height: 1.2,
    }
}
