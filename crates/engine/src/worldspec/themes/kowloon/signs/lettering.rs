//! Atlas metrics are shipped data; layout and reader orientation run in native Rust and Wasm.
use crate::worldspec::themes::kowloon::pen::Pen;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::OnceLock};
use superspec_geometry::{
    V3,
    math::{add, cross, dot, length, sub},
};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Face {
    Hei,
    Song,
    Kai,
    Brush,
    Heavy,
    Round,
    Slim,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Ltr,
    Rtl,
    Ttb,
}
#[derive(Deserialize)]
struct Atlas {
    size: f64,
    cell: f64,
    em: f64,
    columns: usize,
    rows: usize,
    faces: Vec<Face>,
    characters: String,
    latin: String,
    advances: BTreeMap<Face, Vec<f64>>,
}
struct Metrics {
    atlas: Atlas,
    cells: BTreeMap<char, usize>,
    latin: BTreeMap<char, usize>,
}
fn metrics() -> &'static Metrics {
    static DATA: OnceLock<Metrics> = OnceLock::new();
    DATA.get_or_init(|| {
        let atlas: Atlas =
            serde_json::from_str(include_str!("atlas.json")).expect("shipped lettering atlas");
        Metrics {
            cells: atlas
                .characters
                .chars()
                .enumerate()
                .map(|(i, c)| (c, i))
                .collect(),
            latin: atlas
                .latin
                .chars()
                .enumerate()
                .map(|(i, c)| (c, i))
                .collect(),
            atlas,
        }
    })
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Glyph {
    pub character: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
#[derive(Debug, Deserialize, Serialize)]
pub struct Layout {
    pub glyphs: Vec<Glyph>,
    pub missing: Vec<String>,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct Cell {
    pub u0: f64,
    pub v0: f64,
    pub u1: f64,
    pub v1: f64,
}
pub fn glyph_cell(face: Face, character: char) -> Option<Cell> {
    let m = metrics();
    let index = *m.cells.get(&character)?;
    let cell = m.atlas.cell / m.atlas.size;
    let column = index % m.atlas.columns;
    let row =
        m.atlas.faces.iter().position(|f| *f == face)? * m.atlas.rows + index / m.atlas.columns;
    Some(Cell {
        u0: column as f64 * cell,
        v0: 1. - (row + 1) as f64 * cell,
        u1: (column + 1) as f64 * cell,
        v1: 1. - row as f64 * cell,
    })
}
pub fn layout_line(
    text: &str,
    face: Face,
    width: f64,
    height: f64,
    direction: Direction,
) -> Result<Layout, String> {
    if text.len() > 8192
        || text.chars().count() > 2048
        || [width, height]
            .iter()
            .any(|v| !v.is_finite() || *v <= 0. || *v > 100.)
    {
        return Err("Lettering requires bounded text and positive meter dimensions.".into());
    }
    let m = metrics();
    let mut missing = Vec::new();
    let mut characters = Vec::new();
    for c in text.chars().flat_map(char::to_uppercase) {
        if c == ' ' || m.cells.contains_key(&c) {
            characters.push(c)
        } else if !missing.contains(&c.to_string()) {
            missing.push(c.to_string())
        }
    }
    let inked: Vec<_> = characters.iter().copied().filter(|c| *c != ' ').collect();
    let mut glyphs = Vec::new();
    if inked.is_empty() {
        return Ok(Layout { glyphs, missing });
    }
    if direction != Direction::Ttb && characters.iter().any(|c| m.latin.contains_key(c)) {
        let advances: Vec<_> = characters
            .iter()
            .map(|c| m.latin.get(c).map_or(1., |i| m.atlas.advances[&face][*i]))
            .collect();
        let total = advances.iter().sum::<f64>();
        let em = height.min(width / total);
        let mut pen = (width - total * em) / 2.;
        for (c, advance) in characters.into_iter().zip(advances) {
            let advance = advance * em;
            if c != ' ' {
                glyphs.push(Glyph {
                    character: c.to_string(),
                    x: pen + advance / 2.,
                    y: height / 2.,
                    width: em,
                    height: em,
                });
            }
            pen += advance;
        }
    } else {
        let count = inked.len();
        for (index, c) in inked.into_iter().enumerate() {
            let (x, y, w, h) = if direction == Direction::Ttb {
                let slot = height / count as f64;
                let square = slot.min(width);
                let latin = m.latin.contains_key(&c);
                (
                    width / 2.,
                    height - (index as f64 + 0.5) * slot,
                    if latin {
                        square
                    } else {
                        width.min(square * 1.25)
                    },
                    if latin {
                        square
                    } else {
                        slot.min(square * 1.25)
                    },
                )
            } else {
                let slot = width / count as f64;
                let square = slot.min(height);
                let place = if direction == Direction::Rtl {
                    count - 1 - index
                } else {
                    index
                };
                (
                    (place as f64 + 0.5) * slot,
                    height / 2.,
                    slot.min(square * 1.25),
                    height.min(square * 1.25),
                )
            };
            glyphs.push(Glyph {
                character: c.to_string(),
                x,
                y,
                width: w,
                height: h,
            });
        }
    }
    Ok(Layout { glyphs, missing })
}

/// Local meter rectangle oriented for its reader, including reflected host frames.
#[derive(Clone)]
pub struct Surface {
    pub width: f64,
    pub height: f64,
    pub out: V3,
    origin: V3,
    right: V3,
    up: V3,
}
impl Surface {
    pub fn new(pen: &Pen, corner: V3, across: V3, rise: V3, out: V3) -> Result<Self, String> {
        let width = length(across);
        let height = length(rise);
        if corner
            .iter()
            .chain(&across)
            .chain(&rise)
            .chain(&out)
            .any(|v| !v.is_finite() || v.abs() > 10000.)
            || !(0.0001..=100.).contains(&width)
            || !(0.0001..=100.).contains(&height)
            || (length(out) - 1.).abs() > 1e-7
            || dot(across, rise).abs() > 1e-7 * width * height
            || dot(across, out).abs() > 1e-7 * width
            || dot(rise, out).abs() > 1e-7 * height
        {
            return Err(
                "Sign surface requires bounded orthogonal meter edges and a unit normal.".into(),
            );
        }
        let up = rise.map(|v| v / height);
        let zero = pen.world([0.; 3]);
        let world = |v| sub(pen.world(v), zero);
        let left_to_right = dot(world(across), cross(world(up), world(out))) > 0.;
        Ok(Self {
            width,
            height,
            out,
            origin: if left_to_right {
                corner
            } else {
                add(corner, across)
            },
            right: across.map(|v| if left_to_right { v / width } else { -v / width }),
            up,
        })
    }
    pub fn at(&self, x: f64, y: f64, lift: f64) -> V3 {
        std::array::from_fn(|i| {
            self.origin[i] + self.right[i] * x + self.up[i] * y + self.out[i] * lift
        })
    }
    pub fn coordinates(&self, local: V3) -> [f64; 2] {
        let p = sub(local, self.origin);
        [dot(p, self.right), dot(p, self.up)]
    }
}
/// Affine atlas mapping in reader-relative meters. No host callback survives planning.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Mapping {
    pub origin: [f64; 2],
    pub u: [f64; 2],
    pub v: [f64; 2],
}
impl Mapping {
    pub fn repeat(repeat: f64, shift: [f64; 2]) -> Self {
        Self {
            origin: shift.map(|v| v * repeat),
            u: [repeat, 0.],
            v: [0., repeat],
        }
    }
    pub fn at(&self, x: f64, y: f64) -> [f64; 2] {
        std::array::from_fn(|i| self.origin[i] + self.u[i] * x + self.v[i] * y)
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Paint {
    pub slot: String,
    pub points: Vec<[f64; 2]>,
    pub lift: f64,
    pub tint: V3,
    pub mapping: Mapping,
}
#[derive(Clone, Copy)]
pub struct Style {
    pub face: Face,
    pub direction: Direction,
    pub ink: V3,
    pub lift: f64,
    pub shadow: Option<(V3, f64)>,
}
/// Returns ordered artwork, with each shadow immediately before its glyph.
pub fn letter(
    text: &str,
    area: [f64; 4],
    style: Style,
    atlas: bool,
    paint_repeat: f64,
) -> Result<Vec<Paint>, String> {
    if area.iter().any(|v| !v.is_finite() || v.abs() > 10000.)
        || !style.lift.is_finite()
        || style.lift.abs() > 1.
        || !paint_repeat.is_finite()
        || !(0. ..=4096.).contains(&paint_repeat)
        || style
            .ink
            .iter()
            .chain(style.shadow.iter().flat_map(|(c, _)| c))
            .any(|v| !v.is_finite() || !(0. ..=4.).contains(v))
        || style
            .shadow
            .is_some_and(|(_, v)| !v.is_finite() || v.abs() > 100.)
    {
        return Err("Sign artwork requires bounded coordinates, tint and mapping.".into());
    }
    let layout = layout_line(text, style.face, area[2], area[3], style.direction)?;
    let m = metrics();
    let margin = m.atlas.cell / m.atlas.em;
    let inset = 0.5 / m.atlas.size;
    let mut paints = Vec::new();
    let mut layers = Vec::new();
    if let Some((ink, offset)) = style.shadow {
        layers.push((ink, style.lift * 0.5, offset, -offset))
    }
    layers.push((style.ink, style.lift, 0., 0.));
    for glyph in layout.glyphs {
        let cell = glyph_cell(style.face, glyph.character.chars().next().unwrap()).unwrap();
        for &(tint, lift, dx, dy) in &layers {
            let cx = area[0] + glyph.x + dx;
            let cy = area[1] + glyph.y + dy;
            let (hw, hh) = if atlas {
                (glyph.width * margin / 2., glyph.height * margin / 2.)
            } else {
                (glyph.width * 0.36, glyph.height * 0.36)
            };
            let mapping = if atlas {
                let u = (cell.u1 - cell.u0 - 2. * inset) / (2. * hw);
                let v = (cell.v1 - cell.v0 - 2. * inset) / (2. * hh);
                Mapping {
                    origin: [
                        cell.u0 + inset + (-cx + hw) / (2. * hw) * (cell.u1 - cell.u0 - 2. * inset),
                        cell.v0 + inset + (-cy + hh) / (2. * hh) * (cell.v1 - cell.v0 - 2. * inset),
                    ],
                    u: [u, 0.],
                    v: [0., v],
                }
            } else {
                Mapping::repeat(paint_repeat, [0.; 2])
            };
            paints.push(Paint {
                slot: if atlas { "lettering" } else { "paint" }.into(),
                points: vec![
                    [cx - hw, cy - hh],
                    [cx + hw, cy - hh],
                    [cx + hw, cy + hh],
                    [cx - hw, cy + hh],
                ],
                lift,
                tint,
                mapping,
            });
        }
    }
    Ok(paints)
}
