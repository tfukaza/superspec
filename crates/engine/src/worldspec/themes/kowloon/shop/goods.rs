//! Procedural packaged goods and reusable shelf product families.
use super::super::{
    fixture::Fixture,
    palette::{mix, shade, srgb},
    pen::Tint,
};
use super::{
    forms::{self, Tints},
    grocer_props::{self, Options},
    stock::{self, Line, Maker},
    stock_mesh,
};
use superspec_geometry::{
    V3,
    math::{add, cos},
};
use superspec_types::rng::SeededRng;
pub const LABELS: &str = "#c8281e #e8b82c #1f5fa8 #2f8f4f #f0ece0 #e87a1e #7a2a8a #d8486a #1e8a8a #f4d03c #a81e2c #204080 #f08a9a #5a3a1c";
pub const GLASS: &str = "#3f6a38 #6a431c #cfd8d2 #1f3a2a #8a5a22 #d8d0b8";
pub const PLASTICS: &str = "#d8281e #1f6fc8 #e8b020 #2f9f5f #e86aa0 #f0ece0 #e87020";
pub const CONTENTS: &str =
    "#a83a1e #d8a02c #7a8a2c #5a2a1a #e8d8a0 #c84a5a #3a2a1e #e8862c #f0e0b0";
const CAPS: &str = "#c8281e #e8e4d8 #d8a82c #2a2a2c #1f5fa8";
pub fn color(rng: &mut SeededRng, palette: &str) -> V3 {
    srgb(rng.pick(&palette.split_whitespace().collect::<Vec<_>>()))
}
fn brand(rng: &mut SeededRng) -> [V3; 2] {
    let mut palette: Vec<_> = LABELS.split_whitespace().collect();
    let first = palette.remove(rng.integer(0, palette.len() - 1));
    let second = palette.remove(rng.integer(0, palette.len() - 1));
    [srgb(first), srgb(second)]
}
fn check(d: &Fixture<'_, '_>, at: V3, size: &[f64]) -> Result<(), String> {
    if at.iter().any(|v| !v.is_finite() || v.abs() > 1000.)
        || size
            .iter()
            .any(|v| !v.is_finite() || !(0.001..=20.).contains(v))
    {
        return Err("Shop goods require finite positions and bounded positive sizes.".into());
    }
    d.mesh.check_work(0)
}
fn lathe(
    d: &mut Fixture<'_, '_>,
    slot: &str,
    at: V3,
    profile: &[[f64; 2]],
    tints: &[V3],
    sides: usize,
) -> Result<(), String> {
    let tints: Vec<_> = tints.iter().map(|v| Tint::Color(*v)).collect();
    forms::lathe(d, slot, at, profile, &Tints::Bands(&tints), sides, None)
}
pub fn tin(d: &mut Fixture<'_, '_>, at: V3, r: f64, h: f64, label: V3) -> Result<(), String> {
    check(d, at, &[r, h])?;
    lathe(d, "paint", at, &[[r, 0.], [r, h - 0.006]], &[label], 6)?;
    lathe(
        d,
        "metal",
        at,
        &[[r * 1.04, h - 0.006], [r * 1.04, h], [0., h]],
        &[srgb("#a4a9aa")],
        6,
    )
}
pub fn bottle(
    d: &mut Fixture<'_, '_>,
    at: V3,
    r: f64,
    h: f64,
    glass: V3,
    cap: V3,
    label: Option<V3>,
) -> Result<(), String> {
    check(d, at, &[r, h])?;
    lathe(
        d,
        "pane",
        at,
        &[
            [r, 0.],
            [r, h * 0.56],
            [r * 0.42, h * 0.8],
            [r * 0.36, h * 0.93],
        ],
        &[glass],
        6,
    )?;
    lathe(
        d,
        "paint",
        at,
        &[[r * 0.44, h * 0.93], [r * 0.44, h], [0., h]],
        &[cap],
        6,
    )?;
    if let Some(label) = label {
        lathe(
            d,
            "paint",
            at,
            &[[r * 1.04, h * 0.16], [r * 1.04, h * 0.44]],
            &[label],
            6,
        )?;
    }
    Ok(())
}
pub fn jar(
    d: &mut Fixture<'_, '_>,
    at: V3,
    r: f64,
    h: f64,
    content: V3,
    lid: V3,
) -> Result<(), String> {
    check(d, at, &[r, h])?;
    let glass = shade(content, 0.65);
    let stripes = |p: V3| Ok(shade(content, 0.84 + 0.15 * cos(p[0] * 430. + p[2] * 690.)));
    forms::lathe(
        d,
        "pane",
        at,
        &[
            [r * 0.89, 0.],
            [r, h * 0.09],
            [r, h * 0.70],
            [r * 0.89, h * 0.82],
            [r * 0.84, h * 0.87],
        ],
        &Tints::Bands(&[
            Tint::Color(glass),
            Tint::Field(&stripes),
            Tint::Color(glass),
        ]),
        8,
        None,
    )?;
    lathe(
        d,
        "paint",
        at,
        &[
            [r * 0.9, h * 0.86],
            [r * 0.94, h * 0.90],
            [r * 0.94, h * 0.98],
            [r * 0.88, h],
            [0., h],
        ],
        &[shade(lid, 0.7), lid, shade(lid, 1.12), lid],
        8,
    )?;
    let [x, y, z] = at;
    let label_radius = r + 0.001;
    for side in [-1., 1.] {
        let edge = side * label_radius * 0.49;
        let slope = std::f64::consts::SQRT_2 - 1.;
        let depth = -label_radius + edge.abs() * slope;
        d.face(
            "paint",
            &[
                [x, y - label_radius, z + h * 0.27],
                [x + edge, y + depth, z + h * 0.27],
                [x + edge, y + depth, z + h * 0.48],
                [x, y - label_radius, z + h * 0.48],
            ],
            [side * slope, -1., 0.],
            shade(srgb("#ece6d2"), 0.88),
        )?;
    }
    Ok(())
}
pub fn carton(
    d: &mut Fixture<'_, '_>,
    at: V3,
    size: V3,
    color: V3,
    band: V3,
) -> Result<(), String> {
    check(d, at, &size)?;
    let [x, y, z] = at;
    let [w, depth, h] = size;
    d.cuboid(
        "paint",
        [x - w / 2., y - depth / 2., z],
        [x + w / 2., y + depth / 2., z + h],
        color,
        "df",
    )?;
    forms::front(
        d,
        "paint",
        [x - w / 2., x + w / 2., z + h * 0.3, z + h * 0.62],
        y - depth / 2. - 0.001,
        band,
    )
}
pub fn sack(
    d: &mut Fixture<'_, '_>,
    at: V3,
    r: f64,
    h: f64,
    cloth: V3,
    content: V3,
) -> Result<(), String> {
    check(d, at, &[r, h])?;
    lathe(
        d,
        "paint",
        at,
        &[
            [r * 0.78, 0.],
            [r, h * 0.2],
            [r * 0.97, h * 0.78],
            [r * 0.9, h],
            [r * 0.98, h + 0.015],
            [r * 0.94, h - 0.04],
            [r * 0.84, h - 0.03],
        ],
        &[
            cloth,
            cloth,
            cloth,
            shade(cloth, 1.08),
            shade(cloth, 1.08),
            shade(cloth, 0.8),
        ],
        9,
    )?;
    lathe(
        d,
        "paint",
        at,
        &[[r * 0.85, h - 0.035], [r * 0.45, h + 0.02], [0., h + 0.04]],
        &[content],
        9,
    )
}
#[derive(Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Tins,
    Bottles,
    Jars,
    BigJars,
    Cartons,
    Packets,
    Rolls,
    Thermoses,
    Mugs,
    Bowls,
    PaintTins,
    CeramicJars,
}
fn product(
    size: V3,
    stack: usize,
    draw: impl FnMut(&mut Fixture<'_, '_>, V3) -> Result<(), String> + 'static,
) -> Line {
    Line {
        width: size[0],
        depth: size[1],
        height: size[2],
        stack,
        draw: Box::new(draw),
    }
}
/// Chooses one brand and dimensions; subsequent placements retain that product identity.
pub fn line(kind: Kind, rng: &mut SeededRng, textured: bool) -> Result<Line, String> {
    Ok(match kind {
        Kind::Tins => {
            let r = rng.float(0.033, 0.05);
            let h = rng.float(0.08, 0.125);
            let [label, _] = brand(rng);
            product([r * 2.1, r * 2.1, h], 3, move |d, at| {
                tin(d, at, r, h, label)
            })
        }
        Kind::Bottles => {
            let r = rng.float(0.028, 0.04);
            let h = rng.float(0.2, 0.3);
            let glass = color(rng, GLASS);
            let cap = color(rng, CAPS);
            let label = if rng.chance(0.8) {
                Some(color(rng, LABELS))
            } else {
                None
            };
            product([r * 2.15, r * 2.15, h], 1, move |d, at| {
                bottle(d, at, r, h, glass, cap, label)
            })
        }
        Kind::Jars | Kind::BigJars => {
            let big = matches!(kind, Kind::BigJars);
            let r = if big {
                rng.float(0.075, 0.1)
            } else {
                rng.float(0.04, 0.058)
            };
            let h = if big {
                rng.float(0.22, 0.3)
            } else {
                rng.float(0.11, 0.17)
            };
            let content = color(rng, CONTENTS);
            let lid = color(rng, if big { "#c8281e #b8862a #2a2a2c" } else { CAPS });
            let fit = if big { 2.1 } else { 2.12 };
            product([r * fit, r * fit, h], 1, move |d, at| {
                jar(d, at, r, h, content, lid)
            })
        }
        Kind::Cartons => {
            let w = rng.float(0.07, 0.19);
            let depth = rng.float(0.05, 0.12);
            let h = rng.float(0.09, 0.24);
            let [color, band] = brand(rng);
            product(
                [w + 0.004, depth, h],
                if h < 0.14 { 3 } else { 1 },
                move |d, at| carton(d, at, [w, depth, h], color, band),
            )
        }
        Kind::Packets => {
            let w = rng.float(0.1, 0.16);
            let h = rng.float(0.15, 0.22);
            let [color, print] = brand(rng);
            product([w + 0.006, 0.07, h], 1, move |d, at| {
                stock_mesh::snack_packet(d, at, [w, h], mix(color, print, 0.15), textured, 0)
            })
        }
        Kind::Rolls => {
            let tint = color(rng, "#f0ece4 #f0c8d0 #c8dcf0");
            product([0.118, 0.118, 0.1], 3, move |d, at| {
                lathe(
                    d,
                    "paint",
                    at,
                    &[[0.055, 0.], [0.055, 0.098], [0.02, 0.098], [0.02, 0.07]],
                    &[tint, shade(tint, 0.92), shade(srgb("#a8845a"), 0.8)],
                    8,
                )
            })
        }
        Kind::Thermoses => {
            let body = color(rng, PLASTICS);
            product([0.21, 0.13, 0.34], 1, move |d, at| {
                lathe(
                    d,
                    "pane",
                    at,
                    &[[0.055, 0.], [0.06, 0.02], [0.06, 0.24], [0.045, 0.28]],
                    &[body],
                    8,
                )?;
                lathe(
                    d,
                    "metal",
                    at,
                    &[[0.047, 0.28], [0.047, 0.325], [0.02, 0.34], [0., 0.34]],
                    &[shade(srgb("#a4a9aa"), 1.15)],
                    8,
                )?;
                forms::limb(
                    d,
                    "metal",
                    &[
                        add(at, [0.058, 0., 0.22]),
                        add(at, [0.095, 0., 0.2]),
                        add(at, [0.095, 0., 0.09]),
                        add(at, [0.058, 0., 0.07]),
                    ],
                    &[0.007; 4],
                    &Tint::Color(shade(srgb("#a4a9aa"), 1.1)),
                    4,
                    1.,
                )
            })
        }
        Kind::Mugs => {
            let enamel = color(rng, "#ece8dc #2f5f9a #c8281e #3f8a5a");
            product([0.15, 0.1, 0.09], 2, move |d, at| {
                lathe(
                    d,
                    "pane",
                    at,
                    &[
                        [0.04, 0.],
                        [0.043, 0.085],
                        [0.046, 0.09],
                        [0.04, 0.086],
                        [0.037, 0.01],
                        [0., 0.01],
                    ],
                    &[
                        enamel,
                        srgb("#1f2f5a"),
                        srgb("#1f2f5a"),
                        srgb("#ece8dc"),
                        srgb("#ece8dc"),
                    ],
                    6,
                )?;
                forms::limb(
                    d,
                    "pane",
                    &[
                        add(at, [0.042, 0., 0.07]),
                        add(at, [0.068, 0., 0.045]),
                        add(at, [0.042, 0., 0.02]),
                    ],
                    &[0.005; 3],
                    &Tint::Color(enamel),
                    4,
                    1.,
                )
            })
        }
        Kind::Bowls => {
            let glaze = color(rng, "#ece8dc #dfe8e8 #e8d8b8");
            let rim = color(rng, "#2f4f9a #b02a22 #3a7a5a");
            let nested = rng.integer(3, 6);
            product(
                [0.16, 0.17, 0.06 + (nested - 1) as f64 * 0.03],
                1,
                move |d, at| {
                    for i in 0..nested {
                        lathe(
                            d,
                            "pane",
                            [at[0], at[1], at[2] + i as f64 * 0.03],
                            &[[0.03, 0.], [0.07, 0.05], [0.074, 0.058]],
                            &[glaze, rim],
                            8,
                        )?;
                    }
                    lathe(
                        d,
                        "pane",
                        [at[0], at[1], at[2] + (nested - 1) as f64 * 0.03],
                        &[[0.074, 0.058], [0.068, 0.054], [0.03, 0.014], [0., 0.014]],
                        &[rim, glaze, glaze],
                        8,
                    )
                },
            )
        }
        Kind::PaintTins => {
            let paint = color(rng, PLASTICS);
            product([0.17, 0.17, 0.18], 2, move |d, at| {
                lathe(
                    d,
                    "metal",
                    at,
                    &[
                        [0.08, 0.],
                        [0.08, 0.17],
                        [0.083, 0.178],
                        [0.07, 0.178],
                        [0.07, 0.17],
                        [0., 0.17],
                    ],
                    &[shade(srgb("#a4a9aa"), 1.05)],
                    9,
                )?;
                lathe(
                    d,
                    "paint",
                    at,
                    &[[0.0815, 0.03], [0.0815, 0.13]],
                    &[paint],
                    9,
                )?;
                forms::limb(
                    d,
                    "metal",
                    &[
                        add(at, [-0.08, 0., 0.15]),
                        add(at, [-0.06, -0.06, 0.1]),
                        add(at, [0.06, -0.06, 0.1]),
                        add(at, [0.08, 0., 0.15]),
                    ],
                    &[0.003; 4],
                    &Tint::Color(shade(srgb("#a4a9aa"), 0.8)),
                    4,
                    1.,
                )
            })
        }
        Kind::CeramicJars => {
            let glaze = color(rng, "#ece8e0 #dfe8ec #7a4a2a #3a5a4a");
            let band = color(rng, "#2f4f9a #1f3a7a #b8862a");
            let r = rng.float(0.07, 0.095);
            product([r * 2.15, r * 2.15, r * 2.9], 1, move |d, at| {
                lathe(
                    d,
                    "pane",
                    at,
                    &[
                        [r * 0.6, 0.],
                        [r, r * 0.9],
                        [r * 0.95, r * 1.6],
                        [r * 0.55, r * 2.3],
                        [r * 0.6, r * 2.45],
                        [r * 0.62, r * 2.5],
                        [r * 0.3, r * 2.75],
                        [0.012, r * 2.9],
                        [0., r * 2.9],
                    ],
                    &[
                        glaze,
                        band,
                        glaze,
                        glaze,
                        glaze,
                        shade(glaze, 0.9),
                        shade(glaze, 0.9),
                        band,
                    ],
                    9,
                )
            })
        }
    })
}
pub fn shelf(
    d: &mut Fixture<'_, '_>,
    surface: &super::furniture::Shelf,
    kinds: &[Kind],
    rng: &mut SeededRng,
    settings: stock::Settings,
    textured: bool,
) -> Result<(), String> {
    if !(1..=64).contains(&kinds.len()) {
        return Err("Stocking requires 1–64 product families.".into());
    }
    let factories: Vec<_> = kinds
        .iter()
        .map(|&kind| move |rng: &mut SeededRng| line(kind, rng, textured))
        .collect();
    let makers: Vec<Maker<'_>> = factories.iter().map(|f| f as Maker<'_>).collect();
    stock::shelf(d, surface, &makers, rng, settings)
}

pub fn carton_stack(
    d: &mut Fixture<'_, '_>,
    at: V3,
    size: f64,
    height: f64,
    rng: &mut SeededRng,
    textured: bool,
) -> Result<(), String> {
    check(d, at, &[size, height])?;
    let mut z = at[2];
    loop {
        d.mesh.spend_work(1)?;
        let h = size * rng.float(0.55, 0.8);
        if z + h > at[2] + height {
            break;
        }
        let x = at[0] + rng.float(-0.02, 0.02);
        let y = at[1] + rng.float(-0.015, 0.015);
        let brightness = rng.float(0.86, 1.06);
        grocer_props::draw(
            d,
            "stock-box",
            [x, y, z],
            [size, size * 0.72, h],
            0.,
            &Options {
                variant: rng.integer(1, 4),
                supported: true,
                tint: [brightness; 3],
                textured,
            },
        )?;
        z += h;
    }
    Ok(())
}
