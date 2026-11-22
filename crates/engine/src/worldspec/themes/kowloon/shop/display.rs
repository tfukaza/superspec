//! Self-supporting street displays with conservative placement envelopes.
use super::super::{
    fixture::Fixture,
    palette::{shade, srgb},
    pen::Pen,
    timber,
};
use super::{
    furniture::Shelf,
    grocer_props::{self, Options},
    stock::{self, Grocery as G},
    stock_mesh,
};
use superspec_geometry::V3;
use superspec_types::rng::SeededRng;
#[derive(Clone, Copy, serde::Serialize, serde::Deserialize, Debug, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Stool,
    BoxStand,
    CrateShelf,
    Cart,
}
pub const KINDS: [Kind; 4] = [Kind::Stool, Kind::BoxStand, Kind::CrateShelf, Kind::Cart];
impl Kind {
    pub fn size(self) -> V3 {
        match self {
            Self::Stool => [0.46, 0.4, 0.7],
            Self::BoxStand => [0.46, 0.4, 0.85],
            Self::CrateShelf => [0.8, 0.42, 1.05],
            Self::Cart => [0.78, 0.5, 0.95],
        }
    }
}
#[derive(Clone, Copy, serde::Serialize, serde::Deserialize, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Trade {
    Grocer,
    Greengrocer,
    Herbalist,
    Hardware,
}
pub struct Settings<'a> {
    pub timber: &'a timber::Settings,
    pub props: bool,
    pub apples: bool,
}
fn on<'a, 'c>(d: &'a mut Fixture<'_, 'c>, pen: Pen) -> Fixture<'a, 'c> {
    Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    }
}
fn prop(
    d: &mut Fixture<'_, '_>,
    s: &Settings<'_>,
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
            textured: s.props,
            variant,
            ..Default::default()
        },
    )
    .map(|_| ())
}
fn packing(
    d: &mut Fixture<'_, '_>,
    s: &Settings<'_>,
    at: V3,
    size: V3,
    rng: &mut SeededRng,
) -> Result<(), String> {
    prop(d, s, "stock-box", at, size, 0., rng.integer(1, 4))
}
fn board(
    d: &mut Fixture<'_, '_>,
    s: &Settings<'_>,
    low: V3,
    high: V3,
    tint: V3,
) -> Result<(), String> {
    let pen = d.pen.with_timber(s.timber.clone())?;
    on(d, pen).cuboid("timber", low, high, tint, "")
}
fn produce_tray(
    d: &mut Fixture<'_, '_>,
    s: &Settings<'_>,
    z: f64,
    width: f64,
) -> Result<(), String> {
    let scale = width / 0.4;
    prop(
        d,
        s,
        "produce-crate",
        [0., 0., z],
        [width, 0.31 * scale, 0.071 * scale],
        0.,
        0,
    )?;
    stock_mesh::apple_pile(
        d,
        [0., 0., z + 0.014 * scale],
        [0.375 * scale, 0.285 * scale, 0.095 * scale],
        srgb("#b32c24"),
        s.apples,
    )
}
fn goods(
    d: &mut Fixture<'_, '_>,
    s: &Settings<'_>,
    z: f64,
    width: f64,
    depth: f64,
    trade: Trade,
    rng: &mut SeededRng,
) -> Result<(), String> {
    if trade == Trade::Greengrocer || (trade == Trade::Grocer && rng.chance(0.65)) {
        let count = if width > 0.5 { 2 } else { 1 };
        let gap = 0.012;
        let tray_width = 0.4_f64
            .min((width - gap * (count - 1) as f64) / count as f64)
            .min(depth * 0.4 / 0.31);
        for index in 0..count {
            let pen = d.pen.moved([
                (index as f64 - (count - 1) as f64 / 2.) * (tray_width + gap),
                0.,
                0.,
            ])?;
            produce_tray(&mut on(d, pen), s, z, tray_width)?;
        }
        return Ok(());
    }
    let kinds: &[G] = if trade == Trade::Hardware {
        &[G::Cartons, G::Rolls]
    } else {
        &[
            G::SmallJars,
            G::SmallTins,
            G::Tins,
            G::SmallBottles,
            G::DryBags,
        ]
    };
    let callbacks: Vec<_> = kinds
        .iter()
        .map(|&kind| {
            move |r: &mut SeededRng| {
                let mut line = stock::grocery(kind, r, s.props)?;
                line.stack = 1;
                Ok(line)
            }
        })
        .collect();
    let makers: Vec<stock::Maker<'_>> = callbacks.iter().map(|f| f as stock::Maker<'_>).collect();
    stock::shelf(
        d,
        &Shelf {
            x0: -width / 2. + 0.015,
            x1: width / 2. - 0.015,
            y: -depth / 2. + 0.01,
            depth: depth - 0.02,
            z,
            clear: 0.22,
        },
        &makers,
        rng,
        stock::Settings {
            fill: 1.,
            maximum_rows: if width > 0.5 { 2 } else { 3 },
            packed: true,
        },
    )
}
pub fn draw(
    d: &mut Fixture<'_, '_>,
    kind: Kind,
    trade: Trade,
    rng: &mut SeededRng,
    s: &Settings<'_>,
) -> Result<(), String> {
    d.mesh.check_work(0)?;
    let wood = srgb("#94704d");
    match kind {
        Kind::Stool | Kind::BoxStand => {
            let top = if kind == Kind::Stool {
                let size = grocer_props::size("shop-stool-detailed", [0.31, 0.31, 0.45])?;
                prop(d, s, "shop-stool-detailed", [0.; 3], size, 0., 0)?;
                size[2]
            } else {
                packing(d, s, [0.; 3], [0.34, 0.3, 0.24], rng)?;
                packing(d, s, [0.008, -0.008, 0.2405], [0.32, 0.28, 0.22], rng)?;
                0.461
            };
            board(d, s, [-0.21, -0.17, top], [0.21, 0.17, top + 0.018], wood)?;
            goods(d, s, top + 0.018, 0.4, 0.31, trade, rng)
        }
        Kind::CrateShelf => {
            let size = grocer_props::size("bottle-crate", [0.28, 0.28, 0.27])?;
            let base = size[2];
            for x in [-0.22, 0.22] {
                prop(d, s, "bottle-crate", [x, 0., 0.], size, 0., 0)?;
            }
            board(d, s, [-0.39, -0.19, base], [0.39, 0.19, base + 0.02], wood)?;
            for x in [-0.23, 0.23] {
                packing(d, s, [x, 0., base + 0.02], [0.23, 0.29, 0.22], rng)?;
            }
            let shelf = base + 0.2405;
            board(
                d,
                s,
                [-0.38, -0.18, shelf],
                [0.38, 0.18, shelf + 0.018],
                shade(wood, 0.85),
            )?;
            goods(d, s, shelf + 0.018, 0.72, 0.32, trade, rng)
        }
        Kind::Cart => {
            let metal = shade(srgb("#a4a9aa"), 0.7);
            let rubber = srgb("#34332f");
            for x in [-0.31, 0.31] {
                for y in [-0.17, 0.17] {
                    d.tube(
                        "paint",
                        &[[x - 0.019, y, 0.055], [x + 0.019, y, 0.055]],
                        0.055,
                        rubber,
                        12,
                    )?;
                    d.disc(
                        "paint",
                        [x - 0.019, y, 0.055],
                        0.055,
                        [-1., 0., 0.],
                        rubber,
                        12,
                    )?;
                    d.disc(
                        "paint",
                        [x + 0.019, y, 0.055],
                        0.055,
                        [1., 0., 0.],
                        rubber,
                        12,
                    )?;
                    d.bar("metal", [x, y, 0.055], [x, y, 0.15], 0.015, metal)?;
                }
            }
            for z in [0.15, 0.55] {
                board(d, s, [-0.36, -0.22, z], [0.36, 0.22, z + 0.02], wood)?;
                for y in [-0.21, 0.21] {
                    d.bar("metal", [-0.35, y, z], [0.35, y, z], 0.014, metal)?;
                }
            }
            for x in [-0.35, 0.35] {
                for y in [-0.21, 0.21] {
                    d.bar("metal", [x, y, 0.15], [x, y, 0.59], 0.015, metal)?;
                }
            }
            for y in [-0.18, 0.18] {
                d.bar("metal", [0.35, y, 0.56], [0.35, y, 0.88], 0.016, metal)?;
            }
            d.bar(
                "metal",
                [0.35, -0.18, 0.88],
                [0.35, 0.18, 0.88],
                0.018,
                metal,
            )?;
            packing(d, s, [-0.14, 0., 0.17], [0.3, 0.35, 0.23], rng)?;
            prop(
                d,
                s,
                "seasoning-jar",
                [0.19, 0., 0.17],
                [0.12, 0.12, 0.18],
                rng.float(-0.25, 0.25),
                rng.integer(1, 8),
            )?;
            goods(d, s, 0.57, 0.62, 0.37, trade, rng)
        }
    }
}
