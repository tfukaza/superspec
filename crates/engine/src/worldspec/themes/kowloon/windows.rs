//! Window construction, seeded household state and scoped material receivers.
use super::{
    fasteners::bolt,
    fixture::Fixture,
    palette::{shade, srgb},
    pen::Tint,
};
use serde::Deserialize;
use superspec_geometry::{
    V3,
    math::{cos, sin},
};
use superspec_types::rng::SeededRng;

const FRAME_DEPTH: f64 = 0.07;
fn textured_timber() -> bool {
    true
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Casement,
    Slider,
    Timber,
    Louver,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Look {
    pub weathering: Option<super::weathered::Settings>,
    pub kind: Kind,
    pub frame: V3,
    pub trim: V3,
    pub curtain: V3,
    pub reach: f64,
    pub hood: bool,
    pub lit: bool,
    pub boarded: bool,
    #[serde(default)]
    pub fixed: bool,
    #[serde(default)]
    pub recess: f64,
    pub transom_ratio: Option<f64>,
    pub lower_pane_bars: Option<Vec<f64>>,
    /// Whether the bound shop-timber material has an atlas; authored untextured overrides stay untextured.
    #[serde(default = "textured_timber")]
    pub timber_textured: bool,
}

/// Which top light of a bank is a louvre of glass slats instead of a pane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Louvre {
    Left,
    Right,
}
/// What a caller may fix beyond a [`Look`]; the default draws exactly the look's window.
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// How many lights (sliding leaves) stand open: the whole part is a count and the
    /// fraction the chance of one more. `None` is the usual single light, three times in ten.
    /// Casement lights then open side-hung or, seeded, top-hung.
    pub open: Option<f64>,
    /// The width a casement's lights aim at (0.5 m when `None`).
    pub light_width: Option<f64>,
    /// A rail as heavy as the transom halfway up the lower lights, instead of their glazing bars.
    pub mid_rail: bool,
    pub louvre: Option<Louvre>,
    /// Lever handles on the closed casement lights.
    pub handles: bool,
    /// The tint of a lit room's back panel (the unlit room's dark when `None`), and the slot
    /// it is drawn in instead of `interior-lit`.
    pub glow: Option<V3>,
    pub glow_slot: Option<&'static str>,
    /// For a lit window, the depth (m) of a room drawn behind its recess instead of the flat
    /// back panel: see [`lit_room`].
    pub room: Option<f64>,
}

/// The slot of a lit room's walls, ceiling, floor and lamp batten, coloured per vertex in the
/// warm tint of its light: a host leaves it plain or binds a softly glowing material.
pub const ROOM_LIT_SLOT: &str = "room-lit";

