//! Street-level and facade-detail generators: open shop rooms with their stock, street
//! stock, pavements, cornices, conduits, balcony doors, rails and ladders. Conventions are
//! those of the parent module.
use super::{Arguments, draw, fixture, on, pen, room_pen, sign_settings};
use crate::itemspec::definition::{Feature, ItemMesh, NativeOutput, Reach, Values};
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

/// The stone tint pavements and avenues are shaded from: a near-neutral mid grey, so the
/// paving material bound over them decides their colour and the tints only vary it.
const PAVING: &str = "#8f8e8a";
/// The joint between paving and kerb stones, and how far a paving stone's top edge is
/// chamfered into it: together a fine band a step darker than the stone, a light drawn line
/// rather than a dark one.
const JOINT: f64 = 0.007;
const BEVEL: f64 = 0.0035;
/// How dark a stone's chamfer and sides, and the bed in the joints, are of its own tint.
const JOINT_SHADE: f64 = 0.78;

/// A paving stone over `[x0, y0, x1, y1]` (the joint already taken off), its top at `top`
/// and its foot in the bed at z = -0.04: the top face in `tint`, and a chamfer round it over
/// upright sides, both dark.
fn paving_stone(
    d: &mut Fixture<'_, '_>,
    [x0, y0, x1, y1]: [f64; 4],
    top: f64,
    tint: V3,
) -> Result<(), String> {
    let b = BEVEL.min((x1 - x0) / 4.).min((y1 - y0) / 4.);
    let dark = shade(tint, JOINT_SHADE);
    let outer = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
    let inner = [
        [x0 + b, y0 + b],
        [x1 - b, y0 + b],
        [x1 - b, y1 - b],
        [x0 + b, y1 - b],
    ];
    d.face(
        "paving",
        &inner.map(|[x, y]| [x, y, top]),
        [0., 0., 1.],
        tint,
    )?;
    let outward = [[0., -1.], [1., 0.], [0., 1.], [-1., 0.]];
    for k in 0..4 {
        let (n, [ox, oy]) = ((k + 1) % 4, outward[k]);
        d.face(
            "paving",
            &[
                [outer[k][0], outer[k][1], top - b],
                [outer[n][0], outer[n][1], top - b],
                [inner[n][0], inner[n][1], top],
                [inner[k][0], inner[k][1], top],
            ],
            [ox, oy, 1.],
            dark,
        )?;
        d.face(
            "paving",
            &[
                [outer[k][0], outer[k][1], -0.04],
                [outer[n][0], outer[n][1], -0.04],
                [outer[n][0], outer[n][1], top - b],
                [outer[k][0], outer[k][1], top - b],
            ],
            [ox, oy, 0.],
            dark,
        )?;
    }
    Ok(())
}

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
/// `shutter` (#758077), `hanging` (goods hung across the opening, true), `hangClear` and
/// `hangDrop` (m below the head where they start and end, so they hang under an awning's
/// edge), `hangStyle` and `hangSides` (their colours and how much lower they reach at the
/// opening's sides), `fade`, `seed`.
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
    // `hangClear`: m below the head that only the cords cross, so the goods hang beneath an
    // awning's edge (0–3, 0). `hangDrop`: m below the head the longest reach (0.4–4; by
    // default to 1.05 m above the floor at most, 0.8 m below the clear length). Over the way
    // in at the middle of an opening 3 m wide or more they keep 2 m of headroom.
    let hang_clear = a.bounded("hangClear", 0., 0. ..=3.)?;
    let hang_drop = a.optional_number("hangDrop")?;
    if hang_drop.is_some_and(|drop| !(0.4..=4.).contains(&drop)) {
        return Err(format!("{name}: hangDrop must lie in 0.4..4."));
    }
    // `hangStyle`: `bold` (strong packets, dyed cloths and baskets, few pale bags, the band 1.2 m
    // deep under the clear length, down to 0.85 m above the floor, by default) or `plain` (the
    // faded mix; `bold`). `hangSides`: m further down the long pieces at either end of the
    // opening reach, outside the way in (0–1.5; 0.35 for bold goods, 0 for plain), at least
    // 0.3 m above the floor.
    let bold = match a.text("hangStyle", "bold")?.as_str() {
        "bold" => true,
        "plain" => false,
        _ => return Err(format!("{name}: hangStyle must be one of bold, plain.")),
    };
    let hang_sides = a.optional_number("hangSides")?;
    if hang_sides.is_some_and(|sides| !(0. ..=1.5).contains(&sides)) {
        return Err(format!("{name}: hangSides must lie in 0..1.5."));
    }
    let hang_sides = hang_sides.unwrap_or(if bold { 0.35 } else { 0. });
    // `fade`: how much of their saturation the stock's colours have lost (0–1, 0.5).
    let fade = a.bounded("fade", 0.5, 0. ..=1.)?;
    let atlas = a.boolean("catalogStock", false)?;
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
    let shelf_assets = shop::room::Assets {
        posters: atlas,
        grocer_props: atlas,
        snack_packets: atlas,
        market_apple: atlas,
        shop_register: atlas,
        market_trays: atlas,
    };
    let assets = shop::shell::Assets {
        room: shelf_assets,
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
            // The line is 0.2 m under the head, behind the raised shutter.
            let shortest = (height - 2.).clamp(0.3, 0.95);
            let clear = (hang_clear - 0.2).max(0.);
            let drop = hang_drop
                .map_or(
                    if bold {
                        (clear + 1.2).min(height - 1.05)
                    } else {
                        (clear + 0.8).min(height - 1.25)
                    },
                    |drop| drop - 0.2,
                )
                .max(shortest);
            // The long pieces at the sides stop 0.3 m above the floor.
            let sides = hang_sides.min((height - 0.5 - drop).max(0.));
            // Over the way in nothing comes lower than 2 m above the floor.
            let headroom = (height - 2.2).max(0.);
            // Three rows, overlapping: a near-continuous wall of goods under the head.
            shop::provisions::hung(
                &mut on(mesh, inside)?,
                &shop::provisions::Hang {
                    line: [0.2, width - 0.2, 0.16, height - 0.2],
                    clear: clear.min(drop),
                    drop,
                    way: (width >= 3.).then_some([width / 2. - 0.6, width / 2. + 0.6, headroom]),
                    rows: 3,
                    bold,
                    sides,
                },
                &mut kit.rng(&format!("{key}/front-goods"))?,
            )?;
        }
        Ok(())
    })?;
    let mut output = as_stock(
        output,
        &[
            "interior",
            "concrete",
            "tube-light",
            "grocer-props",
            "snack-packet",
            "market-apple",
            "market-trays",
            "shop-register",
            "posters",
        ],
        fade,
    );
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
/// crates and cartons either side of the way in behind a front row of crates, a sack truck by
/// the left jamb, wire racks packed with goods at the pavement edge on the right, and buckets,
/// a stool and brooms by the way in; `crates`,
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
    let atlas = a.boolean("catalogStock", false)?;
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
            atlas,
        )
    })?;
    Ok(as_stock(output, &["grocer-props"], fade))
}

/// `kowloon/pavement`: a raised pavement along a wall: seeded paving stones on a bed, a kerb
/// of longer stones along the road and an optional iron drain cover. Anchor: the wall's foot
/// at its start; the pavement runs along +X from `-before` to `length + after`, reaches
/// `depth` in front of the wall (-Y), its top at z = 0 and the road `height` below. Slots:
/// `paving`, `kerb`, `metal`. Parameters: `length` (m, 0.3–100, 6), `depth` (m, 0.4–10, 1.45),
/// `height` (m of kerb above the road, 0.02–0.5, 0.15), `stone` (m, paving stone size,
/// 0.2–2, 0.55), `before`, `after` (m the pavement continues past the wall's ends, 0–10, 0),
/// `drain` (true), `color` (#8f8e8a), `seed` (stone shades).
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
    let color = a.color("color", PAVING)?;
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
            shade(color, JOINT_SHADE),
            "d",
        )?;
        let columns = ((x1 - x0) / stone).round().max(1.) as usize;
        let pitch = (x1 - x0) / columns as f64;
        let rows = ((depth - kerb) / stone).round().max(1.) as usize;
        let row = (depth - kerb) / rows as f64;
        let joint = JOINT;
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
                let tint = shade(color, rng.float(0.7, 1.08));
                if right - left >= 3. * joint {
                    paving_stone(
                        d,
                        [
                            left + joint / 2.,
                            -((r + 1) as f64) * row + joint / 2.,
                            right - joint / 2.,
                            -(r as f64) * row - joint / 2.,
                        ],
                        -sink,
                        tint,
                    )?;
                }
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
            for bar in 0..4 {
                let x = at - 0.295 + bar as f64 * 0.173;
                d.cuboid(
                    "metal",
                    [x, y - 0.25, 0.006],
                    [x + 0.07, y + 0.25, 0.018],
                    shade(iron, 1.2),
                    "d",
                )?;
            }
        }
        if drain && length >= 1.2 && depth >= 0.9 {
            let iron = srgb("#343632");
            let at = length - 0.75;
            let (y0, y1) = (-depth + kerb + 0.08, -depth + kerb + 0.6);
            // A cover of four heavy bars in its frame.
            d.cuboid("metal", [at, y0, 0.001], [at + 0.4, y1, 0.008], iron, "d")?;
            for bar in 0..4 {
                let y = y0 + 0.0535 + bar as f64 * 0.123;
                d.cuboid(
                    "metal",
                    [at + 0.03, y, 0.008],
                    [at + 0.37, y + 0.045, 0.02],
                    shade(iron, 1.3),
                    "d",
                )?;
            }
        }
        Ok(())
    })
}

