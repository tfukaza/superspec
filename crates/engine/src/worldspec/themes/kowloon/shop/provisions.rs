//! Fixed shelf banks and street stock for the reference corner provisions shop.
use super::{
    fittings, forms, furniture, goods, grocer_props,
    room::{Interior, Room},
    snack_string,
    stock::{self, Grocery, Settings},
};
use crate::worldspec::themes::kowloon::{
    fixture::Fixture,
    kit::Kit,
    laundry,
    palette::{shade, srgb},
    pen::{Pen, Tint},
};
use superspec_geometry::{
    V3,
    math::{cos, sin},
};
use superspec_types::rng::SeededRng;

fn on<'a, 'c>(d: &'a mut Fixture<'_, 'c>, pen: Pen) -> Fixture<'a, 'c> {
    Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    }
}

pub(super) fn interior() -> Interior {
    Interior {
        floor: srgb("#747168"),
        ceiling: srgb("#b3ac99"),
        wall: srgb("#b3aa91"),
        dado: srgb("#4c655b"),
        dado_height: 1.1,
    }
}

/// The fitted shop's room: grimy plaster over a dark green dado, a smoked ceiling and a
/// worn floor, so the packed shop reads dim under its tubes.
pub(super) fn fitted_interior() -> Interior {
    Interior {
        floor: srgb("#5f5a50"),
        ceiling: srgb("#7d7668"),
        wall: srgb("#857a63"),
        dado: srgb("#34463e"),
        dado_height: 1.2,
    }
}

