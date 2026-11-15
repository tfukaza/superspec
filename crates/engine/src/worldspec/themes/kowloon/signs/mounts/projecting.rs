//! Projecting and hanging signs: bounded proposals with owned support geometry plans.
use super::*;
use crate::worldspec::themes::kowloon::{metal::Kind, stains};
use copy::{Brief, Sector};

#[derive(Clone, Copy, Deserialize)]
pub struct PierRoom {
    pub u: f64,
    pub foot: f64,
    pub ceiling: f64,
    pub reach: f64,
}
impl PierRoom {
    fn validate(self) -> Result<(), String> {
        if [self.u, self.foot, self.ceiling, self.reach]
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 1000.)
            || self.ceiling < self.foot
            || self.ceiling - self.foot > 100.
            || !(0. ..=20.).contains(&self.reach)
        {
            return Err("Projecting sign needs bounded pier clearance.".into());
        }
        Ok(())
    }
}
pub struct Projected {
    pub proposal: Proposal,
    pub top: f64,
}
#[derive(Clone, Copy)]
struct Carriage {
    u: f64,
    inner: f64,
    outer: f64,
    bottom: f64,
    top: f64,
    stay: f64,
    strut: f64,
    uprights: bool,
}
#[derive(Clone, Copy)]
enum Extra {
    None,
    Tiles,
    Board { lit: bool, trussed: bool },
}
#[derive(Clone, Copy)]
enum Support {
    Carried {
        carriage: Carriage,
        extra: Extra,
    },
    Hung {
        u: f64,
        standoff: f64,
        width: f64,
        top: f64,
        arm: f64,
    },
}
#[derive(Clone, Copy)]
pub(super) struct Fixings {
    age: f64,
    support: Support,
}
fn blade_pen(site: &Site, u: f64, standoff: f64, z: f64, depth: f64) -> Result<Pen, String> {
    site.pen
        .facing([u + depth / 2., standoff, z], [0., 1., 0.], [1., 0., 0.])
}
fn wall_fixing(
    d: &mut Fixture<'_, '_>,
    site: &Site,
    steel: steel::Steel<'_>,
    age: f64,
    u: f64,
    z: f64,
    height: f64,
) -> Result<(), String> {
    steel::fixing_plate(
        d,
        steel,
        &Plate {
            center: [u, 0., z],
            across: [1., 0., 0.],
            up: [0., 0., 1.],
            out: [0., 1., 0.],
            size: [0.11, height],
            bolts: if site.detail > 0 { 4 } else { 2 },
        },
    )?;
    if let Some(wash) = site.wash
        && age > 0.5
    {
        stains::stain(
            d.mesh,
            &d.pen,
            [
                u - 0.05,
                z - height / 2. - (0.2 + 0.5 * age),
                u + 0.05,
                z - height / 2.,
            ],
            wash,
            0.25 + 0.25 * age,
        )?;
    }
    Ok(())
}
fn carry(
    d: &mut Fixture<'_, '_>,
    site: &Site,
    hardware: &Hardware,
    age: f64,
    c: Carriage,
) -> Result<(), String> {
    let pen = if let Some(s) = &hardware.weathering {
        d.pen.with_weathering(
            d.mesh,
            s.clone(),
            Kind::Frame,
            None,
            vec![hardware.slot.clone()],
        )?
    } else {
        d.pen.clone()
    };
    let d = &mut Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    };
    let steel = hardware.steel();
    let u = c.u;
    let length = c.outer + 0.04;
    let high = c.top + 0.012;
    let low = c.bottom - 0.012;
    steel::angle(
        d,
        steel,
        [[u - 0.02, 0., high], [u - 0.02, length, high]],
        [[1., 0., 0.], [0., 0., 1.]],
        [0.04, 0.005],
    )?;
    steel::angle(
        d,
        steel,
        [[u - 0.02, 0., low], [u - 0.02, length, low]],
        [[1., 0., 0.], [0., 0., -1.]],
        [0.04, 0.005],
    )?;
    wall_fixing(d, site, steel, age, u, high + 0.02, 0.15)?;
    wall_fixing(d, site, steel, age, u, low - 0.02, 0.15)?;
    for (at, direction) in [(high, -1.), (low, 1.)] {
        steel::gusset(
            d,
            steel.toned(0.9),
            [
                [u, 0.012, at],
                [u, 0.14, at],
                [u, 0.012, at + direction * 0.12],
            ],
            [1., 0., 0.],
            0.006,
        )?;
        for offset in [0.035, 0.095] {
            steel::bolt(
                d,
                steel,
                [u + 0.003, 0.025, at + direction * offset],
                [1., 0., 0.],
                0.007,
            )?;
        }
    }
    if c.uprights {
        for y in [c.inner - 0.012, c.outer + 0.012] {
            steel::member(
                d,
                steel.toned(0.95),
                [[u, y, low], [u, y, high]],
                [1., 0., 0.],
                [0.035, 0.006],
            )?;
        }
    }
    if c.top - c.bottom > 2.2 {
        let middle = (c.top + c.bottom) / 2.;
        steel::angle(
            d,
            steel,
            [[u - 0.02, 0., middle], [u - 0.02, c.inner - 0.012, middle]],
            [[1., 0., 0.], [0., 0., 1.]],
            [0.04, 0.005],
        )?;
        wall_fixing(d, site, steel, age, u, middle + 0.02, 0.15)?;
    }
    if c.stay > 0. {
        let ends = [
            [u, c.inner + (c.outer - c.inner) * 0.8, high + 0.045],
            [u, 0.012, high + c.stay],
        ];
        steel::rod(d, steel, &ends, 0.006, 5)?;
        if site.detail > 0 {
            steel::turnbuckle(d, steel.toned(0.85), ends, 0.006)?;
        }
        steel::fixing_plate(
            d,
            steel,
            &Plate {
                center: [u, 0., high + c.stay],
                across: [1., 0., 0.],
                up: [0., 0., 1.],
                out: [0., 1., 0.],
                size: [0.07, 0.09],
                bolts: 2,
            },
        )?;
    } else if c.strut > 0. {
        steel::member(
            d,
            steel,
            [
                [u, 0.006, low - c.strut],
                [u, c.inner + (c.outer - c.inner) * 0.55, low - 0.02],
            ],
            [1., 0., 0.],
            [0.035, 0.006],
        )?;
        wall_fixing(d, site, steel, age, u, low - c.strut, 0.1)?;
    }
    Ok(())
}
impl Fixings {
    pub fn draw(
        self,
        d: &mut Fixture<'_, '_>,
        site: &Site,
        hardware: &Hardware,
    ) -> Result<(), String> {
        let steel = hardware.steel();
        match self.support {
            Support::Carried { carriage: c, extra } => {
                carry(d, site, hardware, self.age, c)?;
                let u = c.u;
                match extra {
                    Extra::None => {}
                    Extra::Tiles => steel::rod(
                        d,
                        steel,
                        &[
                            [u, (c.inner + c.outer) / 2., c.bottom - 0.012],
                            [u, (c.inner + c.outer) / 2., c.top + 0.012],
                        ],
                        0.007,
                        5,
                    )?,
                    Extra::Board { lit, trussed } => {
                        let reach = c.outer + 0.04;
                        if trussed {
                            steel::lattice(
                                d,
                                steel,
                                [
                                    [[u, 0.01, c.top + 0.05], [u, reach, c.top + 0.05]],
                                    [[u, 0.01, c.top + 0.2], [u, reach, c.top + 0.2]],
                                ],
                                (reach / 0.22).round().max(3.) as usize,
                                [0.028, 0.018],
                            )?;
                        }
                        if lit {
                            steel::rod(
                                d,
                                steel::Steel {
                                    slot: "paint",
                                    tint: [0.03; 3],
                                    finish: steel::Finish::Metal(None),
                                },
                                &[
                                    [u + 0.05, 0.01, c.top + 0.3],
                                    [u + 0.05, c.inner * 0.5, c.top + 0.12],
                                    [u + 0.03, c.inner + 0.08, c.top],
                                ],
                                0.009,
                                5,
                            )?;
                        }
                    }
                }
            }
            Support::Hung {
                u,
                standoff,
                width,
                top,
                arm,
            } => {
                let reach = standoff + width + 0.08;
                steel::angle(
                    d,
                    steel,
                    [[u - 0.02, 0., arm], [u - 0.02, reach, arm]],
                    [[1., 0., 0.], [0., 0., 1.]],
                    [0.04, 0.005],
                )?;
                steel::member(
                    d,
                    steel,
                    [
                        [u, 0.006, arm - 0.3],
                        [u, 0.36_f64.min(standoff + 0.1), arm - 0.01],
                    ],
                    [1., 0., 0.],
                    [0.035, 0.006],
                )?;
                wall_fixing(d, site, steel, self.age, u, arm - 0.14, 0.4)?;
                steel::member(
                    d,
                    steel.toned(0.8),
                    [[u, reach, arm - 0.02], [u, reach, arm + 0.07]],
                    [1., 0., 0.],
                    [0.03, 0.03],
                )?;
                for y in [standoff + 0.08, standoff + width - 0.08] {
                    steel::rod(d, steel, &[[u, y, arm], [u, y, top]], 0.004, 4)?;
                    if site.detail > 0 {
                        steel::chain(d, steel, [[u, y, arm], [u, y, top]], 0.035)?;
                    }
                }
            }
        }
        Ok(())
    }
}
struct Bracing {
    bottom: f64,
    strut: f64,
    rise: f64,
}
impl Bracing {
    fn choose(room: PierRoom, rng: &mut SeededRng) -> Self {
        let strutted = rng.chance(0.3) && room.ceiling - room.foot >= 2.3;
        Self {
            bottom: room.foot + if strutted { 0.52 } else { 0.12 },
            strut: if strutted { 0.38 } else { 0. },
            rise: rng.float(0.45, 0.9),
        }
    }
    fn stay(&self, room: PierRoom, top: f64) -> f64 {
        let above = room.ceiling - top - 0.07;
        if self.strut == 0. && above >= 0.4 {
            above.min(self.rise)
        } else {
            0.
        }
    }
    fn carriage(
        &self,
        room: PierRoom,
        inner: f64,
        width: f64,
        top: f64,
        uprights: bool,
    ) -> Carriage {
        Carriage {
            u: room.u,
            inner,
            outer: inner + width,
            bottom: self.bottom,
            top,
            stay: self.stay(room, top),
            strut: self.strut,
            uprights,
        }
    }
}
fn carried(
    site: &Site,
    panels: Vec<PlacedPanel>,
    look: &Look,
    b: &Business,
    c: Carriage,
    extra: Extra,
    form: Form,
) -> Projected {
    let mut proposal = Proposal::new(
        site,
        Mount::Blade,
        panels,
        look,
        (c.bottom + c.top) / 2.,
        b,
        super::Fixings::Projecting(Fixings {
            age: look.age,
            support: Support::Carried { carriage: c, extra },
        }),
    );
    proposal.form = form;
    if form == Form::Ladder {
        proposal.business = None;
    }
    Projected {
        proposal,
        top: c.top + 0.012 + c.stay.max(0.1),
    }
}
pub fn blade_sign(
    site: &Site,
    room: PierRoom,
    b: &Business,
    s: &Settings,
    rng: &mut SeededRng,
) -> Result<Option<Projected>, String> {
    site.validate()?;
    room.validate()?;
    b.validate()?;
    if room.ceiling - room.foot < 1.6 || room.reach < 0.68 {
        return Ok(None);
    }
    let bracing = Bracing::choose(room, rng);
    let bottom = bracing.bottom;
    let height = room.ceiling - bottom - 0.18;
    let title = copy::title(b);
    let mut kind = designs::weighted(
        rng,
        &[
            ("board", 3.4),
            ("lightbox", 1.6),
            ("tiles", if height >= 1.3 { 1.7 } else { 0. }),
            (
                "ladder",
                if height >= 1.1 && room.reach >= 0.86 {
                    1.4
                } else {
                    0.
                },
            ),
            ("wide", if room.reach >= 1. { 1.1 } else { 0. }),
        ],
    );
    if kind == "tiles" {
        let text = [b.trade.as_str(), &title, &b.name]
            .into_iter()
            .find(|s| (3..=5).contains(&s.encode_utf16().count()))
            .unwrap_or(&b.trade);
        let count = text.encode_utf16().count() as f64;
        let gap = 0.035;
        let size = 0.62_f64
            .min(room.reach - 0.24)
            .min((height + gap) / count - gap);
        if size >= 0.36 {
            let look = designs::draw_look(Construction::Tile, &s.weathering, rng)?;
            let standoff = rng.float(0.12, 0.2).min(room.reach - size - 0.06);
            let top = bottom + count * (size + gap) - gap;
            let designs = designs::tile_panels(text, size, 0.05, &look, rng)?;
            let upright = designs[0].outline == Outline::Rectangle;
            let panels = designs
                .into_iter()
                .enumerate()
                .map(|(i, design)| {
                    Ok(PlacedPanel {
                        pen: blade_pen(
                            site,
                            room.u,
                            standoff,
                            top - (i + 1) as f64 * size - i as f64 * gap,
                            design.depth,
                        )?,
                        design,
                        sides: Sides::Both,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            return Ok(Some(carried(
                site,
                panels,
                &look,
                b,
                bracing.carriage(room, standoff, size, top, upright),
                Extra::Tiles,
                Form::Tiles,
            )));
        }
        kind = "board";
    }
    if kind == "ladder" {
        let width = 1.05_f64.min(room.reach - 0.22);
        let rung = rng.float(0.3, 0.42);
        let gap = 0.07;
        let count = rng
            .integer(2, 4)
            .min(((height + gap) / (rung + gap)).floor().max(0.) as usize);
        if width >= 0.62 && count >= 2 {
            let standoff = rng.float(0.12, 0.2).min(room.reach - width - 0.06);
            let top = bottom + count as f64 * (rung + gap) - gap;
            let first = designs::draw_look(Construction::Sheet, &s.weathering, rng)?;
            let mut panels = Vec::new();
            for i in 0..count {
                let floor = rng.integer(1, 4);
                let tenant = copy::invent(
                    rng,
                    &Brief {
                        sectors: Some(vec![Sector::Medical, Sector::Service, Sector::Retail]),
                        floor: Some(floor),
                        trade: None,
                    },
                )?;
                let mut look = if i == 0 {
                    first.clone()
                } else {
                    designs::draw_look(Construction::Sheet, &s.weathering, rng)?
                };
                look.steel = first.steel.clone();
                let design = designs::title_panel(&tenant, [width, rung, 0.04], &look, rng)?;
                panels.push(PlacedPanel {
                    pen: blade_pen(
                        site,
                        room.u,
                        standoff,
                        top - (i + 1) as f64 * rung - i as f64 * gap,
                        design.depth,
                    )?,
                    design,
                    sides: Sides::Both,
                });
            }
            return Ok(Some(carried(
                site,
                panels,
                &first,
                b,
                bracing.carriage(room, standoff, width, top, true),
                Extra::None,
                Form::Ladder,
            )));
        }
        kind = "board";
    }
    let lit = kind == "lightbox" || (kind == "wide" && rng.chance(0.3));
    let look = designs::draw_look(
        if lit {
            Construction::Lightbox
        } else {
            Construction::Sheet
        },
        &s.weathering,
        rng,
    )?;
    let depth = if lit { rng.float(0.15, 0.2) } else { 0.045 };
    let (width, tall) = if kind == "wide" {
        (1.25_f64.min(room.reach - 0.18), rng.float(0.42, 0.6))
    } else {
        let width = rng.float(
            0.42,
            (if lit { 0.7_f64 } else { 0.85 }).min(room.reach - 0.2),
        );
        (
            width,
            (title.encode_utf16().count() as f64 * width * rng.float(0.95, 1.15) + 0.25)
                .max(1.2)
                .min(3.8_f64.min(height)),
        )
    };
    let standoff = rng.float(0.12, 0.3).min(room.reach - width - 0.06);
    let top = bottom + tall;
    let design = designs::title_panel(b, [width, tall, depth], &look, rng)?;
    let c = bracing.carriage(room, standoff, width, top, !lit);
    let trussed = kind == "wide" && c.stay > 0. && site.detail > 0;
    let panels = vec![PlacedPanel {
        pen: blade_pen(site, room.u, standoff, bottom, depth)?,
        design,
        sides: Sides::Both,
    }];
    Ok(Some(carried(
        site,
        panels,
        &look,
        b,
        c,
        Extra::Board { lit, trussed },
        if lit { Form::Lightbox } else { Form::Board },
    )))
}
pub fn hung_sign(
    site: &Site,
    room: PierRoom,
    b: &Business,
    s: &Settings,
    rng: &mut SeededRng,
) -> Result<Option<Projected>, String> {
    site.validate()?;
    room.validate()?;
    b.validate()?;
    let height = room.ceiling - room.foot;
    if height < 1.3 || room.reach < 0.8 {
        return Ok(None);
    }
    let construction = designs::weighted(
        rng,
        &[
            (Construction::Sheet, 3.),
            (Construction::Timber, 2.),
            (Construction::Lightbox, 1.),
        ],
    );
    let look = designs::draw_look(construction, &s.weathering, rng)?;
    let depth = match construction {
        Construction::Lightbox => rng.float(0.13, 0.17),
        Construction::Timber => 0.05,
        _ => 0.04,
    };
    let width = rng.float(0.5, 0.9_f64.min(room.reach - 0.26));
    let tall = rng.float(0.5, 1.1_f64.min(height - 0.7));
    let standoff = rng.float(0.14, 0.3).min(room.reach - width - 0.12);
    let bottom = room.foot + 0.08;
    let top = bottom + tall;
    let arm = top + 0.1;
    let design = designs::title_panel(b, [width, tall, depth], &look, rng)?;
    let panels = vec![PlacedPanel {
        pen: blade_pen(site, room.u, standoff, bottom, depth)?,
        design,
        sides: Sides::Both,
    }];
    Ok(Some(Projected {
        proposal: Proposal::new(
            site,
            Mount::Hung,
            panels,
            &look,
            (bottom + top) / 2.,
            b,
            super::Fixings::Projecting(Fixings {
                age: look.age,
                support: Support::Hung {
                    u: room.u,
                    standoff,
                    width,
                    top,
                    arm,
                },
            }),
        ),
        top: arm + 0.1,
    }))
}
