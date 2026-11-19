//! Shop-sign silhouettes and original trade emblems, drawn on both reader-facing surfaces.
use super::{
    Construction, Outline,
    lettering::{self, Direction, Face, Mapping, Paint},
    panel::{Art, Design, outline_points},
};
use crate::worldspec::themes::kowloon::palette::srgb;
use superspec_geometry::{
    V3,
    math::{cos, sin},
};

pub(in crate::worldspec::themes::kowloon) const STYLES: [&str; 6] = [
    "plain",
    "hardware",
    "grocery",
    "electrical",
    "wholesale",
    "pharmacy",
];

#[derive(Clone, Copy)]
pub(in crate::worldspec::themes::kowloon) struct Style {
    pub outline: Outline,
    pub construction: Construction,
    pub face: Face,
    pub depth: f64,
    mark: Mark,
    accent: &'static str,
}

#[derive(Clone, Copy)]
enum Mark {
    Gear,
    Orange,
    Bolt,
    Crates,
    Mortar,
}

pub(in crate::worldspec::themes::kowloon) fn style(name: &str) -> Result<Option<Style>, String> {
    let (outline, construction, face, depth, mark, accent) = match name {
        "plain" => return Ok(None),
        "hardware" => (
            Outline::Clipped,
            Construction::Sheet,
            Face::Heavy,
            0.055,
            Mark::Gear,
            "#e6bd57",
        ),
        "grocery" => (
            Outline::Rounded,
            Construction::Enamel,
            Face::Brush,
            0.036,
            Mark::Orange,
            "#f0a746",
        ),
        "electrical" => (
            Outline::Rectangle,
            Construction::Lightbox,
            Face::Hei,
            0.105,
            Mark::Bolt,
            "#66b9df",
        ),
        "wholesale" => (
            Outline::Shield,
            Construction::Sheet,
            Face::Song,
            0.055,
            Mark::Crates,
            "#e8ba72",
        ),
        "pharmacy" => (
            Outline::Rounded,
            Construction::Enamel,
            Face::Kai,
            0.042,
            Mark::Mortar,
            "#d55b42",
        ),
        _ => {
            return Err(format!(
                "Unknown board design {name:?}. Expected {}.",
                STYLES.join(", ")
            ));
        }
    };
    Ok(Some(Style {
        outline,
        construction,
        face,
        depth,
        mark,
        accent,
    }))
}

#[derive(Clone, Copy)]
pub(in crate::worldspec::themes::kowloon) struct Artwork<'a> {
    pub style: Style,
    pub subtitle: &'a str,
}

type Point = [f64; 2];

/// A ring of convex strips, so the same artwork can be emitted as geometry or baked.
fn ring(outer: &[Point], inner: &[Point]) -> impl Iterator<Item = [Point; 4]> {
    (0..outer.len()).map(|i| {
        let next = (i + 1) % outer.len();
        [outer[i], outer[next], inner[next], inner[i]]
    })
}

fn radial(count: usize, radius: impl Fn(usize) -> f64) -> Vec<Point> {
    (0..count)
        .map(|i| {
            let angle = std::f64::consts::TAU * i as f64 / count as f64;
            let r = radius(i);
            [r * cos(angle), r * sin(angle)]
        })
        .collect()
}

fn rect(left: f64, bottom: f64, right: f64, top: f64) -> [Point; 4] {
    [[left, bottom], [right, bottom], [right, top], [left, top]]
}

