//! Street-level and facade-detail generators: open shop rooms with their stock, street
//! stock, pavements, cornices, conduits, balcony doors, rails and ladders. Conventions are
//! those of the parent module.
use super::{Arguments, draw, fixture, on, pen, room_pen, sign_settings};
use crate::itemspec::definition::{ItemMesh, NativeOutput, Values};
use crate::worldspec::themes::kowloon::{
    facade_plan,
    fixture::Fixture,
    kit::Kit,
    massing::Lot,
    molding::{Strip, profiled_strip},
    palette::{lot_style, shade, srgb},
    pen::Tint,
    pipes, shop,
};
use std::collections::BTreeMap;
use superspec_geometry::V3;
use superspec_types::rng::SeededRng;

/// `kowloon/shop-room`: an open shop behind its shopfront opening: floor, ceiling and walls,
/// the raised shutter's head box and threshold, and the trade's furniture and stock (shelf
/// banks packed with goods, counter, hung goods, crates and cartons, lights). Walls are in the
/// `interior` slot, the ceiling in `concrete`, the floor in `shop-floor`, the provisions shop's
/// lit tubes in `tube-light` and everything else, coloured per vertex, in `stock`; a host leaves
/// the last three unbound to show them plain. Anchor: bottom-centre of
/// the opening on the wall plane; the room lies behind the wall (+Y) and is as wide as the
/// opening. Parameters: `trade` (`provisions` | `grocer` | `greengrocer` | `hardware` |
/// `herbalist`, `provisions`), `width` (m, 1.8–40, 3), `height` (m from the floor to the
/// opening's head, 2–10, 2.7), `depth` (m, 2.4–20, 3.1), `floor` (m of threshold above the
/// anchor, 0.03), `storey` (m of the ground storey, at least 0.5 above the head, 3.35),
/// `shutter` (#758077), `seed`.
pub(super) fn shop_room(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let trade: shop::shell::Trade = a.choice(
        "trade",
        "provisions",
        &[
            "provisions",
            "grocer",
            "greengrocer",
            "hardware",
            "herbalist",
        ],
    )?;
    let width = a.number("width", 3.)?;
    let height = a.number("height", 2.7)?;
    let depth = a.number("depth", 3.1)?;
    let floor = a.bounded("floor", 0.03, 0.02..=1.)?;
    let storey = a.number("storey", 3.35)?;
    let shutter = a.color("shutter", "#758077")?;
    // `hanging`: goods hung on strings across the whole opening, under its head (true).
    let hanging = a.boolean("hanging", true)?;
    // `fade`: how much of their saturation the stock's colours have lost (0–1, 0.5).
    let fade = a.bounded("fade", 0.5, 0. ..=1.)?;
    let key = a.seed()?;
    a.finish()?;
    // Timber is tinted per vertex here: the shipped atlas is not bound to an item's slots.
    let kit = Kit::new(sign_settings(&key), 1)?;
    let room = facade_plan::Room {
        x0: -width / 2.,
        x1: width / 2.,
        floor,
        head: floor + height,
        depth,
        key: key.clone(),
    };
    let lot = Lot {
        index: 0,
        kind: "tenement",
        x0: -width / 2.,
        x1: width / 2.,
        stories: 1,
        ground: storey,
        story_height: storey,
        roof: storey,
        top: storey,
        front: 0.,
        back: depth,
        style: lot_style(&mut SeededRng::from_text(&format!("{key}/style"))),
        key: key.clone(),
    };
    let plain = shop::room::Assets {
        posters: false,
        grocer_props: false,
        snack_packets: false,
        market_apple: false,
        shop_register: false,
        market_trays: false,
    };
    let assets = shop::shell::Assets {
        room: plain,
        restaurant: shop::roast::Assets {
            food: false,
            metal: false,
            surfaces: false,
            tiles: false,
        },
    };
    let output = draw(cancelled, |mesh| {
        // The shell proposes a fascia sign; signs are items of their own, so it is not drawn.
        shop::shell::draw(
            &mut fixture(mesh, "shop-room")?,
            &shop::shell::Request {
                plan: &room,
                lot: &lot,
                coat: "interior",
                kit: &kit,
                assets,
                trade: Some(trade),
                shutter_color: Some(shutter),
                frontage: None,
            },
        )?;
        if hanging && width >= 1.6 {
            // Strings of packets hung across the opening, just inside its head, in the room's
            // frame (x along the front, y into the room).
            let inside = pen("shop-hanging")?
                .moved([-width / 2., 0., floor])?
                .facing([0.; 3], [1., 0., 0.], [0., -1., 0.])?;
            shop::provisions::hung(
                &mut on(mesh, inside)?,
                [0.2, width - 0.2, 0.16, height - 0.2],
                (height - 2.).clamp(0.3, 0.95),
                &mut kit.rng(&format!("{key}/front-goods"))?,
            )?;
        }
        Ok(())
    })?;
    let mut output = as_stock(output, &["interior", "concrete", "tube-light"], fade);
    floor_slot(&mut output, floor);
    Ok(output)
}