fn rows(
    d: &mut Fixture<'_, '_>,
    shelves: &[furniture::Shelf],
    goods: &[Grocery],
    kit: &Kit,
    key: &str,
    settings: Settings,
    textured: bool,
) -> Result<(), String> {
    let factories: Vec<_> = goods
        .iter()
        .map(|&kind| move |rng: &mut _| stock::grocery(kind, rng, textured))
        .collect();
    let makers: Vec<_> = factories.iter().map(|f| f as stock::Maker<'_>).collect();
    for (index, level) in shelves.iter().enumerate() {
        stock::shelf(
            d,
            level,
            &makers,
            &mut kit.rng(&format!("{key}/{index}"))?,
            settings,
        )?;
    }
    Ok(())
}

pub(super) fn furnish(d: &mut Fixture<'_, '_>, room: &Room<'_>) -> Result<(), String> {
    use Grocery::*;
    let wood = srgb("#78644b");
    let timber = &room.kit.settings().timber;
    let root = d.pen.clone();
    let textured = room.assets.grocer_props;
    for (i, [x, width, height]) in [[0.08, 2.86, 2.22], [3.06, 2.84, 2.46]]
        .into_iter()
        .enumerate()
    {
        let mut bank = on(
            d,
            root.into_part(&format!("back-stock-{i}"))?
                .moved([x, room.depth - 0.39, 0.])?,
        );
        let shelves = furniture::shelving(
            &mut bank,
            [width, 0.35, height],
            6,
            "timber",
            wood,
            0.2,
            timber,
        )?;
        rows(
            &mut bank,
            &shelves,
            &[Boxes, TallBoxes, Tins, SmallJars, Bottles, DryBags],
            room.kit,
            &format!("back-stock-{i}"),
            Settings {
                fill: 0.96,
                maximum_rows: 2,
                packed: true,
            },
            textured,
        )?;
    }
    let mut bank = on(
        d,
        root.into_part("side-stock")?
            .facing([0.25, 0.8, 0.], [0., 1., 0.], [-1., 0., 0.])?,
    );
    let shelves =
        furniture::shelving(&mut bank, [1.8, 0.32, 2.16], 5, "timber", wood, 0.2, timber)?;
    rows(
        &mut bank,
        &shelves,
        &[Cartons, SmallBottles, SmallJars],
        room.kit,
        "side-stock",
        Settings {
            fill: 0.95,
            maximum_rows: 1,
            packed: true,
        },
        textured,
    )?;
    let mut counter = on(d, root.into_part("counter")?.moved([0.1, 0.39, 0.])?);
    let top = furniture::counter(
        &mut counter,
        [1.3, 0.56, 0.93],
        furniture::Finish::Timber,
        wood,
        &mut room.stream("counter")?,
        timber,
    )?;
    rows(
        &mut counter,
        &[top],
        &[SmallJars, Cartons],
        room.kit,
        "counter-stock",
        Settings {
            fill: 0.75,
            maximum_rows: 1,
            packed: false,
        },
        textured,
    )?;
    snack_string::row(
        d,
        [0.22, 2.25, 0.55, 2.53],
        &mut room.stream("hung-packets")?,
        room.assets.snack_packets,
    )?;
    let cloth = d.mesh.has_source_material("balcony-cloth");
    laundry::washing(
        &mut on(d, root.moved([0.36, 0.25, 0.])?),
        [0., 1.45],
        2.48,
        &mut room.stream("shop-cloths")?,
        &laundry::WashingOptions {
            fill: 1.,
            drop: 1.08,
            support: laundry::Support::Pole,
        },
        cloth,
    )?;
    fittings::tube_light(d, [0.38, 1.22, 2.69], [2.3, 1.22, 2.69])?;
    for (i, [x, y, size, height]) in [
        [0.7, 1.42, 0.48, 0.72],
        [1.24, 1.7, 0.58, 1.02],
        [4.65, 2.2, 0.56, 0.8],
        [5.36, 1.69, 0.43, 0.63],
    ]
    .into_iter()
    .enumerate()
    {
        goods::carton_stack(
            d,
            [x, y, 0.025],
            size,
            height,
            &mut room.stream(&format!("inside-cartons-{i}"))?,
            textured,
        )?;
    }
    Ok(())
}

// Low-detail stock for the fitted shop and its street stock. A shop holds thousands of goods
// seen from the street: each is a few flat or arc faces coloured per vertex, without the
// bottoms, backs and far sides nobody sees. Every piece's front faces -Y of its frame.

const PLASTICS: [&str; 6] = [
    "#a8554a", "#557f66", "#d6cba8", "#71899a", "#a8554a", "#d6cba8",
];
const CARDBOARD: [&str; 4] = ["#9c7a4e", "#8f6e47", "#a6875d", "#82653f"];
// Dusty, sun-faded print; a few goods carry one of the saturated `ACCENTS` instead.
const LABELS: [&str; 12] = [
    "#94554a", "#a8925a", "#4f6478", "#58745c", "#b4ad99", "#a0724c", "#66546a", "#9a6a6c",
    "#55736e", "#ab9f66", "#743a36", "#bfb6a0",
];
const GLASS: [&str; 5] = ["#3f6a38", "#6a431c", "#b9c4bd", "#274a36", "#8a5a22"];
const CLOTHS: [&str; 5] = ["#c09a3a", "#cdc7b5", "#8a9cab", "#b58c90", "#c09a3a"];

fn tone(rng: &mut SeededRng, palette: &[&str]) -> V3 {
    shade(srgb(rng.pick(palette)), rng.float(0.74, 1.))
}

/// A label's colour: one of `palette`, or `share` of the time a saturated accent.
fn accented(rng: &mut SeededRng, palette: &[&str], share: f64) -> V3 {
    if rng.chance(share) {
        tone(rng, &ACCENTS)
    } else {
        tone(rng, palette)
    }
}

/// The street-facing arc of a turned profile (`[radius, height]` points from the foot up, one
/// tint per segment): `steps` faces over 210 degrees about -Y, smooth across them.
fn arc(
    d: &mut Fixture<'_, '_>,
    base: V3,
    profile: &[[f64; 2]],
    tints: &[V3],
    steps: usize,
) -> Result<(), String> {
    let ring: Vec<[f64; 2]> = (0..=steps)
        .map(|k| {
            let angle = (-195. + 210. * k as f64 / steps as f64) * std::f64::consts::PI / 180.;
            [cos(angle), sin(angle)]
        })
        .collect();
    let repeat = d.repeat("paint");
    for (i, p) in profile.windows(2).enumerate() {
        if p[1][1] - p[0][1] < 1e-6 {
            continue;
        }
        let tint = Tint::Color(tints[i.min(tints.len() - 1)]);
        for k in 0..steps {
            let (a, b) = (ring[k], ring[k + 1]);
            let at = |c: [f64; 2], q: [f64; 2]| {
                [base[0] + c[0] * q[0], base[1] + c[1] * q[0], base[2] + q[1]]
            };
            d.pen.face(
                d.mesh,
                "paint",
                &[at(a, p[0]), at(b, p[0]), at(b, p[1]), at(a, p[1])],
                [a[0] + b[0], a[1] + b[1], 0.],
                &tint,
                repeat,
                None,
                Some(&[
                    [a[0], a[1], 0.],
                    [b[0], b[1], 0.],
                    [b[0], b[1], 0.],
                    [a[0], a[1], 0.],
                ]),
            )?;
        }
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq)]
enum Ware {
    Tins,
    Jars,
    BigJars,
    Bottles,
    Boxes,
    TallBoxes,
    Bags,
}
impl Ware {
    /// Width, depth, height and how many stand on one another.
    fn size(self) -> (V3, usize) {
        match self {
            Self::Tins => ([0.085, 0.085, 0.112], 3),
            Self::Jars => ([0.09, 0.09, 0.135], 2),
            Self::BigJars => ([0.15, 0.15, 0.24], 1),
            Self::Bottles => ([0.072, 0.072, 0.27], 1),
            Self::Boxes => ([0.27, 0.2, 0.165], 3),
            Self::TallBoxes => ([0.18, 0.16, 0.265], 1),
            Self::Bags => ([0.21, 0.15, 0.2], 1),
        }
    }
}

/// One facing of a ware: `levels` of it stacked with its front-centre foot at `at`.
fn ware(
    d: &mut Fixture<'_, '_>,
    kind: Ware,
    at: V3,
    [w, depth, h]: V3,
    levels: usize,
    [label, body]: [V3; 2],
    rng: &mut SeededRng,
) -> Result<(), String> {
    let [x, y, z] = at;
    let centre = [x, y + depth / 2., z];
    let r = w / 2.;
    match kind {
        Ware::Tins => {
            // A paper label under a band of its second colour.
            let mut profile = vec![[r, 0.]];
            let mut tints = vec![];
            for i in 0..levels {
                let z0 = i as f64 * h;
                profile.extend([[r, z0 + h * 0.6], [r, z0 + h]]);
                tints.extend([shade(label, rng.float(0.85, 1.08)), body]);
            }
            arc(d, centre, &profile, &tints, 3)?;
            d.disc(
                "paint",
                [centre[0], centre[1], z + levels as f64 * h],
                r,
                [0., 0., 1.],
                srgb("#9a9c96"),
                6,
            )
        }
        Ware::Jars | Ware::BigJars => {
            let mut profile = vec![];
            let mut tints = vec![];
            // What the jar holds shows above its label band; the lid is the label's colour.
            let lid = shade(label, 0.7);
            for i in 0..levels {
                let z0 = i as f64 * h;
                profile.extend([
                    [r, z0],
                    [r, z0 + h * 0.46],
                    [r, z0 + h * 0.76],
                    [r * 0.9, z0 + h * 0.76],
                ]);
                tints.extend([label, shade(body, 0.62), lid, lid]);
            }
            profile.push([r * 0.9, levels as f64 * h]);
            arc(d, centre, &profile, &tints, 3)?;
            d.disc(
                "paint",
                [centre[0], centre[1], z + levels as f64 * h],
                r * 0.9,
                [0., 0., 1.],
                shade(label, 0.85),
                6,
            )
        }
        // Glass with a label band round its body and a cap on its neck.
        Ware::Bottles => arc(
            d,
            centre,
            &[
                [r, 0.],
                [r, h * 0.2],
                [r, h * 0.54],
                [r * 0.4, h * 0.76],
                [r * 0.4, h * 0.9],
                [r * 0.48, h * 0.9],
                [r * 0.48, h],
            ],
            &[body, label, body, body, body, shade(label, 0.75)],
            3,
        ),
        Ware::Boxes | Ware::TallBoxes => {
            for level in 0..levels {
                let inset = rng.float(0., 0.006);
                let (z0, z1) = (z + level as f64 * h, z + (level + 1) as f64 * h - 0.002);
                d.cuboid(
                    "paint",
                    [x - r + inset, y + inset, z0],
                    [x + r - inset, y + depth, z1],
                    shade(label, rng.float(0.86, 1.06)),
                    if level + 1 == levels { "df" } else { "dfu" },
                )?;
                // Its printed panel.
                let front = y + inset - 0.002;
                d.face(
                    "paint",
                    &[
                        [x - r * 0.62, front, z0 + h * 0.24],
                        [x + r * 0.62, front, z0 + h * 0.24],
                        [x + r * 0.62, front, z0 + h * 0.74],
                        [x - r * 0.62, front, z0 + h * 0.74],
                    ],
                    [0., -1., 0.],
                    body,
                )?;
            }
            Ok(())
        }
        Ware::Bags => {
            let (top, lean) = (r * 0.78, depth * 0.3);
            let foot = [
                [x - r, y, z],
                [x + r, y, z],
                [x + r, y + depth, z],
                [x - r, y + depth, z],
            ];
            let head = [
                [x - top, y + lean, z + h],
                [x + top, y + lean, z + h],
                [x + top, y + depth, z + h],
                [x - top, y + depth, z + h],
            ];
            d.face(
                "paint",
                &[foot[0], foot[1], head[1], head[0]],
                [0., -1., 0.3],
                label,
            )?;
            d.face(
                "paint",
                &[foot[0], head[0], head[3], foot[3]],
                [-1., 0., 0.2],
                shade(label, 0.85),
            )?;
            d.face(
                "paint",
                &[foot[1], foot[2], head[2], head[1]],
                [1., 0., 0.2],
                shade(label, 0.85),
            )?;
            d.face("paint", &head, [0., 0., 1.], shade(body, 0.9))?;
            // The print on its front.
            let at = |u: f64, t: f64| {
                let (a, b) = (
                    [x - r + (r - top) * t, y + lean * t - 0.003, z + h * t],
                    [x + r - (r - top) * t, y + lean * t - 0.003, z + h * t],
                );
                [a[0] + (b[0] - a[0]) * u, a[1], a[2]]
            };
            d.face(
                "paint",
                &[at(0.2, 0.3), at(0.8, 0.3), at(0.8, 0.72), at(0.2, 0.72)],
                [0., -1., 0.3],
                body,
            )
        }
    }
}

/// Packs one shelf with facings of the given wares, front row only.
fn fill(
    d: &mut Fixture<'_, '_>,
    shelf: &furniture::Shelf,
    wares: &[Ware],
    rng: &mut SeededRng,
    fullness: f64,
) -> Result<(), String> {
    let mut x = shelf.x0;
    while x < shelf.x1 - 0.06 {
        d.mesh.spend_work(1)?;
        let kind = rng.pick(wares);
        let (size, stack) = kind.size();
        let scale = rng.float(0.86, 1.);
        let [w, depth, h] = size.map(|v| v * scale);
        let facings = rng.integer(2, 5);
        if h > shelf.clear || depth > shelf.depth {
            x += 0.03;
            continue;
        }
        if !rng.chance(fullness) {
            x += rng.float(0.08, 0.2);
            continue;
        }
        let levels = stack.min((shelf.clear / h) as usize).max(1);
        let boxed = matches!(kind, Ware::Boxes | Ware::TallBoxes | Ware::Bags);
        let label = if boxed && rng.chance(0.55) {
            tone(rng, &CARDBOARD)
        } else {
            accented(rng, &LABELS, 0.06)
        };
        let body = if matches!(kind, Ware::Bottles) {
            tone(rng, &GLASS)
        } else {
            accented(rng, &LABELS, 0.05)
        };
        let setback = rng.float(0., (shelf.depth - depth).clamp(0., 0.03));
        for _ in 0..facings {
            if x + w > shelf.x1 {
                break;
            }
            let high = if rng.chance(0.85) {
                levels
            } else {
                levels.saturating_sub(1).max(1)
            };
            ware(
                d,
                kind,
                [x + w / 2., shelf.y + setback, shelf.z],
                [w, depth, h],
                high,
                [label, body],
                rng,
            )?;
            x += w + 0.004;
        }
        x += rng.float(0.003, 0.012);
    }
    Ok(())
}

/// A plastic bottle crate with its low front-left corner at `at`: slotted sides, an open top
/// showing its shadowed inside, or bottle necks when `loaded`.
fn plastic_crate(
    d: &mut Fixture<'_, '_>,
    at: V3,
    [w, depth, h]: V3,
    color: V3,
    open: bool,
    loaded: bool,
) -> Result<(), String> {
    let [x, y, z] = at;
    d.cuboid("paint", at, [x + w, y + depth, z + h], color, "df")?;
    let dark = shade(color, 0.42);
    for band in [0.2, 0.58] {
        let (z0, z1) = (z + h * band, z + h * (band + 0.2));
        d.face(
            "paint",
            &[
                [x + 0.035, y - 0.003, z0],
                [x + w - 0.035, y - 0.003, z0],
                [x + w - 0.035, y - 0.003, z1],
                [x + 0.035, y - 0.003, z1],
            ],
            [0., -1., 0.],
            dark,
        )?;
        for (side, out) in [(x - 0.003, -1.), (x + w + 0.003, 1.)] {
            d.face(
                "paint",
                &[
                    [side, y + 0.035, z0],
                    [side, y + depth - 0.035, z0],
                    [side, y + depth - 0.035, z1],
                    [side, y + 0.035, z1],
                ],
                [out, 0., 0.],
                dark,
            )?;
        }
    }
    if open {
        d.face(
            "paint",
            &[
                [x + 0.022, y + 0.022, z + h + 0.004],
                [x + w - 0.022, y + 0.022, z + h + 0.004],
                [x + w - 0.022, y + depth - 0.022, z + h + 0.004],
                [x + 0.022, y + depth - 0.022, z + h + 0.004],
            ],
            [0., 0., 1.],
            shade(color, 0.25),
        )?;
    }
    if loaded {
        let glass = srgb(GLASS[((x * 37. + y * 11.).abs() as usize) % GLASS.len()]);
        for i in 0..4 {
            for j in 0..3 {
                let cx = x + 0.07 + (w - 0.14) * i as f64 / 3.;
                let cy = y + 0.07 + (depth - 0.14) * j as f64 / 2.;
                d.cuboid(
                    "paint",
                    [cx - 0.017, cy - 0.017, z + h + 0.004],
                    [cx + 0.017, cy + 0.017, z + h + 0.075],
                    if (i + j) % 5 == 0 {
                        shade(glass, 1.3)
                    } else {
                        glass
                    },
                    "df",
                )?;
            }
        }
    }
    Ok(())
}

/// A carton with its low front-left corner at `at`, taped along its top.
fn carton(d: &mut Fixture<'_, '_>, at: V3, [w, depth, h]: V3, color: V3) -> Result<(), String> {
    let [x, y, z] = at;
    d.cuboid("paint", at, [x + w, y + depth, z + h], color, "df")?;
    let tape = shade(color, 1.22);
    d.face(
        "paint",
        &[
            [x, y + depth * 0.44, z + h + 0.003],
            [x + w, y + depth * 0.44, z + h + 0.003],
            [x + w, y + depth * 0.56, z + h + 0.003],
            [x, y + depth * 0.56, z + h + 0.003],
        ],
        [0., 0., 1.],
        tape,
    )?;
    d.face(
        "paint",
        &[
            [x + w * 0.3, y - 0.003, z + h * 0.3],
            [x + w * 0.7, y - 0.003, z + h * 0.3],
            [x + w * 0.7, y - 0.003, z + h * 0.7],
            [x + w * 0.3, y - 0.003, z + h * 0.7],
        ],
        [0., -1., 0.],
        shade(color, 0.8),
    )
}

#[derive(Clone, Copy, PartialEq)]
enum Pile {
    Crates,
    Cartons,
}

/// A stack standing on the ground with its front-left foot at `at`, at most `across` wide and
/// `reach` high: plastic crates of one or two colours, the top one open, loaded with bottles or
/// carrying a carton; or cartons, each a little smaller and turned. Returns its width.
fn pile(
    d: &mut Fixture<'_, '_>,
    kind: Pile,
    at: V3,
    across: f64,
    reach: f64,
    rng: &mut SeededRng,
) -> Result<f64, String> {
    let [x, y, z] = at;
    match kind {
        Pile::Crates => {
            let w = rng.float(0.5, 0.57).min(across);
            let (depth, h) = (0.37, rng.float(0.28, 0.31));
            let count = rng.integer(2, 5).min((reach / h) as usize).max(1);
            let color = tone(rng, &PLASTICS);
            for level in 0..count {
                let own = if rng.chance(0.25) {
                    tone(rng, &PLASTICS)
                } else {
                    shade(color, rng.float(0.9, 1.05))
                };
                let last = level + 1 == count;
                let top = rng.integer(0, 9);
                let shift = rng.float(-0.012, 0.012);
                let low = [
                    (x + shift).max(x),
                    y + rng.float(0., 0.015),
                    z + level as f64 * h,
                ];
                plastic_crate(
                    d,
                    low,
                    [w - 0.012, depth, h - 0.004],
                    own,
                    last && top < 4,
                    last && (4..7).contains(&top),
                )?;
                if last && top >= 7 {
                    carton(
                        d,
                        [low[0] + 0.04, low[1] + 0.03, low[2] + h],
                        [w - 0.1, depth - 0.07, rng.float(0.18, 0.3)],
                        tone(rng, &CARDBOARD),
                    )?;
                }
            }
            Ok(w)
        }
        Pile::Cartons => {
            let mut w = rng.float(0.42, 0.58).min(across);
            let width = w;
            let mut depth = w * rng.float(0.68, 0.8);
            let mut top = z;
            for _ in 0..rng.integer(2, 4) {
                let h = w * rng.float(0.5, 0.75);
                if top + h > z + reach {
                    break;
                }
                let pen = forms::rotated(
                    &d.pen,
                    [x + w / 2., y + depth / 2., top],
                    2,
                    rng.float(-0.1, 0.1),
                )?;
                carton(
                    &mut on(d, pen),
                    [-w / 2., -depth / 2., 0.],
                    [w, depth, h],
                    tone(rng, &CARDBOARD),
                )?;
                top += h;
                w *= rng.float(0.82, 0.97);
                depth *= rng.float(0.85, 0.97);
            }
            Ok(width)
        }
    }
}

/// A plastic bucket (or a nest of them) standing at `at`.
fn bucket(d: &mut Fixture<'_, '_>, at: V3, rng: &mut SeededRng) -> Result<(), String> {
    let color = tone(rng, &PLASTICS);
    let nest = rng.integer(1, 3);
    for i in 0..nest {
        let z = at[2] + i as f64 * 0.07;
        let own = shade(color, 1. - i as f64 * 0.06);
        forms::lathe(
            d,
            "paint",
            [at[0], at[1], z],
            &[[0.125, 0.], [0.168, 0.27], [0.18, 0.275], [0.18, 0.31]],
            &forms::Tints::Bands(&[
                Tint::Color(own),
                Tint::Color(own),
                Tint::Color(shade(own, 1.25)),
            ]),
            8,
            None,
        )?;
    }
    d.disc(
        "paint",
        [at[0], at[1], at[2] + (nest - 1) as f64 * 0.07 + 0.26],
        0.165,
        [0., 0., 1.],
        shade(color, 0.3),
        8,
    )
}

/// A broom leaning back against whatever stands behind `foot`: a long handle and a wide head
/// of bristles under its coloured stock.
fn broom(d: &mut Fixture<'_, '_>, foot: V3, rng: &mut SeededRng) -> Result<(), String> {
    let [x, y, z] = foot;
    let lean = rng.float(0.14, 0.22);
    let slide = rng.float(-0.1, 0.1);
    d.bar(
        "paint",
        [x, y, z + 0.2],
        [x + slide, y + lean, z + rng.float(1.4, 1.6)],
        0.032,
        srgb("#c2a56c"),
    )?;
    d.cuboid(
        "paint",
        [x - 0.17, y - 0.04, z],
        [x + 0.17, y + 0.04, z + 0.17],
        tone(rng, &["#c9a85a", "#b89a55", "#a58a4a"]),
        "d",
    )?;
    d.cuboid(
        "paint",
        [x - 0.18, y - 0.045, z + 0.17],
        [x + 0.18, y + 0.045, z + 0.23],
        tone(rng, &["#a8554a", "#557f66", "#71899a"]),
        "d",
    )
}

/// A plastic stool standing at `at` (the centre of its foot).
fn stool(d: &mut Fixture<'_, '_>, at: V3, rng: &mut SeededRng) -> Result<(), String> {
    let [x, y, z] = at;
    let color = tone(rng, &["#a8554a", "#71899a", "#d6cba8", "#557f66"]);
    let (seat, spread, high) = (0.15, 0.18, 0.44);
    for (sx, sy) in [(-1., -1.), (1., -1.), (1., 1.), (-1., 1.)] {
        d.bar(
            "paint",
            [x + sx * spread, y + sy * spread, z],
            [x + sx * (seat - 0.02), y + sy * (seat - 0.02), z + high],
            0.035,
            shade(color, 0.85),
        )?;
    }
    d.cuboid(
        "paint",
        [x - seat, y - seat, z + high],
        [x + seat, y + seat, z + high + 0.035],
        color,
        "d",
    )
}

/// Where goods hang: from a line at `ceiling` across `[x0, x1]`, in the plane `y`.
#[derive(Clone, Copy)]
pub(crate) struct Hang {
    /// `[x0, x1, y, ceiling]`.
    pub line: [f64; 4],
    /// Only cords run this far below the line: the goods hang beneath whatever hides it (a
    /// lintel, an awning's edge).
    pub clear: f64,
    /// No piece reaches further below the line than this.
    pub drop: f64,
    /// A way through, `[x0, x1, drop]`: over it nothing reaches further down than its own drop.
    pub way: Option<[f64; 3]>,
    /// Rows of goods, one behind another (1–3). The first is the single row; a second hangs
    /// 8 cm behind it, overlapping, and fills its gaps; a third, 4 cm in front, is sparse and
    /// half of it bright accents.
    pub rows: usize,
}

/// Goods hung on cords from a line (see [`Hang`]), of varied length and shape: strings of
/// packets with a printed band, bunches of carrier bags, tied sacks, cloths, brushes and
/// cards of small wares. Each is a few faces towards the street (-Y).
pub(crate) fn hung(
    d: &mut Fixture<'_, '_>,
    hang: &Hang,
    rng: &mut SeededRng,
) -> Result<(), String> {
    let Hang {
        line: [x0, x1, _, ceiling],
        clear,
        drop,
        way,
        rows,
    } = *hang;
    if [x0, x1, hang.line[2], ceiling, drop, clear]
        .iter()
        .chain(way.iter().flatten())
        .any(|v| !v.is_finite() || v.abs() > 1000.)
        || x1 - x0 > 40.
        || !(0.2..=5.).contains(&drop)
        || !(0. ..=5.).contains(&clear)
        || !(1..=3).contains(&rows)
    {
        return Err(
            "Hung goods require a bounded line, a drop of 0.2 to 5 meters, a clear length \
             of 0 to 5 and one to three rows."
                .into(),
        );
    }
    for row in 0..rows {
        hung_row(d, hang, row, rng)?;
    }
    Ok(())
}

/// Saturated colours a few goods carry: red, yellow and green labels, an orange, a blue.
const ACCENTS: [&str; 6] = [
    "#c4281f", "#e2b215", "#2a8a3a", "#d4501c", "#1f58a8", "#c4281f",
];

/// One row of [`hung`]: `row` 0 is the first, 1 behind it and 2 in front.
fn hung_row(
    d: &mut Fixture<'_, '_>,
    hang: &Hang,
    row: usize,
    rng: &mut SeededRng,
) -> Result<(), String> {
    let Hang {
        line: [x0, x1, y, ceiling],
        clear,
        drop,
        way,
        ..
    } = *hang;
    let y = y + [0., 0.08, -0.04][row];
    // The share of pieces in an accent colour, and the run of gaps between them.
    let accent = [0.07, 0.05, 0.5][row];
    let gaps = [[0.005, 0.04], [-0.08, 0.], [0.3, 1.1]][row];
    let pick = |rng: &mut SeededRng, palette: &[&str]| accented(rng, palette, accent);
    let cord = srgb("#8f8468");
    let front = |d: &mut Fixture<'_, '_>, [a, b, low, high]: [f64; 4], depth: f64, tint: V3| {
        d.face(
            "paint",
            &[
                [a, y - depth, low],
                [b, y - depth, low],
                [b, y - depth, high],
                [a, y - depth, high],
            ],
            [0., -1., 0.],
            tint,
        )
    };
    let mut x = x0 + rng.float(0.02, if row == 0 { 0.1 } else { 0.3 });
    while x < x1 - 0.08 {
        d.mesh.spend_work(1)?;
        // No piece is wider than 0.4 m: one starting that far before the way keeps clear of it.
        let limit = match way {
            Some([a, b, low]) if x + 0.4 > a && x < b => low.min(drop),
            _ => drop,
        };
        let sort = rng.integer(0, 12);
        // The row behind hangs a little higher, so its pieces show over the first's.
        let start = clear
            + if row == 1 {
                rng.float(0., 0.06)
            } else {
                rng.float(0.02, 0.14)
            };
        let gap = rng.float(gaps[0], gaps[1]);
        // The height its body may take.
        let room = limit - start;
        if room < 0.2 {
            x += 0.24 + gap;
            continue;
        }
        let top = ceiling - start;
        let (width, hung_from) = match sort {
            0..=2 => {
                // A string of packets, each with a printed band.
                let (w, h) = (rng.float(0.1, 0.14), rng.float(0.13, 0.18));
                let (color, band) = (pick(rng, &LABELS), pick(rng, &LABELS));
                let length = room * rng.float(0.6, 1.);
                let mut z = top;
                while z - h >= top - length.max(h) - 1e-9 {
                    let tilt = rng.float(-0.012, 0.012);
                    d.face(
                        "paint",
                        &[
                            [x, y + tilt, z - h],
                            [x + w, y - tilt, z - h],
                            [x + w, y - tilt, z],
                            [x, y + tilt, z],
                        ],
                        [0., -1., 0.],
                        color,
                    )?;
                    front(
                        d,
                        [x + 0.012, x + w - 0.012, z - h * 0.62, z - h * 0.2],
                        0.016,
                        band,
                    )?;
                    z -= h + 0.012;
                }
                (w, top)
            }
            3..=5 => {
                // A bunch of carrier bags, mostly pale: a knotted neck over a full body,
                // creased down its middle.
                let (w, h) = (rng.float(0.24, 0.36), rng.float(0.34, 0.62).min(room));
                let color = pick(
                    rng,
                    &[
                        "#d6d0bf", "#cfcab9", "#b8984a", "#a85046", "#8a9cab", "#d6d0bf",
                    ],
                );
                let (cx, half, low) = (x + w / 2., w / 2., top - h);
                for side in [-1., 1.] {
                    d.face(
                        "paint",
                        &[
                            [cx, y - 0.07, low],
                            [cx + side * half * 0.8, y, low + h * 0.04],
                            [cx + side * half, y, low + h * 0.42],
                            [cx + side * half * 0.22, y, top],
                            [cx, y - 0.02, top],
                        ],
                        [side * 0.5, -1., 0.],
                        shade(color, 1. - (side + 1.) * 0.06),
                    )?;
                }
                (w, top)
            }
            6 | 7 => {
                // A tied sack or a net of balls, banded.
                let r = rng.float(0.07, 0.11);
                let h = rng.float(0.32, 0.62).min(room);
                let (color, band) = (pick(rng, &CLOTHS), pick(rng, &LABELS));
                arc(
                    d,
                    [x + r, y, top - h],
                    &[[r * 0.4, 0.], [r, h * 0.2], [r, h * 0.6], [r * 0.18, h]],
                    &[color, band, color],
                    3,
                )?;
                (r * 2., top)
            }
            8 | 9 => {
                // A long cloth or a garment, folded over.
                let (w, h) = (rng.float(0.26, 0.4), room * rng.float(0.7, 1.));
                let color = pick(rng, &CLOTHS);
                let lean = rng.float(-0.02, 0.02);
                d.sheet(
                    "paint",
                    &[
                        [x, y + lean, top - h],
                        [x + w, y + lean, top - h],
                        [x + w, y, top],
                        [x, y, top],
                    ],
                    color,
                )?;
                front(
                    d,
                    [x + w * 0.52, x + w, top - h * 0.8, top],
                    0.012,
                    shade(color, 0.8),
                )?;
                (w, top)
            }
            10 => {
                // A brush or a duster hung by its handle.
                let h = rng.float(0.4, 0.62).min(room);
                let w = rng.float(0.16, 0.24);
                let cx = x + w / 2.;
                d.bar(
                    "paint",
                    [cx, y, top - h + 0.1],
                    [cx, y, top],
                    0.026,
                    srgb("#b89a62"),
                )?;
                d.cuboid(
                    "paint",
                    [x, y - 0.035, top - h],
                    [x + w, y + 0.035, top - h + 0.11],
                    tone(rng, &["#b08a4a", "#7a8f6a", "#a55a48", "#d6cba8"]),
                    "f",
                )?;
                (w, top)
            }
            _ => {
                // A card of small wares in rows.
                let (w, h) = (rng.float(0.2, 0.3), rng.float(0.4, 0.75).min(room));
                let (first, second) = (pick(rng, &LABELS), pick(rng, &LABELS));
                front(d, [x, x + w, top - h, top], 0., srgb("#d9d2bd"))?;
                let rows = ((h - 0.05) / 0.11) as usize;
                for row in 0..rows {
                    let z = top - 0.05 - row as f64 * 0.11;
                    front(
                        d,
                        [x + 0.025, x + w - 0.025, z - 0.075, z],
                        0.014,
                        if row % 2 == 0 { first } else { second },
                    )?;
                }
                (w, top)
            }
        };
        if ceiling - hung_from > 0.01 {
            let cx = x + width / 2.;
            front(d, [cx - 0.004, cx + 0.004, hung_from, ceiling], 0., cord)?;
        }
        x += width + gap;
    }
    Ok(())
}

/// A fluorescent batten under the ceiling at `at` (its centre), `length` along X. The lit tube
/// is in the `tube-light` slot, for a host to make emissive.
fn tube_light(d: &mut Fixture<'_, '_>, at: V3, length: f64) -> Result<(), String> {
    let [x, y, z] = at;
    let (x0, x1) = (x - length / 2., x + length / 2.);
    let enamel = srgb("#d9d7cc");
    d.cuboid(
        "paint",
        [x0, y - 0.03, z - 0.045],
        [x1, y + 0.03, z],
        enamel,
        "u",
    )?;
    for end in [x0 + 0.012, x1 - 0.042] {
        d.cuboid(
            "paint",
            [end, y - 0.02, z - 0.085],
            [end + 0.03, y + 0.02, z - 0.045],
            shade(enamel, 0.8),
            "u",
        )?;
    }
    let pen = d.pen.into_part("tube-emitter")?;
    on(d, pen).tube(
        "tube-light",
        &[[x0 + 0.042, y, z - 0.066], [x1 - 0.042, y, z - 0.066]],
        0.016,
        [1., 1., 0.96],
        6,
    )
}

/// A wire rack of `size` with its front-left foot at the origin of `d`, packed with goods.
fn wire_rack(
    d: &mut Fixture<'_, '_>,
    [w, depth, h]: V3,
    levels: usize,
    rng: &mut SeededRng,
) -> Result<(), String> {
    let steel = srgb("#8a8f86");
    for x in [0.012, w - 0.012] {
        for y in [0.012, depth - 0.012] {
            d.bar("paint", [x, y, 0.], [x, y, h], 0.022, steel)?;
        }
    }
    let pitch = (h - 0.14) / levels as f64;
    for level in 0..=levels {
        let z = 0.12 + level as f64 * pitch;
        d.cuboid(
            "paint",
            [0., 0., z - 0.016],
            [w, depth, z],
            shade(steel, 0.9),
            if z > 1.45 { "fu" } else { "fd" },
        )?;
        if level == levels {
            break;
        }
        let wares: &[Ware] = if level == 0 {
            &[Ware::Boxes, Ware::BigJars, Ware::Bags]
        } else {
            &[Ware::Tins, Ware::Jars, Ware::Bottles, Ware::Boxes]
        };
        fill(
            d,
            &furniture::Shelf {
                x0: 0.03,
                x1: w - 0.03,
                y: 0.02,
                depth: depth - 0.04,
                z,
                clear: pitch - 0.03,
            },
            wares,
            rng,
            0.97,
        )?;
    }
    Ok(())
}

fn shelf_wares(z: f64, height: f64) -> &'static [Ware] {
    if z < 0.55 {
        &[Ware::Boxes, Ware::TallBoxes, Ware::BigJars, Ware::Bags]
    } else if z > height - 0.75 {
        &[Ware::Boxes, Ware::Boxes, Ware::TallBoxes, Ware::Tins]
    } else {
        &[
            Ware::Tins,
            Ware::Jars,
            Ware::Bottles,
            Ware::Boxes,
            Ware::TallBoxes,
            Ware::Tins,
            Ware::Bottles,
        ]
    }
}