impl Artwork<'_> {
    pub(super) fn draw(
        self,
        sign: &Design,
        [x, z, width, height]: [f64; 4],
        art: Art,
        atlas: bool,
        repeat: f64,
    ) -> Result<([f64; 4], Vec<Paint>), String> {
        let mut paints = Vec::new();
        let accent = srgb(self.style.accent);
        let ink = sign.border.unwrap_or(accent);
        let ground = sign.ground;
        let mut add = |points: &[Point], tint: V3, lift: f64| {
            let mut points = points.to_vec();
            let area: f64 = (0..points.len())
                .map(|i| {
                    let (a, b) = (points[i], points[(i + 1) % points.len()]);
                    a[0] * b[1] - a[1] * b[0]
                })
                .sum();
            if area < 0. {
                points.reverse();
            }
            paints.push(Paint {
                slot: "paint".into(),
                points,
                lift: lift * art.lift,
                tint,
                mapping: Mapping::repeat(repeat, [0.; 2]),
            });
        };
        let short = sign.width.min(sign.height);
        if sign.outline != Outline::Rectangle {
            let contour = outline_points(sign.outline, sign.width, sign.height);
            let inset = |pad: f64| {
                contour
                    .iter()
                    .map(|[u, v]| {
                        [
                            pad + u * (1. - 2. * pad / sign.width),
                            pad + v * (1. - 2. * pad / sign.height),
                        ]
                    })
                    .collect::<Vec<_>>()
            };
            for edge in ring(&inset(0.053 * short), &inset(0.087 * short)) {
                add(&edge, ink, 0.003);
            }
        }

        let horizontal = sign.width > sign.height;
        let long = if horizontal { width } else { height };
        let side = (width.min(height) * 0.84).min(long * 0.42);
        let gap = side * 0.15;
        let (center, mut text) = if horizontal {
            (
                [x + side / 2., z + height / 2.],
                [x + side + gap, z, width - side - gap, height],
            )
        } else if matches!(self.style.mark, Mark::Crates | Mark::Bolt) {
            let bottom = if sign.outline == Outline::Shield {
                sign.height * 0.24
            } else {
                z
            };
            let raised = bottom - z + side + gap;
            (
                [x + width / 2., bottom + side / 2.],
                [x, z + raised, width, height - raised],
            )
        } else {
            (
                [x + width / 2., z + height - side / 2.],
                [x, z, width, height - side - gap],
            )
        };
        let mut mark = |points: &[Point], tint: V3, lift| {
            let placed: Vec<_> = points
                .iter()
                .map(|[u, v]| [center[0] + side * u, center[1] + side * v])
                .collect();
            add(&placed, tint, lift);
        };
        let badge = match self.style.mark {
            Mark::Gear | Mark::Orange => radial(24, |_| 0.5),
            Mark::Bolt | Mark::Mortar => vec![[0., -0.5], [0.5, 0.], [0., 0.5], [-0.5, 0.]],
            Mark::Crates => rect(-0.46, -0.46, 0.46, 0.46).to_vec(),
        };
        mark(&badge, accent, 0.004);
        let field = badge
            .iter()
            .map(|point| point.map(|c| c * 0.86))
            .collect::<Vec<_>>();
        mark(&field, ground, 0.006);
        match self.style.mark {
            Mark::Gear => {
                let teeth = radial(32, |i| if i % 4 == 1 || i % 4 == 2 { 0.33 } else { 0.25 });
                for tooth in ring(&teeth, &radial(32, |_| 0.125)) {
                    mark(&tooth, ink, 0.008);
                }
            }
            Mark::Orange => {
                let fruit = radial(20, |_| 0.245);
                let fruit: Vec<_> = fruit.into_iter().map(|[x, y]| [x, y - 0.055]).collect();
                mark(&fruit, accent, 0.008);
                mark(
                    &[[0., 0.18], [0.09, 0.31], [0.27, 0.34], [0.17, 0.22]],
                    ink,
                    0.008,
                );
            }
            Mark::Bolt => {
                mark(
                    &[
                        [-0.19, -0.037],
                        [-0.06, 0.29],
                        [0.10, 0.29],
                        [0.015, -0.037],
                    ],
                    accent,
                    0.008,
                );
                mark(
                    &[[-0.055, -0.037], [0.21, -0.037], [-0.10, -0.29]],
                    accent,
                    0.008,
                );
            }
            Mark::Crates => {
                for [left, bottom] in [[-0.31, -0.28], [0.03, -0.28], [-0.14, 0.01]] {
                    mark(&rect(left, bottom, left + 0.28, bottom + 0.24), ink, 0.008);
                    mark(
                        &rect(left + 0.065, bottom + 0.09, left + 0.215, bottom + 0.15),
                        ground,
                        0.01,
                    );
                }
            }
            Mark::Mortar => {
                mark(
                    &[[-0.11, -0.27], [0.11, -0.27], [0.25, -0.02], [-0.25, -0.02]],
                    ink,
                    0.008,
                );
                mark(
                    &[[-0.055, -0.02], [0.13, 0.24], [0.235, 0.17], [0.08, -0.02]],
                    accent,
                    0.008,
                );
            }
        }
        if !self.subtitle.is_empty() {
            let footer = (0.22 * text[3]).min(side * if horizontal { 0.33 } else { 0.46 });
            let caption = [text[0], text[1], text[2], footer];
            let line = text[2].min(footer) * 0.08;
            add(
                &rect(
                    text[0],
                    text[1] + footer,
                    text[0] + text[2],
                    text[1] + footer + line,
                ),
                accent,
                0.004,
            );
            text[1] += footer + 2. * line;
            text[3] -= footer + 2. * line;
            paints.extend(lettering::letter(
                self.subtitle,
                caption,
                lettering::Style {
                    face: Face::Slim,
                    direction: Direction::Ltr,
                    ink,
                    lift: 0.005 * art.lift,
                    shadow: None,
                },
                atlas,
                repeat,
            )?);
        }
        Ok((text, paints))
    }
}