/// A profile of `[reach, z]` points swept about the upright axis at (0, `radius`) from one
/// angle to another in `pieces` chords: reach is the distance from the axis, the angle is
/// measured from -Y toward +X, and the profile's visible side is on the right of the way it
/// runs (an outer face is listed from its foot to its head). Faces that stand turn their
/// normals with the sweep; `caps` closes both ends of a convex profile.
#[allow(clippy::too_many_arguments)]
fn swept(
    d: &mut Fixture<'_, '_>,
    slot: &str,
    radius: f64,
    profile: &[[f64; 2]],
    angles: [f64; 2],
    pieces: usize,
    caps: bool,
    tint: V3,
) -> Result<(), String> {
    swept_tinted(d, slot, radius, profile, angles, pieces, caps, &[tint])
}

/// [`swept`] with a tint for each edge of the profile in turn (the list repeating); the caps
/// take the first.
#[allow(clippy::too_many_arguments)]
fn swept_tinted(
    d: &mut Fixture<'_, '_>,
    slot: &str,
    radius: f64,
    profile: &[[f64; 2]],
    angles: [f64; 2],
    pieces: usize,
    caps: bool,
    tints: &[V3],
) -> Result<(), String> {
    use superspec_geometry::math::{cos, hypot, sin};
    let repeat = d.repeat(slot);
    let at = |[reach, z]: [f64; 2], angle: f64| -> V3 {
        [reach * sin(angle), radius - reach * cos(angle), z]
    };
    let step = |k: usize| angles[0] + (angles[1] - angles[0]) * k as f64 / pieces as f64;
    for (index, edge) in profile.windows(2).enumerate() {
        let (p, q) = (edge[0], edge[1]);
        let tint = tints[index % tints.len()];
        let size = hypot(q[0] - p[0], q[1] - p[1]);
        let across = [(q[1] - p[1]) / size, (p[0] - q[0]) / size];
        let normal =
            |angle: f64| -> V3 { [across[0] * sin(angle), -across[0] * cos(angle), across[1]] };
        for k in 0..pieces {
            let (a0, a1) = (step(k), step(k + 1));
            d.pen.face(
                d.mesh,
                slot,
                &[at(p, a0), at(p, a1), at(q, a1), at(q, a0)],
                normal((a0 + a1) / 2.),
                &Tint::Color(tint),
                repeat,
                None,
                Some(&[normal(a0), normal(a1), normal(a1), normal(a0)]),
            )?;
        }
    }
    if caps {
        for (angle, facing) in [(angles[0], -1.), (angles[1], 1.)] {
            let points: Vec<V3> = profile.iter().map(|p| at(*p, angle)).collect();
            d.pen.face(
                d.mesh,
                slot,
                &points,
                [cos(angle) * facing, sin(angle) * facing, 0.],
                &Tint::Color(tints[0]),
                repeat,
                None,
                None,
            )?;
        }
    }
    Ok(())
}

/// `kowloon/pavement-corner`: the pavement round a rounded street corner, between the
/// pavements of its two streets: rings of paving stones that carry their rows on and an arc of
/// kerb stones, concentric with the corner. Anchor: the wall's foot where its round starts;
/// the wall leaves along +X and turns left by `turn` about an upright axis `radius` behind it
/// (+Y), and the pavement reaches `depth` outside it, its top at z = 0 and the road `height`
/// below. Slots: `paving`, `kerb`. Parameters: `radius` (m of the wall's round, 0.05–20,
/// 0.55), `turn` (degrees, 5–175, 90), `depth` (m, 0.4–10, 1.45), `height` (m of kerb above
/// the road, 0.02–0.5, 0.15), `stone` (m, paving stone size, 0.2–2, 0.55), `segments` (chords
/// of the whole turn, 4–48, 12), `color` (#8f8e8a), `seed` (stone shades).
pub(super) fn pavement_corner(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let radius = a.bounded("radius", 0.55, 0.05..=20.)?;
    let turn = a.bounded("turn", 90., 5. ..=175.)?.to_radians();
    let depth = a.bounded("depth", 1.45, 0.4..=10.)?;
    let height = a.bounded("height", 0.15, 0.02..=0.5)?;
    let stone = a.bounded("stone", 0.55, 0.2..=2.)?;
    let segments = a.integer("segments", 12, 4..=48)?;
    let color = a.color("color", PAVING)?;
    let mut rng = a.rng()?;
    a.finish()?;
    let (kerb, joint) = (0.14, JOINT);
    // How many chords a span of the turn takes, so every arc is as round as the whole.
    let chords = |span: f64| ((span / turn * segments as f64).ceil() as usize).max(1);
    draw(cancelled, |mesh| {
        let d = &mut on(mesh, room_pen("pavement-corner", [0.; 3])?)?;
        // The bed shows in the joints between the stones.
        swept(
            d,
            "paving",
            radius,
            &[[radius + depth - kerb, -0.035], [radius * 0.9, -0.035]],
            [0., turn],
            segments,
            false,
            shade(color, JOINT_SHADE),
        )?;
        // The rows of the straight pavements run on as rings, each of its own count of stones.
        let rows = ((depth - kerb) / stone).round().max(1.) as usize;
        let row = (depth - kerb) / rows as f64;
        for r in 0..rows {
            let (inner, outer) = (radius + r as f64 * row, radius + (r + 1) as f64 * row);
            let middle = (inner + outer) / 2.;
            let count = (turn * middle / stone).round().max(1.) as usize;
            let (span, gap) = (turn / count as f64, joint / 2. / middle);
            for c in 0..count {
                let sink = rng.float(0., 0.006);
                let tint = shade(color, rng.float(0.7, 1.08));
                let (low, high) = (inner + joint / 2., outer - joint / 2.);
                // Chamfered into the joints along its rings, as the straight stones are.
                let (b, dark) = (BEVEL.min((high - low) / 4.), shade(tint, JOINT_SHADE));
                swept_tinted(
                    d,
                    "paving",
                    radius,
                    &[
                        [high, -0.04],
                        [high, -sink - b],
                        [high - b, -sink],
                        [low + b, -sink],
                        [low, -sink - b],
                        [low, -0.04],
                    ],
                    [c as f64 * span + gap, (c + 1) as f64 * span - gap],
                    chords(span),
                    true,
                    &[dark, dark, tint, dark, dark],
                )?;
            }
        }
        // Kerb stones with their road-side arris chamfered, as the straight kerb's.
        let edge = radius + depth;
        let stones = (turn * edge / (stone * 1.6)).round().max(1.) as usize;
        let (span, gap) = (turn / stones as f64, joint / 2. / edge);
        let (inner, top) = (edge - kerb + joint / 2., -0.012);
        let arris = 0.02_f64.min((height + top) * 0.4);
        for i in 0..stones {
            let tint = shade(color, rng.float(0.62, 0.92));
            swept(
                d,
                "kerb",
                radius,
                &[
                    [edge, -height],
                    [edge, top - arris],
                    [edge - arris, top],
                    [inner, top],
                    [inner, -height],
                ],
                [i as f64 * span + gap, (i + 1) as f64 * span - gap],
                chords(span),
                true,
                tint,
            )?;
        }
        Ok(())
    })
}

/// Kerb stones of `stone` length at most along `span` of a frame's X, their road face on
/// y = 0 (toward -Y), `kerb` deep and their top at -0.012 with the road-side arris
/// chamfered, as the pavement's kerb; the road is `height` below the paving's top.
#[allow(clippy::too_many_arguments)]
fn kerb_run(
    d: &mut Fixture<'_, '_>,
    span: [f64; 2],
    kerb: f64,
    height: f64,
    stone: f64,
    color: V3,
    rng: &mut SeededRng,
) -> Result<(), String> {
    let joint = JOINT;
    let stones = ((span[1] - span[0]) / stone).round().max(1.) as usize;
    let long = (span[1] - span[0]) / stones as f64;
    let (inner, top) = (kerb - joint / 2., -0.012);
    let arris = 0.02_f64.min((height + top) * 0.4);
    for i in 0..stones {
        let left = span[0] + i as f64 * long;
        let repeat = d.repeat("kerb");
        profiled_strip(
            &d.pen,
            d.mesh,
            "kerb",
            &Strip {
                span: [left + joint / 2., left + long - joint / 2.],
                section: &[
                    [0., -height],
                    [inner, -height],
                    [inner, top],
                    [arris, top],
                    [0., top - arris],
                ],
                omitted_edges: &[0],
                miters: [None; 2],
                setback: 0.,
            },
            &Tint::Color(shade(color, rng.float(0.62, 0.92))),
            repeat,
        )?;
    }
    Ok(())
}

