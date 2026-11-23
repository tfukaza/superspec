//! Wires and small hardware: sagging cables with their clips and junction boxes, bolted
//! wall plates, pipe clips and the loose kit of a flat roof. Conventions are those of the
//! parent module.
use super::{Arguments, draw, fixture, on, room_pen};
use crate::itemspec::definition::{NativeOutput, Values};
use crate::worldspec::themes::kowloon::{
    fasteners,
    fixture::Fixture,
    graphics_detail::GraphicsDetail,
    palette::{mix, shade, srgb},
    pipes,
};
use std::f64::consts::PI;
use superspec_geometry::{
    V3,
    math::{cos, sin},
};
use superspec_types::rng::SeededRng;

/// A steel plate bolted flat to the wall, centred at `[x, z]`: two bolts, or four when it is
/// at least 9 cm wide and high.
pub(super) fn wall_plate(
    d: &mut Fixture<'_, '_>,
    [x, z]: [f64; 2],
    half_width: f64,
    half_height: f64,
    color: V3,
) -> Result<(), String> {
    // Drawn heavy: a thick plate under bolt heads large enough to read from the street.
    const THICK: f64 = 0.012;
    d.cuboid(
        "metal",
        [x - half_width, 0., z - half_height],
        [x + half_width, THICK, z + half_height],
        color,
        "b",
    )?;
    let radius = (half_width.min(half_height) * 0.3).clamp(0.008, 0.014);
    let (dx, dz) = (
        (half_width - radius * 1.7).max(0.),
        (half_height - radius * 1.7).max(0.),
    );
    let corners: &[[f64; 2]] = if half_width >= 0.045 && half_height >= 0.045 {
        &[[-1., -1.], [1., -1.], [1., 1.], [-1., 1.]]
    } else if half_height >= half_width {
        &[[0., -1.], [0., 1.]]
    } else {
        &[[-1., 0.], [1., 0.]]
    };
    for [sx, sz] in corners {
        fasteners::bolt(
            d,
            "metal",
            [x + sx * dx, THICK, z + sz * dz],
            [0., 1., 0.],
            radius,
            shade(color, 1.2),
            None,
        )?;
    }
    Ok(())
}

/// A two-eared saddle clip holding a vertical pipe of `radius`, whose axis stands `y` in
/// front of the wall, at height `z`: a band round the pipe, two ears flat on the wall and a
/// bolt through each.
pub(super) fn pipe_clip(
    d: &mut Fixture<'_, '_>,
    [x, y, z]: V3,
    radius: f64,
    color: V3,
) -> Result<(), String> {
    // Drawn heavy: a broad band, thick ears and bolt heads that read from the street.
    let (band, half) = (radius * 1.26 + 0.003, (radius * 0.65).max(0.03));
    const EAR: f64 = 0.013;
    let detail = d.mesh.graphics_detail();
    if detail.reduced() {
        // Level 1 keeps the band and one plate behind the pipe for both ears; level 2 draws
        // the pipe alone.
        if detail == GraphicsDetail::Low {
            let ear = band + (radius * 1.6).max(0.06);
            d.tube(
                "metal",
                &[[x, y, z - half], [x, y, z + half]],
                band,
                color,
                12,
            )?;
            d.cuboid(
                "metal",
                [x - ear, 0., z - half],
                [x + ear, detail.readable(EAR), z + half],
                color,
                "b",
            )?;
        }
        return Ok(());
    }
    d.tube(
        "metal",
        &[[x, y, z - half], [x, y, z + half]],
        band,
        color,
        12,
    )?;
    for (level, hint) in [(z - half, -1.), (z + half, 1.)] {
        // The band's rims, as rings between the pipe and the band.
        let ring: Vec<V3> = (0..12)
            .map(|k| {
                let angle = k as f64 / 12. * 2. * PI;
                [x + band * cos(angle), y + band * sin(angle), level]
            })
            .collect();
        d.face("metal", &ring, [0., 0., hint], color)?;
    }
    let ear = band + (radius * 1.6).max(0.06);
    for side in [-1., 1.] {
        // The strap back to the wall, then the ear on it.
        let strap = [x + side * band * 0.6, x + side * band];
        d.cuboid(
            "metal",
            [strap[0].min(strap[1]), 0., z - half],
            [strap[0].max(strap[1]), y, z + half],
            color,
            "b",
        )?;
        let plate = [x + side * band * 0.6, x + side * ear];
        d.cuboid(
            "metal",
            [plate[0].min(plate[1]), 0., z - half],
            [plate[0].max(plate[1]), EAR, z + half],
            color,
            "b",
        )?;
        fasteners::bolt(
            d,
            "metal",
            [x + side * (band + ear) / 2., EAR, z],
            [0., 1., 0.],
            (radius * 0.3).clamp(0.01, 0.016),
            shade(color, 1.25),
            None,
        )?;
    }
    Ok(())
}

