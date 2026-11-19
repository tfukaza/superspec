//! Owned mounting proposals: planning does not retain a mutable mesh or host callback.
use super::{
    Construction, Outline,
    copy::{self, Business},
    designs::{self, Look},
    panel::{self, Design, Hardware, Settings, Sides},
};
use crate::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::Mesh,
    pen::Pen,
    steel::{self, Plate},
};
use serde::{Deserialize, Serialize};
use superspec_types::rng::SeededRng;
mod projecting;
pub use projecting::{PierRoom, Projected, blade_sign, hung_sign};
#[derive(Clone)]
pub struct Site {
    pub pen: Pen,
    pub elevation: f64,
    pub key: String,
    pub shown: bool,
    pub detail: u8,
    pub wash: Option<superspec_geometry::V3>,
}
impl Site {
    fn validate(&self) -> Result<(), String> {
        if !self.elevation.is_finite()
            || self.elevation.abs() > 10000.
            || self.key.is_empty()
            || self.key.len() > 2048
            || self.detail > 2
            || self
                .wash
                .is_some_and(|c| c.iter().any(|v| !v.is_finite() || !(0. ..=1.).contains(v)))
        {
            return Err("Sign site requires bounded elevation and identity.".into());
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct WallRect {
    pub x0: f64,
    pub z0: f64,
    pub width: f64,
    pub height: f64,
}
impl WallRect {
    fn validate(&self) -> Result<(), String> {
        if [self.x0, self.z0]
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 1000.)
            || [self.width, self.height]
                .iter()
                .any(|v| !v.is_finite() || *v < 0.08 || *v > 100.)
        {
            return Err("Sign wall rectangle exceeds bounded meter coordinates.".into());
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FlatKind {
    Fascia,
    Long,
    Plate,
    Balcony,
}
pub use superspec_model::theme::signs::{Form, Mount};
fn form(c: Construction) -> Form {
    match c {
        Construction::Sheet | Construction::Cloth => Form::Board,
        Construction::Lightbox => Form::Lightbox,
        Construction::Timber | Construction::Enamel => Form::Plaque,
        Construction::Tile => Form::Tiles,
        Construction::Painted => Form::Painted,
    }
}
#[derive(Clone)]
pub struct PlacedPanel {
    pub pen: Pen,
    pub design: Design,
    pub sides: Sides,
}
#[derive(Clone)]
enum Fixings {
    None,
    Projecting(projecting::Fixings),
    Flat {
        rect: WallRect,
        construction: Construction,
        depth: f64,
    },
    Roof {
        left: f64,
        width: f64,
        tall: f64,
        foot: f64,
    },
}
#[derive(Clone)]
pub struct Proposal {
    pub key: String,
    pub mount: Mount,
    pub form: Form,
    pub panels: Vec<PlacedPanel>,
    pub steel: Hardware,
    pub shown: bool,
    pub above: f64,
    pub business: Option<Business>,
    /// How boldly the panels' artwork is drawn (the facades' own by default).
    pub art: panel::Art,
    /// How many times as thick a projecting sign's rods and flat bars are (1 by default).
    pub gauge: f64,
    site: Site,
    fixings: Fixings,
}
impl Proposal {
    fn new(
        site: &Site,
        mount: Mount,
        panels: Vec<PlacedPanel>,
        look: &Look,
        middle: f64,
        business: &Business,
        fixings: Fixings,
    ) -> Self {
        Self {
            key: site.key.clone(),
            mount,
            form: form(look.construction),
            panels,
            steel: look.steel.clone(),
            shown: site.shown,
            above: if mount == Mount::Roof {
                0.
            } else {
                site.elevation + middle - 1.6
            },
            business: Some(business.clone()),
            art: panel::Art::default(),
            gauge: 1.,
            site: site.clone(),
            fixings,
        }
    }
    /// Call only after any scene-wide placement survey has kept this proposal.
    pub fn draw(
        &self,
        mesh: &mut Mesh<'_>,
        repeats: &dyn Fn(&str) -> f64,
        s: &Settings,
    ) -> Result<(), String> {
        if !self.shown {
            return Ok(());
        }
        mesh.spend_work(1)?;
        if !(1. ..=4.).contains(&self.gauge) {
            return Err("A projecting sign's rods are 1–4 times their usual thickness.".into());
        }
        let mut d = Fixture {
            pen: self.site.pen.clone(),
            mesh,
            repeats,
        };
        let steel = self.steel.steel();
        match self.fixings {
            Fixings::None => {}
            Fixings::Projecting(f) => f.draw(&mut d, &self.site, &self.steel, self.gauge)?,
            Fixings::Flat {
                rect,
                construction,
                depth,
            } => {
                if construction != Construction::Enamel {
                    let head = rect.z0 + rect.height;
                    let count = (rect.width / 1.2).round().max(2.) as usize;
                    for i in 0..count {
                        let x = rect.x0 + (rect.width * (i as f64 + 0.5)) / count as f64;
                        if construction == Construction::Lightbox {
                            steel::member(
                                &mut d,
                                steel,
                                [[x, 0., rect.z0 - 0.02], [x, 0.01 + depth, rect.z0 - 0.02]],
                                [1., 0., 0.],
                                [0.04, 0.03],
                            )?;
                            continue;
                        }
                        for z in [head + 0.012, rect.z0 - 0.012] {
                            steel::fixing_plate(
                                &mut d,
                                steel,
                                &Plate {
                                    center: [x, 0., z],
                                    across: [1., 0., 0.],
                                    up: [0., 0., 1.],
                                    out: [0., 1., 0.],
                                    size: [0.045, 0.06],
                                    bolts: 2,
                                },
                            )?
                        }
                    }
                }
            }
            Fixings::Roof {
                left,
                width,
                tall,
                foot,
            } => {
                let behind = -0.12 - 0.05 - 0.03;
                let head = foot + tall + 0.05;
                let posts = (width / 1.6).ceil().max(1.) as usize + 1;
                for i in 0..posts {
                    let x = left + 0.1 + ((width - 0.2) * i as f64) / (posts - 1) as f64;
                    steel::angle(
                        &mut d,
                        steel,
                        [[x, behind, 0.02], [x, behind, head]],
                        [[1., 0., 0.], [0., -1., 0.]],
                        [0.05, 0.005],
                    )?;
                    steel::member(
                        &mut d,
                        steel.toned(0.9),
                        [
                            [x + 0.02, behind - 0.03, foot + tall * 0.75],
                            [x + 0.02, behind - 1.1, 0.03],
                        ],
                        [1., 0., 0.],
                        [0.04, 0.006],
                    )?;
                    steel::fixing_plate(
                        &mut d,
                        steel,
                        &Plate {
                            center: [x + 0.02, behind - 1.1, 0.02],
                            across: [1., 0., 0.],
                            up: [0., 1., 0.],
                            out: [0., 0., 1.],
                            size: [0.12, 0.12],
                            bolts: 4,
                        },
                    )?;
                }
                for share in [0.18, 0.82] {
                    let z = foot + tall * share;
                    steel::angle(
                        &mut d,
                        steel.toned(0.95),
                        [[left, behind + 0.03, z], [left + width, behind + 0.03, z]],
                        [[0., 0., 1.], [0., -1., 0.]],
                        [0.04, 0.005],
                    )?
                }
            }
        }
        for p in &self.panels {
            panel::draw_art(
                &mut Fixture {
                    pen: p.pen.clone(),
                    mesh: d.mesh,
                    repeats,
                },
                &p.design,
                p.sides,
                &self.steel,
                s,
                self.art,
            )?
        }
        Ok(())
    }
}
pub fn flat_sign(
    site: &Site,
    rect: WallRect,
    kind: FlatKind,
    limit: f64,
    b: &Business,
    s: &Settings,
    rng: &mut SeededRng,
) -> Result<Proposal, String> {
    site.validate()?;
    rect.validate()?;
    b.validate()?;
    if !limit.is_finite() || !(0.06..=100.).contains(&limit) {
        return Err("Flat sign requires at least six centimeters of clear projection.".into());
    }
    let roomy = limit >= 0.01 + 0.1 + 0.02;
    let c = match kind {
        FlatKind::Plate => designs::weighted(
            rng,
            &[
                (Construction::Enamel, 3.),
                (Construction::Sheet, 2.),
                (Construction::Timber, 1.5),
            ],
        ),
        FlatKind::Balcony => Construction::Sheet,
        _ => designs::weighted(
            rng,
            &[
                (Construction::Sheet, 4.),
                (
                    Construction::Timber,
                    if kind == FlatKind::Fascia { 2.5 } else { 1. },
                ),
                (
                    Construction::Lightbox,
                    if roomy {
                        if kind == FlatKind::Fascia { 2. } else { 1.5 }
                    } else {
                        0.
                    },
                ),
            ],
        ),
    };
    let look = designs::draw_look(c, &s.weathering, rng)?;
    let depth = match c {
        Construction::Sheet => 0.04,
        Construction::Timber => 0.05,
        Construction::Lightbox => 0.1,
        Construction::Enamel => 0.012,
        _ => unreachable!(),
    };
    let design = designs::title_panel(b, [rect.width, rect.height, depth], &look, rng)?;
    let mount = match kind {
        FlatKind::Fascia => Mount::Fascia,
        FlatKind::Balcony => Mount::Balcony,
        _ => Mount::Wall,
    };
    Ok(Proposal::new(
        site,
        mount,
        vec![PlacedPanel {
            pen: site.pen.moved([rect.x0, 0.01 + depth, rect.z0])?,
            design,
            sides: Sides::Front,
        }],
        &look,
        rect.z0 + rect.height / 2.,
        b,
        Fixings::Flat {
            rect,
            construction: c,
            depth,
        },
    ))
}
pub fn painted_sign(
    site: &Site,
    rect: WallRect,
    b: &Business,
    s: &Settings,
    rng: &mut SeededRng,
) -> Result<Proposal, String> {
    site.validate()?;
    rect.validate()?;
    b.validate()?;
    let look = designs::draw_look(Construction::Painted, &s.weathering, rng)?;
    let design = designs::painted_field(
        &copy::company_title(b, rng),
        [rect.width, rect.height],
        &look,
        rng,
    )?;
    Ok(Proposal::new(
        site,
        Mount::Wall,
        vec![PlacedPanel {
            pen: site.pen.moved([rect.x0, 0.004, rect.z0])?,
            design,
            sides: Sides::Front,
        }],
        &look,
        rect.z0 + rect.height / 2.,
        b,
        Fixings::None,
    ))
}
pub fn roof_sign(
    site: &Site,
    span: [f64; 2],
    base: f64,
    ceiling: f64,
    b: &Business,
    s: &Settings,
    rng: &mut SeededRng,
) -> Result<Option<Proposal>, String> {
    site.validate()?;
    b.validate()?;
    if span
        .iter()
        .chain([base, ceiling].iter())
        .any(|v| !v.is_finite() || v.abs() > 1000.)
        || span[1] < span[0]
        || span[1] - span[0] > 100.
    {
        return Err("Roof sign room exceeds bounded meter coordinates.".into());
    }
    let width = span[1] - span[0];
    let foot = base + 0.35;
    let headroom = ceiling - foot - 0.1;
    if width < 3.2 || headroom < 0.8 {
        return Ok(None);
    }
    let look = designs::draw_look(
        if rng.chance(0.6) {
            Construction::Sheet
        } else {
            Construction::Tile
        },
        &s.weathering,
        rng,
    )?;
    let title = copy::title(b);
    let count = title.encode_utf16().count() as f64;
    let gap = 0.14;
    let size = 1.3f64.min(headroom).min((width + gap) / count - gap);
    let (panels, tall, left, width) = if look.construction == Construction::Tile && size >= 0.8 {
        let width = count * (size + gap) - gap;
        let left = (span[0] + span[1] - width) / 2.;
        let designs = designs::tile_panels(&title, size, 0.05, &look, rng)?;
        let len = designs.len();
        let panels = designs
            .into_iter()
            .enumerate()
            .map(|(i, mut design)| {
                design.outline = Outline::Rectangle;
                design.border = Some(look.palette.border);
                let place = if site.pen.mirrored { len - 1 - i } else { i };
                Ok(PlacedPanel {
                    pen: site
                        .pen
                        .moved([left + place as f64 * (size + gap), -0.12, foot])?,
                    design,
                    sides: Sides::Front,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        (panels, size, left, width)
    } else {
        let board = Look {
            construction: Construction::Sheet,
            ..look.clone()
        };
        let tall = 1.5f64.min(headroom).min(width / 3.6);
        let width = width.min(tall * rng.float(3.2, 4.6));
        let left = (span[0] + span[1] - width) / 2.;
        (
            vec![PlacedPanel {
                pen: site.pen.moved([left, -0.12, foot])?,
                design: designs::title_panel(b, [width, tall, 0.05], &board, rng)?,
                sides: Sides::Front,
            }],
            tall,
            left,
            width,
        )
    };
    let form = if look.construction == Construction::Tile && panels.len() > 1 {
        Form::Tiles
    } else {
        Form::Board
    };
    let mut proposal = Proposal::new(
        site,
        Mount::Roof,
        panels,
        &look,
        0.,
        b,
        Fixings::Roof {
            left,
            width,
            tall,
            foot,
        },
    );
    proposal.form = form;
    Ok(Some(proposal))
}