/// A lit room `depth` deep behind a window's recess, seen through its lights `span`: back,
/// side walls and ceiling (and a floor) reaching past the opening, darker away from a tube
/// batten hung at the opening's head, in [`ROOM_LIT_SLOT`] tinted from `light` (the lamp's
/// colour); the tube in `tube-light`; and, as plain silhouettes in `stock`, a cupboard or shelf
/// against the back wall and curtains (`curtains`: the share of the width each covers) or a
/// part-lowered blind on the opening.
fn lit_room(
    drawing: &mut Fixture<'_, '_>,
    span: [f64; 4],
    recess: f64,
    depth: f64,
    light: V3,
    curtain: V3,
    curtains: Option<f64>,
    rng: &mut SeededRng,
) -> Result<(), String> {
    let [x0, x1, z0, z1] = span;
    let slot = ROOM_LIT_SLOT;
    let (front, back) = (-recess - 0.01, -recess - depth);
    // The room reaches past the opening on either side, from its floor below the sill to a
    // ceiling above the head.
    let side = rng.float(0.2, 0.42);
    let (left, right) = (x0 - side, x1 + side * rng.float(0.8, 1.2));
    // A low ceiling just over the head, so its lamp hangs into view through the opening.
    let (floor, ceiling) = (z0 - 0.85, z1 + rng.float(0.08, 0.16));
    let lamp = [(left + right) / 2. + rng.float(-0.2, 0.2), back * 0.42];
    // Old distemper, yellowed: darker than the render outside, warm in the lamp's light.
    let walls = shade(
        srgb(rng.pick(&["#c49a68", "#b99468", "#c4a074", "#b3926c"])),
        rng.float(0.66, 0.78),
    );
    // Half the lamp's colour: warm, not orange.
    let tint = |base: V3| -> V3 { std::array::from_fn(|k| base[k] * (0.5 + 0.5 * light[k])) };
    let (walls, ceiling_tint, floor_tint) = (
        tint(walls),
        tint(shade(walls, 1.08)),
        tint(shade(srgb("#7a6550"), 0.8)),
    );
    // Brightest under the lamp, falling off with distance and towards the floor.
    let falloff = move |p: V3, base: V3| -> Result<V3, String> {
        let dx = p[0] - lamp[0];
        let dy = p[1] - lamp[1];
        let dz = (ceiling - p[2]).max(0.) * 1.4;
        let reach = (dx * dx + dy * dy + dz * dz).sqrt();
        let k = (1.25 - reach * 0.32).clamp(0.3, 1.25);
        Ok(shade(base, k))
    };
    let repeat = drawing.repeat(slot);
    // A surface split into `cols` by `rows` quads so its shading varies across it.
    let surface = |drawing: &mut Fixture<'_, '_>,
                   corner: V3,
                   u: V3,
                   v: V3,
                   [cols, rows]: [usize; 2],
                   normal: V3,
                   base: V3|
     -> Result<(), String> {
        let field = |p: V3| falloff(p, base);
        let at = |i: usize, j: usize| -> V3 {
            let (a, b) = (i as f64 / cols as f64, j as f64 / rows as f64);
            std::array::from_fn(|k| corner[k] + u[k] * a + v[k] * b)
        };
        for i in 0..cols {
            for j in 0..rows {
                drawing.pen.face(
                    drawing.mesh,
                    slot,
                    &[at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)],
                    normal,
                    &Tint::Field(&field),
                    repeat,
                    None,
                    None,
                )?;
            }
        }
        Ok(())
    };
    let (width, high, long) = (right - left, ceiling - floor, back - front);
    surface(
        drawing,
        [left, back, floor],
        [width, 0., 0.],
        [0., 0., high],
        [3, 2],
        [0., 1., 0.],
        walls,
    )?;
    for (x, normal, base) in [
        (left, [1., 0., 0.], shade(walls, 0.9)),
        (right, [-1., 0., 0.], shade(walls, 0.84)),
    ] {
        surface(
            drawing,
            [x, front, floor],
            [0., long, 0.],
            [0., 0., high],
            [2, 2],
            normal,
            base,
        )?;
    }
    surface(
        drawing,
        [left, front, ceiling],
        [width, 0., 0.],
        [0., long, 0.],
        [3, 2],
        [0., 0., -1.],
        ceiling_tint,
    )?;
    surface(
        drawing,
        [left, front, floor],
        [width, 0., 0.],
        [0., long, 0.],
        [1, 1],
        [0., 0., 1.],
        floor_tint,
    )?;
    // A cupboard or a shelf of things against the back wall, darker than the wall.
    let piece = rng.float(0.5, 1.1).min(width * 0.45);
    let at = rng.float(left + 0.05, right - piece - 0.05);
    let tall = rng.float(0.9, 1.9).min(high - 0.1);
    let wood = tint(shade(
        srgb(rng.pick(&["#6a5640", "#5c5a52", "#7a6448"])),
        0.75,
    ));
    // Furniture and curtains stand against the light as silhouettes, plain colours unlit by
    // it, in the slot of the goods a storeroom's window shows.
    let figure = "stock";
    drawing.cuboid(
        figure,
        [at, back + 0.01, floor],
        [at + piece, back + 0.45, floor + tall],
        wood,
        "bd",
    )?;
    if rng.chance(0.6) {
        // Things on its top.
        let mut x = at + 0.04;
        while x < at + piece - 0.12 {
            drawing.mesh.spend_work(1)?;
            let (w, h) = (rng.float(0.08, 0.22), rng.float(0.08, 0.3));
            drawing.cuboid(
                figure,
                [x, back + 0.05, floor + tall],
                [x + w, back + 0.3, floor + tall + h],
                tint(shade(
                    srgb(rng.pick(&["#8a6a4c", "#a89a80", "#6a7072", "#9a5a48"])),
                    rng.float(0.6, 0.9),
                )),
                "bd",
            )?;
            x += w + rng.float(0.02, 0.12);
        }
    }
    // The fixture: an enamel batten on two rods, at about the opening's head so it shows from
    // the street, with its tube in the lit lamps' slot.
    let length = (width * 0.5).clamp(0.6, 1.2);
    let [lx, ly] = lamp;
    let hung = (z1 - rng.float(0., 0.08)).min(ceiling - 0.05);
    let enamel = tint(srgb("#e0ddd2"));
    drawing.cuboid(
        slot,
        [lx - length / 2., ly - 0.035, hung - 0.04],
        [lx + length / 2., ly + 0.035, hung],
        enamel,
        "",
    )?;
    for end in [-1., 1.] {
        let x = lx + end * (length / 2. - 0.1);
        drawing.bar(
            slot,
            [x, ly, hung],
            [x, ly, ceiling],
            0.012,
            shade(enamel, 0.6),
        )?;
    }
    let pen = drawing.pen.into_part("tube-emitter")?;
    let mut emitter = Fixture {
        pen,
        mesh: &mut *drawing.mesh,
        repeats: drawing.repeats,
    };
    emitter.tube(
        "tube-light",
        &[
            [lx - length / 2. + 0.04, ly, hung - 0.06],
            [lx + length / 2. - 0.04, ly, hung - 0.06],
        ],
        0.015,
        [1., 0.98, 0.92],
        6,
    )?;
    // Curtains drawn to the sides, in folds, lit from behind; or a blind lowered part way.
    let fabric = tint(shade(curtain, 0.55));
    let hang = -recess - 0.06;
    if let Some(share) = curtains {
        let w = (x1 - x0) * share;
        let starts: &[f64] = if rng.chance(0.5) {
            &[x0, x1 - w]
        } else if rng.chance(0.5) {
            &[x0]
        } else {
            &[x1 - w]
        };
        for &start in starts {
            for fold in 0..3 {
                let a = start + w * fold as f64 / 3.;
                let b = start + w * (fold + 1) as f64 / 3.;
                let (ya, yb) = (
                    hang - (fold % 2) as f64 * 0.03,
                    hang - ((fold + 1) % 2) as f64 * 0.03,
                );
                drawing.sheet(
                    figure,
                    &[
                        [a, ya, z0 - 0.1],
                        [b, yb, z0 - 0.1],
                        [b, yb, z1 + 0.08],
                        [a, ya, z1 + 0.08],
                    ],
                    shade(fabric, if fold % 2 == 0 { 1. } else { 0.85 }),
                )?;
            }
        }
    }
    if rng.chance(0.3) {
        let low = z1 - (z1 - z0) * rng.float(0.15, 0.32);
        let blind = tint(shade(
            srgb(rng.pick(&["#d8ccaa", "#c9b98e", "#b9c0b0"])),
            0.7,
        ));
        drawing.sheet(
            figure,
            &[
                [x0 - 0.02, hang - 0.04, low],
                [x1 + 0.02, hang - 0.04, low],
                [x1 + 0.02, hang - 0.04, z1 + 0.08],
                [x0 - 0.02, hang - 0.04, z1 + 0.08],
            ],
            blind,
        )?;
        drawing.cuboid(
            figure,
            [x0 - 0.02, hang - 0.055, low - 0.02],
            [x1 + 0.02, hang - 0.025, low],
            shade(blind, 0.7),
            "",
        )?;
    }
    Ok(())
}