/// The `[width, height, depth]` of a junction box where none is given.
const BOX: V3 = [0.16, 0.2, 0.075];

/// A lidded steel junction box on the wall, centred on x = 0 with its foot at z = 0: body,
/// overlapping lid with a bolt at each corner, and `stubs` conduit stubs out of its foot.
pub(super) fn junction(
    d: &mut Fixture<'_, '_>,
    [width, height, depth]: V3,
    color: V3,
    stubs: usize,
) -> Result<(), String> {
    let half = width / 2.;
    d.cuboid("metal", [-half, 0., 0.], [half, depth, height], color, "b")?;
    let detail = d.mesh.graphics_detail();
    if detail == GraphicsDetail::VeryLow {
        // The body alone: its lid and stubs are a centimetre or two across.
        return Ok(());
    }
    let lid = shade(color, 0.88);
    // Drawn heavy: a thick lid lapping well over the body, and bolt heads to match.
    const LID: f64 = 0.012;
    d.cuboid(
        "metal",
        [-half - 0.01, depth, -0.01],
        [half + 0.01, depth + LID, height + 0.01],
        lid,
        "b",
    )?;
    let bolt = (width.min(height) * 0.08).clamp(0.008, 0.016);
    for sx in [-1., 1.] {
        for z in [bolt * 2., height - bolt * 2.] {
            fasteners::bolt(
                d,
                "metal",
                [sx * (half - bolt * 2.), depth + LID, z],
                [0., 1., 0.],
                bolt,
                shade(color, 1.15),
                None,
            )?;
        }
    }
    for k in 0..stubs {
        let x = -half + width * (k as f64 + 0.5) / stubs as f64;
        d.tube(
            "metal",
            &[[x, depth * 0.5, 0.], [x, depth * 0.5, -0.08]],
            (width / stubs as f64 * 0.22).clamp(0.011, 0.018),
            shade(color, 0.7),
            8,
        )?;
    }
    Ok(())
}

/// `kowloon/junction-box`: a lidded steel junction box bolted to the wall. Anchor:
/// bottom-centre on the wall plane. Parameters: `width` (m, 0.05–1, 0.16), `height` (m,
/// 0.05–1, 0.2), `depth` (m, 0.02–0.4, 0.075), `stubs` (conduit stubs below, 0–4, 2),
/// `color` (#8d8b80).
pub(super) fn junction_box(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let width = a.bounded("width", BOX[0], 0.05..=1.)?;
    let height = a.bounded("height", BOX[1], 0.05..=1.)?;
    let depth = a.bounded("depth", BOX[2], 0.02..=0.4)?;
    let stubs = a.integer("stubs", 2, 0..=4)?;
    let color = a.color("color", "#8d8b80")?;
    a.finish()?;
    draw(cancelled, |mesh| {
        junction(
            &mut fixture(mesh, "junction-box")?,
            [width, height, depth],
            color,
            stubs,
        )
    })
}