/// Moves the room's floor (the upward `concrete` faces at `level`) to a `shop-floor` slot of
/// its own: a material baked over the closed room's concrete darkens it, and a floor that
/// stays plain shows the stock standing on it.
fn floor_slot(output: &mut NativeOutput, level: f64) {
    let Some(at) = output
        .meshes
        .iter()
        .position(|m| m.material_slot == "concrete")
    else {
        return;
    };
    let source = output.meshes.remove(at);
    let empty = |slot: &str| ItemMesh {
        id: slot.into(),
        positions: vec![],
        normals: vec![],
        uvs: vec![],
        additional_uvs: source
            .additional_uvs
            .keys()
            .map(|set| (*set, vec![]))
            .collect(),
        colors: vec![],
        tangents: vec![],
        indices: vec![],
        material_slot: slot.into(),
    };
    let mut parts = [empty("concrete"), empty("shop-floor")];
    let mut remaps = [BTreeMap::new(), BTreeMap::new()];
    for corners in source.indices.chunks_exact(3) {
        let floor = corners.iter().all(|i| {
            let i = *i as usize;
            (source.positions[i][2] - level).abs() < 1e-6 && source.normals[i][2] > 0.5
        });
        let (part, remap) = (
            &mut parts[usize::from(floor)],
            &mut remaps[usize::from(floor)],
        );
        for index in corners {
            let next = part.positions.len() as u32;
            let target = *remap.entry(*index).or_insert_with(|| {
                let i = *index as usize;
                part.positions.push(source.positions[i]);
                part.normals.push(source.normals[i]);
                part.uvs.push(source.uvs[i]);
                part.colors.push(source.colors[i]);
                if let Some(tangent) = source.tangents.get(i) {
                    part.tangents.push(*tangent);
                }
                for (set, uvs) in &source.additional_uvs {
                    if let (Some(uv), Some(target)) = (uvs.get(i), part.additional_uvs.get_mut(set))
                    {
                        target.push(*uv);
                    }
                }
                next
            });
            part.indices.push(target);
        }
    }
    let [rest, floor] = parts;
    for (offset, part) in [rest, floor]
        .into_iter()
        .filter(|part| !part.indices.is_empty())
        .enumerate()
    {
        output.meshes.insert(at + offset, part);
    }
}

/// Moves every mesh but those of the `kept` slots to the `stock` slot: the goods, furniture
/// and fittings of a shop are thousands of small pieces coloured per vertex, which a host shows
/// as they are rather than baking a material over each. Their colours lose `fade` of their
/// saturation and take a little dust, as stock that has stood in the sun.
fn as_stock(mut output: NativeOutput, kept: &[&str], fade: f64) -> NativeOutput {
    for mesh in &mut output.meshes {
        if kept.contains(&mesh.material_slot.as_str()) {
            continue;
        }
        mesh.material_slot = "stock".into();
        for color in &mut mesh.colors {
            let grey = 0.2126 * color[0] + 0.7152 * color[1] + 0.0722 * color[2];
            let dust = [1., 0.985, 0.95];
            for k in 0..3 {
                let faded = grey + (color[k] - grey) * (1. - fade);
                color[k] = faded * (1. + (dust[k] - 1.) * fade) + 0.03 * fade * dust[k];
            }
        }
    }
    output
}

