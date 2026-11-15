//! One upper-story household bay, assembled from independently seeded fixture choices.
pub(super) mod balcony;
use super::{
    aircon,
    fixture::Fixture,
    household,
    kit::Kit,
    palette::{Flat, LotStyle, shade, srgb},
    pen::Pen,
    signs::mounts::Proposal,
    stains, windows,
};
use serde::{Deserialize, Serialize};
use superspec_geometry::V3;
use superspec_types::rng::SeededRng;
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Window,
    Band,
    Balcony,
    Stair,
    Service,
    Blank,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Front,
    Back,
    Side,
    Party,
    Well,
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Opening {
    pub x: f64,
    pub z: f64,
    pub w: f64,
    pub h: f64,
    pub depth: f64,
}
pub struct Cell<'a> {
    pub kind: Kind,
    pub role: Role,
    pub story: usize,
    pub width: f64,
    pub height: f64,
    pub reach: f64,
    pub balcony: f64,
    pub ground: f64,
    pub story_height: f64,
    pub top: bool,
    pub lot_key: &'a str,
    pub key: &'a str,
    pub wash: V3,
    pub style: &'a LotStyle,
    pub flat: &'a Flat,
    pub kit: &'a Kit,
    pub plants: bool,
    pub cloth: bool,
}
impl Cell<'_> {
    fn rng(&self, suffix: &str) -> Result<SeededRng, String> {
        self.kit.rng(&format!("{}{}", self.key, suffix))
    }
    fn validate(&self) -> Result<(), String> {
        if [
            self.width,
            self.height,
            self.reach,
            self.balcony,
            self.ground,
            self.story_height,
        ]
        .iter()
        .any(|v| !v.is_finite())
            || !(0.6..=40.).contains(&self.width)
            || !(2. ..=50.).contains(&self.height)
            || !(0. ..=2.).contains(&self.reach)
            || !(0.6..=1.5).contains(&self.balcony)
            || !(0. ..=50.).contains(&self.ground)
            || !(0. ..=50.).contains(&self.story_height)
            || self.story > 32
            || self.key.is_empty()
            || self.key.len() > 1024
            || self.lot_key.is_empty()
            || self.lot_key.len() > 1024
            || !self.flat.clutter.is_finite()
            || !(0. ..=1.).contains(&self.flat.clutter)
            || [
                self.wash,
                self.style.wall,
                self.style.trim,
                self.style.frame,
                self.style.accent,
                self.style.cage,
                self.flat.curtain,
            ]
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || !(0. ..=4.).contains(v))
        {
            return Err(
                "Facade cell requires bounded dimensions, colors and household identity.".into(),
            );
        }
        if self.kind == Kind::Balcony && (self.width < 1.8 || self.reach < 0.6) {
            return Err("Balcony cell requires room for its door and projecting slab.".into());
        }
        window_kind(self.style.window)?;
        guard(self.style.bars)?;
        if !["solid", "half", "rail"].contains(&self.style.balustrade)
            || !["open", "caged", "glazed"].contains(&self.flat.enclosure)
            || self.wash.iter().any(|v| *v > 1.)
        {
            return Err(
                "Facade cell requires known balcony construction and normalized wall wash.".into(),
            );
        }
        Ok(())
    }
    fn stair(&self) -> Result<&'static str, String> {
        Ok(self
            .kit
            .rng(&format!("{}/stair", self.lot_key))?
            .pick(&["open", "grid", "strip"]))
    }
    fn look(&self, kind: windows::Kind, rng: &mut SeededRng) -> Result<windows::Look, String> {
        Ok(windows::Look {
            weathering: Some(self.kit.settings().weathering.clone()),
            kind,
            frame: if self.flat.replaced {
                srgb("#b4b8ba")
            } else {
                self.style.frame
            },
            trim: self.style.trim,
            curtain: self.flat.curtain,
            reach: self.reach,
            hood: self.style.hoods,
            lit: rng.chance(0.55),
            boarded: self.flat.vacant,
            fixed: false,
            recess: self.opening()?.map_or(0., |o| o.depth),
            transom_ratio: None,
            lower_pane_bars: None,
            timber_textured: self.kit.settings().timber.textured,
        })
    }
    pub fn opening(&self) -> Result<Option<Opening>, String> {
        self.validate()?;
        let w = self.width;
        let h = self.height;
        Ok(match self.kind {
            Kind::Window => {
                let small = self.role != Role::Front;
                let ww = (w - 0.7).min(if small {
                    0.9
                } else if self.style.window == "timber" {
                    1.3
                } else {
                    1.7
                });
                let z = if small { 1.2 } else { 0.95 };
                let wh = (if small { 0.9_f64 } else { 1.3 }).min(h - z - 0.45);
                if ww < 0.6 || wh < 0.5 {
                    None
                } else {
                    Some(Opening {
                        x: (w - ww) / 2.,
                        z,
                        w: ww,
                        h: wh,
                        depth: 0.42,
                    })
                }
            }
            Kind::Band => {
                let wh = 1.3_f64.min(h - 0.95 - 0.42);
                if w - 0.32 < 0.9 || wh < 0.5 {
                    None
                } else {
                    Some(Opening {
                        x: 0.16,
                        z: 0.95,
                        w: w - 0.32,
                        h: wh,
                        depth: 0.42,
                    })
                }
            }
            Kind::Stair => {
                let variant = self.stair()?;
                if variant == "strip" {
                    if h < 2.2 {
                        None
                    } else {
                        Some(Opening {
                            x: w / 2. - 0.32,
                            z: 0.55,
                            w: 0.64,
                            h: h - 1.05,
                            depth: 0.42,
                        })
                    }
                } else {
                    let z = if variant == "open" { 1.1 } else { 0.45 };
                    if h - 0.32 - z < 0.6 {
                        None
                    } else {
                        Some(Opening {
                            x: 0.28,
                            z,
                            w: w - 0.56,
                            h: h - 0.32 - z,
                            depth: 0.4,
                        })
                    }
                }
            }
            Kind::Service => {
                if w >= 0.85 && h > 2.4 {
                    Some(Opening {
                        x: (w - 0.5) / 2.,
                        z: 1.45,
                        w: 0.5,
                        h: 0.6,
                        depth: 0.1,
                    })
                } else {
                    None
                }
            }
            _ => None,
        })
    }
}
fn window_kind(kind: &str) -> Result<windows::Kind, String> {
    match kind {
        "casement" => Ok(windows::Kind::Casement),
        "slider" => Ok(windows::Kind::Slider),
        "timber" => Ok(windows::Kind::Timber),
        "louver" => Ok(windows::Kind::Louver),
        _ => Err("Unknown household window kind.".into()),
    }
}
fn guard(kind: &str) -> Result<windows::Guard, String> {
    match kind {
        "bars" => Ok(windows::Guard::Bars),
        "grid" => Ok(windows::Guard::Grid),
        "diamond" => Ok(windows::Guard::Diamond),
        _ => Err("Unknown household security bars.".into()),
    }
}
fn on<'a, 'c>(d: &'a mut Fixture<'_, 'c>, pen: Pen) -> Fixture<'a, 'c> {
    Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    }
}
fn runoff(
    d: &mut Fixture<'_, '_>,
    b: aircon::Bounds,
    c: &Cell<'_>,
    wash: V3,
) -> Result<(), String> {
    stains::stain(
        d.mesh,
        &d.pen,
        [b.x0, (b.z - 1.1).max(0.02), b.x1, b.z - 0.01],
        wash,
        c.kit.settings().weathering.grime * 0.85,
    )
}
fn window(d: &mut Fixture<'_, '_>, c: &Cell<'_>, o: Opening) -> Result<(), String> {
    let Opening {
        x,
        z: sill,
        w: ww,
        h: wh,
        ..
    } = o;
    let kit = c.kit;
    let mut rng = c.rng("")?;
    let busy = c.flat.clutter;
    let bars = kit.option(&format!("{}/bars", c.key), 0.3)?;
    let caging = kit.option(&format!("{}/cage", c.key), 0.34 * busy)?;
    let kind = if c.flat.replaced {
        windows::Kind::Slider
    } else {
        window_kind(c.style.window)?
    };
    let mut look = c.look(kind, &mut rng)?;
    look.fixed = bars.planned || caging.planned;
    let opening_pen = d.pen.into_part("openings")?;
    windows::window_sized(&mut on(d, opening_pen), [x, sill, ww, wh], &look, &mut rng)?;
    if c.flat.vacant {
        return Ok(());
    }
    let fixture_pen = d.pen.into_part("fixtures")?;
    let d = &mut on(d, fixture_pen);
    let weather = Some(&kit.settings().weathering);
    if c.reach >= 0.5 && caging.planned {
        if caging.shown {
            let mut rng = c.rng("/cage")?;
            let depth = (c.reach - 0.05).min(rng.float(0.4, 0.6));
            household::cage(
                d,
                &household::Cage {
                    span: [x - 0.1, x + ww + 0.1],
                    heights: [sill - 0.3, sill + wh + 0.08],
                    depth,
                    color: c.style.cage,
                    stocked: kit.detail() > 0,
                },
                &mut rng,
                weather,
                c.cloth,
            )?;
        }
        return Ok(());
    }
    if bars.shown {
        windows::security_bars_sized(
            d,
            [x + 0.02, sill + 0.02, ww - 0.04, wh - 0.04],
            guard(c.style.bars)?,
            shade(c.style.frame, 0.8),
            weather,
        )?;
    }
    let mut ac = c.rng("/ac")?;
    let above = c.height - sill - wh >= 0.62 && ac.chance(0.5);
    let cooling = kit.option(&format!("{}/ac", c.key), 0.7 * busy)?;
    let cooled = c.reach >= 0.45 && cooling.planned;
    if cooled && cooling.shown {
        let bounds = if above {
            let x = x + ww * ac.float(0.3, 0.7);
            aircon::air_conditioner(d, x, sill + wh + 0.14, 0.1, &mut ac, weather)?
        } else {
            let center = x + if ac.chance(0.5) { 0.38 } else { ww - 0.38 };
            let pen = d.pen.moved([0., -0.1, 0.])?;
            aircon::air_conditioner(
                &mut on(d, pen),
                center,
                sill + 0.03,
                sill - 0.1,
                &mut ac,
                weather,
            )?
        };
        runoff(d, bounds, c, c.wash)?;
    }
    let racking = kit.option(&format!("{}/rack", c.key), 0.34 * busy)?;
    if c.reach >= 0.65 && racking.planned {
        if racking.shown {
            let mut rng = c.rng("/rack")?;
            let depth = (c.reach - 0.05).min(rng.float(0.5, 0.75));
            household::drying_rack(
                d,
                [x - 0.05, x + ww + 0.05],
                sill - 0.16,
                depth,
                &mut rng,
                weather,
                c.cloth,
            )?;
        }
    } else if c.reach >= 1. && kit.has(&format!("{}/pole", c.key), 0.25 * busy)? {
        let mut rng = c.rng("/pole")?;
        let x = x + ww * rng.float(0.2, 0.8);
        let depth = (c.reach - 0.05).min(rng.float(0.9, 1.3));
        household::drying_pole(d, x, sill - 0.12, depth, &mut rng, weather, c.cloth)?;
    }
    if kit.detail() > 0 && c.reach >= 0.6 && kit.has(&format!("{}/pots", c.key), 0.3 * busy)? {
        d.cuboid(
            "wall",
            [x, 0., sill - 0.02],
            [x + ww, 0.26, sill + 0.005],
            c.style.trim,
            "b",
        )?;
        household::plants(
            d,
            [x, x + ww],
            0.17,
            sill + 0.005,
            &mut c.rng("/pots")?,
            c.plants,
        )?;
    }
    if !(cooled && above) && c.reach >= 0.6 && kit.has(&format!("{}/awning", c.key), 0.22 * busy)? {
        household::awning(
            d,
            super::tarps::Envelope {
                x0: x - 0.12,
                x1: x + ww + 0.12,
                z: sill + wh + 0.32,
                depth: (c.reach - 0.05).min(0.7),
                drop: 0.28,
                valance: 0.,
            },
            &c.style.awning,
            &format!("{}/awning", c.key),
            weather,
        )?;
    }
    Ok(())
}
fn stair(d: &mut Fixture<'_, '_>, c: &Cell<'_>, o: Opening) -> Result<(), String> {
    let pen = d.pen.into_part("openings")?;
    let d = &mut on(d, pen);
    let variant = c.stair()?;
    let Opening { x, z, w, h, depth } = o;
    let trim = c.style.trim;
    if variant == "strip" {
        let mut look = c.look(windows::Kind::Casement, &mut c.rng("")?)?;
        look.hood = false;
        look.boarded = false;
        return windows::window_sized(d, [x, z, w, h], &look, &mut c.rng("")?);
    }
    d.cuboid(
        "wall",
        [x - 0.06, -depth, z - 0.09],
        [x + w + 0.06, 0.07, z],
        trim,
        "b",
    )?;
    d.cuboid(
        "wall",
        [x - 0.06, 0., z + h],
        [x + w + 0.06, 0.06, z + h + 0.08],
        trim,
        "b",
    )?;
    if variant == "open" {
        d.cuboid(
            "wall",
            [x, -depth + 0.02, z],
            [x + w, -depth + 0.1, z + 0.35],
            shade(trim, 0.6),
            "b",
        )?;
        return d.bar(
            "paint",
            [x, -0.05, z + 0.5],
            [x + w, -0.05, z + 0.5],
            0.03,
            shade(c.style.frame, 0.7),
        );
    }
    let color = shade(trim, 0.92);
    let columns = (w / 0.2).round().max(2.) as usize;
    let rows = (h / 0.2).round().max(2.) as usize;
    for i in 1..columns {
        let u = x + w * i as f64 / columns as f64;
        d.cuboid(
            "wall",
            [u - 0.025, -0.1, z],
            [u + 0.025, 0., z + h],
            color,
            "bdu",
        )?;
    }
    for j in 1..rows {
        let v = z + h * j as f64 / rows as f64;
        d.cuboid(
            "wall",
            [x, -0.1, v - 0.025],
            [x + w, -0.005, v + 0.025],
            color,
            "blr",
        )?;
    }
    Ok(())
}
fn band(d: &mut Fixture<'_, '_>, c: &Cell<'_>, o: Opening) -> Result<(), String> {
    use super::{facade_material, massing::Rect, metal::Binding, skin, wall};
    let kit = c.kit;
    let weather = Some(&kit.settings().weathering);
    let mut rng = c.rng("")?;
    let x0 = o.x;
    let x1 = o.x + o.w;
    let sill = o.z;
    let wh = o.h;
    let accent = c.style.accent;
    let pen = d.pen.into_part("openings")?;
    {
        let d = &mut on(d, pen);
        d.cuboid(
            "wall",
            [x0, 0., 0.06],
            [x1, 0.035, sill - 0.09],
            accent,
            "bf",
        )?;
        let seed = c.rng("/spandrel")?.integer(1, 2147483647) as u32;
        let pen = d.pen.moved([0., 0.035, 0.])?;
        let part = pen.part.clone();
        let wall_repeat = (d.repeats)("wall");
        let concrete_repeat = (d.repeats)("concrete");
        skin::draw(
            &mut on(d, pen),
            "wall",
            &skin::Settings {
                rects: vec![Rect {
                    u0: x0,
                    u1: x1,
                    v0: 0.06,
                    v1: sill - 0.09,
                }],
                openings: vec![],
                u_marks: vec![],
                z_marks: vec![],
                finish: facade_material::Settings {
                    concrete: false,
                    coat: facade_material::Coat {
                        material: "wall".into(),
                        repeat: wall_repeat,
                        tint: accent,
                    },
                    backing: Binding {
                        material: "concrete".into(),
                        repeat: concrete_repeat,
                    },
                    wear: kit.settings().weathering.wear,
                    density: kit.settings().weathering.texture_density,
                    variant: seed % 2,
                },
                condition: wall::Settings {
                    base: None,
                    ledges: vec![wall::Ledge {
                        u0: x0,
                        u1: x1,
                        z: sill - 0.09,
                        flow: 0.6,
                    }],
                    openings: vec![],
                    grime: kit.settings().weathering.grime,
                    wear: kit.settings().weathering.wear,
                    seed: f64::from(seed),
                },
                washes: vec![],
                tiles: None,
            },
            &part,
        )?;
    }
    let kind = if c.style.window == "timber" {
        windows::Kind::Casement
    } else {
        window_kind(c.style.window)?
    };
    let caging = kit.option(&format!("{}/cage", c.key), 0.22 * c.flat.clutter)?;
    let mut look = c.look(kind, &mut rng)?;
    look.hood = false;
    look.fixed = caging.planned;
    let pen = d.pen.into_part("openings")?;
    {
        let d = &mut on(d, pen);
        windows::window_sized(d, [x0, sill, x1 - x0, wh], &look, &mut rng)?;
        if c.style.hoods && c.reach >= 0.35 {
            d.cuboid(
                "wall",
                [x0 - 0.06, 0., sill + wh + 0.06],
                [x1 + 0.06, 0.3, sill + wh + 0.13],
                c.style.trim,
                "b",
            )?;
        }
    }
    if c.flat.vacant {
        return Ok(());
    }
    let pen = d.pen.into_part("fixtures")?;
    let d = &mut on(d, pen);
    let caged = c.reach >= 0.5 && x1 - x0 > 1.6 && caging.planned;
    let mut cage_rng = c.rng("/cage")?;
    let start = if caged {
        x0 + cage_rng.float(0., x1 - x0 - 1.3)
    } else {
        x1
    };
    let beside = if start - x0 >= x1 - start - 1.3 {
        [x0, start]
    } else {
        [start + 1.3, x1]
    };
    let [free0, free1] = if caged { beside } else { [x0, x1] };
    let cooling = kit.option(&format!("{}/ac", c.key), 0.65 * c.flat.clutter)?;
    let cooled = c.reach >= 0.49
        && free1 - free0 >= aircon::CONDENSER_ROOM
        && sill - 0.4 >= aircon::CONDENSER_HEIGHT
        && cooling.planned;
    if cooled && cooling.shown {
        let mut rng = c.rng("/ac")?;
        let pen = d.pen.moved([0., 0.035, 0.])?;
        let d = &mut on(d, pen);
        let bounds = if rng.chance(0.4) {
            aircon::condenser(d, [free0, free1, 0.38, sill - 0.4], &mut rng, weather)?
        } else {
            let center = free0 + 0.44 + (free1 - free0 - 0.88) * rng.next_f64();
            aircon::air_conditioner(d, center, 0.36, 0.3, &mut rng, weather)?
        };
        runoff(d, bounds, c, accent)?;
    }
    if caged {
        if caging.shown {
            household::cage(
                d,
                &household::Cage {
                    span: [start, start + 1.3],
                    heights: [sill - 0.3, sill + wh + 0.08],
                    depth: (c.reach - 0.05).min(0.5),
                    color: c.style.cage,
                    stocked: kit.detail() > 0,
                },
                &mut cage_rng,
                weather,
                c.cloth,
            )?;
        }
    } else if !cooled
        && c.reach >= 0.65
        && kit.has(&format!("{}/rack", c.key), 0.3 * c.flat.clutter)?
    {
        household::drying_rack(
            d,
            [x0 + 0.1, x1 - 0.1],
            sill - 0.16,
            (c.reach - 0.05).min(0.7),
            &mut c.rng("/rack")?,
            weather,
            c.cloth,
        )?;
    }
    Ok(())
}
fn service(d: &mut Fixture<'_, '_>, c: &Cell<'_>, o: Option<Opening>) -> Result<(), String> {
    if let Some(o) = o {
        let mut rng = c.rng("")?;
        let mut look = c.look(windows::Kind::Louver, &mut rng)?;
        look.hood = false;
        look.boarded = false;
        let pen = d.pen.into_part("openings")?;
        windows::window_sized(&mut on(d, pen), [o.x, o.z, o.w, o.h], &look, &mut rng)?;
    }
    let end = if c.role == Role::Well {
        c.width - 0.24
    } else {
        c.width
    };
    let height = o.map_or(c.height - 0.3, |o| o.z) - 0.72 - 0.06;
    if c.reach >= 0.5
        && end - 0.22 >= aircon::CONDENSER_ROOM
        && height >= aircon::CONDENSER_HEIGHT
        && c.kit.has(&format!("{}/condenser", c.key), 0.8)?
    {
        let pen = d.pen.into_part("fixtures")?;
        let d = &mut on(d, pen);
        let bounds = aircon::condenser(
            d,
            [0.22, end, 0.72, height],
            &mut c.rng("/condenser")?,
            Some(&c.kit.settings().weathering),
        )?;
        runoff(d, bounds, c, c.wash)?;
    }
    Ok(())
}
/// Returns deferred signs; the caller owns wall skin, clearance and scene-wide sign selection.
pub fn draw(d: &mut Fixture<'_, '_>, c: &Cell<'_>) -> Result<Vec<Proposal>, String> {
    c.validate()?;
    d.mesh.spend_work(1)?;
    let opening = c.opening()?;
    match c.kind {
        Kind::Window => {
            if let Some(o) = opening {
                window(d, c, o)?
            }
        }
        Kind::Stair => {
            if let Some(o) = opening {
                stair(d, c, o)?
            }
        }
        Kind::Service => service(d, c, opening)?,
        Kind::Blank => (),
        Kind::Band => {
            if let Some(o) = opening {
                band(d, c, o)?
            }
        }
        Kind::Balcony => return balcony::draw(d, c),
    }
    Ok(vec![])
}
