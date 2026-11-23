//! Fitted washing: deterministic cloth cuts, folds and support fittings.
mod cuts;
pub use super::balcony_props::{Room, Support};
use super::{
    balcony_props,
    fixture::Fixture,
    palette::{shade, srgb},
    pen::PatchVertex,
};
use cuts::{Cut, Fitting};
use std::f64::consts::PI;
use superspec_geometry::{
    V3,
    math::{cos, hypot, sin},
};
use superspec_types::rng::SeededRng;

pub enum Garment {
    Mesh(balcony_props::Garment),
    Cut(CutGarment),
}
pub struct CutGarment {
    width: f64,
    drop: f64,
    cut: Cut,
    tint: V3,
    fitting_tint: V3,
    tilt: f64,
    folds: Folds,
}
struct Folds {
    depth: f64,
    rate: [f64; 2],
    phase: [f64; 2],
    skew: f64,
}
pub struct WashingOptions {
    pub fill: f64,
    pub drop: f64,
    pub support: Support,
}
fn row_valid(span: [f64; 2], z: f64) -> bool {
    span.iter()
        .chain([z].iter())
        .all(|v| v.is_finite() && v.abs() <= 1000.)
        && span[1] > span[0]
        && span[1] - span[0] <= 100.
}
pub fn washing(
    drawing: &mut Fixture<'_, '_>,
    span: [f64; 2],
    z: f64,
    rng: &mut SeededRng,
    options: &WashingOptions,
    textured: bool,
) -> Result<(), String> {
    washing_with(drawing, span, z, rng, options, textured, 1.)
}
/// [`washing`] with pegs, hanger bars and hooks of `weight`: 1 is their own size, 2 the
/// weight of a drawing (1–3).
pub fn washing_with(
    drawing: &mut Fixture<'_, '_>,
    span: [f64; 2],
    z: f64,
    rng: &mut SeededRng,
    options: &WashingOptions,
    textured: bool,
    weight: f64,
) -> Result<(), String> {
    if !row_valid(span, z)
        || !room_valid(Room {
            width: span[1] - span[0],
            drop: options.drop,
        })
        || !options.fill.is_finite()
        || !(0. ..=1.).contains(&options.fill)
        || options.support == Support::Hanger
    {
        return Err("Invalid washing row.".into());
    }
    let mut x = span[0] + rng.float(0., 0.12);
    while span[1] - x > 0.3 {
        drawing.mesh.spend_work(1)?;
        let Some(g) = choose(
            rng,
            Room {
                width: span[1] - x,
                drop: options.drop,
            },
            options.support,
        )?
        else {
            break;
        };
        if rng.chance(options.fill) {
            hang_with(
                drawing,
                &g,
                [x, 0., z],
                [x + g.width(), 0., z],
                rng.float(-0.005, 0.005),
                textured,
                weight,
            )?;
        }
        x += g.width() + rng.float(0.03, 0.16);
    }
    Ok(())
}
pub struct HangerRow {
    pub span: [f64; 2],
    pub z: f64,
    pub slope: f64,
    pub reach: f64,
    pub drop: f64,
}
pub fn hangers(
    drawing: &mut Fixture<'_, '_>,
    row: &HangerRow,
    rng: &mut SeededRng,
    textured: bool,
) -> Result<(), String> {
    hangers_with(drawing, row, rng, textured, 1.)
}
/// [`hangers`] with fittings of `weight` (as [`washing_with`]).
pub fn hangers_with(
    drawing: &mut Fixture<'_, '_>,
    row: &HangerRow,
    rng: &mut SeededRng,
    textured: bool,
    weight: f64,
) -> Result<(), String> {
    if !row_valid(row.span, row.z)
        || !room_valid(Room {
            width: row.reach * 2.,
            drop: row.drop,
        })
        || !row.slope.is_finite()
        || row.slope.abs() > 10.
    {
        return Err("Invalid hanger row.".into());
    }
    let mut x = row.span[0] + rng.float(0.04, 0.14);
    while x < row.span[1] - 0.04 {
        drawing.mesh.spend_work(1)?;
        if let Some(g) = choose(
            rng,
            Room {
                width: row.reach * 2.,
                drop: row.drop,
            },
            Support::Hanger,
        )? && !rng.chance(0.15)
        {
            let half = g.width() / 2.;
            let height = row.z + row.slope * x;
            hang_with(
                drawing,
                &g,
                [x, -half, height],
                [x, half, height],
                rng.float(-0.01, 0.01),
                textured,
                weight,
            )?;
        }
        x += rng.float(0.13, 0.3);
    }
    Ok(())
}
impl Garment {
    pub fn width(&self) -> f64 {
        match self {
            Self::Mesh(g) => g.width,
            Self::Cut(g) => g.width,
        }
    }
    pub fn drop(&self) -> f64 {
        match self {
            Self::Mesh(g) => g.drop,
            Self::Cut(g) => g.drop,
        }
    }
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Mesh(_) => "mesh",
            Self::Cut(_) => "cut",
        }
    }
}
thread_local! {
    static DYES: std::cell::RefCell<Option<SeededRng>> = const { std::cell::RefCell::new(None) };
}
/// Runs `draw` with the garments it chooses dyed from the stream of `key` (`None`: from the
/// stream that shapes them, as ever), restoring the caller's dyes. A fixture whose washing is
/// a catalogue entry's hangs the entry's garments in colours of its own, so two fixtures of
/// one entry are one geometry in two colourings.
pub(super) fn with_dyes<T>(key: Option<&str>, draw: impl FnOnce() -> T) -> T {
    struct Restore(Option<SeededRng>);
    impl Drop for Restore {
        fn drop(&mut self) {
            DYES.with(|dyes| *dyes.borrow_mut() = self.0.take());
        }
    }
    let stream = key.map(|key| SeededRng::from_text(&format!("{key}/dyes")));
    let _restore = Restore(DYES.with(|dyes| dyes.replace(stream)));
    draw()
}
/// A colour choice drawn from the dye stream, when one is set.
fn dyed<T>(choose: impl FnOnce(&mut SeededRng) -> T) -> Option<T> {
    DYES.with(|dyes| dyes.borrow_mut().as_mut().map(choose))
}
const WHITES: &str = "#dedbd2 #d9d7d0 #e0dccf #d3d6d8 #dcd6c6";
const PALES: &str = "#c3ccd6 #d6c7b4 #d9c4c0 #c6d2c4 #d8d0aa #cfc6d2";
const MIDS: &str = "#8fa3b8 #a89a84 #b88f88 #7f9488 #b5a06a #8f8aa0 #9a9c98";
const DARKS: &str = "#3f4a5c #50545a #3b3f45 #5a5146 #4a5a52 #6b4542";
const LINENS: &str = "#d0cec5 #bfc1bb #c7cbc7 #b8b7b0 #cec9bd";
fn room_valid(room: Room) -> bool {
    [room.width, room.drop]
        .iter()
        .all(|v| v.is_finite() && *v > 0. && *v <= 100.)
}
/// Invalid admission leaves the seeded stream unchanged.
#[allow(clippy::approx_constant)] // Preserve the authored 6.28 phase range and seeded appearance.
pub fn choose(
    rng: &mut SeededRng,
    room: Room,
    support: Support,
) -> Result<Option<Garment>, String> {
    if !room_valid(room) {
        return Err("Laundry room requires positive dimensions within 100 m.".into());
    }
    if rng.chance(0.72)
        && let Some(mut g) = balcony_props::choose(rng, room, support)?
    {
        if let Some(tint) = dyed(|dyes| srgb(dyes.pick(&balcony_props::DYES))) {
            g.tint = tint;
        }
        return Ok(Some(Garment::Mesh(g)));
    }
    for _ in 0..5 {
        let mut roll = rng.next_f64() * 17.;
        let mut design = 5;
        for (i, weight) in [2., 3., 3., 2., 3., 4.].iter().enumerate() {
            roll -= weight;
            if roll <= 0. {
                design = i;
                break;
            }
        }
        let cut = match design {
            0 => cuts::shirt(rng, support, false),
            1 => cuts::shirt(rng, support, true),
            2 => cuts::bottoms(rng, support, false),
            3 => cuts::bottoms(rng, support, true),
            _ => {
                let Some(c) = cuts::flat(rng, support, room, design == 5) else {
                    continue;
                };
                c
            }
        };
        let mut width: f64 = 0.;
        let mut drop: f64 = 0.;
        for f in cut.panels.iter().flat_map(|p| p.grid.iter().flatten()) {
            width = width.max(f.x.abs() * 2.);
            drop = drop.max(f.y);
        }
        if width > room.width || drop + 0.03 > room.drop {
            continue;
        }
        let dyes = match design {
            0 | 1 => [WHITES, PALES],
            2 => [MIDS, DARKS],
            3 => [PALES, MIDS],
            _ => [LINENS, ""],
        };
        let colors: Vec<_> = dyes.iter().flat_map(|s| s.split_whitespace()).collect();
        // The cloth's dye and its shade, then what its pegs or hanger are made of.
        let colours = |rng: &mut SeededRng| {
            let color = srgb(rng.pick(&colors));
            let tint = shade(color, rng.float(0.86, 1.));
            let fitting_tint = if support == Support::Hanger {
                rng.pick(&[
                    shade(srgb("#a4a9aa"), 0.8),
                    srgb("#d8d6cc"),
                    srgb("#7f97ad"),
                    srgb("#3c3c3a"),
                ])
            } else {
                srgb(rng.pick(&[
                    "#b89868", "#b89868", "#9db4c4", "#c9a3a3", "#a9c2a4", "#d6d2c4",
                ]))
            };
            (tint, fitting_tint)
        };
        // The stream that shapes the garment draws its colours always, so the shape is the
        // same whoever dyes it.
        let own = colours(&mut *rng);
        let (tint, fitting_tint) = dyed(colours).unwrap_or(own);
        let tilt = if cut.follows {
            0.
        } else {
            rng.float(-0.07, 0.07)
        };
        let folds = Folds {
            depth: rng.float(0.008, 0.014),
            rate: [
                2. * PI / (cut.grain * rng.float(4.5, 6.5)),
                2. * PI / (cut.grain * rng.float(4., 5.)),
            ],
            phase: [rng.float(0., 6.28), rng.float(0., 6.28)],
            skew: rng.float(-1.6, 1.6),
        };
        return Ok(Some(Garment::Cut(CutGarment {
            width,
            drop: drop + 0.03,
            cut,
            tint,
            fitting_tint,
            tilt,
            folds,
        })));
    }
    Ok(None)
}
pub fn hang(
    drawing: &mut Fixture<'_, '_>,
    garment: &Garment,
    left: V3,
    right: V3,
    lean: f64,
    textured: bool,
) -> Result<(), String> {
    hang_with(drawing, garment, left, right, lean, textured, 1.)
}
/// A reduced level's meshed garment: its outline cut from plain cotton in its own dye, both
/// faces, hung from the same anchors with the same lean. A shirt keeps its sleeves and a
/// pair of trousers its legs, so a line of washing still reads as clothes.
fn silhouette(
    drawing: &mut Fixture<'_, '_>,
    g: &balcony_props::Garment,
    left: V3,
    right: V3,
    lean: f64,
) -> Result<(), String> {
    use super::pen::Tint;
    let run = hypot(right[0] - left[0], right[1] - left[1]);
    if run < 1e-6 {
        return Ok(());
    }
    let along = [(right[0] - left[0]) / run, (right[1] - left[1]) / run];
    let middle: V3 = std::array::from_fn(|i| (left[i] + right[i]) / 2.);
    let (w, drop) = (g.width, g.drop);
    let place = |[x, y]: [f64; 2]| {
        let z = lean * (y / drop).clamp(0., 1.).powf(1.3);
        [
            middle[0] + along[0] * x - along[1] * z,
            middle[1] + along[1] * x + along[0] * z,
            middle[2] - y,
        ]
    };
    let top = 0.02_f64.min(drop * 0.1);
    let pieces: Vec<[[f64; 2]; 4]> = match g.asset {
        balcony_props::GarmentName::HangingShirt => vec![
            [
                [-0.29 * w, top],
                [0.29 * w, top],
                [0.29 * w, drop],
                [-0.29 * w, drop],
            ],
            [
                [-0.29 * w, top],
                [-0.29 * w, top + 0.24 * drop],
                [-0.5 * w, 0.52 * drop],
                [-0.5 * w, 0.3 * drop],
            ],
            [
                [0.29 * w, top],
                [0.5 * w, 0.3 * drop],
                [0.5 * w, 0.52 * drop],
                [0.29 * w, top + 0.24 * drop],
            ],
        ],
        balcony_props::GarmentName::HangingTrousers => vec![
            [
                [-0.5 * w, top],
                [0.5 * w, top],
                [0.5 * w, 0.28 * drop],
                [-0.5 * w, 0.28 * drop],
            ],
            [
                [-0.5 * w, 0.28 * drop],
                [-0.04 * w, 0.28 * drop],
                [-0.08 * w, drop],
                [-0.46 * w, drop],
            ],
            [
                [0.04 * w, 0.28 * drop],
                [0.5 * w, 0.28 * drop],
                [0.46 * w, drop],
                [0.08 * w, drop],
            ],
        ],
        balcony_props::GarmentName::HangingBedsheet => {
            vec![[
                [-0.5 * w, top],
                [0.5 * w, top],
                [0.5 * w, drop],
                [-0.5 * w, drop],
            ]]
        }
    };
    let across = [-along[1], along[0], 0.];
    for piece in pieces {
        let points = piece.map(place);
        // The atlas's plain cotton chart, as the cut garments use it.
        let uv = piece.map(|[x, y]| {
            [
                (520.5 + (x / w + 0.5).clamp(0., 1.) * 495.) / 1024.,
                1. - (520.5 + (y / drop).clamp(0., 1.) * 495.) / 1024.,
            ]
        });
        for side in [1., -1.] {
            drawing.pen.face(
                drawing.mesh,
                "balcony-cloth",
                &points,
                across.map(|v| v * side),
                &Tint::Color(shade(g.tint, if side > 0. { 0.94 } else { 0.88 })),
                1.,
                Some(&uv),
                None,
            )?;
        }
    }
    Ok(())
}
/// [`hang`] with fittings of `weight` (as [`washing_with`]); a meshed garment has none.
pub fn hang_with(
    drawing: &mut Fixture<'_, '_>,
    garment: &Garment,
    left: V3,
    right: V3,
    lean: f64,
    textured: bool,
    weight: f64,
) -> Result<(), String> {
    if !weight.is_finite() || !(1. ..=3.).contains(&weight) {
        return Err("Laundry fitting weight must lie in 1..3.".into());
    }
    let heavy = |plain: f64, bold: f64| plain + (bold - plain) * (weight - 1.);
    if left
        .iter()
        .chain(right.iter())
        .any(|v| !v.is_finite() || v.abs() > 10000.)
        || !lean.is_finite()
        || lean.abs() > 10.
    {
        return Err("Laundry anchors or lean exceed finite bounds.".into());
    }
    let detail = drawing.mesh.graphics_detail();
    if let Garment::Mesh(g) = garment {
        if detail.reduced() {
            return silhouette(drawing, g, left, right, lean);
        }
        return balcony_props::hang(drawing, g, left, right, lean, textured);
    }
    let Garment::Cut(g) = garment else {
        unreachable!()
    };
    let run = hypot(right[0] - left[0], right[1] - left[1]);
    if run < 1e-6 {
        return Ok(());
    }
    // A reduced level keeps every `step`th row and column of a cut's grid, and its edges.
    let step = detail.pick(1, 2, 4);
    fn thinned<T>(all: &[T], step: usize) -> impl Iterator<Item = &T> {
        let last = all.len().saturating_sub(1);
        all.iter()
            .enumerate()
            .filter(move |(i, _)| i % step == 0 || *i == last)
            .map(|(_, item)| item)
    }
    let along = [(right[0] - left[0]) / run, (right[1] - left[1]) / run];
    let middle: V3 = std::array::from_fn(|i| (left[i] + right[i]) / 2.);
    let depth = g.folds.depth * (1. + 0.8 * ((lean.abs() - 0.01) / 0.03).clamp(0., 1.));
    let slope = if g.cut.follows {
        (right[2] - left[2]) / run
    } else {
        0.
    };
    let place = |x: f64, y: f64, z: f64| {
        [
            middle[0] + along[0] * x - along[1] * z,
            middle[1] + along[1] * x + along[0] * z,
            middle[2] + slope * x - y,
        ]
    };
    for panel in &g.cut.panels {
        drawing
            .mesh
            .spend_work(panel.grid.iter().map(Vec::len).sum())?;
        let mut front = Vec::new();
        let mut back = Vec::new();
        for row in thinned(&panel.grid, step) {
            let mut a = Vec::new();
            let mut b = Vec::new();
            for f in thinned(row, step) {
                let wave = sin(f.x * g.folds.rate[0] + g.folds.phase[0] + f.y * g.folds.skew)
                    + 0.4
                        * sin(f.x * g.folds.rate[1] + g.folds.phase[1] - f.y * g.folds.skew * 1.7);
                let fall = f.y.max(0.) / g.drop;
                let y = f.y + g.tilt * f.x - f.loose * fall * depth * 0.5 * wave.abs();
                let x = f.x * (1. - 0.05 * f.loose * fall);
                let z = f.z
                    + f.loose * depth * wave
                    + lean * fall.powf(1.3) * (1. + 2.5 * x).clamp(0.3, 1.7);
                let crease = if f.facing[1] == 0. {
                    1.
                } else {
                    f.facing[1].signum()
                } * f.loose
                    * wave
                    * 0.1;
                let uv = [
                    (520.5 + f.s * 495.) / 1024.,
                    1. - (520.5 + f.t * 495.) / 1024.,
                ];
                a.push(PatchVertex {
                    at: place(x, y + f.gap * f.facing[0], z + f.gap * f.facing[1]),
                    uv,
                    tint: shade(g.tint, 0.94 + crease.min(0.06) - (-crease).max(0.)),
                });
                b.push(PatchVertex {
                    at: place(x, y - f.gap * f.facing[0], z - f.gap * f.facing[1]),
                    uv,
                    tint: shade(g.tint, 0.94 + (-crease).min(0.06) - crease.max(0.)),
                });
            }
            front.push(a);
            back.push(b);
        }
        drawing
            .pen
            .patch(drawing.mesh, "balcony-cloth", &front, false)?;
        drawing
            .pen
            .patch(drawing.mesh, "balcony-cloth", &back, true)?;
    }
    // Pegs, hanger bars and hooks are hairlines: a reduced level leaves them out.
    if detail.reduced() {
        return Ok(());
    }
    for fitting in &g.cut.fittings {
        match *fitting {
            Fitting::Pin(x) => drawing.bar(
                "paint",
                place(x, 0.045, 0.),
                place(x, -0.03, 0.),
                heavy(0.013, 0.02),
                g.fitting_tint,
            )?,
            Fitting::Bar([x0, x1, y]) => drawing.tube(
                "paint",
                &[place(x0, y, 0.), place(x1, y, 0.)],
                heavy(0.005, 0.009),
                g.fitting_tint,
                4,
            )?,
            Fitting::Hook(y) => {
                let mut points = vec![place(0., y, 0.)];
                let loop_radius = heavy(0.019, 0.026);
                for step in 0..=6 {
                    let angle = PI / 2. - step as f64 / 6. * PI * 1.25;
                    points.push(place(
                        loop_radius * cos(angle),
                        loop_radius * sin(angle),
                        0.,
                    ));
                }
                drawing.tube("paint", &points, heavy(0.003, 0.007), g.fitting_tint, 4)?;
            }
        }
    }
    Ok(())
}
