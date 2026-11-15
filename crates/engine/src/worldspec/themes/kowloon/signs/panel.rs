//! Complete sign panels. Geometry, reader orientation and artwork are shared by all mounting styles.
use super::{
    Construction, Outline,
    lettering::{self, Direction, Face, Mapping, Paint, Style, Surface},
    weathering,
};
use crate::worldspec::themes::kowloon::{
    fixture::Fixture,
    metal::Kind,
    palette::{blend, mix, mottle, shade, srgb},
    pen::{Pen, Tint},
    steel::{self, Finish, Steel},
    timber, weathered,
};
use serde::{Deserialize, Serialize};
use superspec_geometry::{
    V3,
    math::{cos, sin},
};
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Stack {
    Down,
    Across,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Sides {
    Front,
    Both,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Block {
    pub text: String,
    pub face: Face,
    pub direction: Direction,
    pub ink: V3,
    pub share: f64,
    pub shadow: Option<V3>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Design {
    pub width: f64,
    pub height: f64,
    pub depth: f64,
    pub construction: Construction,
    pub outline: Outline,
    pub ground: V3,
    pub border: Option<V3>,
    pub stack: Stack,
    pub blocks: Vec<Block>,
    pub age: f64,
    pub phase: f64,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Settings {
    pub weathering: weathered::Settings,
    pub lettering: bool,
    pub timber: timber::Settings,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Hardware {
    pub slot: String,
    pub tint: V3,
    pub weathering: Option<weathered::Settings>,
}
impl Hardware {
    pub(super) fn steel(&self) -> Steel<'_> {
        Steel {
            slot: &self.slot,
            tint: self.tint,
            finish: Finish::Metal(self.weathering.as_ref()),
        }
    }
    fn toned(&self, factor: f64) -> Self {
        Self {
            tint: shade(self.tint, factor),
            ..self.clone()
        }
    }
}
impl Design {
    pub fn validate(&self) -> Result<(), String> {
        if [self.width, self.height]
            .iter()
            .any(|v| !v.is_finite() || *v < 0.08 || *v > 100.)
            || !self.depth.is_finite()
            || !(0. ..=2.).contains(&self.depth)
            || (self.depth == 0.
                && !matches!(
                    self.construction,
                    Construction::Painted | Construction::Cloth
                ))
            || !self.age.is_finite()
            || !(0. ..=1.).contains(&self.age)
            || !self.phase.is_finite()
            || self.phase.abs() > 4294967296.
            || self.blocks.len() > 32
            || self.blocks.iter().map(|b| b.text.len()).sum::<usize>() > 2048
            || self
                .ground
                .iter()
                .chain(self.border.iter().flatten())
                .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
            || self.blocks.iter().any(|b| {
                !b.share.is_finite()
                    || b.share <= 0.
                    || b.share > 1000.
                    || b.ink
                        .iter()
                        .chain(b.shadow.iter().flatten())
                        .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
            })
        {
            return Err(
                "Sign design exceeds bounded dimensions, artwork or normalized colors.".into(),
            );
        }
        let frame = frame_width(self.construction);
        if self.width <= 2. * frame || self.height <= 2. * frame {
            return Err("Sign panel is smaller than its frame.".into());
        }
        Ok(())
    }
}
pub fn outline_points(outline: Outline, width: f64, height: f64) -> Vec<[f64; 2]> {
    match outline {
        Outline::Diamond => vec![
            [width / 2., 0.],
            [width, height / 2.],
            [width / 2., height],
            [0., height / 2.],
        ],
        Outline::Round => (0..16)
            .map(|k| {
                let a = k as f64 / 16. * std::f64::consts::PI * 2.;
                [
                    width / 2. + cos(a) * width / 2.,
                    height / 2. + sin(a) * height / 2.,
                ]
            })
            .collect(),
        Outline::Rectangle => vec![[0., 0.], [width, 0.], [width, height], [0., height]],
    }
}
fn frame_width(c: Construction) -> f64 {
    match c {
        Construction::Sheet => 0.028,
        Construction::Tile => 0.018,
        Construction::Lightbox => 0.032,
        Construction::Timber => 0.05,
        _ => 0.,
    }
}
fn face_slot(c: Construction) -> &'static str {
    match c {
        Construction::Sheet | Construction::Tile => "board",
        Construction::Timber => "timber",
        Construction::Painted => "wall",
        _ => "paint",
    }
}
fn fronts(design: &Design, sides: Sides) -> Vec<(f64, V3)> {
    let mut faces = vec![(0., [0., 1., 0.])];
    if sides == Sides::Both {
        faces.push((-design.depth, [0., -1., 0.]))
    }
    faces
}
fn timber_settings(s: &Settings, aging: Option<&weathered::Settings>) -> timber::Settings {
    timber::Settings {
        slots: vec!["timber".into()],
        front_wear: Some(timber::FrontWear {
            width: 0.008,
            strength: 1.2f64.min(aging.map_or(0.2, |a| a.wear) * 4.),
            direction: Some([0., 1., 0.]),
        }),
        ..s.timber.clone()
    }
}
fn body_pen(
    d: &mut Fixture<'_, '_>,
    s: &Settings,
    hardware: &Hardware,
    slot: &str,
    timber: bool,
) -> Result<Pen, String> {
    if timber {
        return d
            .pen
            .with_timber(timber_settings(s, hardware.weathering.as_ref()));
    }
    let Some(aging) = &hardware.weathering else {
        return Ok(d.pen.clone());
    };
    d.pen
        .with_weathering(d.mesh, aging.clone(), Kind::Frame, None, vec![slot.into()])
}
fn member(
    d: &mut Fixture<'_, '_>,
    h: &Hardware,
    s: &Settings,
    ends: [V3; 2],
    side: V3,
    section: [f64; 2],
) -> Result<(), String> {
    if h.slot != "timber" {
        return steel::member(d, h.steel(), ends, side, section);
    }
    let pen = body_pen(d, s, h, "timber", true)?;
    pen.beveled_bar(
        d.mesh,
        "timber",
        ends[0],
        ends[1],
        side,
        section[0],
        section[1],
        if h.weathering.is_some() { 0.0015 } else { 0. },
        &Tint::Color(h.tint),
        d.repeat("timber"),
    )
}
fn body(
    d: &mut Fixture<'_, '_>,
    design: &Design,
    sides: Sides,
    h: &Hardware,
    s: &Settings,
) -> Result<(), String> {
    let (w, height, depth, c) = (
        design.width,
        design.height,
        design.depth,
        design.construction,
    );
    let edge_slot = match c {
        Construction::Timber => "timber",
        Construction::Enamel => "paint",
        _ => &h.slot,
    };
    let edge_tint = match c {
        Construction::Timber => shade(design.ground, 0.7),
        Construction::Enamel => shade(design.ground, 0.75),
        _ => h.tint,
    };
    let pen = body_pen(d, s, h, edge_slot, c == Construction::Timber)?;
    let mut body = Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    };
    if design.outline != Outline::Rectangle {
        let points = outline_points(design.outline, w, height);
        for k in 0..points.len() {
            let [x0, z0] = points[k];
            let [x1, z1] = points[(k + 1) % points.len()];
            body.face(
                edge_slot,
                &[
                    [x0, 0., z0],
                    [x1, 0., z1],
                    [x1, -depth, z1],
                    [x0, -depth, z0],
                ],
                [z1 - z0, 0., x0 - x1],
                edge_tint,
            )?;
        }
        if sides == Sides::Front {
            body.face(
                edge_slot,
                &points
                    .iter()
                    .map(|p| [p[0], -depth, p[1]])
                    .collect::<Vec<_>>(),
                [0., -1., 0.],
                shade(edge_tint, 0.7),
            )?
        }
        return Ok(());
    }
    body.cuboid(
        edge_slot,
        [0., -depth, 0.],
        [w, 0., height],
        edge_tint,
        "bf",
    )?;
    if sides == Sides::Front {
        body.face(
            edge_slot,
            &[
                [0., -depth, 0.],
                [w, -depth, 0.],
                [w, -depth, height],
                [0., -depth, height],
            ],
            [0., -1., 0.],
            shade(edge_tint, 0.7),
        )?
    }
    let frame = frame_width(c);
    let proud = match c {
        Construction::Timber => 0.014,
        Construction::Lightbox => 0.006,
        _ => 0.009,
    };
    let frame_h = if c == Construction::Timber {
        Hardware {
            slot: "timber".into(),
            tint: shade(design.ground, 0.55),
            weathering: h.weathering.clone(),
        }
    } else {
        h.clone()
    };
    if frame > 0. {
        for (y, out) in fronts(design, sides) {
            let middle = y + out[1] * (proud / 2.);
            for z in [frame / 2., height - frame / 2.] {
                member(
                    d,
                    &frame_h,
                    s,
                    [[0., middle, z], [w, middle, z]],
                    [0., 0., 1.],
                    [frame, proud],
                )?
            }
            for x in [frame / 2., w - frame / 2.] {
                member(
                    d,
                    &frame_h.toned(0.92),
                    s,
                    [[x, middle, frame], [x, middle, height - frame]],
                    [1., 0., 0.],
                    [frame, proud],
                )?
            }
        }
    }
    if c == Construction::Lightbox {
        member(
            d,
            &h.toned(0.85),
            s,
            [
                [-0.015, -depth / 2., height + 0.006],
                [w + 0.015, -depth / 2., height + 0.006],
            ],
            [0., 1., 0.],
            [depth + 0.04, 0.012],
        )?
    }
    if c == Construction::Enamel
        || (h.weathering.is_some() && matches!(c, Construction::Sheet | Construction::Tile))
    {
        let screws = Hardware {
            slot: "metal".into(),
            tint: if h.weathering.is_some() {
                [0.25, 0.26, 0.24]
            } else {
                shade(srgb("#7a4a2a"), 1.4)
            },
            weathering: h.weathering.clone(),
        };
        let inset = 0.03f64.min(w.min(height) * 0.12);
        for (y, out) in fronts(design, sides) {
            for x in [inset, w - inset] {
                for z in [inset, height - inset] {
                    steel::bolt(d, screws.steel(), [x, y, z], out, 0.006)?
                }
            }
        }
    }
    Ok(())
}
fn tone(design: &Design, s: &Settings, p: V3) -> V3 {
    let smooth = matches!(
        design.construction,
        Construction::Lightbox | Construction::Enamel
    );
    let grime = s.weathering.grime * (0.35 + 0.65 * design.age) * if smooth { 0.55 } else { 1. };
    let noise = mottle(p[0] * 1.9, p[2] * 1.4, design.phase);
    let dirt = grime
        * (0.12 + 0.34 * (-(design.height - p[2]) / 0.2).exp() + 0.26 * (-p[2] / 0.09).exp())
        * (0.72 + 0.28 * noise);
    let value = 0.3f64.max(1. - dirt);
    [value, value * 0.985, value * 0.94]
}
fn faded(design: &Design, s: &Settings, p: V3) -> f64 {
    1f64.min(
        s.weathering.wear
            * design.age
            * (0.5 + 0.3 * mottle(p[0] * 3.1, p[2] * 2.3, design.phase + 9.)),
    )
}
fn polygon(
    d: &mut Fixture<'_, '_>,
    surface: &Surface,
    p: &Paint,
    tint: &Tint<'_>,
    mapped: bool,
) -> Result<(), String> {
    let points: Vec<_> = p
        .points
        .iter()
        .map(|v| surface.at(v[0], v[1], p.lift))
        .collect();
    let uv: Vec<_> = p.points.iter().map(|v| p.mapping.at(v[0], v[1])).collect();
    d.pen.face(
        d.mesh,
        &p.slot,
        &points,
        surface.out,
        tint,
        d.repeat(&p.slot),
        if mapped { Some(&uv) } else { None },
        None,
    )
}
fn rectangle(slot: &str, bounds: [f64; 4], lift: f64, tint: V3, repeat: f64) -> Paint {
    let [x0, y0, x1, y1] = bounds;
    Paint {
        slot: slot.into(),
        points: vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]],
        lift,
        tint,
        mapping: Mapping::repeat(repeat, [0.; 2]),
    }
}
pub fn draw(
    d: &mut Fixture<'_, '_>,
    design: &Design,
    sides: Sides,
    hardware: &Hardware,
    s: &Settings,
) -> Result<(), String> {
    design.validate()?;
    s.weathering.validate()?;
    if let Some(aging) = &hardware.weathering {
        aging.validate()?;
    }
    if hardware.slot.is_empty()
        || hardware.slot.len() > 4096
        || hardware
            .tint
            .iter()
            .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
    {
        return Err("Invalid sign hardware finish.".into());
    }
    d.mesh
        .spend_work(design.blocks.iter().map(|b| b.text.len()).sum::<usize>() + 1)?;
    let (w, h, c) = (design.width, design.height, design.construction);
    let raster = s.weathering.texture_density > 0.;
    if !matches!(c, Construction::Painted | Construction::Cloth) {
        body(d, design, sides, hardware, s)?
    }
    for (y, out) in fronts(design, sides) {
        let surface = Surface::new(&d.pen, [0., y, 0.], [w, 0., 0.], [0., 0., h], out)?;
        let slot = face_slot(c);
        let mut paints = vec![];
        let shift = [design.phase * 0.37, design.phase * 0.61];
        if raster || design.outline != Outline::Rectangle {
            let p = Paint {
                slot: slot.into(),
                points: outline_points(design.outline, w, h),
                lift: 0.,
                tint: design.ground,
                mapping: Mapping::repeat(d.repeat(slot), shift),
            };
            if raster {
                paints.push(p)
            } else {
                polygon(
                    d,
                    &surface,
                    &p,
                    &Tint::Field(&|point| {
                        Ok(blend(
                            mix(
                                design.ground,
                                [0.72, 0.72, 0.7],
                                faded(design, s, point) * 0.4,
                            ),
                            tone(design, s, point),
                        ))
                    }),
                    true,
                )?
            }
        } else {
            let cols = (w / 0.24).ceil().max(1.) as usize;
            let rows = (h / 0.24).ceil().max(1.) as usize;
            for i in 0..cols {
                for j in 0..rows {
                    let mut p = rectangle(
                        slot,
                        [
                            w * i as f64 / cols as f64,
                            h * j as f64 / rows as f64,
                            w * (i + 1) as f64 / cols as f64,
                            h * (j + 1) as f64 / rows as f64,
                        ],
                        0.,
                        design.ground,
                        d.repeat(slot),
                    );
                    p.mapping = Mapping::repeat(d.repeat(slot), shift);
                    polygon(
                        d,
                        &surface,
                        &p,
                        &Tint::Field(&|point| {
                            Ok(blend(
                                mix(
                                    design.ground,
                                    [0.72, 0.72, 0.7],
                                    faded(design, s, point) * 0.4,
                                ),
                                tone(design, s, point),
                            ))
                        }),
                        true,
                    )?;
                }
            }
        }
        let frame = frame_width(c);
        let edge = frame + 0.012f64.max(0.035 * w.min(h));
        if let Some(border) = design
            .border
            .filter(|_| design.outline == Outline::Rectangle)
        {
            let line = 0.008f64.max(0.012 * w.min(h) + 0.004);
            for bounds in [
                [edge, edge, w - edge, edge + line],
                [edge, h - edge - line, w - edge, h - edge],
                [edge, edge + line, edge + line, h - edge - line],
                [w - edge - line, edge + line, w - edge, h - edge - line],
            ] {
                let p = rectangle("paint", bounds, 0.003, border, d.repeat("paint"));
                if raster {
                    paints.push(p)
                } else {
                    polygon(
                        d,
                        &surface,
                        &p,
                        &Tint::Field(&|point| {
                            Ok(blend(
                                mix(border, design.ground, faded(design, s, point) * 0.5),
                                tone(design, s, point),
                            ))
                        }),
                        false,
                    )?
                }
            }
        }
        let inset = if design.outline == Outline::Rectangle {
            edge + if design.border.is_some() {
                0.03 * w.min(h) + 0.012
            } else {
                0.01
            }
        } else {
            w.min(h) * 0.22
        };
        let area = [inset, inset, w - 2. * inset, h - 2. * inset];
        let total = design.blocks.iter().map(|b| b.share).sum::<f64>();
        let mut used = 0.;
        for b in &design.blocks {
            let from = used / total;
            let to = (used + b.share) / total;
            used += b.share;
            let gap = 0.04
                * if design.stack == Stack::Down {
                    area[3]
                } else {
                    area[2]
                };
            let box_ = if design.stack == Stack::Down {
                [
                    area[0],
                    area[1] + area[3] * (1. - to) + gap / 2.,
                    area[2],
                    area[3] * (to - from) - gap,
                ]
            } else {
                [
                    area[0] + area[2] * from + gap / 2.,
                    area[1],
                    area[2] * (to - from) - gap,
                    area[3],
                ]
            };
            if box_[2] <= 0. || box_[3] <= 0. {
                continue;
            }
            let glyphs = lettering::letter(
                &b.text,
                box_,
                Style {
                    face: b.face,
                    direction: b.direction,
                    ink: b.ink,
                    lift: 0.005,
                    shadow: b.shadow.map(|c| (c, 0.035 * box_[2].min(box_[3]))),
                },
                s.lettering,
                d.repeat("paint"),
            )?;
            for p in glyphs {
                if raster {
                    paints.push(p)
                } else {
                    let color = |point| {
                        Ok(if p.lift < 0.005 {
                            blend(p.tint, tone(design, s, point))
                        } else {
                            blend(
                                mix(p.tint, design.ground, faded(design, s, point) * 0.55),
                                tone(design, s, point),
                            )
                        })
                    };
                    polygon(d, &surface, &p, &Tint::Field(&color), s.lettering)?;
                }
            }
        }
        if raster {
            weathering::draw(
                d,
                &surface,
                weathering::Recipe {
                    width: w,
                    height: h,
                    construction: c,
                    outline: design.outline,
                    age: design.age,
                    phase: design.phase,
                    grime: s.weathering.grime,
                    wear: s.weathering.wear,
                    rust: s.weathering.rust,
                    underpaint: s.weathering.paint.material.clone(),
                    oxide: "rust".into(),
                    paints,
                },
                s.weathering.texture_density,
            )?;
        } else if design.outline == Outline::Rectangle
            && design.age > 0.45
            && c == Construction::Sheet
        {
            let count = (w / 0.22).round().max(2.) as usize;
            let stained = mix(design.ground, srgb("#7a4a2a"), 0.45 + 0.3 * design.age);
            let top = h - frame;
            for i in 0..count {
                let wobble = mottle(i as f64 * 2.7, design.phase, design.phase);
                if wobble < -0.25 {
                    continue;
                }
                let x = frame
                    + 0.02
                    + ((w - 2. * frame - 0.04) * (i as f64 + 0.5 + 0.35 * wobble)) / count as f64;
                let length = (h * 0.45).min((0.05 + 0.3 * design.age) * (0.6 + 0.5 * wobble));
                let half = 0.003 + 0.004 * design.age;
                d.pen.face(
                    d.mesh,
                    "paint",
                    &[
                        surface.at(x - half, top, 0.002),
                        surface.at(x + half, top, 0.002),
                        surface.at(x, top - length, 0.002),
                    ],
                    surface.out,
                    &Tint::Field(&|point| Ok(blend(stained, tone(design, s, point)))),
                    d.repeat("paint"),
                    None,
                    None,
                )?;
            }
        }
    }
    Ok(())
}