fn interior(
    drawing: &mut Fixture<'_, '_>,
    span: [f64; 4],
    look: &Look,
    options: &Options,
    rng: &mut SeededRng,
) -> Result<(), String> {
    let [x0, x1, z0, z1] = span;
    let depth = look.recess;
    if let (true, Some(room)) = (look.lit, options.room) {
        // The shared stream advances as for a flat back, so the sashes open alike.
        let curtained = rng.chance(0.6);
        let share = rng.float(0.12, 0.24);
        let mut own = SeededRng::from_text(&format!("{}/{x0}/{z0}/lit-room", drawing.pen.part));
        return lit_room(
            drawing,
            span,
            depth,
            room,
            options.glow.unwrap_or(srgb("#ffe7ad")),
            look.curtain,
            curtained.then_some(share * 1.4),
            &mut own,
        );
    }
    drawing.panel(
        if look.lit {
            options.glow_slot.unwrap_or("interior-lit")
        } else {
            "pane"
        },
        span,
        if depth > 0. { -depth + 0.003 } else { 0.012 },
        options.glow.filter(|_| look.lit).unwrap_or(srgb("#1b2225")),
    )?;
    let curtained = rng.chance(0.6);
    let width = (x1 - x0) * rng.float(0.12, 0.24);
    let mut appearance =
        SeededRng::from_text(&format!("{}/{x0}/{z0}/window-interior", drawing.pen.part));
    if depth >= 0.2 && appearance.chance(0.65) {
        let width = 0.48_f64.min((x1 - x0) * appearance.float(0.24, 0.48));
        let start = appearance.float(x0 + 0.02, (x1 - width - 0.02).max(x0 + 0.02));
        let fabric = shade(
            srgb(appearance.pick(&["#777f76", "#948a70", "#657278"])),
            appearance.float(0.3, 0.65),
        );
        for fold in 0..4 {
            let a = start + width * fold as f64 / 4.;
            let b = start + width * (fold + 1) as f64 / 4.;
            let ya = -depth * 0.82 + (fold % 2) as f64 * 0.012;
            let yb = -depth * 0.82 + ((fold + 1) % 2) as f64 * 0.012;
            drawing.sheet(
                "paint",
                &[[a, ya, z0], [b, yb, z0], [b, yb, z1], [a, ya, z1]],
                shade(fabric, if fold % 2 == 0 { 1. } else { 0.88 }),
            )?;
        }
    }
    if depth < 0.2 || !curtained {
        return Ok(());
    }
    for start in [x0, x1 - width] {
        for fold in 0..3 {
            let a = start + width * fold as f64 / 3.;
            let b = start + width * (fold + 1) as f64 / 3.;
            let y0 = -depth * 0.65 + (fold % 2) as f64 * 0.025;
            let y1 = -depth * 0.65 + ((fold + 1) % 2) as f64 * 0.025;
            drawing.sheet(
                "paint",
                &[[a, y0, z0], [b, y1, z0], [b, y1, z1], [a, y0, z1]],
                shade(look.curtain, 0.24),
            )?;
        }
    }
    Ok(())
}

