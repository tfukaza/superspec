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
            hang(
                drawing,
                &g,
                [x, 0., z],
                [x + g.width(), 0., z],
                rng.float(-0.005, 0.005),
                textured,
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
            hang(
                drawing,
                &g,
                [x, -half, height],
                [x, half, height],
                rng.float(-0.01, 0.01),
                textured,
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
        && let Some(g) = balcony_props::choose(rng, room, support)?
    {
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
    if left
        .iter()
        .chain(right.iter())
        .any(|v| !v.is_finite() || v.abs() > 10000.)
        || !lean.is_finite()
        || lean.abs() > 10.
    {
        return Err("Laundry anchors or lean exceed finite bounds.".into());
    }
    if let Garment::Mesh(g) = garment {
        return balcony_props::hang(drawing, g, left, right, lean, textured);
    }
    let Garment::Cut(g) = garment else {
        unreachable!()
    };
    let run = hypot(right[0] - left[0], right[1] - left[1]);
    if run < 1e-6 {
        return Ok(());
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
        for row in &panel.grid {
            let mut a = Vec::new();
            let mut b = Vec::new();
            for f in row {
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
    for fitting in &g.cut.fittings {
        match *fitting {
            Fitting::Pin(x) => drawing.bar(
                "paint",
                place(x, 0.045, 0.),
                place(x, -0.03, 0.),
                0.013,
                g.fitting_tint,
            )?,
            Fitting::Bar([x0, x1, y]) => drawing.tube(
                "paint",
                &[place(x0, y, 0.), place(x1, y, 0.)],
                0.005,
                g.fitting_tint,
                4,
            )?,
            Fitting::Hook(y) => {
                let mut points = vec![place(0., y, 0.)];
                for step in 0..=6 {
                    let angle = PI / 2. - step as f64 / 6. * PI * 1.25;
                    points.push(place(0.019 * cos(angle), 0.019 * sin(angle), 0.));
                }
                drawing.tube("paint", &points, 0.003, g.fitting_tint, 4)?;
            }
        }
    }
    Ok(())
}
