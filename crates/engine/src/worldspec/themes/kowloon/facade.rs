//! Complete facade composition; the host owns immutable clearance and collision inputs.
pub(super) mod services;
use super::{
    doors,
    facade_cells::{self, Cell, Role},
    facade_details,
    facade_envelope::{self, Finish},
    facade_plan::{self, Plan, Request, Zone, inside},
    fixture::Fixture,
    ground, household,
    kit::Kit,
    massing::Rect,
    metal::Binding,
    palette::{mix, shade, srgb},
    pen::Pen,
    shop,
    signs::{copy::Business, mounts::Proposal, street},
    tarps, windows,
};
use superspec_geometry::V3;
use superspec_geometry::obstructions::Budget;
use superspec_types::rng::SeededRng;
pub struct Settings<'a> {
    pub kit: &'a Kit,
    pub finish: &'a Finish,
    pub concrete: Binding,
    pub plants: bool,
    pub cloth: bool,
    pub posters: bool,
    pub shops: Option<shop::shell::Assets>,
}
pub type ShopLookup<'a> = dyn Fn(V3, f64, &[Proposal]) -> Result<Option<Business>, String> + 'a;
pub struct Queries<'a> {
    pub geometry: facade_plan::Queries<'a>,
    pub miters: &'a dyn Fn(Zone, Rect) -> Result<[Option<f64>; 2], String>,
    /// Includes this facade's earlier proposals before choosing street-sign copy.
    pub shop_near: &'a ShopLookup<'a>,
    pub frontage: Option<shop::frontage::Available<'a>>,
}
fn on<'a, 'c>(d: &'a mut Fixture<'_, 'c>, pen: Pen) -> Fixture<'a, 'c> {
    Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    }
}
fn alley(d: &mut Fixture<'_, '_>, r: &Request<'_>, rng: &mut SeededRng) -> Result<(), String> {
    let pen = d.pen.into_part("openings")?;
    let d = &mut on(d, pen);
    let x0 = 0.12;
    let x1 = r.width - 0.12;
    let head = 2.3_f64.min(r.lot.ground - 0.4);
    d.panel("paint", [x0, x1, 0., head], 0.004, srgb("#2a2927"))?;
    d.cuboid(
        "paint",
        [x0 - 0.05, 0., head],
        [x1 + 0.05, 0.08, head + 0.1],
        srgb("#b4b0a4"),
        "b",
    )?;
    let pen = d.pen.moved([0., -0.05, 0.])?;
    let width = (x1 - x0) * rng.pick(&[1., 0.5]);
    windows::security_bars_sized(
        &mut on(d, pen),
        [x0, 0.03, width, head - 0.06],
        windows::Guard::Diamond,
        mix(shade(srgb("#a4a9aa"), 0.6), srgb("#7a4a2a"), 0.5),
        None,
    )
}
fn ground(
    d: &mut Fixture<'_, '_>,
    r: &Request<'_>,
    p: &Plan,
    s: &Settings<'_>,
    q: &facade_plan::Queries<'_>,
    rng: &mut SeededRng,
    frontage: Option<shop::frontage::Available<'_>>,
) -> Result<Vec<Proposal>, String> {
    if r.role == Role::Well {
        let rect = Rect {
            u0: 0.,
            u1: r.width,
            v0: 0.,
            v1: r.lot.ground,
        };
        if inside(r.ground, rect) && q.reach(Zone::Ground, rect, 0.15)? > 0. {
            alley(d, r, rng)?;
        }
        return Ok(vec![]);
    }
    let mut signs = vec![];
    let l = r.lot;
    let kit = s.kit;
    let style = ground::Style {
        frame: l.style.frame,
        trim: l.style.trim,
        shutter: l.style.shutter,
        accent: l.style.accent,
    };
    let settings = ground::Settings {
        kit,
        style: &style,
        posters: s.posters,
    };
    for bay in &p.bays {
        let key = format!("{}/unit-{}", r.key, bay.index);
        if let Some(room) = &bay.room {
            signs.extend(shop::shell::draw(
                d,
                &shop::shell::Request {
                    plan: room,
                    lot: l,
                    coat: &s.finish.coat.material,
                    kit,
                    assets: s
                        .shops
                        .ok_or("Open shops require explicit material asset bindings.")?,
                    trade: None,
                    shutter_color: None,
                    frontage,
                },
            )?);
        } else {
            let pen = d.pen.into_part("openings")?;
            signs.extend(ground::draw(
                &mut on(d, pen),
                bay.kind,
                &ground::Unit {
                    key: key.clone(),
                    u0: bay.u0,
                    u1: bay.u1,
                    height: l.ground,
                },
                &settings,
                &mut kit.rng(&key)?,
            )?);
        }
        if !matches!(
            bay.kind,
            ground::Kind::Shutter | ground::Kind::Shop | ground::Kind::Gate
        ) || r.shift > 0.
            || l.ground < 3.2
            || (l.style.canopy && r.shift == 0.)
        {
            continue;
        }
        let key = format!("{key}/awning");
        if !kit.has(&key, 0.45)? {
            continue;
        }
        let reach = q.reach(
            Zone::Ground,
            Rect {
                u0: bay.u0,
                u1: bay.u1,
                v0: 2.52,
                v1: l.ground,
            },
            1.1,
        )?;
        if reach >= 0.7 {
            let pen = d.pen.into_part("fixtures")?;
            household::awning(
                &mut on(d, pen),
                tarps::Envelope {
                    x0: bay.u0 + 0.15,
                    x1: bay.u1 - 0.15,
                    z: l.ground - 0.12,
                    depth: reach - 0.05,
                    drop: l.ground - 0.12 - 2.72,
                    valance: 0.,
                },
                &l.style.awning,
                &key,
                Some(&kit.settings().weathering),
            )?;
        }
    }
    Ok(signs)
}
/// Produces owned visual geometry, stocked shops and deferred signs from immutable site queries.
pub fn draw(
    d: &mut Fixture<'_, '_>,
    r: &Request<'_>,
    s: &Settings<'_>,
    q: &Queries<'_>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<Proposal>, String> {
    let kit = s.kit;
    let p = facade_plan::plan(r, kit, &q.geometry, cancelled)?;
    if s.shops.is_none() && p.bays.iter().any(|b| b.room.is_some()) {
        return Err("Open shops require explicit material asset bindings.".into());
    }
    d.mesh.spend_work(1)?;
    facade_envelope::draw(d, r, &p, s.finish)?;
    let body = d.pen.moved([0., r.shift, 0.])?;
    let mut signs = vec![];
    let l = r.lot;
    for c in &p.cells {
        let cell = Cell {
            kind: c.kind,
            role: r.role,
            story: c.story,
            width: c.width,
            height: c.height,
            reach: c.reach,
            balcony: p.balcony,
            ground: l.ground,
            story_height: l.story_height,
            top: c.top,
            lot_key: &l.key,
            key: &c.key,
            wash: p.base,
            style: &l.style,
            flat: &c.flat,
            kit,
            plants: s.plants,
            cloth: s.cloth,
        };
        signs.extend(facade_cells::draw(
            &mut on(d, body.moved([c.u, 0., c.z])?),
            &cell,
        )?);
    }
    let details = facade_details::Settings {
        weathering: kit.settings().weathering.clone(),
        concrete: s.concrete.clone(),
        detail: kit.detail() as usize,
        trim: l.style.trim,
    };
    facade_details::draw(
        &mut on(d, body.into_part("fixtures")?),
        &p.construction,
        &details,
        r.key,
        &|r| (q.miters)(Zone::Body, r),
    )?;
    let mut thresholds = vec![];
    for b in &p.bays {
        if matches!(b.kind, ground::Kind::Service) {
            continue;
        }
        if let Some(room) = &b.room {
            thresholds.push([room.x0 - 0.07, room.x1 + 0.07]);
        } else if matches!(b.kind, ground::Kind::Door) {
            let key = format!("{}/unit-{}", r.key, b.index);
            let bounds = doors::entrance_bounds(
                &doors::Unit {
                    key: key.clone(),
                    u0: b.u0,
                    u1: b.u1,
                    height: l.ground,
                },
                &mut kit.rng(&key)?,
            )?;
            thresholds.push([bounds.x0 - 0.11, bounds.x1 + 0.11]);
        } else {
            thresholds.push([b.u0 + 0.15, b.u1 - 0.15]);
        }
    }
    if r.role != Role::Well {
        facade_details::plinth(
            d,
            r.ground,
            &thresholds,
            l.ground,
            &details,
            r.key,
            &|r, limit| q.geometry.reach(Zone::Ground, r, limit),
            &|r| (q.miters)(Zone::Ground, r),
        )?;
    }
    let mut rng = p.continuation.clone();
    signs.extend(ground(d, r, &p, s, &q.geometry, &mut rng, q.frontage)?);
    if matches!(r.role, Role::Front | Role::Back | Role::Side) {
        services::draw(d, r, &p, s, &q.geometry, &mut rng)?;
    }
    if l.kind == "tenement" {
        let request = street::Request {
            key: r.key.into(),
            width: r.width,
            ground: l.ground,
            roof: l.roof,
            wash: p.base,
            body: r.body.to_vec(),
            bays: p
                .columns
                .iter()
                .map(|c| street::Bay {
                    kind: match c.kind {
                        facade_cells::Kind::Window => "window",
                        facade_cells::Kind::Band => "band",
                        facade_cells::Kind::Balcony => "balcony",
                        facade_cells::Kind::Stair => "stair",
                        facade_cells::Kind::Service => "service",
                        facade_cells::Kind::Blank => "blank",
                    }
                    .into(),
                    u0: c.u0,
                    u1: c.u1,
                })
                .collect(),
            floors: p.floors.clone(),
            limit: p.limit,
        };
        let lookup = |at, radius| (q.shop_near)(at, radius, &signs);
        let queries = street::Queries {
            reach: &|rect, limit| q.geometry.reach(Zone::Body, rect, limit),
            shop_near: &lookup,
        };
        let mut budget = Budget::new(4 * 1024 * 1024, cancelled)?;
        let proposed = if r.role == Role::Front {
            street::street_signs(&request, kit, &body, &queries, &mut budget)?
        } else if matches!(r.role, Role::Party | Role::Side) {
            street::wall_advertisement(&request, kit, &body, &queries, &mut budget)?
                .into_iter()
                .collect()
        } else {
            vec![]
        };
        signs.extend(proposed);
    }
    Ok(signs)
}