/// `kowloon/street-stock`: stock on the pavement in front of a wall, low-detail and coloured
/// per vertex (slot `stock`). `kind` `shop` (the default) is a shop's display: stacked plastic
/// crates and cartons either side of the way in, a sack truck by the left jamb, wire racks
/// packed with goods at the pavement edge on the right, buckets and brooms; `crates`,
/// `cartons` and `mixed` are a heap of those across the whole width. Anchor: bottom-centre of
/// the frontage on the wall plane; the stock stands within 1.1 m of the wall. Parameters:
/// `width` (m of frontage, 0.6–40, 3), `kind`, `fade` (of the colours' saturation, 0–1, 0.5),
/// `seed`.
pub(super) fn street_stock(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let width = a.number("width", 3.)?;
    let kind: shop::provisions::Street =
        a.choice("kind", "shop", &["shop", "crates", "cartons", "mixed"])?;
    let fade = a.bounded("fade", 0.5, 0. ..=1.)?;
    let key = a.seed()?;
    a.finish()?;
    let kit = Kit::new(sign_settings(&key), 1)?;
    let output = draw(cancelled, |mesh| {
        shop::provisions::street_fitted(
            &mut on(mesh, pen("street-stock")?.moved([-width / 2., 0., 0.])?)?,
            &kit,
            &key,
            width,
            kind,
        )
    })?;
    Ok(as_stock(output, &[], fade))
}

/// `kowloon/pavement`: a raised pavement along a wall: seeded paving stones on a bed, a kerb
/// of longer stones along the road and an optional iron drain cover. Anchor: the wall's foot
/// at its start; the pavement runs along +X from `-before` to `length + after`, reaches
/// `depth` in front of the wall (-Y), its top at z = 0 and the road `height` below. Slots:
/// `paving`, `kerb`, `metal`. Parameters: `length` (m, 0.3–100, 6), `depth` (m, 0.4–10, 1.45),
/// `height` (m of kerb above the road, 0.02–0.5, 0.15), `stone` (m, paving stone size,
/// 0.2–2, 0.55), `before`, `after` (m the pavement continues past the wall's ends, 0–10, 0),
/// `drain` (true), `color` (#a8a394), `seed` (stone shades).
pub(super) fn pavement(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let length = a.bounded("length", 6., 0.3..=100.)?;
    let depth = a.bounded("depth", 1.45, 0.4..=10.)?;
    let height = a.bounded("height", 0.15, 0.02..=0.5)?;
    let stone = a.bounded("stone", 0.55, 0.2..=2.)?;
    let before = a.bounded("before", 0., 0. ..=10.)?;
    let after = a.bounded("after", 0., 0. ..=10.)?;
    let drain = a.boolean("drain", true)?;
    // `manhole`: a cast iron cover in the paving (false).
    let manhole = a.boolean("manhole", false)?;
    let color = a.color("color", "#a8a394")?;
    let mut rng = a.rng()?;
    a.finish()?;
    let kerb = 0.14;
    let (x0, x1) = (-before, length + after);
    draw(cancelled, |mesh| {
        let d = &mut on(mesh, room_pen("pavement", [0.; 3])?)?;
        // The bed shows in the joints between the stones.
        d.cuboid(
            "paving",
            [x0, -depth + kerb, -height],
            [x1, 0., -0.035],
            shade(color, 0.55),
            "d",
        )?;
        let columns = ((x1 - x0) / stone).round().max(1.) as usize;
        let pitch = (x1 - x0) / columns as f64;
        let rows = ((depth - kerb) / stone).round().max(1.) as usize;
        let row = (depth - kerb) / rows as f64;
        let joint = 0.006;
        for r in 0..rows {
            // Alternate rows break joint by half a stone.
            let offset = if r % 2 == 1 { pitch / 2. } else { 0. };
            let mut left = x0;
            let mut c = 0;
            while left < x1 - 1e-6 {
                let right = if c == 0 && offset > 0. {
                    left + offset
                } else {
                    (left + pitch).min(x1)
                };
                let sink = rng.float(0., 0.006);
                d.cuboid(
                    "paving",
                    [
                        left + joint / 2.,
                        -((r + 1) as f64) * row + joint / 2.,
                        -0.04,
                    ],
                    [right - joint / 2., -(r as f64) * row - joint / 2., -sink],
                    shade(color, rng.float(0.7, 1.08)),
                    "d",
                )?;
                left = right;
                c += 1;
            }
        }
        let stones = ((x1 - x0) / (stone * 1.6)).round().max(1.) as usize;
        let long = (x1 - x0) / stones as f64;
        for i in 0..stones {
            let left = x0 + i as f64 * long;
            // A kerb stone with its road-side arris chamfered; its bed (edge 0) is not drawn.
            let (inner, top) = (-depth + kerb - joint / 2., -0.012);
            let arris = 0.02_f64.min((height + top) * 0.4);
            let repeat = d.repeat("kerb");
            profiled_strip(
                &d.pen,
                d.mesh,
                "kerb",
                &Strip {
                    span: [left + joint / 2., left + long - joint / 2.],
                    section: &[
                        [-depth, -height],
                        [inner, -height],
                        [inner, top],
                        [-depth + arris, top],
                        [-depth, top - arris],
                    ],
                    omitted_edges: &[0],
                    miters: [None; 2],
                    setback: 0.,
                },
                &Tint::Color(shade(color, rng.float(0.62, 0.92))),
                repeat,
            )?;
        }
        if manhole && length >= 2.4 && depth >= 1. {
            // A cast iron cover in its frame, ribbed, a third of the way along.
            let iron = srgb("#4a3b30");
            let (at, y) = (length * 0.3, -(depth - kerb) / 2.);
            d.cuboid(
                "metal",
                [at - 0.36, y - 0.3, 0.001],
                [at + 0.36, y + 0.3, 0.006],
                shade(iron, 0.7),
                "d",
            )?;
            for bar in 0..6 {
                let x = at - 0.3 + bar as f64 * 0.11;
                d.cuboid(
                    "metal",
                    [x, y - 0.25, 0.006],
                    [x + 0.06, y + 0.25, 0.013],
                    iron,
                    "d",
                )?;
            }
        }
        if drain && length >= 1.2 && depth >= 0.9 {
            let iron = srgb("#343632");
            let at = length - 0.75;
            let (y0, y1) = (-depth + kerb + 0.08, -depth + kerb + 0.6);
            d.cuboid("metal", [at, y0, 0.001], [at + 0.4, y1, 0.008], iron, "d")?;
            for bar in 0..7 {
                let y = y0 + 0.05 + bar as f64 * 0.066;
                d.cuboid(
                    "metal",
                    [at + 0.015, y, 0.008],
                    [at + 0.385, y + 0.026, 0.02],
                    shade(iron, 1.25),
                    "d",
                )?;
            }
        }
        Ok(())
    })
}

