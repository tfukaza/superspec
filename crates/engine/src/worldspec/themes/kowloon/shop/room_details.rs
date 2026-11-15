//! Wall-adjacent electrical services and localized interior wear.
use super::{
    super::{
        fixture::Fixture,
        palette::{shade, srgb},
        posters,
    },
    room::Room,
};
use superspec_types::rng::SeededRng;

pub(super) fn services(d: &mut Fixture<'_, '_>, room: &Room<'_>) -> Result<(), String> {
    let (width, depth, height) = (room.width, room.depth, room.height);
    if height < 2.3 {
        return Ok(());
    }
    let pen = d.pen.into_part(&format!("{}/services", d.pen.part))?;
    let mut d = Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    };
    let conduit = srgb("#5c6866");
    let mut rng = room.stream("services")?;
    let box_color = srgb(rng.pick(&["#727d78", "#657677", "#85877c"]));
    for line in 0..3 {
        let inset = 0.022 + line as f64 * 0.018;
        let z = height - 0.035 - line as f64 * 0.019;
        d.tube(
            "metal",
            &[
                [inset, 0.12, z],
                [inset, depth - inset, z],
                [width - inset, depth - inset, z],
                [width - inset, 0.12, z],
            ],
            0.0055,
            conduit,
            6,
        )?;
    }
    let mut y = 0.35;
    while y < depth - 0.15 {
        for x in [0.015, width - 0.065] {
            d.cuboid(
                "metal",
                [x, y - 0.012, height - 0.092],
                [x + 0.05, y + 0.012, height - 0.022],
                srgb("#848b80"),
                "",
            )?;
        }
        y += 0.9;
    }
    for x in [0.008, width - 0.068] {
        d.cuboid(
            "metal",
            [x, depth - 0.29, height - 0.115],
            [x + 0.06, depth - 0.11, height - 0.012],
            box_color,
            "",
        )?;
        let face = if x < width / 2. { x + 0.062 } else { x - 0.002 };
        d.bar(
            "paint",
            [face, depth - 0.24, height - 0.075],
            [face, depth - 0.16, height - 0.075],
            0.005,
            srgb("#28322e"),
        )?;
    }
    Ok(())
}

fn deposits(
    d: &mut Fixture<'_, '_>,
    width: f64,
    height: f64,
    rng: &mut SeededRng,
) -> Result<(), String> {
    for index in 0..(width * 3.5).ceil() as usize {
        let x = rng.float(0.06, width - 0.06);
        let top = if index % 3 == 0 {
            height - rng.float(0.10, 0.23)
        } else {
            rng.float(0.72, 1.42_f64.min(height - 0.25))
        };
        let width = rng.float(0.007, 0.017);
        let length = (top - 0.07).min(rng.float(0.11, 0.40));
        let edges: [_; 7] = std::array::from_fn(|index| {
            let down = index as f64 / 6.;
            let center = x + rng.float(-width * 0.5, width * 0.5);
            let half =
                width * (0.08 + (down * std::f64::consts::PI).sin() * 0.72) * rng.float(0.24, 0.48);
            [
                [center - half, 0., top - down * length],
                [center + half, 0., top - down * length],
            ]
        });
        let tint = shade(srgb("#a5977e"), rng.float(0.88, 1.04));
        for (index, edge) in edges.windows(2).enumerate() {
            if !rng.chance(if index == 0 { 0.38 } else { 0.28 }) {
                d.face(
                    "paint",
                    &[edge[0][0], edge[0][1], edge[1][1], edge[1][0]],
                    [0., 1., 0.],
                    tint,
                )?;
            }
        }
    }
    for _ in 0..(width * 9.).ceil() as usize {
        let x = rng.float(0.04, width - 0.05);
        let z = rng.float(0.035, 0.18);
        let size = rng.float(0.006, 0.020);
        d.face(
            "paint",
            &[
                [x, 0., z],
                [x + size, 0., z + size * 0.25],
                [x + size * 0.65, 0., z + size],
                [x - size * 0.2, 0., z + size * 0.6],
            ],
            [0., 1., 0.],
            shade(srgb("#7c6c51"), rng.float(0.65, 0.95)),
        )?;
    }
    Ok(())
}

pub(super) fn walls(d: &mut Fixture<'_, '_>, room: &Room<'_>, tiled: bool) -> Result<(), String> {
    let out = if tiled { 0.006 } else { 0.026 };
    let root = d.pen.into_part(&format!("{}/room-detail", d.pen.part))?;
    let left = root.facing([out, 0., 0.], [0., 1., 0.], [1., 0., 0.])?;
    let right = root.facing(
        [room.width - out, room.depth, 0.],
        [0., -1., 0.],
        [-1., 0., 0.],
    )?;
    for (side, pen) in [("left", &left), ("right", &right)] {
        deposits(
            &mut Fixture {
                pen: pen.clone(),
                mesh: d.mesh,
                repeats: d.repeats,
            },
            room.depth,
            room.height,
            &mut room.stream(&format!("room-detail/{side}"))?,
        )?;
    }
    if room.height >= 2.15 && room.assets.posters {
        let top = 1.77_f64.min(room.height - 0.30);
        for (pen, design, x, z, width) in [
            (&left, "hiring", 0.96, top, 0.21),
            (&left, "tea-ad", 1.28, top - 0.16, 0.25),
            (&right, "no-smoking", room.depth - 0.36, top - 0.04, 0.20),
        ] {
            let pen = pen.moved([x, 0., z])?;
            let seed = format!("{}/notice/{design}", room.key);
            let mut rng = SeededRng::from_text(&seed);
            posters::draw(
                &mut Fixture {
                    pen,
                    mesh: d.mesh,
                    repeats: d.repeats,
                },
                &posters::Options {
                    design: design.into(),
                    width,
                    rotation: rng.float(-5., 5.),
                    layer: 0,
                    wear: 0.8,
                    seed: Some(seed),
                },
                true,
            )?;
        }
    }
    Ok(())
}