/// `kowloon/pedestrian-avenue`: a paved street for people on foot, with a planted median down
/// its middle: courses of large slabs breaking joint (every `bands`th course a darker band) on
/// a bed, kerb stones where it meets the road, a raised planter of kerb stones round a bed of
/// soil (its top `soilDepth` below the planter's, for grass or a lawn laid over it), a linear
/// drain beside the planter on either side, backless benches against it and rows of bollards
/// across its open ends. Anchor: the start of its near side (a building's wall line, say);
/// it runs along +X for `length` and reaches `width` toward -Y, its top at z = 0 and the road
/// `height` below its open ends and far side. Slots: `paving`, `kerb`, `soil`, `metal`,
/// `timber`. Parameters: `length` (m, 2–80, 30), `width` (m, 2–40, 14), `height` (m, 0.05–0.5,
/// 0.12), `slab` (m along, 0.2–3, 0.9) and `course` (m across, 0.2–3, 0.6), `bands` (0–40, 5;
/// 0 none), `mouth`, `end` and `side` (kerbs at its start, end and far side, true),
/// `medianWidth` (m, 0 for none, 0.8–10, 2.4), `medianOffset` (m from the near side to the
/// median's middle, half the width), `medianStart` (m, 3) and `medianEnd` (m along, 3 m short
/// of the end), `planterKerb` (m, 0.08–0.6, 0.2), `planterRise` (m above the paving,
/// 0.05–1, 0.3), `soilDepth` (m below the planter's top, 0.02–1, 0.14), `drains` (true),
/// `bollardSpacing` (m, 0 for none, 0.6–10, 1.6) and `bollardColor` (#3d403b), `benches`
/// (pairs, 0–40, 3), `benchFirst` (m along, where the median's first third starts, or a
/// quarter of the length without a median) and `benchSpacing` (m, 0.5–80, 7), `bins` (a
/// litter bin past the ends of each bench station, false) and `binColor` (#8c7f6c), `stagger`
/// (courses breaking joint, true; false a grid), `color` (#8f8e8a), `seed` (stone shades).
/// The timber and the bins are tinted near neutral, so the materials bound to them decide.
///
/// Features ([`pedestrian_avenue_features`]): `foot`, on the paving plane (z = 0, facing up),
/// round the base of every bollard, litter bin and bench support and round the planter's outer
/// foot, each a closed ring whose `width` is the piece's breadth, for wear and grime that
/// gather where things stand.
pub(super) fn pedestrian_avenue(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let (plan, mut rng) = Avenue::read(name, values)?;
    let Avenue {
        length,
        width,
        height,
        slab,
        course,
        bands,
        stagger,
        mouth,
        end,
        side,
        median,
        median_start,
        median_end,
        planter_kerb,
        rise,
        soil_depth,
        drains,
        bollard_color,
        bins,
        bin_color,
        color,
        ..
    } = plan;
    let (kerb, joint) = (AVENUE_KERB, JOINT);
    // The paved field inside the kerbs, and the planter's outline (near side y_near, toward 0).
    let [x0, x1] = plan.field();
    let y0 = -width + if side { kerb } else { 0. };
    let [y_near, y_far] = plan.median_sides();
    let under_median = |l: f64, r: f64, low: f64, high: f64| {
        median && l >= median_start && r <= median_end && low >= y_far && high <= y_near
    };
    draw(cancelled, |mesh| {
        {
            let d = &mut on(mesh, room_pen("pedestrian-avenue", [0.; 3])?)?;
            // The bed shows in the joints between the slabs, and as the avenue's near edge.
            d.cuboid(
                "paving",
                [x0, y0, -height],
                [x1, 0., -0.035],
                shade(color, JOINT_SHADE),
                "d",
            )?;
            let rows = ((-y0) / course).round().max(1.) as usize;
            let row = -y0 / rows as f64;
            let columns = ((x1 - x0) / slab).round().max(1.) as usize;
            let pitch = (x1 - x0) / columns as f64;
            for r in 0..rows {
                let (high, low) = (-(r as f64) * row, -((r + 1) as f64) * row);
                let band = bands > 0 && r % bands == bands / 2;
                let offset = if stagger && r % 2 == 1 {
                    pitch / 2.
                } else {
                    0.
                };
                let (mut left, mut c) = (x0, 0);
                while left < x1 - 1e-6 {
                    let right = if c == 0 && offset > 0. {
                        left + offset
                    } else {
                        (left + pitch).min(x1)
                    };
                    let (l, rr) = (left, right);
                    left = right;
                    c += 1;
                    if rr - l < 3. * joint || under_median(l, rr, low, high) {
                        continue;
                    }
                    let sink = rng.float(0., 0.005);
                    let tint = if band {
                        shade(color, rng.float(0.5, 0.6))
                    } else {
                        shade(color, rng.float(0.74, 1.06))
                    };
                    paving_stone(
                        d,
                        [
                            l + joint / 2.,
                            low + joint / 2.,
                            rr - joint / 2.,
                            high - joint / 2.,
                        ],
                        -sink,
                        tint,
                    )?;
                }
            }
            if median {
                // The planter: kerb stones along both sides and across both ends, over soil.
                let k = planter_kerb;
                let pieces = ((median_end - median_start) / 1.).round().max(1.) as usize;
                let piece = (median_end - median_start) / pieces as f64;
                for (low, high) in [(y_near - k, y_near), (y_far, y_far + k)] {
                    for i in 0..pieces {
                        let l = median_start + i as f64 * piece;
                        d.cuboid(
                            "kerb",
                            [l + joint / 2., low, -0.03],
                            [l + piece - joint / 2., high, rise],
                            shade(color, rng.float(0.78, 0.98)),
                            "d",
                        )?;
                    }
                }
                for (l, r) in [
                    (median_start, median_start + k),
                    (median_end - k, median_end),
                ] {
                    d.cuboid(
                        "kerb",
                        [l, y_far + k + joint / 2., -0.03],
                        [r, y_near - k - joint / 2., rise],
                        shade(color, rng.float(0.78, 0.98)),
                        "d",
                    )?;
                }
                d.cuboid(
                    "soil",
                    [median_start + k, y_far + k, 0.],
                    [median_end - k, y_near - k, rise - soil_depth],
                    srgb("#4a3b2c"),
                    "lrbfd",
                )?;
                if drains {
                    // A slot drain 0.2 m wide, 0.8 m out from either kerb, under heavy iron
                    // bars 6 cm wide, one every 0.25 m at most.
                    let iron = srgb("#343632");
                    let run = median_end - median_start;
                    let bars = (run / 0.25).ceil().max(1.) as usize;
                    let pitch = run / bars as f64;
                    for centre in [y_near + 0.8, y_far - 0.8] {
                        d.cuboid(
                            "metal",
                            [median_start, centre - 0.1, -0.004],
                            [median_end, centre + 0.1, 0.003],
                            shade(iron, 0.5),
                            "d",
                        )?;
                        for bar in 0..bars {
                            let x = median_start + (bar as f64 + 0.5) * pitch;
                            d.cuboid(
                                "metal",
                                [x - 0.03, centre - 0.085, 0.003],
                                [x + 0.03, centre + 0.085, 0.013],
                                shade(iron, 1.7),
                                "d",
                            )?;
                        }
                    }
                }
            }
            // Backless benches: stone plinths and two heavy timber slats, their backs to the
            // planter (or along the avenue's middle without one), a pair at each station.
            // The timber is weathered to a silver grey, near neutral, so its material decides.
            let timber = srgb("#a39c90");
            for seat in plan.bench_seats() {
                let [at, y_from, y_to] = seat;
                for [l, low, r, high] in Avenue::bench_legs(seat) {
                    d.cuboid(
                        "kerb",
                        [l, low, -0.01],
                        [r, high, 0.4],
                        shade(color, rng.float(0.8, 0.95)),
                        "d",
                    )?;
                }
                let slat = (y_to - y_from - 0.04) / 2.;
                for s in 0..2 {
                    let y = y_from + s as f64 * (slat + 0.04);
                    d.cuboid(
                        "timber",
                        [at - 0.9, y, 0.4],
                        [at + 0.9, y + slat, 0.46],
                        shade(timber, rng.float(0.88, 1.04)),
                        "",
                    )?;
                }
            }
            // Bollards across the open ends, keeping vehicles out.
            let steel = bollard_color;
            for [x, y] in plan.bollards() {
                // A stout shaft under a broad pale band and a heavy cap.
                d.drum("metal", [x, y, -0.01], BOLLARD, 0.8, steel, 12)?;
                d.drum(
                    "metal",
                    [x, y, 0.56],
                    BOLLARD + 0.006,
                    0.11,
                    srgb("#c9c2a8"),
                    12,
                )?;
                d.drum(
                    "metal",
                    [x, y, 0.77],
                    BOLLARD + 0.012,
                    0.055,
                    shade(steel, 0.8),
                    12,
                )?;
            }
        }
        // Kerb stones where the avenue meets the road: across its start and end, along its far side.
        let stone = slab * 1.6;
        let base = room_pen("pedestrian-avenue", [0.; 3])?;
        if mouth {
            let d = &mut on(mesh, base.facing([0.; 3], [0., -1., 0.], [1., 0., 0.])?)?;
            kerb_run(d, [0., width], kerb, height, stone, color, &mut rng)?;
        }
        if end {
            let d = &mut on(
                mesh,
                base.facing([length, -width, 0.], [0., 1., 0.], [-1., 0., 0.])?,
            )?;
            kerb_run(d, [0., width], kerb, height, stone, color, &mut rng)?;
        }
        if side {
            let d = &mut on(
                mesh,
                base.facing([0., -width, 0.], [1., 0., 0.], [0., 1., 0.])?,
            )?;
            kerb_run(d, [x0, x1], kerb, height, stone, color, &mut rng)?;
        }
        if bins {
            for [x, y] in plan.bin_spots() {
                let d = &mut on(mesh, base.moved([x, y, 0.])?)?;
                litter_bin(d, bin_color)?;
            }
        }
        Ok(())
    })
}