/// A provisions shop fitted to any room, packed: shelf banks to the ceiling along the back
/// (one per 3 m) and both side walls, a counter where there is room, a display rack,
/// crates and cartons stacked on the floor either side of the way in, buckets, brooms and a
/// stool, and up to three tube lights (slot `tube-light`). Goods are low-detail (see above).
pub(super) fn furnish_fitted(d: &mut Fixture<'_, '_>, room: &Room<'_>) -> Result<(), String> {
    room.validate()?;
    // Dark, oiled timber, older than the reference shop's.
    let wood = srgb("#4f3f2d");
    let timber = &room.kit.settings().timber;
    let root = d.pen.clone();
    let (w, depth, h) = (room.width, room.depth, room.height);
    let tall = (h - 0.08).min(3.6);
    let levels = (((tall - 0.1) / 0.31).round() as usize).clamp(4, 11);
    let banks = ((w / 3.).round() as usize).max(1);
    let pitch = (w - 0.16) / banks as f64;
    for i in 0..banks {
        let mut bank = on(
            d,
            root.into_part(&format!("back-stock-{i}"))?.moved([
                0.08 + i as f64 * pitch,
                depth - 0.39,
                0.,
            ])?,
        );
        let shelves = furniture::shelving(
            &mut bank,
            [pitch - 0.1, 0.35, tall],
            levels,
            "timber",
            wood,
            0.,
            timber,
        )?;
        let mut rng = room.stream(&format!("back-stock-{i}"))?;
        for shelf in &shelves {
            fill(&mut bank, shelf, shelf_wares(shelf.z, tall), &mut rng, 0.97)?;
        }
    }
    let side = (depth - 1.25).min(2.4);
    if side >= 0.9 && w >= 2.6 {
        let mut walls = vec![("left", room.left_wall(&root, 0.75, 0.34)?)];
        if w >= 3.4 {
            walls.push(("right", room.right_wall(&root, 0.75 + side, 0.34)?));
        }
        for (name, pen) in walls {
            let mut bank = on(d, pen.into_part(&format!("side-stock-{name}"))?);
            let shelves = furniture::shelving(
                &mut bank,
                [side, 0.32, tall],
                levels,
                "timber",
                wood,
                0.,
                timber,
            )?;
            let mut rng = room.stream(&format!("side-stock-{name}"))?;
            for shelf in &shelves {
                fill(&mut bank, shelf, shelf_wares(shelf.z, tall), &mut rng, 0.95)?;
            }
        }
    }
    if w >= 4.6 && depth >= 2.9 {
        let mut counter = on(
            d,
            root.into_part("counter")?
                .moved([w - 2.05, depth - 1.5, 0.])?,
        );
        let top = furniture::counter(
            &mut counter,
            [1.3, 0.56, 0.93],
            furniture::Finish::Timber,
            wood,
            &mut room.stream("counter")?,
            timber,
        )?;
        fill(
            &mut counter,
            &top,
            &[Ware::Jars, Ware::BigJars, Ware::Boxes, Ware::Tins],
            &mut room.stream("counter-stock")?,
            0.7,
        )?;
        fittings::stool(d, [w - 2.4, depth - 1.75, 0.], shade(wood, 1.15))?;
    }
    // Goods hang across the opening, which is its host's to draw (see [`hung`]): a second line
    // under the ceiling would show as a band over an awning.
    let tubes = ((w / 1.8).round() as usize).clamp(1, 3);
    for i in 0..tubes {
        tube_light(
            d,
            [
                w * (i as f64 + 0.5) / tubes as f64,
                (depth * 0.3).min(1.1),
                h - 0.01,
            ],
            (w / tubes as f64 - 0.4).clamp(0.6, 1.2),
        )?;
    }
    // The floor either side of the way in, and at the foot of the back shelves.
    let mut rng = room.stream("floor-stock")?;
    let (way0, way1) = if w >= 3. {
        (w * 0.5 - 0.6, w * 0.5 + 0.6)
    } else {
        (w, w)
    };
    let inset = if side >= 0.9 && w >= 2.6 { 0.42 } else { 0.08 };
    // A display rack just inside the opening, left of the way in, faces the street.
    let display = if w >= 4.6 { (way0 - 0.65).min(1.9) } else { 0. };
    if display >= 0.9 {
        let pen = root.into_part("display")?.moved([0.45, 0.2, 0.])?;
        wire_rack(
            &mut on(d, pen),
            [display, 0.36, (h - 0.55).min(2.1)],
            6,
            &mut room.stream("display")?,
        )?;
    }
    let mut x = if display >= 0.9 {
        0.45 + display + 0.08
    } else {
        inset
    };
    while x < w - inset - 0.45 {
        if x + 0.58 > way0 && x < way1 {
            x = way1;
            continue;
        }
        let kind = if rng.chance(0.55) {
            Pile::Crates
        } else {
            Pile::Cartons
        };
        let limit = if x < way0 { way0 } else { w - inset };
        let across = (limit - x).min(0.6);
        if across < 0.4 {
            x += across.max(0.05);
            continue;
        }
        let y = rng.float(0.3, 0.55);
        let reach = rng.float(0.65, 1.1);
        let width = pile(d, kind, [x, y, 0.], across, reach, &mut rng)?;
        if rng.chance(0.4) && depth > 3. {
            pile(
                d,
                Pile::Cartons,
                [x, y + 0.5, 0.],
                across,
                rng.float(0.9, 1.6),
                &mut rng,
            )?;
        }
        x += width + rng.float(0.03, 0.14);
    }
    let mut x = inset + rng.float(0.1, 0.5);
    while x < w - inset - 0.6 && depth >= 2.8 {
        if w >= 4.6 && x + 0.6 > w - 2.5 {
            break;
        }
        let width = pile(
            d,
            Pile::Cartons,
            [x, depth - rng.float(0.95, 1.1), 0.],
            0.58,
            rng.float(0.5, 1.2),
            &mut rng,
        )?;
        x += width + rng.float(0.15, 0.7);
    }
    if w >= 2.6 {
        bucket(d, [inset + 0.2, 1.05, 0.], &mut rng)?;
        broom(d, [w - inset - 0.25, 1., 0.], &mut rng)?;
    }
    Ok(())
}