/// Draws the complete fixture into an owned host job; errors discard that job.
/// Span is [left, right, bottom, top] in the facade's local X/Z plane.
pub fn window(
    drawing: &mut Fixture<'_, '_>,
    span: [f64; 4],
    look: &Look,
    rng: &mut SeededRng,
) -> Result<(), String> {
    let [x, x1, z, z1] = span;
    window_sized(drawing, [x, z, x1 - x, z1 - z], look, rng)
}

/// Origin and dimensions [X, Z, width, height] retain exact branch boundaries such as 1.7 m sliders.
pub fn window_sized(
    drawing: &mut Fixture<'_, '_>,
    rectangle: [f64; 4],
    look: &Look,
    rng: &mut SeededRng,
) -> Result<(), String> {
    window_with(drawing, rectangle, look, &Options::default(), rng)
}

/// [`window_sized`] with whatever `options` fix of it.
pub fn window_with(
    drawing: &mut Fixture<'_, '_>,
    rectangle: [f64; 4],
    look: &Look,
    options: &Options,
    rng: &mut SeededRng,
) -> Result<(), String> {
    if options.open.is_some_and(|v| !(0. ..=16.).contains(&v))
        || options
            .light_width
            .is_some_and(|v| !(0.15..=10.).contains(&v))
        || options
            .glow
            .is_some_and(|c| c.iter().any(|v| !v.is_finite() || *v < 0.))
    {
        return Err("Window options need a bounded open count, light width and glow.".into());
    }
    let [x, z, w, h] = rectangle;
    let (x1, z1) = (x + w, z + h);
    let span = [x, x1, z, z1];
    if span
        .iter()
        .chain(&look.frame)
        .chain(&look.trim)
        .chain(&look.curtain)
        .any(|v| !v.is_finite())
        || look
            .frame
            .iter()
            .chain(&look.trim)
            .chain(&look.curtain)
            .any(|v| *v < 0.)
        || !look.reach.is_finite()
        || !look.recess.is_finite()
        || look.reach < 0.
        || look.recess < 0.
        || !(0.2..=100.).contains(&w)
        || !(0.2..=100.).contains(&h)
    {
        return Err(
            "Window requires bounded positive dimensions, finite nonnegative tints and clearance."
                .into(),
        );
    }
    let member = if look.kind == Kind::Timber {
        0.065
    } else {
        0.045
    };
    let inner_bottom = z + member;
    let inner_top = z + h - member;
    if look.kind == Kind::Slider && (w - 2. * member) / if w > 1.7 { 3. } else { 2. } <= 0.06 + 1e-9
    {
        return Err(
            "Slider leaves require positive glazing clearance inside their perimeter frames."
                .into(),
        );
    }
    let transom = look
        .transom_ratio
        .map_or(if h >= 1. { z + h * 0.72 } else { inner_top }, |r| {
            z + h * r
        });
    let defaults: &[f64] = if look.kind == Kind::Timber {
        &[0.34, 0.67]
    } else {
        &[0.5]
    };
    let bars = look.lower_pane_bars.as_deref().unwrap_or(defaults);
    if look.transom_ratio.is_some_and(|r| {
        !r.is_finite()
            || r <= 0.
            || r >= 1.
            || transom <= inner_bottom + 0.04
            || transom >= inner_top - 0.04
    }) {
        return Err(
            "A window transom must leave positive glazing clearance above and below its frame."
                .into(),
        );
    }
    if look.lower_pane_bars.is_some() {
        if bars.len() > 4096 || bars.iter().any(|r| !r.is_finite() || *r <= 0. || *r >= 1.) {
            return Err("Invalid window lower-pane bars.".into());
        }
        let mut centers: Vec<_> = bars
            .iter()
            .map(|r| inner_bottom + (transom - inner_bottom) * r)
            .collect();
        centers.sort_by(f64::total_cmp);
        if centers.iter().enumerate().any(|(i, c)| {
            *c <= inner_bottom + 0.024
                || *c >= transom - 0.024
                || (i > 0 && *c - centers[i - 1] <= 0.024)
        }) {
            return Err("Window lower-pane bars must be distinct, separated and clear of their perimeter frames.".into());
        }
    }
    let recess = look.recess;
    drawing.molding(
        "concrete",
        [x - 0.07, x1 + 0.07],
        &[
            [-recess, z - 0.07],
            [0.09, z - 0.07],
            [0.115, z - 0.045],
            [0.115, z - 0.02],
            [-recess, z],
        ],
        look.trim,
    )?;
    drawing.panel(
        "paint",
        [x - 0.04, x1 + 0.04, z - 0.042, z - 0.036],
        0.1155,
        shade(look.trim, 0.5),
    )?;
    if look.hood && look.reach >= 0.3 {
        drawing.molding(
            "concrete",
            [x - 0.12, x1 + 0.12],
            &[
                [0., z1 + 0.05],
                [0.25, z1 + 0.05],
                [0.28, z1 + 0.08],
                [0.28, z1 + 0.105],
                [0., z1 + 0.13],
            ],
            look.trim,
        )?;
        drawing.cuboid(
            "concrete",
            [x - 0.12, 0.25, z1 + 0.02],
            [x1 + 0.12, 0.28, z1 + 0.05],
            shade(look.trim, 0.85),
            "bu",
        )?;
    }
    let original = drawing.pen.clone();
    let mut frame = original.moved([0., if recess > 0. { -0.035 } else { 0. }, 0.])?;
    if look.kind == Kind::Timber {
        frame = frame.with_timber(super::timber::Settings {
            slots: vec!["paint".into()],
            textured: look.timber_textured,
            front_wear: Some(super::timber::FrontWear {
                width: 0.008,
                strength: look
                    .weathering
                    .as_ref()
                    .map_or(0.8, |s| (s.wear * 4.).min(1.2)),
                direction: Some([0., 1., 0.]),
            }),
            timber_repeat: if drawing.mesh.surveying() {
                1.
            } else {
                (drawing.repeats)("timber")
            },
            atlas_repeat: if drawing.mesh.surveying() {
                1.
            } else {
                (drawing.repeats)("shop-timber")
            },
            ..Default::default()
        })?;
    } else if let Some(settings) = &look.weathering {
        frame = frame.with_weathering(
            drawing.mesh,
            settings.clone(),
            super::metal::Kind::Frame,
            None,
            vec!["paint".into(), "metal".into()],
        )?;
    }
    drawing.pen = frame;
    // Restore the caller's frame on every exit, including cancellation/admission failure.
    let result = frame_and_leaves(
        drawing,
        span,
        [w, h],
        look,
        options,
        rng,
        member,
        transom,
        bars,
        &original,
    );
    drawing.pen = original;
    result
}

