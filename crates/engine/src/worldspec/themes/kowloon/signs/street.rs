//! Street frontage planning. Clearance and nearby-shop queries are explicit read-only inputs.
use super::{
    copy::{self, Brief, Business, Sector},
    mounts::{self, FlatKind, PierRoom, Proposal, Site, WallRect},
};
use crate::worldspec::themes::kowloon::{kit::Kit, massing::Rect, pen::Pen};
use serde::Deserialize;
use superspec_geometry::V3;
use superspec_geometry::obstructions::Budget;
use superspec_types::rng::SeededRng;
#[derive(Clone, Deserialize)]
pub struct Bay {
    pub kind: String,
    pub u0: f64,
    pub u1: f64,
}
#[derive(Clone, Deserialize)]
pub struct Request {
    pub key: String,
    pub width: f64,
    pub ground: f64,
    pub roof: f64,
    pub wash: V3,
    pub body: Vec<Rect>,
    pub bays: Vec<Bay>,
    pub floors: Vec<f64>,
    pub limit: f64,
}
impl Request {
    fn validate(&self) -> Result<(), String> {
        if self.key.len() > 2000
            || self.key.is_empty()
            || self.body.len() > 256
            || self.bays.len() > 64
            || self.floors.len() > 32
            || [self.width, self.ground, self.roof, self.limit]
                .iter()
                .chain(&self.floors)
                .any(|v| !v.is_finite() || v.abs() > 1000.)
            || self.width < 0.6
            || self.ground < 0.
            || self.roof < self.ground
            || !(0. ..=20.).contains(&self.limit)
            || self
                .wash
                .iter()
                .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
            || self.body.iter().any(|r| {
                [r.u0, r.u1, r.v0, r.v1]
                    .iter()
                    .any(|v| !v.is_finite() || v.abs() > 1000.)
                    || r.u0 >= r.u1
                    || r.v0 >= r.v1
            })
            || self.bays.iter().any(|b| {
                b.kind.len() > 64
                    || !b.u0.is_finite()
                    || !b.u1.is_finite()
                    || b.u0 > b.u1
                    || (b.u0 == b.u1 && b.kind != "blank")
                    || b.u0.abs() > 1000.
                    || b.u1.abs() > 1000.
            })
        {
            return Err("Street sign planning exceeds bounded identity or facade geometry.".into());
        }
        for (i, r) in self.body.iter().enumerate() {
            for other in &self.body[..i] {
                if r.u0 < other.u1 && r.u1 > other.u0 && r.v0 < other.v1 && r.v1 > other.v0 {
                    return Err(
                        "Street facade coverage rectangles must have disjoint interiors.".into(),
                    );
                }
            }
        }
        Ok(())
    }
}
pub struct Queries<'a> {
    pub reach: &'a dyn Fn(Rect, f64) -> Result<f64, String>,
    pub shop_near: &'a dyn Fn(V3, f64) -> Result<Option<Business>, String>,
}
impl Queries<'_> {
    fn clear_depth(&self, r: Rect, limit: f64) -> Result<f64, String> {
        let depth = (self.reach)(r, limit)?;
        if !depth.is_finite() || depth < 0. || depth > limit + 1e-8 {
            return Err("Sign clearance query returned invalid depth.".into());
        }
        Ok(depth)
    }
}
fn inside(walls: &[Rect], r: Rect) -> bool {
    walls
        .iter()
        .map(|w| {
            (w.u1.min(r.u1) - w.u0.max(r.u0)).max(0.) * (w.v1.min(r.v1) - w.v0.max(r.v0)).max(0.)
        })
        .sum::<f64>()
        >= (r.u1 - r.u0) * (r.v1 - r.v0) - 1e-4
}
fn clear_wall(r: &Request, q: &Queries<'_>, rect: WallRect) -> Result<bool, String> {
    let area = Rect {
        u0: rect.x0 - 0.02,
        u1: rect.x0 + rect.width + 0.02,
        v0: rect.z0 - 0.06,
        v1: rect.z0 + rect.height + 0.06,
    };
    Ok(area.v1 <= r.roof - 0.1 && inside(&r.body, area) && q.clear_depth(area, 0.13)? > 0.)
}
fn tenant(
    q: &Queries<'_>,
    pen: &Pen,
    u: f64,
    z: f64,
    story: usize,
    rng: &mut SeededRng,
) -> Result<Business, String> {
    if story == 0
        && rng.chance(0.6)
        && let Some(shop) = (q.shop_near)(pen.world([u, 0., z]), 2.4)?
    {
        shop.validate()?;
        return Ok(shop);
    }
    let floor = (story + rng.integer(1, 2)).min(9);
    copy::invent(
        rng,
        &Brief {
            sectors: Some(vec![
                Sector::Medical,
                Sector::Medical,
                Sector::Service,
                Sector::Retail,
                Sector::Industry,
            ]),
            floor: Some(floor),
            trade: None,
        },
    )
}
fn site(pen: &Pen, kit: &Kit, key: String, shown: bool, wash: Option<V3>) -> Site {
    Site {
        pen: pen.clone(),
        elevation: 0.,
        key,
        shown,
        detail: kit.detail(),
        wash,
    }
}
struct Planned {
    proposals: Vec<Proposal>,
    bytes: usize,
}
impl Planned {
    fn push(&mut self, p: Proposal) -> Result<(), String> {
        let mut bytes = std::mem::size_of::<Proposal>()
            + p.key.len()
            + p.steel.slot.len()
            + p.steel.weathering.as_ref().map_or(0, |s| {
                s.seed.len()
                    + s.metal.material.len()
                    + s.paint.material.len()
                    + s.sheet.material.len()
            });
        for panel in &p.panels {
            bytes += serde_json::to_vec(&panel.design)
                .map_err(|e| e.to_string())?
                .len()
                * 2
                + panel.pen.part.len()
                + std::mem::size_of::<mounts::PlacedPanel>();
        }
        if let Some(b) = &p.business {
            bytes += serde_json::to_vec(b).map_err(|e| e.to_string())?.len() * 2;
        }
        self.bytes += bytes;
        if self.bytes > 4 * 1024 * 1024 || self.proposals.len() >= 1024 {
            return Err("Street sign proposals exceed their owned planning allowance.".into());
        }
        super::book::surveyed(&p, self.proposals.len())?;
        self.proposals.push(p);
        Ok(())
    }
}
pub fn street_signs(
    r: &Request,
    kit: &Kit,
    body_pen: &Pen,
    q: &Queries<'_>,
    budget: &mut Budget<'_>,
) -> Result<Vec<Proposal>, String> {
    r.validate()?;
    budget.spend(1)?;
    let pen = body_pen.into_part("signage")?;
    let make_site = |key, shown| site(&pen, kit, key, shown, Some(r.wash));
    let mut result = Planned {
        proposals: vec![],
        bytes: 0,
    };
    let mut piers = Vec::new();
    for bay in &r.bays {
        for u in [bay.u0, bay.u1] {
            if u > 0.3 && u < r.width - 0.3 && !piers.contains(&u) {
                piers.push(u);
            }
        }
    }
    piers.extend([0.22, r.width - 0.22]);
    for (p, &u) in piers.iter().enumerate() {
        budget.spend(1)?;
        if r.bays
            .iter()
            .any(|b| b.kind == "balcony" && u > b.u0 - 0.3 && u < b.u1 + 0.3)
        {
            continue;
        }
        let mut free = r.ground + 0.2;
        for (story, &floor) in r.floors.iter().take(4).enumerate() {
            budget.spend(1)?;
            let key = format!("{}/sign-{p}-{story}", r.key);
            let choice = kit.option(&key, if story == 0 { 0.48 } else { 0.16 })?;
            if !choice.planned {
                continue;
            }
            let mut rng = kit.rng(&key)?;
            let foot = free.max(floor + 0.25);
            let ceiling = (r.roof - 0.2).min(foot + rng.float(2.2, 4.6));
            if ceiling - foot < 1.3
                || !inside(
                    &r.body,
                    Rect {
                        u0: u - 0.1,
                        u1: u + 0.1,
                        v0: foot,
                        v1: ceiling,
                    },
                )
            {
                continue;
            }
            let room = PierRoom {
                u,
                foot,
                ceiling,
                reach: q.clear_depth(
                    Rect {
                        u0: u - 0.12,
                        u1: u + 0.12,
                        v0: foot,
                        v1: ceiling,
                    },
                    r.limit,
                )?,
            };
            let business = tenant(q, &pen, u, foot, story, &mut rng)?;
            let site = make_site(key, choice.shown);
            let hung = if rng.chance(0.24) {
                mounts::hung_sign(&site, room, &business, kit.settings(), &mut rng)?
            } else {
                None
            };
            let sign = match hung {
                Some(p) => Some(p),
                None => mounts::blade_sign(&site, room, &business, kit.settings(), &mut rng)?,
            };
            if let Some(sign) = sign {
                result.push(sign.proposal)?;
                free = sign.top + 0.5;
            }
        }
    }
    for (b, pair) in r.bays.windows(2).enumerate() {
        if pair[0].kind != "window" || pair[1].kind != "window" {
            continue;
        }
        for (story, &floor) in r.floors.iter().take(3).enumerate() {
            budget.spend(1)?;
            let key = format!("{}/plate-{b}-{story}", r.key);
            let choice = kit.option(&key, 0.16)?;
            if !choice.planned {
                continue;
            }
            let mut rng = kit.rng(&key)?;
            let width = rng.float(0.28, 0.44);
            let height = width * rng.float(1.5, 3.2);
            let z0 = floor + rng.float(0.5, 0.9);
            let rect = WallRect {
                x0: pair[0].u1 - width / 2.,
                z0,
                width,
                height,
            };
            if !clear_wall(r, q, rect)? {
                continue;
            }
            let business = copy::invent(
                &mut rng,
                &Brief {
                    sectors: Some(vec![Sector::Medical, Sector::Medical, Sector::Service]),
                    floor: Some((story + 1).min(9)),
                    trade: None,
                },
            )?;
            result.push(mounts::flat_sign(
                &make_site(key, choice.shown),
                rect,
                FlatKind::Plate,
                0.13,
                &business,
                kit.settings(),
                &mut rng,
            )?)?;
        }
    }
    for (story, &floor) in r.floors.iter().take(3).enumerate() {
        budget.spend(1)?;
        let key = format!("{}/long-{story}", r.key);
        let choice = kit.option(&key, 0.34)?;
        if !choice.planned {
            continue;
        }
        let mut rng = kit.rng(&key)?;
        let mut runs: Vec<[f64; 2]> = vec![];
        for bay in &r.bays {
            if bay.kind != "window" {
                continue;
            }
            if let Some(last) = runs.last_mut()
                && (last[1] - bay.u0).abs() < 1e-6
            {
                last[1] = bay.u1;
            } else {
                runs.push([bay.u0, bay.u1]);
            }
        }
        let clear: Vec<_> = runs
            .into_iter()
            .map(|run| [run[0].max(0.5), run[1].min(r.width - 0.5)])
            .filter(|run| run[1] - run[0] >= 2.4)
            .collect();
        if clear.is_empty() {
            continue;
        }
        let run = rng.pick(&clear);
        let width = (run[1] - run[0]).min(rng.float(2.2, 6.5));
        let rect = WallRect {
            x0: run[0] + rng.float(0., run[1] - run[0] - width),
            z0: floor + 0.08,
            width,
            height: rng.float(0.38, 0.52),
        };
        if !clear_wall(r, q, rect)? {
            continue;
        }
        let business = tenant(q, &pen, rect.x0 + width / 2., rect.z0, story, &mut rng)?;
        result.push(mounts::flat_sign(
            &make_site(key, choice.shown),
            rect,
            FlatKind::Long,
            0.13,
            &business,
            kit.settings(),
            &mut rng,
        )?)?;
    }
    Ok(result.proposals)
}
pub fn wall_advertisement(
    r: &Request,
    kit: &Kit,
    body_pen: &Pen,
    q: &Queries<'_>,
    budget: &mut Budget<'_>,
) -> Result<Option<Proposal>, String> {
    r.validate()?;
    budget.spend(1)?;
    let key = format!("{}/painted", r.key);
    let choice = kit.option(&key, 0.6)?;
    if !choice.planned {
        return Ok(None);
    }
    let area = |r: &Rect| (r.u1 - r.u0) * (r.v1 - r.v0);
    let largest = r
        .body
        .iter()
        .reduce(|a, b| if area(b) > area(a) { b } else { a });
    let Some(largest) = largest else {
        return Ok(None);
    };
    if largest.u1 - largest.u0 < 2.4 || largest.v1 - largest.v0 < 2.4 {
        return Ok(None);
    }
    let mut rng = kit.rng(&key)?;
    let room = [largest.u1 - largest.u0 - 0.8, largest.v1 - largest.v0 - 0.8];
    let upright = room[1] > room[0] * 1.2;
    let width = room[0].min(if upright {
        rng.float(1.1, 1.8)
    } else {
        rng.float(3.2, 6.)
    });
    let height = room[1].min(if upright {
        width * rng.float(3.2, 4.4)
    } else {
        width / rng.float(3., 4.2)
    });
    let rect = WallRect {
        x0: largest.u0 + 0.4 + rng.float(0., room[0] - width),
        z0: largest.v1 - 0.4 - height - rng.float(0., 1_f64.min(room[1] - height)),
        width,
        height,
    };
    if q.clear_depth(
        Rect {
            u0: rect.x0,
            u1: rect.x0 + width,
            v0: rect.z0,
            v1: rect.z0 + height,
        },
        0.15,
    )? == 0.
    {
        return Ok(None);
    }
    let business = copy::invent(
        &mut rng,
        &Brief {
            sectors: Some(vec![
                Sector::Food,
                Sector::Retail,
                Sector::Industry,
                Sector::Service,
            ]),
            ..Default::default()
        },
    )?;
    Ok(Some(mounts::painted_sign(
        &site(
            &body_pen.into_part("signage")?,
            kit,
            key,
            choice.shown,
            None,
        ),
        rect,
        &business,
        kit.settings(),
        &mut rng,
    )?))
}