/// The radius of an avenue bollard's shaft, and of a litter bin's foot.
const BOLLARD: f64 = 0.082;
const BIN_FOOT: f64 = 0.2;
/// How deep the kerb stones on an avenue's road sides are.
const AVENUE_KERB: f64 = 0.14;

/// What `kowloon/pedestrian-avenue` lays out, read once for its meshes and for its features.
struct Avenue {
    length: f64,
    width: f64,
    height: f64,
    slab: f64,
    course: f64,
    bands: usize,
    stagger: bool,
    mouth: bool,
    end: bool,
    side: bool,
    median: bool,
    median_width: f64,
    offset: f64,
    median_start: f64,
    median_end: f64,
    planter_kerb: f64,
    rise: f64,
    soil_depth: f64,
    drains: bool,
    bollard_spacing: f64,
    bollard_color: V3,
    benches: usize,
    bench_spacing: f64,
    bench_first: f64,
    bins: bool,
    bin_color: V3,
    color: V3,
}

impl Avenue {
    /// The avenue and the stream its stones are shaded from.
    fn read(name: &str, values: &Values) -> Result<(Self, SeededRng), String> {
        let mut a = Arguments::new(name, values);
        let length = a.bounded("length", 30., 2. ..=80.)?;
        let width = a.bounded("width", 14., 2. ..=40.)?;
        let height = a.bounded("height", 0.12, 0.05..=0.5)?;
        let slab = a.bounded("slab", 0.9, 0.2..=3.)?;
        let course = a.bounded("course", 0.6, 0.2..=3.)?;
        let bands = a.integer("bands", 5, 0..=40)?;
        // `stagger`: alternate courses break joint by half a slab (true); false lays the slabs
        // in a grid, their joints running straight both ways.
        let stagger = a.boolean("stagger", true)?;
        let mouth = a.boolean("mouth", true)?;
        let end = a.boolean("end", true)?;
        let side = a.boolean("side", true)?;
        let median_width = a.bounded("medianWidth", 2.4, 0. ..=10.)?;
        let offset = a.number("medianOffset", width / 2.)?;
        let median_start = a.number("medianStart", 3_f64.min(length / 4.))?;
        let median_end = a.number("medianEnd", length - 3_f64.min(length / 4.))?;
        let planter_kerb = a.bounded("planterKerb", 0.2, 0.08..=0.6)?;
        let rise = a.bounded("planterRise", 0.3, 0.05..=1.)?;
        let soil_depth = a.bounded("soilDepth", 0.14, 0.02..=1.)?;
        let drains = a.boolean("drains", true)?;
        let bollard_spacing = a.bounded("bollardSpacing", 1.6, 0. ..=10.)?;
        // `bollardColor`: the bollards' paint (#3d403b) under a pale reflective band.
        let bollard_color = a.color("bollardColor", "#3d403b")?;
        let benches = a.integer("benches", 3, 0..=40)?;
        let bench_spacing = a.bounded("benchSpacing", 7., 0.5..=80.)?;
        let bench_first = a.number(
            "benchFirst",
            if median_width > 0. {
                median_start + (median_end - median_start) / 3.
            } else {
                length / 4.
            },
        )?;
        // `bins`: a litter bin past either end of each bench station, one on each side (false),
        // painted `binColor` (a muted grey-brown, #8c7f6c) under a steel lid.
        let bins = a.boolean("bins", false)?;
        let bin_color = a.color("binColor", "#8c7f6c")?;
        let color = a.color("color", PAVING)?;
        let rng = a.rng()?;
        a.finish()?;
        let median = median_width > 0.;
        if median {
            if median_width < 0.8 || median_width < 2. * planter_kerb + 0.2 {
                return Err(format!(
                    "{name}: medianWidth must be 0 or at least 0.8 and leave room inside its planter kerbs."
                ));
            }
            if offset - median_width / 2. < 0.3 || offset + median_width / 2. > width - 0.3 {
                return Err(format!(
                    "{name}: the median ({median_width} m about {offset} m) must stand 0.3 m inside the avenue's {width} m."
                ));
            }
            if median_start < 0.
                || median_end > length
                || median_end - median_start < 2. * planter_kerb + 0.4
            {
                return Err(format!(
                    "{name}: medianStart and medianEnd must lie in 0..length, at least {} m apart.",
                    2. * planter_kerb + 0.4
                ));
            }
        }
        if soil_depth >= rise {
            return Err(format!("{name}: soilDepth must be less than planterRise."));
        }
        if bollard_spacing > 0. && bollard_spacing < 0.6 {
            return Err(format!("{name}: bollardSpacing must be 0 or 0.6..10."));
        }
        Ok((
            Self {
                length,
                width,
                height,
                slab,
                course,
                bands,
                stagger,
                mouth,
                end,
                side,
                median,
                median_width,
                offset,
                median_start,
                median_end,
                planter_kerb,
                rise,
                soil_depth,
                drains,
                bollard_spacing,
                bollard_color,
                benches,
                bench_spacing,
                bench_first,
                bins,
                bin_color,
                color,
            },
            rng,
        ))
    }

    /// Where the paved field starts and ends along x, inside the kerbs of its open ends.
    fn field(&self) -> [f64; 2] {
        [
            if self.mouth { AVENUE_KERB } else { 0. },
            self.length - if self.end { AVENUE_KERB } else { 0. },
        ]
    }

    /// The planter's near and far sides (y; both the median's middle without one).
    fn median_sides(&self) -> [f64; 2] {
        [
            -(self.offset - self.median_width / 2.),
            -(self.offset + self.median_width / 2.),
        ]
    }

    /// The planter's outline in plan, `[x0, y0, x1, y1]`, when there is a median.
    fn planter(&self) -> Option<[f64; 4]> {
        let [near, far] = self.median_sides();
        self.median
            .then_some([self.median_start, far, self.median_end, near])
    }

    /// The bench stations that keep clear of the planter's (or the field's) ends.
    fn stations(&self) -> Vec<f64> {
        let [x0, x1] = self.field();
        let (lo, hi) = if self.median {
            (self.median_start + 1., self.median_end - 1.)
        } else {
            (x0 + 1., x1 - 1.)
        };
        (0..self.benches)
            .map(|i| self.bench_first + i as f64 * self.bench_spacing)
            .filter(|at| *at >= lo && *at <= hi)
            .collect()
    }

    /// Every bench, a pair a station with their backs to the planter (or along the avenue's
    /// middle without one): `[station, y_from, y_to]`, the seat reaching 0.9 m either way.
    fn bench_seats(&self) -> Vec<[f64; 3]> {
        let [y_near, y_far] = self.median_sides();
        let (near, far) = if self.median {
            (y_near + 0.15, y_far - 0.15)
        } else {
            (-self.offset + 0.5, -self.offset - 0.5)
        };
        self.stations()
            .into_iter()
            .flat_map(|at| [[at, near, near + 0.42], [at, far - 0.42, far]])
            .collect()
    }

    /// The footprints of a bench's two stone supports, `[x0, y0, x1, y1]`.
    fn bench_legs([at, y_from, y_to]: [f64; 3]) -> [[f64; 4]; 2] {
        [at - 0.75, at + 0.6].map(|leg| [leg, y_from + 0.04, leg + 0.15, y_to - 0.04])
    }