/// `kowloon/tile-deck`: a flat roof's paving of square tiles laid in patches of two colours
/// (old terracotta and the grey of later repairs), with a few lifted or missing. Anchor: the
/// centre of the deck at roof level; it spans `width` by `depth` and stands `thickness` high.
/// Slot: `roof-tile` (tinted per vertex). Parameters: `width` (m, 0.5–60, 5), `depth` (m,
/// 0.5–60, 5), `tile` (m, 0.15–1, 0.3), `thickness` (m, 0.01–0.2, 0.03), `color` (#a4553f),
/// `repair` (#8c8b84), `share` (of the repair colour, 0–1, 0.4), `seed`.
pub(super) fn tile_deck(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let width = a.bounded("width", 5., 0.5..=60.)?;
    let depth = a.bounded("depth", 5., 0.5..=60.)?;
    let tile = a.bounded("tile", 0.3, 0.15..=1.)?;
    let thickness = a.bounded("thickness", 0.03, 0.01..=0.2)?;
    let color = a.color("color", "#a4553f")?;
    let repair = a.color("repair", "#8c8b84")?;
    let share = a.bounded("share", 0.4, 0. ..=1.)?;
    let key = a.seed()?;
    a.finish()?;
    let columns = (width / tile).round().max(1.) as usize;
    let rows = (depth / tile).round().max(1.) as usize;
    let (dx, dy) = (width / columns as f64, depth / rows as f64);
    // Patches: a coarse seeded grid decides each block's colour, so repairs come in areas.
    let block = 4;
    let patch = |c: usize, r: usize| {
        SeededRng::from_text(&format!("{key}/patch/{}/{}", c / block, r / block)).next_f64() < share
    };
    let mut rng = SeededRng::from_text(&key);
    draw(cancelled, |mesh| {
        let d = &mut on(mesh, room_pen("tile-deck", [-width / 2., -depth / 2., 0.])?)?;
        d.cuboid(
            "roof-tile",
            [0., 0., 0.],
            [width, depth, thickness * 0.6],
            shade(repair, 0.5),
            "d",
        )?;
        for r in 0..rows {
            for c in 0..columns {
                let roll = rng.next_f64();
                if roll < 0.015 {
                    continue;
                }
                let base = if patch(c, r) != (roll > 0.95) {
                    repair
                } else {
                    color
                };
                let lift = if roll > 0.985 { 0.008 } else { 0. };
                d.cuboid(
                    "roof-tile",
                    [
                        c as f64 * dx + 0.004,
                        r as f64 * dy + 0.004,
                        thickness * 0.6,
                    ],
                    [
                        (c + 1) as f64 * dx - 0.004,
                        (r + 1) as f64 * dy - 0.004,
                        thickness + lift,
                    ],
                    shade(base, rng.float(0.72, 1.08)),
                    "d",
                )?;
            }
        }
        Ok(())
    })
}