#[allow(clippy::too_many_arguments)]
fn frame_and_leaves(
    drawing: &mut Fixture<'_, '_>,
    span: [f64; 4],
    dimensions: [f64; 2],
    look: &Look,
    options: &Options,
    rng: &mut SeededRng,
    member: f64,
    transom: f64,
    bars: &[f64],
    original: &super::pen::Pen,
) -> Result<(), String> {
    let [x, x1, z, z1] = span;
    let [w, h] = dimensions;
    let d = FRAME_DEPTH;
    drawing.frame_box([x, 0., z], [x + member, d, z1], look.frame)?;
    drawing.frame_box([x1 - member, 0., z], [x1, d, z1], look.frame)?;
    drawing.frame_box(
        [x + member, 0., z],
        [x1 - member, d, z + member],
        look.frame,
    )?;
    drawing.frame_box(
        [x + member, 0., z1 - member],
        [x1 - member, d, z1],
        look.frame,
    )?;
    let inner = [x + member, x1 - member, z + member, z1 - member];
    let bead = shade(look.frame, 0.72);
    for u in [inner[0] - 0.012, inner[1]] {
        drawing.frame_box(
            [u, d - 0.015, inner[2]],
            [u + 0.012, d + 0.008, inner[3]],
            bead,
        )?;
    }
    for v in [inner[2] - 0.012, inner[3]] {
        drawing.molding(
            "paint",
            [inner[0], inner[1]],
            &[
                [d - 0.015, v],
                [d + 0.008, v],
                [d + 0.004, v + 0.012],
                [d - 0.015, v + 0.012],
            ],
            bead,
        )?;
    }
    if matches!(look.kind, Kind::Casement | Kind::Timber) {
        for v in [z + h * 0.2, z + h * 0.66] {
            drawing.frame_box(
                [x + 0.006, d - 0.004, v - 0.006],
                [x + member - 0.004, d + 0.002, v + 0.08],
                look.frame,
            )?;
            drawing.tube(
                "metal",
                &[
                    [x + member * 0.5, d + 0.005, v],
                    [x + member * 0.5, d + 0.005, v + 0.075],
                ],
                0.009,
                shade(look.frame, 0.6),
                10,
            )?;
            for at in [v + 0.01, v + 0.065] {
                bolt(
                    drawing,
                    "metal",
                    [x + member * 0.25, d + 0.002, at],
                    [0., 1., 0.],
                    0.003,
                    look.frame,
                    look.weathering.as_ref(),
                )?;
            }
        }
    }
    if look.boarded {
        let board = srgb("#8a7a5c");
        drawing.panel("timber", inner, 0.03, board)?;
        drawing.bar(
            "timber",
            [inner[0], 0.045, inner[2]],
            [inner[1], 0.045, inner[3]],
            0.035,
            shade(board, 0.8),
        )?;
        drawing.bar(
            "timber",
            [inner[0], 0.045, inner[3]],
            [inner[1], 0.045, inner[2]],
            0.035,
            shade(board, 0.8),
        )?;
        return Ok(());
    }
    let moved = drawing.pen.clone();
    drawing.pen = original.clone();
    let result = interior(drawing, inner, look, options, rng);
    drawing.pen = moved;
    result?;
    if look.kind == Kind::Louver {
        let slats = ((inner[3] - inner[2]) / 0.11).round().max(3.) as usize;
        let pitch = (inner[3] - inner[2]) / slats as f64;
        for i in 0..slats {
            let base = inner[2] + i as f64 * pitch;
            drawing.sheet(
                "glass",
                &[
                    [inner[0], 0.02, base + pitch * 0.92],
                    [inner[1], 0.02, base + pitch * 0.92],
                    [inner[1], d, base + pitch * 0.12],
                    [inner[0], d, base + pitch * 0.12],
                ],
                shade(srgb("#2a3a40"), 2.6),
            )?;
        }
        return Ok(());
    }
    if look.kind == Kind::Slider {
        let leaves = if w > 1.7 { 3 } else { 2 };
        let pitch = (inner[1] - inner[0]) / leaves as f64;
        let open = opened(options, !look.fixed, leaves, rng);
        for y in [d * 0.3, d * 0.7] {
            drawing.frame_box(
                [inner[0], y - 0.004, inner[2]],
                [inner[1], y + 0.004, inner[2] + 0.012],
                bead,
            )?;
            drawing.frame_box(
                [inner[0], y - 0.004, inner[3] - 0.012],
                [inner[1], y + 0.004, inner[3]],
                bead,
            )?;
        }
        for i in 0..leaves {
            let l0 = inner[0] + i as f64 * pitch;
            let l1 = l0 + pitch;
            let y = if i % 2 != 0 { d * 0.4 } else { d * 0.8 };
            if open.contains(&i) {
                continue;
            }
            drawing.frame_box(
                [l0, y - 0.02, inner[2]],
                [l0 + 0.03, y, inner[3]],
                look.frame,
            )?;
            drawing.frame_box(
                [l1 - 0.03, y - 0.02, inner[2]],
                [l1, y, inner[3]],
                look.frame,
            )?;
            drawing.frame_box(
                [l0, y - 0.02, inner[2]],
                [l1, y, inner[2] + 0.03],
                look.frame,
            )?;
            drawing.frame_box(
                [l0, y - 0.02, inner[3] - 0.03],
                [l1, y, inner[3]],
                look.frame,
            )?;
            drawing.panel(
                "glass",
                [l0 + 0.03, l1 - 0.03, inner[2] + 0.03, inner[3] - 0.03],
                y - 0.006,
                [0.88, 0.94, 0.93],
            )?;
            let latch = (inner[2] + inner[3]) / 2.;
            drawing.frame_box(
                [l1 - 0.026, y, latch - 0.045],
                [l1 - 0.008, d.min(y + 0.008), latch + 0.045],
                bead,
            )?;
        }
        return Ok(());
    }
    let lights = if look.kind == Kind::Timber {
        2
    } else {
        ((inner[1] - inner[0]) / options.light_width.unwrap_or(0.5))
            .round()
            .max(2.) as usize
    };
    let pitch = (inner[1] - inner[0]) / lights as f64;
    let open = opened(
        options,
        !look.fixed && look.reach >= 0.6 + look.recess,
        lights,
        rng,
    );
    // A mid rail takes the glazing bars' place, and an opened sash is then the light above it.
    let middle = inner[2] + (transom - inner[2]) * 0.5;
    let railed = options.mid_rail && transom - inner[2] > 0.3;
    let bars = if railed { &[][..] } else { bars };
    let foot = if railed { middle + 0.018 } else { inner[2] };
    let louvre = options
        .louvre
        .filter(|_| transom + 0.1 < inner[3])
        .map(|side| match side {
            Louvre::Left => 0,
            Louvre::Right => lights - 1,
        });
    for i in 0..lights {
        let l0 = inner[0] + i as f64 * pitch;
        let l1 = l0 + pitch;
        if i > 0 {
            drawing.frame_box(
                [l0 - 0.017, 0., inner[2]],
                [l0 + 0.017, d * 0.85, inner[3]],
                look.frame,
            )?;
        }
        if open.contains(&i) && options.open.is_some() && rng.chance(0.5) {
            // Top-hung: hinged under the transom, pushed out at its foot on a stay.
            let drop = transom - foot;
            let out = sin(rng.float(0.3, 0.6)).min(0.45 / drop);
            let tip = [d + out * drop, transom - (1. - out * out).sqrt() * drop];
            let (a, b) = (l0 + 0.018, l1 - 0.018);
            drawing.sheet(
                "glass",
                &[
                    [a, d, transom],
                    [b, d, transom],
                    [b, tip[0], tip[1]],
                    [a, tip[0], tip[1]],
                ],
                [0.85, 0.9, 0.9],
            )?;
            for x in [a + 0.012, b - 0.012] {
                drawing.bar(
                    "paint",
                    [x, d, transom - 0.015],
                    [x, tip[0], tip[1]],
                    0.03,
                    look.frame,
                )?;
            }
            drawing.bar(
                "paint",
                [a, tip[0], tip[1]],
                [b, tip[0], tip[1]],
                0.03,
                look.frame,
            )?;
            drawing.tube(
                "metal",
                &[[b - 0.03, d, foot + 0.02], [b - 0.03, tip[0], tip[1]]],
                0.004,
                shade(look.frame, 0.6),
                5,
            )?;
        } else if open.contains(&i) {
            let angle = rng.float(0.7, 1.3);
            let left = (i as f64) < lights as f64 / 2.;
            let hinge = if left { l0 } else { l1 };
            let swing = if left { 1. } else { -1. } * cos(angle) * pitch;
            let tip = [hinge + swing, d + sin(angle) * pitch];
            drawing.sheet(
                "glass",
                &[
                    [hinge, d, foot],
                    [tip[0], tip[1], foot],
                    [tip[0], tip[1], transom],
                    [hinge, d, transom],
                ],
                [0.85, 0.9, 0.9],
            )?;
            for zb in [foot + 0.015, transom - 0.015] {
                drawing.bar(
                    "paint",
                    [hinge, d, zb],
                    [tip[0], tip[1], zb],
                    0.03,
                    look.frame,
                )?;
            }
            drawing.bar(
                "paint",
                [tip[0], tip[1], foot],
                [tip[0], tip[1], transom],
                0.03,
                look.frame,
            )?;
        }
        if Some(i) == louvre {
            // Glass slats in clips down both sides, tilted to shed rain.
            let (a, b, z0, z1) = (l0 + 0.018, l1 - 0.018, transom + 0.02, inner[3] - 0.012);
            let slats = ((z1 - z0) / 0.075).round().max(2.) as usize;
            let rise = (z1 - z0) / slats as f64;
            for slat in 0..slats {
                let base = z0 + slat as f64 * rise;
                drawing.sheet(
                    "glass",
                    &[
                        [a, 0.015, base + rise * 0.95],
                        [b, 0.015, base + rise * 0.95],
                        [b, d, base + rise * 0.1],
                        [a, d, base + rise * 0.1],
                    ],
                    shade(srgb("#2a3a40"), 2.6),
                )?;
            }
            for x in [a, b - 0.012] {
                drawing.frame_box([x, 0.01, z0], [x + 0.012, d, z1], bead)?;
            }
        } else if transom < inner[3] {
            drawing.panel(
                "glass",
                [l0 + 0.018, l1 - 0.018, transom + 0.02, inner[3] - 0.012],
                d * 0.4 - 0.003,
                [0.88, 0.94, 0.93],
            )?;
        }
        if open.contains(&i) {
            if railed {
                drawing.panel(
                    "glass",
                    [l0 + 0.018, l1 - 0.018, inner[2] + 0.012, middle - 0.02],
                    d * 0.4 - 0.003,
                    [0.88, 0.94, 0.93],
                )?;
            }
            continue;
        }
        drawing.panel(
            "glass",
            [l0 + 0.018, l1 - 0.018, inner[2] + 0.012, transom - 0.02],
            d * 0.4 - 0.003,
            [0.88, 0.94, 0.93],
        )?;
        let latch = inner[2] + (transom - inner[2]) * 0.44;
        drawing.frame_box(
            [l1 - 0.024, d * 0.85, latch - 0.032],
            [l1 - 0.006, d, latch + 0.032],
            bead,
        )?;
        if options.handles {
            drawing.tube(
                "metal",
                &[
                    [l1 - 0.015, d, latch + 0.012],
                    [l1 - 0.015, d + 0.024, latch + 0.012],
                    [l1 - 0.015, d + 0.024, latch - 0.075],
                ],
                0.0055,
                srgb("#8d8468"),
                6,
            )?;
        }
        for t in bars {
            let bar = inner[2] + (transom - inner[2]) * t;
            drawing.frame_box(
                [l0, 0.02, bar - 0.012],
                [l1, d * 0.6, bar + 0.012],
                look.frame,
            )?;
        }
    }
    if transom < inner[3] {
        drawing.frame_box(
            [inner[0], 0., transom - 0.018],
            [inner[1], d * 0.9, transom + 0.018],
            look.frame,
        )?;
    }
    if railed {
        drawing.frame_box(
            [inner[0], 0., middle - 0.018],
            [inner[1], d * 0.9, middle + 0.018],
            look.frame,
        )?;
    }
    Ok(())
}