/// `kowloon/cable`: electric cables strung along a wall, each hanging in a catenary between
/// its fixings. Anchor: the first fixing, on the wall plane; the cables run `span` along X
/// (negative runs left) and end `rise` higher. With `corner` they are clipped where the wall
/// ends, `corner` m along, pass round a corner of `cornerRadius` that turns back by a right
/// angle and carry on along the next wall, `span` being the whole developed length. With
/// `out` the far end leaves the wall for a fixture that stands that far in front of it (a
/// sign's pole, an air conditioner). Slots: `cable` (tinted per vertex), `metal` (clips and
/// boxes). Parameters: `span` (m, -60–60, 3), `rise` (m, -30–30, 0), `sag` (m of the longest
/// run, 0–5, 0.25), `count` (cables in the bundle, 1–12, 1), `spread` (m between them at the
/// fixings, 0–0.5, 0.045), `radius` (m, 0.002–0.05, 0.0075: drawn heavier than a real cable,
/// which is a hairline from the street), `standoff` (m from the wall,
/// 0–2, 0.03), `out` (m from the wall at the far end; default `standoff`), `corner` (m,
/// 0–60, 0: none), `cornerRadius` (m, 0.02–5, 0.25), `boxes` (`none` | `start` | `end` |
/// `both`: junction boxes at the fixings, `none`), `clips` (true), `color` (#24231f), `seed`
/// (each cable's own slack: the first hangs within a tenth of `sag`, the others between
/// three quarters and half as much again).
pub(super) fn cable(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let span = a.bounded("span", 3., -60. ..=60.)?;
    let rise = a.bounded("rise", 0., -30. ..=30.)?;
    let sag = a.bounded("sag", 0.25, 0. ..=5.)?;
    let count = a.integer("count", 1, 1..=12)?;
    let spread = a.bounded("spread", 0.045, 0. ..=0.5)?;
    let radius = a.bounded("radius", 0.0075, 0.002..=0.05)?;
    let standoff = a.bounded("standoff", 0.03, 0. ..=2.)?;
    let out = a.optional_number("out")?.unwrap_or(standoff);
    let corner = a.bounded("corner", 0., 0. ..=60.)?;
    let bend = a.bounded("cornerRadius", 0.25, 0.02..=5.)?;
    let boxes = a.text("boxes", "none")?;
    let clips = a.boolean("clips", true)?;
    let color = a.color("color", "#24231f")?;
    let mut rng = a.rng()?;
    a.finish()?;
    if !["none", "start", "end", "both"].contains(&boxes.as_str()) {
        return Err(format!(
            "{name}: boxes must be one of none, start, end, both."
        ));
    }
    if !(0. ..=5.).contains(&out) {
        return Err(format!("{name}: out must lie in 0..5."));
    }
    let length = span.abs();
    let arc = bend * PI / 2.;
    if length < 0.05 {
        return Err(format!("{name}: the cable has no length."));
    }
    if corner > 0. && (span < corner + arc + 0.05) {
        return Err(format!(
            "{name}: a cable round a corner runs right and reaches past it (span > corner + {arc:.3})."
        ));
    }
    let direction = span.signum();
    // Runs between fixings, by developed distance; the turn itself hangs taut.
    let knots: Vec<f64> = if corner > 0. {
        vec![0., corner, corner + arc, length]
    } else {
        vec![0., length]
    };
    let longest = knots
        .windows(2)
        .enumerate()
        .filter(|(i, _)| corner == 0. || *i != 1)
        .map(|(_, w)| w[1] - w[0])
        .fold(0., f64::max);
    // Plan position (x, distance from the wall plane) at developed distance `s`.
    let plan = |s: f64, away: f64| -> [f64; 2] {
        if corner == 0. {
            [direction * s, away]
        } else if s <= corner {
            [s, away]
        } else if s <= corner + arc {
            let angle = (s - corner) / bend;
            [
                corner + (bend + away) * sin(angle),
                -bend + (bend + away) * cos(angle),
            ]
        } else {
            [corner + bend + away, -bend - (s - corner - arc)]
        }
    };
    let last = knots.len() - 2;
    let slack: Vec<f64> = (0..count)
        .map(|i| {
            if i == 0 {
                rng.float(0.9, 1.1)
            } else {
                rng.float(0.75, 1.5)
            }
        })
        .collect();
    draw(cancelled, |mesh| {
        let d = &mut fixture(mesh, "cable")?;
        // Wires are hairlines from across the street: level 2 draws none of the bundle, its
        // clips or its boxes, and level 1 samples each catenary half as often.
        let detail = d.mesh.graphics_detail();
        if detail == GraphicsDetail::VeryLow {
            return Ok(());
        }
        for (i, slack) in slack.iter().enumerate() {
            let lift = i as f64 * spread;
            let mut path: Vec<V3> = vec![];
            for (run, pair) in knots.windows(2).enumerate() {
                let (from, to) = (pair[0], pair[1]);
                let turning = corner > 0. && run == 1;
                let samples = if turning {
                    detail.pick(6, 3, 3)
                } else if detail.reduced() {
                    (((to - from) / 0.5).ceil() as usize).clamp(3, 12)
                } else {
                    (((to - from) / 0.2).ceil() as usize).clamp(6, 28)
                };
                let droop = if turning {
                    0.
                } else {
                    sag * slack * (to - from) / longest
                };
                for k in usize::from(run > 0)..=samples {
                    let t = k as f64 / samples as f64;
                    let s = from + (to - from) * t;
                    let away = if run == last {
                        standoff + (out - standoff) * t
                    } else {
                        standoff
                    } + i as f64 * radius * 0.5;
                    let [x, y] = plan(s, away);
                    path.push([x, y, rise * s / length + lift - droop * 4. * t * (1. - t)]);
                }
            }
            d.tube(
                "cable",
                &path,
                radius,
                shade(color, 1. - 0.12 * (i % 3) as f64),
                6,
            )?;
        }
        let steel = srgb("#77766c");
        // A clip is a block that swallows the bundle: broad enough to read from the street.
        let clip = (radius * 2.).max(0.018);
        let high = (count - 1) as f64 * spread + clip;
        if clips {
            for (index, s) in knots.iter().enumerate() {
                let end = index == knots.len() - 1;
                if end && out > standoff + 0.02 {
                    continue;
                }
                let z = rise * s / length;
                let reach = standoff + (count - 1) as f64 * radius * 0.5 + radius + 0.008;
                if corner > 0. && *s > corner + arc - 1e-9 {
                    // On the wall past the corner, which faces +X here.
                    let y = plan(*s, 0.)[1];
                    d.cuboid(
                        "metal",
                        [corner + bend, y - clip, z - clip],
                        [corner + bend + reach, y + clip, z + high],
                        steel,
                        "",
                    )?;
                } else {
                    let x = plan(*s, 0.)[0];
                    d.cuboid(
                        "metal",
                        [x - clip, 0., z - clip],
                        [x + clip, reach, z + high],
                        steel,
                        "b",
                    )?;
                }
            }
        }
        let body = srgb("#8d8b80");
        let below = BOX[1] + 0.02;
        if boxes == "start" || boxes == "both" {
            let pen = d.pen.moved([0., 0., -below])?;
            junction(&mut on(d.mesh, pen)?, BOX, body, 1)?;
        }
        if (boxes == "end" || boxes == "both") && out <= standoff + 0.02 {
            let pen = if corner > 0. {
                let [x, y] = plan(length, 0.);
                d.pen
                    .facing([x, y, rise - below], [0., -1., 0.], [1., 0., 0.])?
            } else {
                d.pen.moved([span, 0., rise - below])?
            };
            junction(&mut on(d.mesh, pen)?, BOX, body, 1)?;
        }
        Ok(())
    })
}