/// `kowloon/cornice`: a moulded concrete string course along a wall, mitred at its ends.
/// Anchor: the wall plane at the course's start, at the level it marks (a floor line or a
/// parapet top); it runs along +X. Parameters: `length` (m, 0.05–100, 6), `profile` (`ledge`:
/// a thin splayed string course 0.11 m proud, from 0.06 below to 0.06 above the level; `coping`: a flat
/// cap 0.08 thick over a wall `thickness` deep, 0.08 proud; `ledge`), `thickness` (m, of the
/// coped wall, 0.05–1, 0.25), `miterStart`, `miterEnd` (the tangent of half the plan turn at
/// each end: 1 at a square corner, 0.414 at a 45 degree one, 0 for a square end; 0),
/// `color` (#ddd2bc), `bevel` (m of rounding at the section's corners, 0.012).
pub(super) fn cornice(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let length = a.bounded("length", 6., 0.05..=100.)?;
    let profile = a.text("profile", "ledge")?;
    if !["ledge", "coping"].contains(&profile.as_str()) {
        return Err(format!("{name}: profile must be one of ledge, coping."));
    }
    let thickness = a.bounded("thickness", 0.25, 0.05..=1.)?;
    let start = a.bounded("miterStart", 0., -4. ..=4.)?;
    let end = a.bounded("miterEnd", 0., -4. ..=4.)?;
    let color = a.color("color", "#ddd2bc")?;
    // `bevel`: m each corner of the section (its nose above all) is rounded back, in two chords
    // with turning normals (0–0.03, 0.012; at most a quarter of either neighbouring face).
    let bevel = a.bounded("bevel", 0.012, 0. ..=0.03)?;
    a.finish()?;
    let ledge = [
        [-0.01, -0.06],
        [0.07, -0.06],
        [0.11, -0.02],
        [0.11, 0.035],
        [0.02, 0.06],
    ];
    let coping = [
        [-thickness - 0.02, 0.],
        [0.08, 0.],
        [0.08, 0.08],
        [-thickness - 0.02, 0.08],
    ];
    let section: &[[f64; 2]] = if profile == "ledge" { &ledge } else { &coping };
    draw(cancelled, |mesh| {
        profiled_strip(
            &pen("cornice")?,
            mesh,
            "concrete",
            &Strip {
                span: [0., length],
                section,
                omitted_edges: &[],
                miters: [(start != 0.).then_some(-start), (end != 0.).then_some(end)],
                setback: bevel,
            },
            &Tint::Color(color),
            1.,
        )
        .map(|_| ())
    })
}