/// The lights that stand open among `count`. Without an option: one, three times in ten,
/// drawing from `rng` exactly as before the options existed.
fn opened(options: &Options, free: bool, count: usize, rng: &mut SeededRng) -> Vec<usize> {
    let Some(share) = options.open else {
        return if free && rng.chance(0.3) {
            vec![rng.integer(0, count - 1)]
        } else {
            vec![]
        };
    };
    if !free {
        return vec![];
    }
    let wanted =
        share.floor() as usize + usize::from(share.fract() > 0. && rng.chance(share.fract()));
    let mut open = vec![];
    for _ in 0..wanted.min(count) * 4 {
        let light = rng.integer(0, count - 1);
        if !open.contains(&light) && open.len() < wanted {
            open.push(light);
        }
    }
    open
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Guard {
    Bars,
    Grid,
    Diamond,
}
pub fn security_bars(
    drawing: &mut Fixture<'_, '_>,
    span: [f64; 4],
    pattern: Guard,
    color: V3,
    weathering: Option<&super::weathered::Settings>,
) -> Result<(), String> {
    let [x, x1, z, z1] = span;
    security_bars_sized(drawing, [x, z, x1 - x, z1 - z], pattern, color, weathering)
}
/// Keep authored dimensions intact: reconstructing them from endpoints can rotate
/// planar UV axes on exactly diagonal bars through floating-point cancellation.
pub fn security_bars_sized(
    drawing: &mut Fixture<'_, '_>,
    dimensions: [f64; 4],
    pattern: Guard,
    color: V3,
    weathering: Option<&super::weathered::Settings>,
) -> Result<(), String> {
    let original = drawing.pen.clone();
    if let Some(settings) = weathering {
        drawing.pen = original.with_weathering(
            drawing.mesh,
            settings.clone(),
            super::metal::Kind::Cage,
            None,
            vec!["paint".into(), "metal".into()],
        )?;
    }
    let result = security_geometry(drawing, dimensions, pattern, color);
    drawing.pen = original;
    result
}
fn security_geometry(
    drawing: &mut Fixture<'_, '_>,
    dimensions: [f64; 4],
    pattern: Guard,
    color: V3,
) -> Result<(), String> {
    let [x, z, w, h] = dimensions;
    if dimensions.iter().chain(&color).any(|v| !v.is_finite())
        || color.iter().any(|v| *v < 0.)
        || !(0.05..=100.).contains(&w)
        || !(0.05..=100.).contains(&h)
    {
        return Err("Security bars require bounded positive dimensions.".into());
    }
    let y = FRAME_DEPTH + 0.025;
    let mut rod = |ax, az, bx, bz, thickness| {
        drawing.bar("paint", [ax, y, az], [bx, y, bz], thickness, color)
    };
    if matches!(pattern, Guard::Diamond) {
        let mut s = -h;
        while s < w {
            let a = 0f64.max(s);
            let b = w.min(s + h);
            if b - a >= 1e-6 {
                rod(x + a, z + (a - s), x + b, z + (b - s), 0.012)?;
                rod(x + a, z + h - (a - s), x + b, z + h - (b - s), 0.012)?;
            }
            s += 0.24;
        }
    } else {
        let grid = matches!(pattern, Guard::Grid);
        let count = (w / if grid { 0.2 } else { 0.13 }).round().max(2.) as usize;
        for i in 1..count {
            rod(
                x + w * i as f64 / count as f64,
                z,
                x + w * i as f64 / count as f64,
                z + h,
                0.012,
            )?;
        }
        let rails = if grid {
            (h / 0.25).round().max(2.) as usize
        } else {
            2
        };
        for i in 1..rails {
            rod(
                x,
                z + h * i as f64 / rails as f64,
                x + w,
                z + h * i as f64 / rails as f64,
                0.014,
            )?;
        }
    }
    rod(x, z, x, z + h, 0.02)?;
    rod(x + w, z, x + w, z + h, 0.02)?;
    rod(x, z, x + w, z, 0.02)?;
    rod(x, z + h, x + w, z + h, 0.02)
}