    /// Where the bollards stand: a row across each open end, leaving out any in the planter.
    fn bollards(&self) -> Vec<[f64; 2]> {
        if self.bollard_spacing <= 0. || self.width <= 1.2 {
            return vec![];
        }
        let [y_near, y_far] = self.median_sides();
        let count = ((self.width - 1.2) / self.bollard_spacing).floor() as usize + 1;
        let first = -(self.width - (count - 1) as f64 * self.bollard_spacing) / 2.;
        let mut spots = vec![];
        for (open, x) in [(self.mouth, 0.45), (self.end, self.length - 0.45)] {
            if !open {
                continue;
            }
            for b in 0..count {
                let y = first - b as f64 * self.bollard_spacing;
                let planted = self.median
                    && x >= self.median_start
                    && x <= self.median_end
                    && y >= y_far
                    && y <= y_near;
                if !planted {
                    spots.push([x, y]);
                }
            }
        }
        spots
    }

    /// Where the litter bins stand: one past either end of each bench station, on either side.
    fn bin_spots(&self) -> Vec<[f64; 2]> {
        if !self.bins {
            return vec![];
        }
        let [x0, x1] = self.field();
        let [y_near, y_far] = self.median_sides();
        let (near, far) = if self.median {
            (y_near + 0.4, y_far - 0.4)
        } else {
            (-self.offset + 0.75, -self.offset - 0.75)
        };
        self.stations()
            .into_iter()
            .flat_map(|at| [[at - 1.35, near], [at + 1.35, far]])
            .filter(|[x, _]| *x >= x0 + 0.4 && *x <= x1 - 0.4)
            .collect()
    }
}

/// A `foot` feature: a closed ring on the plane z = 0 of a fixture standing on it, facing up,
/// `width` the breadth of what stands there. `reach` and `within` carry it to the receivers
/// of other items (the paving a lamp stands on); a fixture's own paving needs neither.
fn foot(points: Vec<V3>, width: f64, reach: Reach, within: f64) -> Feature {
    Feature {
        name: "foot".into(),
        part: String::new(),
        source: None,
        face: "top".into(),
        points,
        closed: true,
        normal: [0., 0., 1.],
        width,
        reach,
        within,
        zone: None,
    }
}

/// A round foot of `radius` about (`x`, `y`) in `sides` chords, counterclockwise from above.
fn round_foot([x, y]: [f64; 2], radius: f64, sides: usize) -> Feature {
    let points = (0..sides)
        .map(|k| {
            let angle = k as f64 / sides as f64 * std::f64::consts::TAU;
            [
                x + radius * superspec_geometry::math::cos(angle),
                y + radius * superspec_geometry::math::sin(angle),
                0.,
            ]
        })
        .collect();
    foot(points, 2. * radius, Reach::Part, 0.)
}

/// A rectangular foot `[x0, y0, x1, y1]`, counterclockwise from above.
fn square_foot([x0, y0, x1, y1]: [f64; 4], reach: Reach, within: f64) -> Feature {
    foot(
        vec![[x0, y0, 0.], [x1, y0, 0.], [x1, y1, 0.], [x0, y1, 0.]],
        (x1 - x0).min(y1 - y0),
        reach,
        within,
    )
}

/// What stands on `kowloon/pedestrian-avenue`'s paving (see [`pedestrian_avenue`]), from the
/// positions it is drawn at. Parameters it refuses give none; drawing reports why.
pub(super) fn pedestrian_avenue_features(name: &str, values: &Values) -> Vec<Feature> {
    let Ok((plan, _)) = Avenue::read(name, values) else {
        return vec![];
    };
    let mut features = vec![];
    if let Some(outline) = plan.planter() {
        let mut kerb = square_foot(outline, Reach::Part, 0.);
        kerb.width = plan.planter_kerb;
        features.push(kerb);
    }
    for seat in plan.bench_seats() {
        for leg in Avenue::bench_legs(seat) {
            features.push(square_foot(leg, Reach::Part, 0.));
        }
    }
    features.extend(
        plan.bollards()
            .into_iter()
            .map(|at| round_foot(at, BOLLARD, 12)),
    );
    features.extend(
        plan.bin_spots()
            .into_iter()
            .map(|at| round_foot(at, BIN_FOOT, 16)),
    );
    features
}

/// A round litter bin standing on z = 0 at the frame's origin: a slightly flared body in
/// `tint` with a dark band round its mouths, under a domed steel lid (slot `metal`).
fn litter_bin(d: &mut Fixture<'_, '_>, tint: V3) -> Result<(), String> {
    use std::f64::consts::TAU;
    let steel = srgb("#8d918b");
    swept(
        d,
        "metal",
        0.,
        &[[BIN_FOOT, -0.01], [0.22, 0.8]],
        [0., TAU],
        16,
        false,
        tint,
    )?;
    swept(
        d,
        "metal",
        0.,
        &[[0.226, 0.6], [0.226, 0.7]],
        [0., TAU],
        16,
        false,
        srgb("#2b2b28"),
    )?;
    swept(
        d,
        "metal",
        0.,
        &[[0.22, 0.8], [0.24, 0.8], [0.24, 0.86], [0.07, 0.95]],
        [0., TAU],
        16,
        false,
        steel,
    )?;
    d.drum("metal", [0., 0., 0.94], 0.075, 0.03, shade(steel, 0.8), 12)
}

/// Frame paints a parked cart is seen in.
const CART_PAINTS: [&str; 6] = [
    "#2f4a5a", "#5a2c28", "#2e2f2c", "#3f5a3a", "#8a8778", "#6b4f2e",
];

/// `kowloon/street-cart`: something parked on a pavement: a utility bicycle (`bicycle`: a
/// roadster frame on two spoked wheels under mudguards, a rear rack, a chain wheel and pedals,
/// a saddle, swept-back bars and by seed a wire basket on the front) or a flat-bed hand cart
/// (`trolley`: a steel deck on four castors with a push bar at one end, loaded by seed with a
/// stack of cartons and plastic crates). Anchor: on the ground at the cart's middle; it runs
/// along +X (the bicycle's front, the trolley's bar end toward -X). Slots: `metal` (frame,
/// wheels and fittings, tinted per vertex) and `stock` (the saddle, grips and a trolley's load,
/// coloured per vertex). Parameters: `kind` (`bicycle` | `trolley`, `bicycle`), `color` (the
/// frame's paint; the seed's when absent), `load` (a trolley's, 0–1, 0.7), `seed`.
pub(super) fn street_cart(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let trolley = match a.text("kind", "bicycle")?.as_str() {
        "bicycle" => false,
        "trolley" => true,
        _ => return Err(format!("{name}: kind must be one of bicycle, trolley.")),
    };
    let given = match a.optional_text("color")? {
        Some(_) => Some(a.color("color", "#000000")?),
        None => None,
    };
    let load = a.bounded("load", 0.7, 0. ..=1.)?;
    let mut rng = a.rng()?;
    a.finish()?;
    let paint = given.unwrap_or_else(|| shade(srgb(rng.pick(&CART_PAINTS)), rng.float(0.85, 1.1)));
    draw(cancelled, |mesh| {
        let d = &mut on(mesh, room_pen("street-cart", [0.; 3])?)?;
        if trolley {
            hand_cart(d, paint, load, &mut rng)
        } else {
            bicycle(d, paint, &mut rng)
        }
    })
}

/// Points round a circle of `radius` about `centre` in the XZ plane, from `from` to `to`
/// radians (measured from +X toward +Z), `pieces` chords.
fn arc(centre: V3, radius: f64, [from, to]: [f64; 2], pieces: usize) -> Vec<V3> {
    use superspec_geometry::math::{cos, sin};
    (0..=pieces)
        .map(|k| {
            let t = from + (to - from) * k as f64 / pieces as f64;
            [
                centre[0] + radius * cos(t),
                centre[1],
                centre[2] + radius * sin(t),
            ]
        })
        .collect()
}