/// `kowloon/conduit`: a surface electrical conduit clipped to the wall: up `rise`, along
/// `run`, then up `rise2`, with a junction box at each bend and at its foot. Anchor: the foot
/// of the first rise on the wall plane. Parameters: `rise` (m, 0–40, 2.8), `run` (m along X,
/// negative runs left, 2.7), `rise2` (m, 0–40, 0), `radius` (m, 0.005–0.05, 0.012), `color`
/// (#4d5048), `boxes` (true), `endBox` (m; the width of a pale lidded junction box at the
/// conduit's far end, 0–1, 0: none).
pub(super) fn conduit(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let rise = a.bounded("rise", 2.8, 0. ..=40.)?;
    let run = a.bounded("run", 2.7, -40. ..=40.)?;
    let rise2 = a.bounded("rise2", 0., 0. ..=40.)?;
    let radius = a.bounded("radius", 0.012, 0.005..=0.05)?;
    let color = a.color("color", "#4d5048")?;
    let boxes = a.boolean("boxes", true)?;
    let end_box = a.bounded("endBox", 0., 0. ..=1.)?;
    a.finish()?;
    let y = radius + 0.028;
    let mut points: Vec<V3> = vec![[0., y, 0.]];
    for step in [[0., rise], [run, 0.], [0., rise2]] {
        if step[0].abs() + step[1].abs() > 0.05 {
            let last = *points.last().unwrap();
            points.push([last[0] + step[0], y, last[2] + step[1]]);
        }
    }
    if points.len() < 2 {
        return Err(format!("{name}: the conduit has no length."));
    }
    let system = pipes::PipeSystem::new(pipes::Options {
        radius,
        color,
        sides: 10,
        variant: "electrical".into(),
        weathering: None,
    })?;
    draw(cancelled, |mesh| {
        let d = &mut fixture(mesh, "conduit")?;
        system.run(d, &points, Some(0.))?;
        if boxes {
            for (index, p) in points.iter().enumerate().take(points.len() - 1) {
                let half = if index == 0 { 0.05 } else { 0.042 };
                d.cuboid(
                    "metal",
                    [p[0] - half, 0., p[2] - half * 1.25],
                    [p[0] + half, y + radius + 0.012, p[2] + half * 1.25],
                    shade(color, 1.15),
                    "b",
                )?;
                d.cuboid(
                    "metal",
                    [p[0] - half * 0.8, y + radius + 0.012, p[2] - half],
                    [p[0] + half * 0.8, y + radius + 0.018, p[2] + half],
                    shade(color, 0.9),
                    "b",
                )?;
            }
        }
        if end_box >= 0.05 {
            let p = points[points.len() - 1];
            let (half, body) = (end_box / 2., srgb("#b9b6a8"));
            d.cuboid(
                "metal",
                [p[0] - half, 0., p[2]],
                [p[0] + half, 0.11, p[2] + end_box * 1.3],
                body,
                "b",
            )?;
            d.cuboid(
                "metal",
                [p[0] - half - 0.008, 0.11, p[2] - 0.008],
                [p[0] + half + 0.008, 0.122, p[2] + end_box * 1.3 + 0.008],
                shade(body, 0.9),
                "b",
            )?;
        }
        Ok(())
    })
}

/// `kowloon/balcony-door`: a steel-framed balcony door in its opening: a solid lower panel,
/// a screened glazed upper light under a transom light, lever handle, hinges and a concrete
/// threshold. Anchor: bottom-centre of the opening on the wall plane; the leaf stands in the
/// recess behind it. Parameters: `width` (m, 0.6–2, 0.97), `height` (m, 1.8–3.2, 2.69),
/// `frame` (#3f554b), `trim` (#ddd2bc).
pub(super) fn balcony_door(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let width = a.bounded("width", 0.97, 0.6..=2.)?;
    let height = a.bounded("height", 2.69, 1.8..=3.2)?;
    let green = a.color("frame", "#3f554b")?;
    let trim = a.color("trim", "#ddd2bc")?;
    a.finish()?;
    let (left, right, bottom, top) = (-width / 2., width / 2., 0., height);
    draw(cancelled, |mesh| {
        let d = &mut fixture(mesh, "balcony-door")?;
        d.panel("pane", [left, right, bottom, top], -0.237, srgb("#252c29"))?;
        for x in [left, right - 0.065] {
            d.frame_box([x, -0.15, bottom], [x + 0.065, -0.055, top], green)?;
        }
        for z in [bottom, bottom + 0.69, top - 0.36, top - 0.065] {
            d.frame_box(
                [left + 0.065, -0.15, z],
                [right - 0.065, -0.055, z + 0.065],
                green,
            )?;
        }
        d.cuboid(
            "paint",
            [left + 0.065, -0.14, bottom + 0.065],
            [right - 0.065, -0.09, bottom + 0.69],
            green,
            "",
        )?;
        let [x0, x1, z0, z1] = [left + 0.07, right - 0.07, bottom + 0.76, top - 0.37];
        let paint = srgb("#70756a");
        for column in 0..=((x1 - x0) / 0.095) as usize {
            let x = x0 + column as f64 * 0.095;
            d.tube(
                "metal",
                &[[x, -0.105, z0], [x, -0.105, z1]],
                0.0025,
                paint,
                4,
            )?;
        }
        for row in 0..=((z1 - z0) / 0.1) as usize {
            let z = z0 + row as f64 * 0.1;
            d.tube(
                "metal",
                &[[x0, -0.105, z], [x1, -0.105, z]],
                0.0025,
                paint,
                4,
            )?;
        }
        let x = right - 0.14;
        d.tube(
            "metal",
            &[
                [x, -0.04, bottom + 1.05],
                [x, -0.015, bottom + 1.05],
                [x, -0.015, bottom + 1.24],
            ],
            0.012,
            srgb("#dfcfaa"),
            8,
        )?;
        for z in [bottom + 0.35, top - 0.45] {
            d.cuboid(
                "metal",
                [left + 0.025, -0.06, z],
                [left + 0.085, -0.035, z + 0.12],
                green,
                "",
            )?;
        }
        d.cuboid(
            "concrete",
            [left - 0.03, -0.24, bottom - 0.03],
            [right + 0.03, 0.13, bottom + 0.025],
            trim,
            "",
        )
    })
}