/// What a street stock holds: a shop's whole display, or a heap of crates, cartons or both.
#[derive(Clone, Copy, PartialEq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Street {
    Shop,
    Crates,
    Cartons,
    Mixed,
}

/// Street stock along a frontage of `width`, in the wall's frame (x along it from 0, y out
/// from the wall, the pavement at z = 0), within 1.1 m of the wall. A `Shop` leaves the way
/// in clear at its middle: stacked plastic crates and cartons either side behind one front row
/// of crates, a sack truck by the left jamb, wire racks packed with goods at the pavement edge
/// on the right, and buckets, a stool and brooms by the way in. The other kinds are a heap of
/// crates, cartons or both across the whole width.
pub(crate) fn street_fitted(
    d: &mut Fixture<'_, '_>,
    kit: &Kit,
    key: &str,
    width: f64,
    kind: Street,
) -> Result<(), String> {
    if !width.is_finite() || !(0.6..=40.).contains(&width) || key.len() > 1024 {
        return Err("Street stock requires a frontage of 0.6 to 40 meters.".into());
    }
    // Pieces face the street: their frame's -Y is the wall's +Y.
    let root = d.pen.into_part("street-stock")?;
    let facing = |x: f64, out: f64| root.facing([x, out, 0.], [1., 0., 0.], [0., -1., 0.]);
    let mut rng = kit.rng(&format!("{key}/street-stock"))?;
    let shop = kind == Street::Shop;
    let racks = if !shop || width < 3. {
        0
    } else if width < 5. {
        1
    } else {
        2
    };
    let dolly = shop && width >= 4.2;
    let (way0, way1) = if shop && width >= 3. {
        (width * 0.5 - 0.6, width * 0.5 + 0.6)
    } else {
        (width, width)
    };
    let rack_start = width - 0.08 - racks as f64 * 0.9;
    let mut x = if dolly { 0.72 } else { 0.05 };
    let mut index = 0;
    while x < width - 0.45 {
        if x + 0.5 > way0 && x < way1 {
            x = way1;
            continue;
        }
        let limit = if x < way0 { way0 } else { width - 0.05 };
        let across = (limit - x).min(0.6);
        if across < 0.4 {
            x += across.max(0.05);
            continue;
        }
        let sort = match kind {
            Street::Crates => Pile::Crates,
            Street::Cartons => Pile::Cartons,
            Street::Mixed if index % 2 == 0 => Pile::Crates,
            Street::Mixed => Pile::Cartons,
            Street::Shop if rng.chance(0.72) => Pile::Crates,
            Street::Shop => Pile::Cartons,
        };
        index += 1;
        // Behind the racks only low stock fits, close to the wall.
        let behind = racks > 0 && x + across > rack_start;
        let out = if behind { 0.5 } else { rng.float(0.5, 0.64) };
        let reach = if behind {
            0.7
        } else if shop {
            rng.float(0.62, 1.)
        } else {
            rng.float(0.95, 1.55)
        };
        let stood = pile(
            &mut on(d, facing(x, out)?),
            sort,
            [0.; 3],
            across,
            reach,
            &mut rng,
        )?;
        if shop && !behind {
            // A shop's front row is one line of plastic crates, one or two high, with a
            // carton here and there.
            if rng.chance(0.9) {
                let low = rng.float(0.3, 0.68);
                pile(
                    &mut on(d, facing(x, (out + 0.4).min(1.))?),
                    if rng.chance(0.85) {
                        Pile::Crates
                    } else {
                        Pile::Cartons
                    },
                    [0.; 3],
                    stood.min(across),
                    low,
                    &mut rng,
                )?;
            }
        } else if !behind && rng.chance(0.5) {
            // A heap's second row in front is half as high.
            let low = rng.float(0.35, 0.7);
            pile(
                &mut on(d, facing(x + rng.float(0., 0.08), (out + 0.4).min(1.))?),
                if sort == Pile::Crates && kind != Street::Crates {
                    Pile::Cartons
                } else {
                    sort
                },
                [0.; 3],
                across,
                low,
                &mut rng,
            )?;
        }
        x += stood + rng.float(0.02, if shop { 0.05 } else { 0.1 });
    }
    for i in 0..racks {
        let mut rack = on(
            d,
            facing(rack_start + i as f64 * 0.9, 0.93)?.into_part(&format!("rack-{i}"))?,
        );
        wire_rack(
            &mut rack,
            [0.82, 0.36, rng.float(1.65, 1.85)],
            5,
            &mut kit.rng(&format!("{key}/rack-{i}"))?,
        )?;
        // Goods hung on the rack's front.
        if rng.chance(0.6) {
            hung(
                &mut rack,
                &Hang {
                    line: [0.04, 0.78, -0.03, 1.6],
                    clear: 0.,
                    drop: 0.6,
                    way: None,
                    rows: 1,
                },
                &mut kit.rng(&format!("{key}/rack-hung-{i}"))?,
            )?;
        }
    }
    if shop && way1 < width {
        // By the way in: buckets and a stool at its sides, brooms leaning on the stock.
        bucket(&mut on(d, facing(way0 + 0.2, 0.86)?), [0.; 3], &mut rng)?;
        if rng.chance(0.6) {
            bucket(&mut on(d, facing(way0 + 0.22, 0.44)?), [0.; 3], &mut rng)?;
        }
        stool(&mut on(d, facing(way1 - 0.2, 0.6)?), [0.; 3], &mut rng)?;
        broom(&mut on(d, facing(way0 - 0.2, 1.04)?), [0.; 3], &mut rng)?;
        if racks == 0 || way1 + 0.5 < rack_start {
            broom(&mut on(d, facing(way1 + 0.25, 1.04)?), [0.; 3], &mut rng)?;
        }
    } else if shop && width >= 2.4 {
        let at = if racks > 0 {
            rack_start - 0.2
        } else {
            width - 0.3
        };
        if at > way1 + 0.2 || racks == 0 {
            bucket(&mut on(d, facing(at, 0.85)?), [0.; 3], &mut rng)?;
        }
        broom(&mut on(d, facing(width - 0.2, 0.2)?), [0.; 3], &mut rng)?;
    }
    if kind == Street::Mixed && width >= 1.2 {
        broom(&mut on(d, facing(width - 0.2, 0.16)?), [0.; 3], &mut rng)?;
        bucket(&mut on(d, facing(0.2, 0.9)?), [0.; 3], &mut rng)?;
    }
    if dolly {
        let mut truck = on(d, root.into_part("dolly")?.moved([0.12, 0.24, 0.])?);
        let ivory = srgb("#cfc2a2");
        for x in [0., 0.4] {
            truck.tube(
                "paint",
                &[[x, 0., 0.15], [x, 0., 1.38], [x, 0.16, 1.46]],
                0.02,
                ivory,
                5,
            )?;
            truck.disc(
                "paint",
                [x, 0.1, 0.12],
                0.12,
                [if x > 0.2 { 1. } else { -1. }, 0., 0.],
                srgb("#343632"),
                8,
            )?;
        }
        for z in [0.32, 0.76, 1.24] {
            truck.bar("paint", [0., 0., z], [0.4, 0., z], 0.03, ivory)?;
        }
        truck.cuboid(
            "paint",
            [-0.02, 0., 0.06],
            [0.42, 0.33, 0.095],
            srgb("#55665c"),
            "",
        )?;
        // Its load of cartons leans back on the frame.
        let mut load = on(d, facing(0.14, 0.56)?);
        pile(
            &mut load,
            Pile::Cartons,
            [0., 0., 0.095],
            0.4,
            1.05,
            &mut rng,
        )?;
    }
    Ok(())
}

