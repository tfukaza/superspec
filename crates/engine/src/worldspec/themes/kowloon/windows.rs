//! Window construction, seeded household state and scoped material receivers.
use super::{
    fasteners::bolt,
    fixture::Fixture,
    palette::{shade, srgb},
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

fn interior(
    drawing: &mut Fixture<'_, '_>,
    span: [f64; 4],
    look: &Look,
    rng: &mut SeededRng,
) -> Result<(), String> {
    let [x0, x1, z0, z1] = span;
    let depth = look.recess;
    drawing.panel(
        if look.lit { "interior-lit" } else { "pane" },
        span,
        if depth > 0. { -depth + 0.003 } else { 0.012 },
        srgb("#1b2225"),
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
    let result = interior(drawing, inner, look, rng);
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
        let open = if !look.fixed && rng.chance(0.3) {
            Some(rng.integer(0, leaves - 1))
        } else {
            None
        };
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
            if Some(i) == open {
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
        ((inner[1] - inner[0]) / 0.5).round().max(2.) as usize
    };
    let pitch = (inner[1] - inner[0]) / lights as f64;
    let open = if !look.fixed && look.reach >= 0.6 + look.recess && rng.chance(0.3) {
        Some(rng.integer(0, lights - 1))
    } else {
        None
    };
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
        if Some(i) == open {
            let angle = rng.float(0.7, 1.3);
            let left = (i as f64) < lights as f64 / 2.;
            let hinge = if left { l0 } else { l1 };
            let swing = if left { 1. } else { -1. } * cos(angle) * pitch;
            let tip = [hinge + swing, d + sin(angle) * pitch];
            drawing.sheet(
                "glass",
                &[
                    [hinge, d, inner[2]],
                    [tip[0], tip[1], inner[2]],
                    [tip[0], tip[1], transom],
                    [hinge, d, transom],
                ],
                [0.85, 0.9, 0.9],
            )?;
            for zb in [inner[2] + 0.015, transom - 0.015] {
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
                [tip[0], tip[1], inner[2]],
                [tip[0], tip[1], transom],
                0.03,
                look.frame,
            )?;
        }
        if transom < inner[3] {
            drawing.panel(
                "glass",
                [l0 + 0.018, l1 - 0.018, transom + 0.02, inner[3] - 0.012],
                d * 0.4 - 0.003,
                [0.88, 0.94, 0.93],
            )?;
        }
        if Some(i) == open {
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
    Ok(())
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