/// `kowloon/fan-divider`: a quarter fan of flat steel bars between two balconies, standing
/// square to the wall: bars radiate from a hinge at the wall's foot to a solid curved rim.
/// Anchor: the hinge, on the wall plane; the fan reaches `reach` out and `height` up.
/// Parameters: `reach` (m, 0.3–3, 1), `height` (m, 0.3–3, 1.6), `bars` (3–40, 14), `color`
/// (#3f554b).
pub(super) fn fan_divider(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let reach = a.bounded("reach", 1., 0.3..=3.)?;
    let height = a.bounded("height", 1.6, 0.3..=3.)?;
    let bars = a.integer("bars", 14, 3..=40)?;
    let color = a.color("color", "#3f554b")?;
    a.finish()?;
    draw(cancelled, |mesh| {
        let d = &mut fixture(mesh, "fan-divider")?;
        let at = |angle: f64, r: f64| {
            [
                0.,
                0.03 + r * reach * angle.cos(),
                0.03 + r * height * angle.sin(),
            ]
        };
        let quarter = std::f64::consts::FRAC_PI_2;
        for i in 0..bars {
            let angle = quarter * i as f64 / (bars - 1) as f64;
            d.tube("metal", &[at(angle, 0.05), at(angle, 1.)], 0.011, color, 6)?;
        }
        let rim: Vec<V3> = (0..=20).map(|i| at(quarter * i as f64 / 20., 1.)).collect();
        d.tube("metal", &rim, 0.022, color, 8)?;
        let inner: Vec<V3> = (0..=10)
            .map(|i| at(quarter * i as f64 / 10., 0.55))
            .collect();
        d.tube("metal", &inner, 0.012, color, 6)?;
        d.cuboid(
            "metal",
            [-0.03, 0., 0.],
            [0.03, 0.09, 0.09],
            shade(color, 0.8),
            "b",
        )
    })
}

/// `kowloon/rail`: a tubular steel guard rail on posts with base plates. Anchor: the foot of
/// its first post; it runs along +X. Parameters: `length` (m, 0.3–100, 4), `height` (m,
/// 0.2–2, 0.55), `rails` (1–4, 2), `spacing` (m between posts at most, 0.4–4, 1.3), `color`
/// (#3f554b).
pub(super) fn rail(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let length = a.bounded("length", 4., 0.3..=100.)?;
    let height = a.bounded("height", 0.55, 0.2..=2.)?;
    let rails = a.integer("rails", 2, 1..=4)?;
    let spacing = a.bounded("spacing", 1.3, 0.4..=4.)?;
    let color = a.color("color", "#3f554b")?;
    a.finish()?;
    draw(cancelled, |mesh| {
        let d = &mut on(mesh, room_pen("rail", [0.; 3])?)?;
        let posts = (length / spacing).ceil().max(1.) as usize;
        for i in 0..=posts {
            let x = length * i as f64 / posts as f64;
            d.tube("metal", &[[x, 0., 0.], [x, 0., height]], 0.019, color, 8)?;
            d.cuboid(
                "metal",
                [x - 0.045, -0.045, 0.],
                [x + 0.045, 0.045, 0.008],
                shade(color, 0.8),
                "d",
            )?;
        }
        for k in 0..rails {
            let z = height * (1. - k as f64 / rails as f64);
            d.tube("metal", &[[0., 0., z], [length, 0., z]], 0.016, color, 8)?;
        }
        Ok(())
    })
}