pub(crate) fn street(d: &mut Fixture<'_, '_>, kit: &Kit, textured: bool) -> Result<(), String> {
    let root = d.pen.into_part("street-stock")?;
    for (i, (x, y, rows, width, pigment)) in [
        (3.3, 0.48, 3, 0.48, "#a46752"),
        (3.94, 0.39, 2, 0.55, "#586c5d"),
        (4.54, 0.55, 3, 0.49, "#a87359"),
        (7.66, 0.28, 2, 0.56, "#717451"),
        (8.4, 0.53, 1, 0.49, "#866b4f"),
    ]
    .into_iter()
    .enumerate()
    {
        let color = srgb(pigment);
        for row in 0..rows {
            let mut crate_fixture = on(
                d,
                root.moved([
                    x + f64::from(row % 2) * 0.026,
                    y,
                    0.015 + f64::from(row) * 0.29,
                ])?,
            );
            let floor = furniture::crate_box(
                &mut crate_fixture,
                [width, 0.4, 0.29],
                color,
                &mut kit.rng(&format!("crate-{i}-{row}"))?,
                &kit.settings().timber,
            )?;
            if row + 1 != rows {
                continue;
            }
            let center = (floor.x0 + floor.x1) / 2.;
            if i == 1 {
                let rim = floor.z + floor.clear;
                crate_fixture.cuboid(
                    "timber",
                    [-0.008, -0.008, rim],
                    [width + 0.008, 0.408, rim + 0.018],
                    color,
                    "",
                )?;
                goods::carton_stack(
                    &mut crate_fixture,
                    [center, 0.2, rim + 0.018],
                    0.36,
                    0.52,
                    &mut kit.rng("street-cartons")?,
                    textured,
                )?;
            } else {
                let size = grocer_props::size(
                    "bottle-load",
                    [
                        floor.x1 - floor.x0 - 0.012,
                        floor.depth - 0.012,
                        floor.clear + 0.085,
                    ],
                )?;
                grocer_props::draw(
                    &mut crate_fixture,
                    "bottle-load",
                    [center, floor.y + floor.depth / 2., floor.z],
                    size,
                    0.,
                    &grocer_props::Options {
                        variant: i + 1,
                        textured,
                        ..Default::default()
                    },
                )?;
            }
        }
    }
    let mut rack = on(
        d,
        root.into_part("threshold-rack")?.facing(
            [8.29, 0.26, 0.03],
            [1., 0., 0.],
            [0., -1., 0.],
        )?,
    );
    let shelves = furniture::shop_shelving(
        &mut rack,
        furniture::Family::Wire,
        [0.66, 0.34, 2.22],
        6,
        srgb("#808579"),
        0.,
        kit,
    )?;
    rows(
        &mut rack,
        &shelves,
        &[
            Grocery::Tins,
            Grocery::SmallJars,
            Grocery::SmallBottles,
            Grocery::Cartons,
        ],
        kit,
        "threshold-rack",
        Settings {
            fill: 0.97,
            maximum_rows: 1,
            packed: true,
        },
        textured,
    )?;
    let mut dolly = on(d, root.into_part("dolly")?.moved([2.83, 0.24, 0.])?);
    let ivory = srgb("#dfcfaa");
    for x in [0., 0.4] {
        dolly.tube(
            "metal",
            &[[x, 0., 0.15], [x, 0., 1.58], [x, 0.2, 1.64]],
            0.021,
            ivory,
            8,
        )?;
        dolly.disc(
            "paint",
            [x, 0.14, 0.14],
            0.13,
            [1., 0., 0.],
            srgb("#343632"),
            20,
        )?;
    }
    for z in [0.32, 0.76, 1.24] {
        dolly.tube("metal", &[[0., 0., z], [0.4, 0., z]], 0.018, ivory, 8)?;
    }
    dolly.cuboid(
        "metal",
        [-0.02, 0., 0.06],
        [0.42, 0.33, 0.095],
        srgb("#3f554b"),
        "",
    )
}
