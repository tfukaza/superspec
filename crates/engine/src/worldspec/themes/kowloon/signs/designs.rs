//! Seeded composition retains independent steel paint streams and ordered source random draws.
use super::{
    Construction, Outline,
    copy::{self, Business},
    lettering::{Direction, Face},
    panel::{Block, Design, Hardware, Stack},
};
use crate::worldspec::themes::kowloon::{
    palette::{shade, srgb},
    weathered,
};
use serde::{Deserialize, Serialize};
use superspec_geometry::V3;
use superspec_types::rng::SeededRng;
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Palette {
    pub ground: V3,
    pub ink: V3,
    pub accent: V3,
    pub border: V3,
}
fn palette(text: &str) -> Palette {
    let colors: Vec<_> = text.split_whitespace().map(srgb).collect();
    Palette {
        ground: colors[0],
        ink: colors[1],
        accent: colors[2],
        border: colors[3],
    }
}
pub(super) fn weighted<T: Clone>(rng: &mut SeededRng, values: &[(T, f64)]) -> T {
    let mut roll = rng.next_f64() * values.iter().map(|v| v.1).sum::<f64>();
    for (value, w) in values {
        roll -= w;
        if roll <= 0. {
            return value.clone();
        }
    }
    values.last().unwrap().0.clone()
}
fn colors(c: Construction, rng: &mut SeededRng) -> Palette {
    let text = match c {
        Construction::Timber => rng.pick(&[
            "#17110e #e0b64a #d8c9a0 #e0b64a",
            "#5a1410 #e6bc50 #f0e4c8 #e6bc50",
            "#16382a #e0b64a #ece4c8 #e0b64a",
            "#c49a62 #17100c #6a1810 #17100c",
        ]),
        Construction::Lightbox => rng.pick(&[
            "#f2f0e8 #c01c1c #1c1c20 #c01c1c",
            "#f2f0e8 #1c3e9c #c01c1c #1c3e9c",
            "#f0d040 #b01818 #1c1c1e #b01818",
            "#f2f0e8 #166a3c #c01c1c #166a3c",
            "#c41c1c #f4f0e4 #f0d040 #f4f0e4",
        ]),
        Construction::Enamel => rng.pick(&[
            "#f0eee6 #1a2c6c #a01818 #1a2c6c",
            "#f0eee6 #18181c #a01818 #18181c",
            "#1c3a7c #f0eee6 #f0d870 #f0eee6",
        ]),
        _ => weighted(
            rng,
            &[
                ("#ece8dc #b21e1c #1e1e22 #b21e1c", 5.),
                ("#eeece4 #1e1e22 #b21e1c #1e1e22", 3.),
                ("#eae8e0 #1c3e8c #b21e1c #1c3e8c", 3.),
                ("#e8c43c #aa1c1a #1c1c1e #aa1c1a", 2.5),
                ("#e8c43c #1c1c1e #aa1c1a #1c1c1e", 1.),
                ("#b01e1e #f0e8d6 #ecc448 #f0e8d6", 3.),
                ("#a81a1c #ecc448 #f0e8d6 #ecc448", 2.),
                ("#22468e #eeece4 #ecc448 #eeece4", 2.),
                ("#1e5c46 #ece8d6 #ecc448 #ece8d6", 2.),
                ("#1a1a1c #e2ba4e #e6e2d6 #e2ba4e", 1.),
                ("#e2d6ba #1e543e #b21e1c #1e543e", 2.),
            ],
        ),
    };
    palette(text)
}
fn faces(c: Construction) -> &'static [Face] {
    use Face::*;
    match c {
        Construction::Sheet => &[Kai, Kai, Brush, Song, Hei, Heavy],
        Construction::Tile => &[Heavy, Kai, Hei, Brush],
        Construction::Cloth => &[Brush, Kai, Heavy],
        Construction::Painted => &[Heavy, Hei, Kai, Brush],
        Construction::Timber => &[Kai, Brush, Song],
        Construction::Lightbox => &[Hei, Round, Heavy, Slim, Round],
        Construction::Enamel => &[Song, Hei, Kai],
    }
}
fn steel_paints() -> [V3; 6] {
    [
        srgb("#2a2c2e"),
        srgb("#3a3c3a"),
        srgb("#2f4f43"),
        srgb("#6a2c22"),
        shade(srgb("#a4a9aa"), 0.85),
        shade(srgb("#a4a9aa"), 0.6),
    ]
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Look {
    pub construction: Construction,
    pub palette: Palette,
    pub face: Face,
    pub age: f64,
    pub phase: f64,
    pub steel: Hardware,
}
pub fn draw_look(
    c: Construction,
    s: &weathered::Settings,
    rng: &mut SeededRng,
) -> Result<Look, String> {
    s.validate()?;
    let age = 1f64.min(rng.float(0.05, 1.) * (0.5 + s.wear + 0.3 * s.grime));
    let painted = age < 0.42;
    let palette = colors(c, rng);
    let face = rng.pick(faces(c));
    let phase = rng.float(0., 100.);
    let mut steel = if painted {
        Hardware {
            slot: "metal".into(),
            tint: shade(rng.pick(&steel_paints()), rng.float(0.85, 1.1)),
            weathering: None,
        }
    } else {
        Hardware {
            slot: "rust".into(),
            tint: shade([1.; 3], rng.float(0.7, 1.)),
            weathering: None,
        }
    };
    if s.texture_density > 0. {
        let mut js = ryu_js::Buffer::new();
        let mut paint =
            SeededRng::from_text(&format!("{}/sign-steel/{}", s.seed, js.format(phase)));
        let mut aging = s.clone();
        aging.seed = format!("{}/sign-paint", s.seed);
        steel = Hardware {
            slot: "metal".into(),
            tint: shade(paint.pick(&steel_paints()), paint.float(0.85, 1.1)),
            weathering: Some(aging),
        };
    }
    Ok(Look {
        construction: c,
        palette,
        face,
        age,
        phase,
        steel,
    })
}
fn panel(look: &Look, size: [f64; 3], stack: Stack, blocks: Vec<Block>) -> Design {
    Design {
        width: size[0],
        height: size[1],
        depth: size[2],
        construction: look.construction,
        outline: Outline::Rectangle,
        ground: look.palette.ground,
        border: Some(look.palette.border),
        stack,
        blocks: blocks.into_iter().filter(|b| !b.text.is_empty()).collect(),
        age: look.age,
        phase: look.phase,
    }
}
fn validate(look: &Look, size: [f64; 3]) -> Result<(), String> {
    panel(look, size, Stack::Down, vec![]).validate()?;
    if look
        .palette
        .ink
        .iter()
        .chain(&look.palette.accent)
        .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
    {
        return Err("Sign ink must be normalized.".into());
    }
    Ok(())
}
fn length(text: &str) -> usize {
    text.encode_utf16().count()
}
fn direction(text: &str, look: &Look, rng: &mut SeededRng) -> Direction {
    if matches!(
        look.construction,
        Construction::Sheet | Construction::Timber
    ) && !text.chars().any(|c| c.is_ascii_alphanumeric())
        && rng.chance(if look.construction == Construction::Timber {
            0.6
        } else {
            0.3
        })
    {
        Direction::Rtl
    } else {
        Direction::Ltr
    }
}
fn shadow(look: &Look, rng: &mut SeededRng) -> Option<V3> {
    rng.chance(match look.construction {
        Construction::Timber => 0.5,
        Construction::Sheet | Construction::Painted => 0.3,
        _ => 0.,
    })
    .then(|| shade(srgb("#2a2927"), 0.9))
}
pub fn title_panel(
    b: &Business,
    size: [f64; 3],
    look: &Look,
    rng: &mut SeededRng,
) -> Result<Design, String> {
    b.validate()?;
    validate(look, size)?;
    let [w, h, _] = size;
    let shadow = shadow(look, rng);
    let title = copy::title(b);
    let floor = copy::floor_line(b);
    let p = &look.palette;
    let block = |text: String, direction, ink, share, shadow| Block {
        text,
        face: look.face,
        direction,
        ink,
        share,
        shadow,
    };
    let mut blocks = vec![];
    if h >= w * 1.5 {
        let slender = h / w;
        let head = floor.or_else(|| (length(&b.tagline) <= 2).then(|| b.tagline.clone()));
        if let Some(head) = head.filter(|_| slender >= 2.6 && rng.chance(0.6)) {
            blocks.push(block(head, Direction::Ltr, p.accent, 0.6, None))
        }
        if length(&b.name) == 3 && slender >= 3. {
            blocks.push(block(b.name.clone(), Direction::Ttb, p.ink, 3., shadow));
            blocks.push(block(
                b.trade.clone(),
                Direction::Ttb,
                p.accent,
                length(&b.trade) as f64 * 0.72,
                None,
            ))
        } else {
            let share = length(&title) as f64;
            blocks.push(block(title, Direction::Ttb, p.ink, share, shadow))
        }
        if slender >= 3.2 && rng.chance(0.45) {
            blocks.push(block(b.phone.clone(), Direction::Ltr, p.accent, 0.42, None))
        }
        return Ok(panel(look, size, Stack::Down, blocks));
    }
    if w >= h * 1.5 {
        let broad = w / h;
        let direction = direction(&title, look, rng);
        let composition = weighted(
            rng,
            &[
                ("plain", 2.),
                ("capitals", if broad >= 2.2 { 3. } else { 0. }),
                (
                    "ends",
                    if broad >= 4. && length(&b.tagline) == 4 {
                        2.5
                    } else {
                        0.
                    },
                ),
                ("slogan", if (2.5..6.).contains(&broad) { 1.5 } else { 0. }),
                ("telephone", if broad >= 3. { 1. } else { 0. }),
            ],
        );
        let mut main = block(title, direction, p.ink, 1., shadow);
        match composition {
            "capitals" => {
                main.share = 0.68;
                return Ok(panel(
                    look,
                    size,
                    Stack::Down,
                    vec![
                        main,
                        block(b.english.clone(), Direction::Ltr, p.accent, 0.32, None),
                    ],
                ));
            }
            "telephone" => {
                main.share = 0.72;
                return Ok(panel(
                    look,
                    size,
                    Stack::Down,
                    vec![
                        main,
                        block(copy::phone_line(b), Direction::Ltr, p.accent, 0.28, None),
                    ],
                ));
            }
            "slogan" => {
                main.share = 0.74;
                return Ok(panel(
                    look,
                    size,
                    Stack::Down,
                    vec![
                        block(b.tagline.clone(), direction, p.accent, 0.26, None),
                        main,
                    ],
                ));
            }
            "ends" => {
                main.share = 0.76;
                let units: Vec<_> = b.tagline.encode_utf16().collect();
                let mut ends = [
                    String::from_utf16_lossy(&units[..2]),
                    String::from_utf16_lossy(&units[2..]),
                ];
                if direction == Direction::Rtl {
                    ends.swap(0, 1)
                }
                return Ok(panel(
                    look,
                    size,
                    Stack::Across,
                    vec![
                        block(ends[0].clone(), Direction::Ttb, p.accent, 0.12, None),
                        main,
                        block(ends[1].clone(), Direction::Ttb, p.accent, 0.12, None),
                    ],
                ));
            }
            _ => return Ok(panel(look, size, Stack::Across, vec![main])),
        }
    }
    blocks.push(block(b.name.clone(), Direction::Ltr, p.ink, 0.5, shadow));
    blocks.push(block(
        b.trade.clone(),
        Direction::Ltr,
        p.accent,
        if length(&b.trade) > 2 { 0.32 } else { 0.5 },
        None,
    ));
    if let Some(up) = copy::way_up(b).filter(|_| rng.chance(0.5)) {
        blocks.push(block(up, Direction::Ltr, p.ink, 0.2, None))
    }
    Ok(panel(look, size, Stack::Down, blocks))
}
pub fn tile_panels(
    text: &str,
    size: f64,
    depth: f64,
    look: &Look,
    rng: &mut SeededRng,
) -> Result<Vec<Design>, String> {
    validate(look, [size, size, depth])?;
    if text.len() > 1024 {
        return Err("Tile lettering exceeds text budget.".into());
    }
    let outline = weighted(
        rng,
        &[
            (Outline::Rectangle, 3.),
            (Outline::Diamond, 2.),
            (Outline::Round, 2.),
        ],
    );
    let alternate = rng.chance(0.35);
    Ok(text
        .chars()
        .enumerate()
        .map(|(i, c)| {
            let swapped = alternate && i % 2 == 1;
            let mut p = panel(
                look,
                [size, size, depth],
                Stack::Down,
                vec![Block {
                    text: c.to_string(),
                    face: look.face,
                    direction: Direction::Ttb,
                    ink: if swapped {
                        look.palette.ground
                    } else {
                        look.palette.ink
                    },
                    share: 1.,
                    shadow: None,
                }],
            );
            p.outline = outline;
            p.ground = if swapped {
                look.palette.ink
            } else {
                look.palette.ground
            };
            if outline != Outline::Rectangle {
                p.border = None
            }
            p
        })
        .collect())
}
pub fn painted_field(
    text: &str,
    size: [f64; 2],
    look: &Look,
    rng: &mut SeededRng,
) -> Result<Design, String> {
    validate(look, [size[0], size[1], 0.])?;
    if text.len() > 2048 {
        return Err("Painted lettering exceeds text budget.".into());
    }
    let direction = if size[1] > size[0] {
        Direction::Ttb
    } else {
        direction(text, look, rng)
    };
    Ok(panel(
        look,
        [size[0], size[1], 0.],
        Stack::Down,
        vec![Block {
            text: text.into(),
            face: look.face,
            direction,
            ink: look.palette.ink,
            share: 1.,
            shadow: shadow(look, rng),
        }],
    ))
}