/// `kowloon/ladder`: a steel cat ladder fixed to a wall, its stiles turning over the top onto
/// the roof behind. Anchor: the wall plane below the ladder's centre, at its foot; it stands
/// 0.16 m in front of the wall. Parameters: `height` (m, 0.6–20, 2.2), `width` (m, 0.25–1,
/// 0.37), `color` (#3f554b).
pub(super) fn ladder(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let height = a.bounded("height", 2.2, 0.6..=20.)?;
    let width = a.bounded("width", 0.37, 0.25..=1.)?;
    let color = a.color("color", "#3f554b")?;
    a.finish()?;
    draw(cancelled, |mesh| {
        let d = &mut fixture(mesh, "ladder")?;
        let y = 0.16;
        for x in [-width / 2., width / 2.] {
            d.tube(
                "metal",
                &[
                    [x, y, 0.05],
                    [x, y, height - 0.1],
                    [x, -0.12, height + 0.02],
                ],
                0.022,
                color,
                10,
            )?;
            for z in [0.5, height - 0.5] {
                d.tube("metal", &[[x, y, z], [x, 0., z]], 0.012, color, 6)?;
            }
        }
        let rungs = ((height - 0.4) / 0.3) as usize;
        for i in 0..=rungs {
            let z = 0.28 + i as f64 * 0.3;
            if z < height - 0.08 {
                d.tube(
                    "metal",
                    &[[-width / 2., y, z], [width / 2., y, z]],
                    0.015,
                    color,
                    8,
                )?;
            }
        }
        Ok(())
    })
}

/// Triangular concrete brackets under a cantilevered slab whose top is z = 0: one every
/// `spacing` at most across `width`, `depth` long and 0.29 m deep at the wall.
pub(super) fn brackets(
    d: &mut Fixture<'_, '_>,
    width: f64,
    depth: f64,
    thickness: f64,
    spacing: f64,
    color: V3,
) -> Result<(), String> {
    let count = (width / spacing).ceil().max(1.) as usize + 1;
    let inset = 0.45_f64.min(width / 4.);
    for i in 0..count {
        let x = -width / 2. + inset + (width - 2. * inset) * i as f64 / (count - 1).max(1) as f64;
        let reach = (depth - 0.11).max(0.2);
        let a = [
            [x - 0.11, 0.02, -thickness - 0.29],
            [x - 0.11, reach, -thickness],
            [x - 0.11, 0.02, -thickness],
        ];
        let b = a.map(|[x, y, z]| [x + 0.22, y, z]);
        d.face("concrete", &a, [-1., 0., 0.], color)?;
        d.face("concrete", &b, [1., 0., 0.], color)?;
        d.face(
            "concrete",
            &[a[0], a[1], b[1], b[0]],
            [0., 0.29, -1.],
            color,
        )?;
    }
    Ok(())
}

/// Steel poles from an awning's front edge down to the ground: `count` of them across
/// `[x0, x1]`, at `depth` from the wall, from `top` down to `ground`.
pub(super) fn poles(
    d: &mut Fixture<'_, '_>,
    [x0, x1]: [f64; 2],
    depth: f64,
    top: f64,
    ground: f64,
    count: usize,
) -> Result<(), String> {
    let steel = srgb("#6d6f66");
    for i in 0..count {
        let x = if count == 1 {
            (x0 + x1) / 2.
        } else {
            x0 + 0.06 + (x1 - x0 - 0.12) * i as f64 / (count - 1) as f64
        };
        d.tube(
            "metal",
            &[[x, depth - 0.04, top], [x, depth - 0.04, ground]],
            0.019,
            steel,
            8,
        )?;
        d.cuboid(
            "metal",
            [x - 0.06, depth - 0.1, ground],
            [x + 0.06, depth + 0.02, ground + 0.012],
            shade(steel, 0.8),
            "d",
        )?;
    }
    Ok(())
}
