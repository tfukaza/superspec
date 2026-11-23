//! Shelf stocking keeps product selection separate from placement and label variation.
use super::super::{fixture::Fixture, graphics_detail::GraphicsDetail};
use super::{
    forms,
    furniture::Shelf,
    grocer_props::{self, Group, Options},
};
use std::f64::consts::PI;
use superspec_geometry::{
    V3,
    math::{cos, sin},
};
use superspec_types::rng::SeededRng;
type Draw = Box<dyn FnMut(&mut Fixture<'_, '_>, V3) -> Result<(), String>>;
pub struct Line {
    pub width: f64,
    pub depth: f64,
    pub height: f64,
    pub stack: usize,
    pub draw: Draw,
}
pub type Maker<'a> = &'a dyn Fn(&mut SeededRng) -> Result<Line, String>;
impl Line {
    fn validate(&self) -> Result<(), String> {
        if [self.width, self.depth, self.height]
            .iter()
            .any(|v| !v.is_finite() || !(0.001..=100.).contains(v))
            || !(1..=64).contains(&self.stack)
        {
            Err("Stock line requires bounded positive dimensions and stack count.".into())
        } else {
            Ok(())
        }
    }
}
#[derive(Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Grocery {
    Bottles,
    Tins,
    Cartons,
    Rolls,
    Jars,
    BigJars,
    SmallJars,
    SmallBottles,
    SmallTins,
    LargeTins,
    DryBags,
    Boxes,
    TallBoxes,
}
pub fn grocery(kind: Grocery, rng: &mut SeededRng, textured: bool) -> Result<Line, String> {
    if matches!(kind, Grocery::DryBags) {
        let variation = rng.float(0.88, 1.);
        let envelope = [0.41, 0.17, 0.22].map(|v| v * variation);
        let mut arrangement = rng.integer(0, 3);
        let mut size = [0_f64; 3];
        for index in 0..4 {
            for mirrored in [false, true] {
                let fit = grocer_props::bag_size("dry-goods-bag", envelope, mirrored, index)?;
                for axis in 0..3 {
                    size[axis] = size[axis].max(fit[axis]);
                }
            }
        }
        let mut variant = rng.integer(1, 4);
        return Ok(Line {
            width: size[0] + 0.002,
            depth: size[1] + 0.002,
            height: size[2],
            stack: 1,
            draw: Box::new(move |d, at| {
                grocer_props::bag_cluster(
                    d,
                    "dry-goods-bag",
                    at,
                    envelope,
                    &Group {
                        yaw: 0.,
                        variant,
                        arrangement,
                        textured,
                    },
                )?;
                variant = variant % 4 + 1;
                arrangement = (arrangement + 1) % 4;
                Ok(())
            }),
        });
    }
    let (name, envelope, stack) = match kind {
        Grocery::Bottles => ("drink-bottle", [0.075, 0.075, 0.28], 1),
        Grocery::Tins => ("food-tin", [0.09, 0.09, 0.12], 3),
        Grocery::Cartons => ("tea-carton", [0.14, 0.10, 0.21], 2),
        Grocery::Rolls => ("tissue-roll", [0.11, 0.11, 0.1], 3),
        Grocery::Jars => ("seasoning-jar", [0.11, 0.11, 0.16], 1),
        Grocery::BigJars => ("seasoning-jar", [0.17, 0.17, 0.26], 1),
        Grocery::SmallJars => ("seasoning-jar", [0.075, 0.075, 0.115], 2),
        Grocery::SmallBottles => ("drink-bottle", [0.052, 0.052, 0.17], 1),
        Grocery::SmallTins => ("food-tin", [0.065, 0.065, 0.075], 3),
        Grocery::LargeTins => ("food-tin", [0.13, 0.13, 0.18], 2),
        Grocery::Boxes => ("stock-box", [0.30, 0.21, 0.19], 2),
        Grocery::TallBoxes => ("stock-box", [0.20, 0.19, 0.29], 1),
        Grocery::DryBags => unreachable!(),
    };
    let variation = rng.float(0.88, 1.);
    let size = grocer_props::size(name, envelope.map(|v| v * variation))?;
    let tint = [
        rng.float(0.94, 1.),
        rng.float(0.94, 1.),
        rng.float(0.94, 1.),
    ];
    let count = grocer_props::variants(name)?;
    let mut variant = rng.integer(1, count);
    Ok(Line {
        width: size[0] + 0.002,
        depth: size[1] + 0.002,
        height: size[2],
        stack,
        draw: Box::new(move |d, at| {
            grocer_props::draw(
                d,
                name,
                at,
                size,
                0.,
                &Options {
                    tint,
                    supported: true,
                    variant,
                    textured,
                },
            )?;
            variant = variant % count + 1;
            Ok(())
        }),
    })
}
#[derive(Clone, Copy, serde::Deserialize)]
pub struct Settings {
    pub fill: f64,
    pub maximum_rows: usize,
    pub packed: bool,
}
fn fitting(
    makers: &[Maker<'_>],
    shelf: &Shelf,
    available: f64,
    rng: &mut SeededRng,
) -> Result<Option<Line>, String> {
    let mut order: Vec<_> = (0..makers.len()).collect();
    for i in (1..order.len()).rev() {
        order.swap(i, rng.integer(0, i));
    }
    for i in order {
        let product = makers[i](rng)?;
        product.validate()?;
        if product.width <= available
            && product.height <= shelf.clear
            && product.depth <= shelf.depth
        {
            return Ok(Some(product));
        }
    }
    Ok(None)
}
/// Packed shelves use measured rotated footprints; sold gaps can expose a rear row.
pub fn shelf(
    d: &mut Fixture<'_, '_>,
    shelf: &Shelf,
    makers: &[Maker<'_>],
    rng: &mut SeededRng,
    s: Settings,
) -> Result<(), String> {
    if [
        shelf.x0,
        shelf.x1,
        shelf.y,
        shelf.depth,
        shelf.z,
        shelf.clear,
        s.fill,
    ]
    .iter()
    .any(|v| !v.is_finite() || v.abs() > 1000.)
        || shelf.x1 < shelf.x0
        || shelf.x1 - shelf.x0 > 40.
        || !(0.001..=20.).contains(&shelf.depth)
        || !(0.001..=20.).contains(&shelf.clear)
        || !(0. ..=1.).contains(&s.fill)
        || !(1..=8).contains(&s.maximum_rows)
        || !(1..=64).contains(&makers.len())
    {
        return Err(
            "Shelf stocking requires bounded surfaces, fill, rows and product choices.".into(),
        );
    }
    let packed = s.packed;
    let pen = d.pen.with_shelf_patina(super::patina::Shelf::Stock {
        inverse: d.pen.matrix.inverse()?,
        shelf: *shelf,
    });
    let d = &mut Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    };
    let mut x = shelf.x0;
    let mut stocked = false;
    while x < shelf.x1 - 0.05 {
        d.mesh.spend_work(1)?;
        let product = if packed {
            fitting(makers, shelf, shelf.x1 - x, rng)?
        } else {
            Some(makers[rng.integer(0, makers.len() - 1)](rng)?)
        };
        let Some(mut line) = product else {
            break;
        };
        line.validate()?;
        let mut placement = SeededRng::from_u32((rng.next_f64() * 4294967295.) as u32);
        let facings = rng.integer(1, 4);
        let rows = s
            .maximum_rows
            .min((shelf.depth / (line.depth + 0.006)).floor() as usize)
            .max(1);
        let levels = line
            .stack
            .min((shelf.clear / line.height).floor() as usize)
            .max(1);
        if line.height > shelf.clear || line.depth > shelf.depth {
            x += 0.02;
            continue;
        }
        let row_depth = shelf.depth / rows as f64;
        let setback = placement.float(0.05, if packed { 0.4 } else { 0.85 });
        let massed = d.mesh.graphics_detail() == GraphicsDetail::VeryLow;
        let mut run = None;
        for _ in 0..facings {
            if x + line.width > shelf.x1 {
                break;
            }
            let mut turn = placement.float(
                if packed { -0.25 } else { -0.38 },
                if packed { 0.25 } else { 0.38 },
            );
            if placement.chance(0.16) {
                turn += placement.pick(&[-1., 1.]) * PI / 2.;
            }
            let mut width = cos(turn).abs() * line.width + sin(turn).abs() * line.depth;
            let mut depth = sin(turn).abs() * line.width + cos(turn).abs() * line.depth;
            if width > shelf.x1 - x || depth > row_depth {
                turn = 0.;
                width = line.width;
                depth = line.depth;
            }
            let gap = (if packed {
                placement.float(0.001, 0.003)
            } else {
                placement.float(0.003, 0.012)
            })
            .min(shelf.x1 - x - width);
            let sold = rng.chance(if packed { 0.005 } else { 0.04 }) && stocked;
            for row in 0..rows {
                if if row == 0 {
                    sold
                } else {
                    !sold && shelf.z > 1.
                } {
                    continue;
                }
                let high = if packed {
                    if rng.chance(0.85) {
                        levels
                    } else {
                        levels.saturating_sub(1).max(1)
                    }
                } else if row == 0 {
                    rng.integer(1, levels)
                } else {
                    levels
                };
                let offset =
                    (setback + placement.float(-0.22, 0.22)).clamp(0., 1.) * (row_depth - depth);
                let slip = placement.float(0., gap);
                // A reduced level stocks the front row only; a rear row shows where the
                // front facing is sold.
                if row > 0 && !sold && d.mesh.graphics_detail().reduced() {
                    continue;
                }
                if massed {
                    // The lowest level sweeps the line's front row as one block below.
                    if row == 0 {
                        let (from, tallest, y) = run.map_or(
                            (x, 0, shelf.y + depth / 2. + offset),
                            |(from, _, tallest, y): (f64, f64, usize, f64)| (from, tallest, y),
                        );
                        run = Some((from, x + width, tallest.max(high), y));
                    }
                    stocked = true;
                    continue;
                }
                let pen = forms::rotated(
                    &d.pen,
                    [
                        x + width / 2. + slip,
                        shelf.y + row as f64 * row_depth + depth / 2. + offset,
                        shelf.z,
                    ],
                    2,
                    turn,
                )?;
                let stack = &mut Fixture {
                    pen,
                    mesh: d.mesh,
                    repeats: d.repeats,
                };
                for level in 0..high {
                    (line.draw)(stack, [0., 0., level as f64 * line.height])?;
                }
                stocked = true;
            }
            x += width + gap;
        }
        if let Some((from, to, tallest, y)) = run {
            let piece = grocer_props::captured(d, |scratch| (line.draw)(scratch, [0.; 3]))?;
            let stretch = (to - from) / line.width;
            let points: Vec<_> = piece
                .into_iter()
                .map(|(p, color)| {
                    (
                        [
                            (from + to) / 2. + p[0] * stretch,
                            y + p[1],
                            shelf.z + p[2] * tallest as f64,
                        ],
                        color,
                    )
                })
                .collect();
            let pen = d.pen.clone();
            grocer_props::mass(d, &pen, &points, GraphicsDetail::VeryLow, true)?;
        }
        x += if packed {
            rng.float(0.002, 0.006)
        } else if rng.chance(s.fill) {
            rng.float(0.006, 0.018)
        } else {
            rng.float(0.035, 0.07)
        };
    }
    Ok(())
}
pub fn groceries(
    d: &mut Fixture<'_, '_>,
    surface: &Shelf,
    kinds: &[Grocery],
    rng: &mut SeededRng,
    s: Settings,
    textured: bool,
) -> Result<(), String> {
    if !(1..=64).contains(&kinds.len()) {
        return Err("Shelf stocking requires 1–64 product choices.".into());
    }
    let factories: Vec<_> = kinds
        .iter()
        .map(|&kind| move |rng: &mut SeededRng| grocery(kind, rng, textured))
        .collect();
    let makers: Vec<Maker<'_>> = factories.iter().map(|f| f as Maker<'_>).collect();
    shelf(d, surface, &makers, rng, s)
}