/// `kowloon/roof-clutter`: the loose kit of a flat roof (roof frame): vent stubs with cowls
/// or goosenecks along one side, service pipes on sleepers along another, a few potted
/// plants along a third, and a cable slung between two points (an aerial and the stair hut).
/// Anchor: the centre of the deck at roof level; the deck spans `width` by `depth` inside
/// its parapets, the street edge at y = -depth/2. Slots: `metal`, `clutter` (pots, plants
/// and sleepers, tinted per vertex), `cable`. Parameters: `width` (m, 1–60, 9), `depth` (m,
/// 1–60, 6), `vents` (0–12, 3), `ventSide` (`left` | `right` | `front` | `rear`, `right`),
/// `parapet` (m of parapet the vent stubs stand clear of, by 0.2 to 0.75 m; 0–3, 0.3),
/// `ventInset` (m from the deck's edge to the stubs, 0.05–3; the seed's 0.32–0.5 by default),
/// `pipes` (0–6, 2), `pipeSide` (`rear`), `pots` (0–16, 4), `potSide` (`left`), `cable`
/// (false), `cableX0`, `cableY0`, `cableZ0`, `cableX1`, `cableY1`, `cableZ1` (m, its ends;
/// 0, 0, 1.5 and 2, 0, 2), `cableSag` (m, 0–3, 0.2), `color` (pipes and vents, #7d7b74),
/// `seed`.
pub(super) fn roof_clutter(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    const SIDES: [&str; 4] = ["left", "right", "front", "rear"];
    let mut a = Arguments::new(name, values);
    let width = a.bounded("width", 9., 1. ..=60.)?;
    let depth = a.bounded("depth", 6., 1. ..=60.)?;
    let vents = a.integer("vents", 3, 0..=12)?;
    let pipe_count = a.integer("pipes", 2, 0..=6)?;
    let pots = a.integer("pots", 4, 0..=16)?;
    let parapet = a.bounded("parapet", 0.3, 0. ..=3.)?;
    // `ventInset`: how far onto the deck the vent stubs stand (m, 0.05–3; the seed's 0.32 to
    // 0.5 when left out), each up to 5 cm more by the seed.
    let vent_inset = a.optional_number("ventInset")?;
    if vent_inset.is_some_and(|v| !(0.05..=3.).contains(&v)) {
        return Err(format!("{name}: ventInset must lie in 0.05..3."));
    }
    let sides = [
        a.text("ventSide", "right")?,
        a.text("pipeSide", "rear")?,
        a.text("potSide", "left")?,
    ];
    let cable = a.boolean("cable", false)?;
    let ends = [
        [
            a.bounded("cableX0", 0., -60. ..=60.)?,
            a.bounded("cableY0", 0., -60. ..=60.)?,
            a.bounded("cableZ0", 1.5, 0. ..=30.)?,
        ],
        [
            a.bounded("cableX1", 2., -60. ..=60.)?,
            a.bounded("cableY1", 0., -60. ..=60.)?,
            a.bounded("cableZ1", 2., 0. ..=30.)?,
        ],
    ];
    let cable_sag = a.bounded("cableSag", 0.2, 0. ..=3.)?;
    let color = a.color("color", "#7d7b74")?;
    let key = a.seed()?;
    a.finish()?;
    if let Some(side) = sides.iter().find(|side| !SIDES.contains(&side.as_str())) {
        return Err(format!(
            "{name}: {side} is not one of left, right, front, rear."
        ));
    }
    // A side's start, its direction along the parapet, the direction onto the deck and its length.
    let frame = |side: &str| -> (V3, V3, V3, f64) {
        let (w, h) = (width / 2., depth / 2.);
        match side {
            "left" => ([-w, -h, 0.], [0., 1., 0.], [1., 0., 0.], depth),
            "right" => ([w, -h, 0.], [0., 1., 0.], [-1., 0., 0.], depth),
            "front" => ([-w, -h, 0.], [1., 0., 0.], [0., 1., 0.], width),
            _ => ([-w, h, 0.], [1., 0., 0.], [0., -1., 0.], width),
        }
    };
    let at = |(origin, along, inward, _): (V3, V3, V3, f64), s: f64, inset: f64, z: f64| -> V3 {
        [
            origin[0] + along[0] * s + inward[0] * inset,
            origin[1] + along[1] * s + inward[1] * inset,
            z,
        ]
    };
    let pipework = (pipe_count > 0)
        .then(|| {
            pipes::PipeSystem::new(pipes::Options {
                radius: 0.034,
                color,
                sides: 10,
                variant: "pipe-standard".into(),
                weathering: None,
            })?
            .weighted(2.)
        })
        .transpose()?;
    draw(cancelled, |mesh| {
        let d = &mut on(mesh, room_pen("roof-clutter", [0.; 3])?)?;
        let rng = &mut SeededRng::from_text(&format!("{key}/vents"));
        let side = frame(&sides[0]);
        for i in 0..vents {
            let s = side.3 * (0.18 + 0.68 * (i as f64 + rng.float(0.2, 0.8)) / vents as f64);
            let inset = rng.float(0.32, 0.5);
            let inset = vent_inset.map_or(inset, |v| v + (inset - 0.32) / 0.18 * 0.05);
            let base = at(side, s, inset, 0.);
            let (r, tall) = (rng.float(0.04, 0.062), parapet + rng.float(0.2, 0.75));
            let tint = mix(color, srgb("#6b4a34"), rng.float(0., 0.45));
            d.cuboid(
                "metal",
                [base[0] - r * 2.2, base[1] - r * 2.2, 0.],
                [base[0] + r * 2.2, base[1] + r * 2.2, 0.025],
                shade(tint, 0.75),
                "d",
            )?;
            let top = [base[0], base[1], tall];
            d.tube("metal", &[base, top], r, tint, 10)?;
            d.tube(
                "metal",
                &[[base[0], base[1], 0.025], [base[0], base[1], 0.1]],
                r * 1.25,
                shade(tint, 0.85),
                10,
            )?;
            if rng.chance(0.5) {
                // A cowl: a cap standing clear of the mouth on three legs.
                for k in 0..3 {
                    let angle = k as f64 / 3. * 2. * PI;
                    let leg = [base[0] + r * cos(angle), base[1] + r * sin(angle), tall];
                    d.tube(
                        "metal",
                        &[leg, [leg[0], leg[1], tall + 0.06]],
                        0.009,
                        tint,
                        5,
                    )?;
                }
                d.drum(
                    "metal",
                    [base[0], base[1], tall + 0.06],
                    r * 1.9,
                    0.035,
                    shade(tint, 0.9),
                    10,
                )?;
                d.disc(
                    "metal",
                    [base[0], base[1], tall + 0.06],
                    r * 1.9,
                    [0., 0., -1.],
                    shade(tint, 0.7),
                    10,
                )?;
            } else {
                // A gooseneck turned away from the parapet.
                let reach = |out: f64, z: f64| -> V3 {
                    [
                        base[0] + side.2[0] * out,
                        base[1] + side.2[1] * out,
                        tall + z,
                    ]
                };
                d.tube(
                    "metal",
                    &[
                        top,
                        reach(r * 0.8, r * 1.6),
                        reach(r * 2.6, r * 2.2),
                        reach(r * 4.2, r * 1.2),
                        reach(r * 4.6, -r * 0.6),
                    ],
                    r,
                    tint,
                    10,
                )?;
            }
        }
        let rng = &mut SeededRng::from_text(&format!("{key}/pipes"));
        let side = frame(&sides[1]);
        if let Some(system) = &pipework {
            for i in 0..pipe_count {
                let inset = 0.16 + i as f64 * 0.13;
                let z = 0.16 + 0.02 * (i % 2) as f64;
                let (from, to) = (
                    side.3 * rng.float(0.06, 0.2),
                    side.3 * rng.float(0.55, 0.94),
                );
                // Up out of the deck, along the parapet, and down again.
                system.run(
                    d,
                    &[
                        at(side, from, inset, 0.),
                        at(side, from, inset, z),
                        at(side, to, inset, z),
                        at(side, to, inset, 0.),
                    ],
                    None,
                )?;
                let sleepers = ((to - from) / 1.5).ceil().max(1.) as usize;
                for k in 0..=sleepers {
                    let s = from + 0.25 + (to - from - 0.5) * k as f64 / sleepers as f64;
                    let (p, q) = (
                        at(side, s - 0.06, inset - 0.09, 0.),
                        at(side, s + 0.06, inset + 0.09, z - 0.03),
                    );
                    d.cuboid(
                        "clutter",
                        [p[0].min(q[0]), p[1].min(q[1]), 0.],
                        [p[0].max(q[0]), p[1].max(q[1]), q[2]],
                        srgb("#8a857a"),
                        "d",
                    )?;
                }
            }
        }
        let rng = &mut SeededRng::from_text(&format!("{key}/pots"));
        let side = frame(&sides[2]);
        for i in 0..pots {
            let s = side.3 * (0.1 + 0.5 * (i as f64 + rng.float(0.1, 0.9)) / pots as f64);
            let base = at(side, s, rng.float(0.28, 0.55), 0.);
            let (r, tall) = (rng.float(0.1, 0.17), rng.float(0.16, 0.26));
            let clay = shade(srgb("#9a5a3c"), rng.float(0.7, 1.05));
            d.tube("clutter", &[base, [base[0], base[1], tall]], r, clay, 10)?;
            d.tube(
                "clutter",
                &[[base[0], base[1], tall - 0.03], [base[0], base[1], tall]],
                r * 1.12,
                shade(clay, 1.1),
                10,
            )?;
            d.disc(
                "clutter",
                [base[0], base[1], tall - 0.015],
                r * 1.12,
                [0., 0., 1.],
                srgb("#3a2f25"),
                10,
            )?;
            if rng.chance(0.8) {
                // A tuft of blades leaning out of the pot.
                let leaf = shade(srgb("#4c6a3a"), rng.float(0.7, 1.15));
                let blades = rng.integer(5, 9);
                for k in 0..blades {
                    let angle = (k as f64 + rng.float(0., 0.7)) / blades as f64 * 2. * PI;
                    let (lean, long) = (rng.float(0.3, 1.1), rng.float(0.25, 0.55));
                    let (dx, dy) = (cos(angle), sin(angle));
                    let root = [base[0] + dx * r * 0.3, base[1] + dy * r * 0.3, tall - 0.015];
                    let tip = [
                        root[0] + dx * long * lean * 0.6,
                        root[1] + dy * long * lean * 0.6,
                        root[2] + long,
                    ];
                    let half = 0.022;
                    d.sheet(
                        "clutter",
                        &[
                            [root[0] - dy * half, root[1] + dx * half, root[2]],
                            [root[0] + dy * half, root[1] - dx * half, root[2]],
                            tip,
                        ],
                        leaf,
                    )?;
                }
            }
        }
        if cable {
            let [from, to] = ends;
            let path: Vec<V3> = (0..=16)
                .map(|k| {
                    let t = k as f64 / 16.;
                    [
                        from[0] + (to[0] - from[0]) * t,
                        from[1] + (to[1] - from[1]) * t,
                        from[2] + (to[2] - from[2]) * t - cable_sag * 4. * t * (1. - t),
                    ]
                })
                .collect();
            d.tube("cable", &path, 0.0075, srgb("#24231f"), 6)?;
        }
        Ok(())
    })
}