/// A utility bicycle along +X, standing on z = 0 about the origin (see [`street_cart`]).
fn bicycle(d: &mut Fixture<'_, '_>, paint: V3, rng: &mut SeededRng) -> Result<(), String> {
    use std::f64::consts::{PI, TAU};
    use superspec_geometry::math::{cos, sin};
    let (tyre, steel, black) = (srgb("#1c1c1a"), srgb("#a3a59f"), srgb("#262624"));
    let radius = 0.33;
    let (rear, front) = ([-0.52, 0., radius], [0.53, 0., radius]);
    for axle in [rear, front] {
        // A fat tyre on its rim as closed rings, and six stout spokes from a heavy hub: drawn
        // as a wheel is drawn, not as it is built.
        let mut ring = arc(axle, radius - 0.024, [0., TAU], 24);
        ring.pop();
        let first = ring[0];
        ring.push(first);
        d.tube("metal", &ring, 0.024, tyre, 6)?;
        let mut rim = arc(axle, radius - 0.056, [0., TAU], 24);
        rim.pop();
        let first = rim[0];
        rim.push(first);
        d.tube("metal", &rim, 0.011, steel, 5)?;
        d.tube(
            "metal",
            &[[axle[0], -0.05, axle[2]], [axle[0], 0.05, axle[2]]],
            0.03,
            steel,
            8,
        )?;
        let turn = rng.float(0., TAU / 6.);
        for k in 0..6 {
            let angle = k as f64 * TAU / 6. + turn;
            let r = radius - 0.058;
            d.tube(
                "metal",
                &[
                    [axle[0], 0., axle[2]],
                    [axle[0] + r * cos(angle), 0., axle[2] + r * sin(angle)],
                ],
                0.009,
                steel,
                4,
            )?;
        }
        // A mudguard over the top of the wheel.
        let mut guard = arc(axle, radius + 0.032, [0.15 * PI, 0.95 * PI], 12);
        if axle[0] > 0. {
            guard = arc(axle, radius + 0.032, [0.05 * PI, 0.85 * PI], 12);
        }
        d.tube("metal", &guard, 0.022, shade(paint, 0.9), 6)?;
    }
    let bracket = [-0.06, 0., 0.29];
    let seat = [-0.21, 0., 0.84];
    let head = ([0.39, 0., 0.86], [0.43, 0., 0.7]);
    let tube =
        |d: &mut Fixture<'_, '_>, a: V3, b: V3, r: f64| d.tube("metal", &[a, b], r, paint, 8);
    // Seat tube, down tube, top tube and the head tube.
    tube(d, bracket, seat, 0.019)?;
    tube(d, bracket, head.1, 0.021)?;
    tube(d, [-0.19, 0., 0.79], [0.395, 0., 0.83], 0.018)?;
    tube(d, head.1, head.0, 0.023)?;
    // Stays to the rear axle and the fork's blades to the front, either side of the wheels.
    for y in [-0.052, 0.052] {
        tube(
            d,
            [bracket[0], y * 0.6, bracket[2]],
            [rear[0], y, rear[2]],
            0.012,
        )?;
        tube(d, [-0.2, y * 0.6, 0.77], [rear[0], y, rear[2]], 0.011)?;
        tube(
            d,
            [head.1[0], y * 0.6, head.1[2]],
            [front[0], y, front[2]],
            0.013,
        )?;
    }
    // Seat post and saddle; stem and swept-back bars with grips.
    d.tube("metal", &[seat, [-0.225, 0., 0.93]], 0.014, steel, 8)?;
    d.cuboid(
        "stock",
        [-0.335, -0.08, 0.93],
        [-0.115, 0.08, 0.99],
        black,
        "",
    )?;
    d.tube("metal", &[head.0, [0.38, 0., 0.99]], 0.015, steel, 8)?;
    let bars = [
        [0.27, -0.29, 1.0],
        [0.33, -0.22, 1.0],
        [0.38, -0.08, 0.99],
        [0.38, 0.08, 0.99],
        [0.33, 0.22, 1.0],
        [0.27, 0.29, 1.0],
    ];
    d.tube("metal", &bars, 0.012, steel, 6)?;
    for (a, b) in [(bars[0], bars[1]), (bars[5], bars[4])] {
        d.tube(
            "stock",
            &[
                a,
                [a[0] + (b[0] - a[0]) * 0.6, a[1] + (b[1] - a[1]) * 0.6, a[2]],
            ],
            0.019,
            black,
            6,
        )?;
    }
    // The chain wheel, cranks and pedals; the chain to the rear sprocket.
    d.disc(
        "metal",
        [bracket[0], 0.055, bracket[2]],
        0.095,
        [0., 1., 0.],
        steel,
        16,
    )?;
    d.disc(
        "metal",
        [bracket[0], 0.054, bracket[2]],
        0.095,
        [0., -1., 0.],
        steel,
        16,
    )?;
    let crank = rng.float(0., PI);
    for (side, turn) in [(0.065, 0.), (-0.065, PI)] {
        let (c, s) = (cos(crank + turn), sin(crank + turn));
        let end = [bracket[0] + 0.165 * c, side * 1.6, bracket[2] + 0.165 * s];
        d.tube(
            "metal",
            &[[bracket[0], side, bracket[2]], end],
            0.011,
            steel,
            6,
        )?;
        d.cuboid(
            "metal",
            [end[0] - 0.052, end[1].min(side * 2.6), end[2] - 0.013],
            [end[0] + 0.052, end[1].max(side * 2.6), end[2] + 0.013],
            black,
            "",
        )?;
    }
    for z in [bracket[2] + 0.09, bracket[2] - 0.09] {
        d.tube(
            "metal",
            &[
                [bracket[0], 0.06, z],
                [rear[0], 0.06, rear[2] + (z - bracket[2]) * 0.45],
            ],
            0.008,
            srgb("#3a3631"),
            4,
        )?;
    }
    // A rear rack over the wheel on stays, and a kickstand.
    d.cuboid(
        "metal",
        [-0.78, -0.1, 0.738],
        [-0.3, 0.1, 0.762],
        shade(paint, 0.85),
        "",
    )?;
    for y in [-0.09, 0.09] {
        d.tube(
            "metal",
            &[[-0.72, y, 0.74], [rear[0], y * 0.7, rear[2]]],
            0.01,
            steel,
            4,
        )?;
    }
    d.tube(
        "metal",
        &[[-0.3, 0.05, 0.3], [-0.38, 0.17, 0.005]],
        0.011,
        steel,
        5,
    )?;
    if rng.chance(0.6) {
        // A basket on the front in boards, its contents a dark mass.
        let (x0, x1) = (0.48, 0.8);
        d.cuboid("metal", [x0, -0.16, 0.78], [x1, 0.16, 0.8], steel, "")?;
        for (lo, hi) in [
            ([x0, -0.16, 0.8], [x1, -0.142, 1.0]),
            ([x0, 0.142, 0.8], [x1, 0.16, 1.0]),
            ([x0, -0.142, 0.8], [x0 + 0.018, 0.142, 1.0]),
            ([x1 - 0.018, -0.142, 0.8], [x1, 0.142, 1.0]),
        ] {
            d.cuboid("metal", lo, hi, steel, "")?;
        }
        if rng.chance(0.6) {
            d.cuboid(
                "stock",
                [x0 + 0.03, -0.12, 0.8],
                [x1 - 0.03, 0.12, 0.92],
                shade(srgb(rng.pick(&["#b6a27a", "#6d7a5a", "#c9c1b0"])), 0.9),
                "d",
            )?;
        }
    }
    Ok(())
}

/// A flat-bed hand cart along +X with its push bar at -X, loaded (see [`street_cart`]).
fn hand_cart(
    d: &mut Fixture<'_, '_>,
    paint: V3,
    load: f64,
    rng: &mut SeededRng,
) -> Result<(), String> {
    let (rubber, steel) = (srgb("#1c1c1a"), srgb("#9a9c96"));
    let (half_x, half_y, deck) = (0.46, 0.29, 0.17);
    d.cuboid(
        "metal",
        [-half_x, -half_y, deck - 0.04],
        [half_x, half_y, deck],
        paint,
        "",
    )?;
    for x in [-half_x + 0.08, half_x - 0.08] {
        for y in [-half_y + 0.06, half_y - 0.06] {
            // A castor: its fork plate and the wheel across it.
            d.cuboid(
                "metal",
                [x - 0.03, y - 0.03, 0.1],
                [x + 0.03, y + 0.03, deck - 0.04],
                steel,
                "",
            )?;
            d.tube(
                "metal",
                &[[x, y - 0.018, 0.065], [x, y + 0.018, 0.065]],
                0.065,
                rubber,
                12,
            )?;
        }
    }
    // The push bar: two uprights and a grip across.
    let bar = -half_x + 0.02;
    let top = 0.95;
    d.tube(
        "metal",
        &[
            [bar, -half_y + 0.03, deck],
            [bar - 0.05, -half_y + 0.03, top],
            [bar - 0.05, half_y - 0.03, top],
            [bar, half_y - 0.03, deck],
        ],
        0.017,
        steel,
        8,
    )?;
    // The load: cartons and crates stacked on the deck.
    let cardboard = ["#b08a5a", "#a3814f", "#c29a66", "#9b7a52"];
    let crates = ["#3a6a9a", "#c0402e", "#3f7a4a", "#d0b030"];
    let mut z = deck;
    let layers = (load * 4.).round() as usize;
    for layer in 0..layers {
        let crate_layer = rng.chance(0.3);
        let tint = if crate_layer {
            srgb(rng.pick(&crates))
        } else {
            shade(srgb(rng.pick(&cardboard)), rng.float(0.85, 1.05))
        };
        let height = if crate_layer {
            0.26
        } else {
            rng.float(0.22, 0.34)
        };
        let shrink = layer as f64 * 0.02 + rng.float(0., 0.03);
        let shift = rng.float(-0.03, 0.03);
        d.cuboid(
            "stock",
            [-half_x + 0.06 + shrink + shift, -half_y + 0.02 + shrink, z],
            [
                half_x - 0.02 - shrink + shift,
                half_y - 0.02 - shrink,
                z + height,
            ],
            tint,
            "d",
        )?;
        z += height;
    }
    Ok(())
}

/// Half the side of a street lamp's square base plate.
const LAMP_PLATE: f64 = 0.2;

/// Where `kowloon/street-lamp` stands (see [`street_lamp`]): its base plate's outline, given
/// to the receivers of the scene within half a meter (the paving under it).
pub(super) fn street_lamp_features(_name: &str, _values: &Values) -> Vec<Feature> {
    let plate = LAMP_PLATE;
    vec![square_foot(
        [-plate, -plate, plate, plate],
        Reach::Scene,
        0.5,
    )]
}

/// `kowloon/street-lamp`: a street lamp on a tapered steel pole: a base plate under a cast
/// collar, the pole with its number plate, and at its head either a swan-neck arm with a
/// shallow luminaire (`arm`) or a four-sided lantern under a pyramid roof (`lantern`), and
/// optionally a pair of banner brackets with a banner hung between them. Anchor: the pole's
/// foot; an arm reaches toward -Y and the banner hangs toward +X. Slots: `metal` (tinted per
/// vertex), `lamp` (the luminaire's diffuser or the lantern's glass, pale, for a material
/// that makes it glow) and `banner` (with `banner`). Parameters: `style` (`arm` |
/// `lantern`, `arm`), `height` (m to the pole's head, 2.5–14; 6 for an arm, 3.6 for a
/// lantern), `reach` (m of the arm, 0.3–3, 1.3), `arms` (1–2, 1: two reach either way),
/// `banner` (false), `color` (#3d4842), `light` (#f2ead2), `bannerColor` (#a3352c).
///
/// Features ([`street_lamp_features`]): `foot`, the base plate's outline on the plane it
/// stands on (z = 0, facing up), which reaches the paving of whatever item lies under it.
pub(super) fn street_lamp(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    use std::f64::consts::{FRAC_PI_4, TAU};
    let mut a = Arguments::new(name, values);
    let lantern = match a.text("style", "arm")?.as_str() {
        "arm" => false,
        "lantern" => true,
        _ => return Err(format!("{name}: style must be one of arm, lantern.")),
    };
    let height = a.bounded("height", if lantern { 3.6 } else { 6. }, 2.5..=14.)?;
    let reach = a.bounded("reach", 1.3, 0.3..=3.)?;
    let arms = a.integer("arms", 1, 1..=2)?;
    let banner = a.boolean("banner", false)?;
    let color = a.color("color", "#3d4842")?;
    let light = a.color("light", "#f2ead2")?;
    let cloth = a.color("bannerColor", "#a3352c")?;
    a.finish()?;
    if banner && height < 3.4 {
        return Err(format!("{name}: a banner needs a pole 3.4 m tall or more."));
    }
    let dark = shade(color, 0.75);
    draw(cancelled, |mesh| {
        let d = &mut on(mesh, room_pen("street-lamp", [0.; 3])?)?;
        let round = |d: &mut Fixture<'_, '_>, slot: &str, profile: &[[f64; 2]], tint: V3| {
            swept(d, slot, 0., profile, [0., TAU], 16, false, tint)
        };
        // The base plate with its four bolts, and the cast collar the pole rises from.
        let plate = LAMP_PLATE;
        d.cuboid(
            "metal",
            [-plate, -plate, 0.],
            [plate, plate, 0.025],
            dark,
            "d",
        )?;
        for (x, y) in [(-0.15, -0.15), (0.15, -0.15), (0.15, 0.15), (-0.15, 0.15)] {
            d.drum("metal", [x, y, 0.025], 0.016, 0.03, shade(dark, 0.8), 6)?;
        }
        round(
            d,
            "metal",
            &[
                [0.15, 0.025],
                [0.15, 0.09],
                [0.12, 0.13],
                [0.11, 0.62],
                [0.13, 0.66],
                [0.13, 0.7],
                [0.085, 0.76],
            ],
            color,
        )?;
        // The pole tapers from the collar to its head.
        let (foot, head) = (0.085, 0.06);
        round(d, "metal", &[[foot, 0.7], [head, height]], color)?;
        // A number plate at eye height on the pole's back.
        let plate = foot + (head - foot) * (2.32 - 0.7) / (height - 0.7);
        d.cuboid(
            "metal",
            [-0.05, plate - 0.004, 2.1],
            [0.05, plate + 0.008, 2.32],
            srgb("#d8cc8a"),
            "",
        )?;
        if banner {
            // Two brackets toward +X and a banner hung between them.
            let (low, high) = (height - 2.6, height - 0.6);
            for z in [low, high] {
                d.tube("metal", &[[0., 0., z], [0.72, 0., z]], 0.018, dark, 6)?;
                d.drum("metal", [0.72, 0., z - 0.028], 0.03, 0.056, dark, 8)?;
            }
            d.pen.sheet(
                d.mesh,
                "banner",
                &[
                    [0.1, 0., low + 0.04],
                    [0.66, 0., low + 0.04],
                    [0.66, 0., high - 0.03],
                    [0.1, 0., high - 0.03],
                ],
                &Tint::Color(cloth),
                &Tint::Color(shade(cloth, 0.9)),
                1.,
            )?;
        }
        if lantern {
            // A ring at the pole's head carries the lantern: glass tapering out to its
            // shoulder, a pyramid roof over it and a finial.
            let square = |d: &mut Fixture<'_, '_>, slot: &str, profile: &[[f64; 2]], tint: V3| {
                swept(
                    d,
                    slot,
                    0.,
                    profile,
                    [FRAC_PI_4, FRAC_PI_4 + TAU],
                    4,
                    false,
                    tint,
                )
            };
            round(
                d,
                "metal",
                &[
                    [head, height],
                    [0.09, height],
                    [0.09, height + 0.06],
                    [head, height + 0.1],
                ],
                dark,
            )?;
            square(
                d,
                "metal",
                &[
                    [0.05, height + 0.1],
                    [0.17, height + 0.1],
                    [0.17, height + 0.14],
                    [0.14, height + 0.14],
                ],
                dark,
            )?;
            square(
                d,
                "lamp",
                &[[0.15, height + 0.14], [0.23, height + 0.62]],
                light,
            )?;
            // Corner bars up the glass's edges.
            for k in 0..4 {
                let angle = FRAC_PI_4 + k as f64 * TAU / 4.;
                let at = |r: f64, z: f64| {
                    [
                        r * superspec_geometry::math::sin(angle),
                        -r * superspec_geometry::math::cos(angle),
                        z,
                    ]
                };
                d.tube(
                    "metal",
                    &[at(0.16, height + 0.14), at(0.24, height + 0.62)],
                    0.014,
                    dark,
                    5,
                )?;
            }
            square(
                d,
                "metal",
                &[
                    [0.22, height + 0.62],
                    [0.29, height + 0.64],
                    [0.29, height + 0.68],
                    [0.04, height + 0.88],
                ],
                dark,
            )?;
            d.drum("metal", [0., 0., height + 0.86], 0.03, 0.08, dark, 8)?;
            return Ok(());
        }
        // A cap on the pole's head, and swan-neck arms: up out of the head and over to
        // horizontal at their reach, with a brace beneath, each holding a shallow luminaire.
        d.drum(
            "metal",
            [0., 0., height - 0.04],
            head + 0.012,
            0.06,
            dark,
            12,
        )?;
        let rise = 0.45_f64.min(reach * 0.6);
        for side in [-1., 1.].into_iter().take(arms) {
            let curve = |t: f64| -> V3 {
                let s = 1. - t;
                // A quadratic Bezier from the head, up, then out along Y.
                let y = side * reach * t * t;
                let z = height - 0.15 + 2. * s * t * (rise + 0.15) + t * t * (rise + 0.15);
                [0., y, z]
            };
            let path: Vec<V3> = (0..=12).map(|k| curve(k as f64 / 12.)).collect();
            d.tube("metal", &path, 0.036, color, 10)?;
            let mid = curve(0.55);
            d.tube(
                "metal",
                &[[0., 0., height - 0.9], [0., mid[1] * 0.85, mid[2] - 0.06]],
                0.018,
                color,
                6,
            )?;
            // A scroll under the brace: a heavy ring.
            let ring: Vec<V3> = (0..=12)
                .map(|k| {
                    let angle = k as f64 / 12. * TAU;
                    [
                        0.,
                        side * (0.16 + 0.09 * superspec_geometry::math::cos(angle)),
                        height - 0.55 + 0.09 * superspec_geometry::math::sin(angle),
                    ]
                })
                .collect();
            d.tube("metal", &ring, 0.015, color, 6)?;
            // The luminaire: a housing that runs on past the arm's end, its diffuser below.
            let end = curve(1.);
            let (y0, y1) = if side < 0. {
                (end[1] - 0.55, end[1] + 0.12)
            } else {
                (end[1] - 0.12, end[1] + 0.55)
            };
            let z = end[2];
            d.cuboid(
                "metal",
                [-0.17, y0, z - 0.06],
                [0.17, y1, z + 0.05],
                color,
                "",
            )?;
            d.cuboid(
                "metal",
                [-0.12, y0 + 0.06, z + 0.05],
                [0.12, y1 - 0.06, z + 0.09],
                shade(color, 1.1),
                "d",
            )?;
            d.cuboid(
                "lamp",
                [-0.14, y0 + 0.03, z - 0.1],
                [0.14, y1 - 0.03, z - 0.06],
                light,
                "u",
            )?;
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
/// negative runs left, 2.7), `rise2` (m, 0–40, 0), `radius` (m, 0.005–0.05, 0.0125), `color`
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
    let radius = a.bounded("radius", 0.0125, 0.005..=0.05)?;
    let color = a.color("color", "#4d5048")?;
    let boxes = a.boolean("boxes", true)?;
    let end_box = a.bounded("endBox", 0., 0. ..=1.)?;
    // `weight`: how heavy the clips, sockets and boxes are drawn (1 their own size, 2 the
    // weight of a drawing; 1–3, 2).
    let weight = a.bounded("weight", 2., 1. ..=3.)?;
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
    })?
    .weighted(weight)?;
    draw(cancelled, |mesh| {
        let d = &mut fixture(mesh, "conduit")?;
        system.run(d, &points, Some(0.))?;
        if boxes {
            // A box grows with its conduit and with the weight, and so does its lid.
            let (grown, lid) = (
                (radius / 0.012).max(1.) * (0.8 + 0.2 * weight),
                0.006 + 0.005 * (weight - 1.),
            );
            for (index, p) in points.iter().enumerate().take(points.len() - 1) {
                let half = if index == 0 { 0.05 } else { 0.042 } * grown.min(1.6);
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
                    [p[0] + half * 0.8, y + radius + 0.012 + lid, p[2] + half],
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
/// Parameters: `reach` (m, 0.3–3, 1), `height` (m, 0.3–3, 1.6), `bars` (3–40, 9: few bold
/// bars, 2 cm thick under a 4 cm rim, read as drawn lines from the street), `color` (#3f554b).
pub(super) fn fan_divider(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let reach = a.bounded("reach", 1., 0.3..=3.)?;
    let height = a.bounded("height", 1.6, 0.3..=3.)?;
    let bars = a.integer("bars", 9, 3..=40)?;
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
            d.tube("metal", &[at(angle, 0.05), at(angle, 1.)], 0.01, color, 6)?;
        }
        let rim: Vec<V3> = (0..=20).map(|i| at(quarter * i as f64 / 20., 1.)).collect();
        d.tube("metal", &rim, 0.02, color, 8)?;
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
/// 0.2–2, 0.55), `rails` (1–4, 2), `spacing` (m between posts at most, 0.4–4, 1.6), `color`
/// (#3f554b). Its posts are 4 cm and its rails 3.6 cm thick: a drawn line against the sky.
pub(super) fn rail(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let length = a.bounded("length", 4., 0.3..=100.)?;
    let height = a.bounded("height", 0.55, 0.2..=2.)?;
    let rails = a.integer("rails", 2, 1..=4)?;
    let spacing = a.bounded("spacing", 1.6, 0.4..=4.)?;
    let color = a.color("color", "#3f554b")?;
    a.finish()?;
    draw(cancelled, |mesh| {
        let d = &mut on(mesh, room_pen("rail", [0.; 3])?)?;
        let posts = (length / spacing).ceil().max(1.) as usize;
        for i in 0..=posts {
            let x = length * i as f64 / posts as f64;
            d.tube("metal", &[[x, 0., 0.], [x, 0., height]], 0.02, color, 8)?;
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
            d.tube("metal", &[[0., 0., z], [length, 0., z]], 0.018, color, 8)?;
        }
        Ok(())
    })
}

/// `kowloon/ladder`: a steel cat ladder fixed to a wall, its stiles turning over the top onto
/// the roof behind. Anchor: the wall plane below the ladder's centre, at its foot; it stands
/// 0.16 m in front of the wall. Parameters: `height` (m, 0.6–20, 2.2), `width` (m, 0.25–1,
/// 0.42), `color` (#3f554b) and `grab` (m, 0–1.5, 0): grab rails, the stiles carried this far
/// above the top and bent over in a hoop down onto the roof behind (0: they turn straight over).
/// Its stiles and grab rails are 4 cm thick, its rungs 2.4 cm and 0.34 m apart, and its wall
/// stays 2.4 cm: bolder than a real ladder's, so they read as drawn lines.
pub(super) fn ladder(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let height = a.bounded("height", 2.2, 0.6..=20.)?;
    let width = a.bounded("width", 0.42, 0.25..=1.)?;
    let color = a.color("color", "#3f554b")?;
    let grab = a.bounded("grab", 0., 0. ..=1.5)?;
    if grab > 0. && grab < 0.3 {
        return Err(format!("{name}: grab must be 0 or 0.3..1.5."));
    }
    a.finish()?;
    draw(cancelled, |mesh| {
        let d = &mut fixture(mesh, "ladder")?;
        let y = 0.16;
        for x in [-width / 2., width / 2.] {
            if grab > 0. {
                // Up past the top, over a hoop of 0.14 m radius and down to the roof 0.12 m
                // behind the wall, with a stay to the wall's head.
                let (radius, crown) = (0.14, height + grab - 0.14);
                let mut path = vec![[x, y, 0.05]];
                path.extend((0..=8).map(|i| {
                    let angle = std::f64::consts::PI * i as f64 / 8.;
                    [
                        x,
                        y - radius + radius * superspec_geometry::math::cos(angle),
                        crown + radius * superspec_geometry::math::sin(angle),
                    ]
                }));
                path.push([x, y - 2. * radius, height + 0.02]);
                d.tube("metal", &path, 0.02, color, 10)?;
                d.tube(
                    "metal",
                    &[[x, y, height - 0.08], [x, 0., height - 0.08]],
                    0.012,
                    color,
                    6,
                )?;
            } else {
                d.tube(
                    "metal",
                    &[
                        [x, y, 0.05],
                        [x, y, height - 0.1],
                        [x, -0.12, height + 0.02],
                    ],
                    0.02,
                    color,
                    10,
                )?;
            }
            for z in [0.5, height - 0.5] {
                d.tube("metal", &[[x, y, z], [x, 0., z]], 0.012, color, 6)?;
            }
        }
        let rungs = ((height - 0.4) / 0.34) as usize;
        for i in 0..=rungs {
            let z = 0.28 + i as f64 * 0.34;
            if z < height - 0.08 {
                d.tube(
                    "metal",
                    &[[-width / 2., y, z], [width / 2., y, z]],
                    0.012,
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
            0.022,
            steel,
            8,
        )?;
        d.cuboid(
            "metal",
            [x - 0.07, depth - 0.11, ground],
            [x + 0.07, depth + 0.03, ground + 0.02],
            shade(steel, 0.8),
            "d",
        )?;
    }
    Ok(())
}

/// Keep all six faces in one material receiver. The concrete program measures the
/// member's top, foot, and convex creases across the boundaries between faces.
pub(super) fn concrete_member(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let width = a.bounded("width", 0.5, 0.05..=40.)?;
    let depth = a.bounded("depth", 0.5, 0.05..=40.)?;
    let height = a.bounded("height", 3., 0.05..=30.)?;
    a.finish()?;
    draw(cancelled, |mesh| {
        on(mesh, room_pen("concrete-member", [0.; 3])?)?.cuboid(
            "concrete",
            [0.; 3],
            [width, depth, height],
            [1.; 3],
            "",
        )
    })
}

/// A carriageway segment with its near corner at the wearing surface.
pub(super) fn road(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let length = a.bounded("length", 32., 0.1..=200.)?;
    let width = a.bounded("width", 7.3, 0.1..=50.)?;
    a.finish()?;
    draw(cancelled, |mesh| {
        on(mesh, room_pen("road", [0.; 3])?)?.cuboid(
            "asphalt",
            [0., -width, -0.1],
            [length, 0., 0.],
            srgb("#545453"),
            "",
        )
    })
}

/// Static street-tree item, using TreeSpec's trunk, branches and backing canopy.
pub(super) fn street_tree(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    use crate::{itemspec::definition::ItemMesh, treespec::tree};
    Arguments::new(name, values).finish()?;
    let def = tree::parse(
        "street.tree.json",
        include_str!("../../../../treespec/fixtures/street.tree.json"),
    )
    .map_err(|e| e.to_string())?;
    let tree = tree::generate(&def, cancelled).map_err(|e| e.to_string())?;
    let lod = &tree.lods[1];
    let crown = lod
        .hull_mesh
        .ok_or("Street tree requires a backing canopy.")?;
    let mut meshes = vec![];
    for i in [Some(lod.trunk), lod.branches, Some(crown)]
        .into_iter()
        .flatten()
    {
        let m = &tree.meshes[i];
        let tint = srgb(if i == crown { "#618246" } else { "#766857" });
        meshes.push(ItemMesh {
            id: m.id.clone(),
            positions: m.positions.clone(),
            normals: m.normals.clone(),
            uvs: m.uvs.clone(),
            indices: m.indices.clone(),
            colors: vec![[tint[0], tint[1], tint[2], 1.]; m.positions.len()],
            material_slot: m.material_slot.clone(),
            additional_uvs: Default::default(),
            tangents: vec![],
        });
    }
    Ok(NativeOutput {
        meshes,
        attachments: vec![],
    })
}
