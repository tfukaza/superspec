//! Façade choices and exposure are planned without emitting mesh or mutating scene queries.
use super::{
    facade_cells::{self, Cell, Kind, Role},
    facade_details, ground,
    kit::Kit,
    massing::{Lot, Rect},
    palette::{self, Flat, shade, srgb},
    wall,
};
use serde::Serialize;
use superspec_geometry::V3;
use superspec_geometry::obstructions::Budget;
use superspec_types::rng::SeededRng;
pub struct Request<'a> {
    pub lot: &'a Lot,
    pub role: Role,
    pub key: &'a str,
    pub width: f64,
    pub shift: f64,
    pub body_span: [f64; 2],
    pub ground: &'a [Rect],
    pub body: &'a [Rect],
    pub shops: f64,
}
#[derive(Clone, Copy, Serialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Zone {
    Ground,
    Body,
}
pub struct Queries<'a> {
    pub reach: &'a dyn Fn(Zone, Rect, f64) -> Result<f64, String>,
    pub hollow: &'a dyn Fn(f64, f64, f64, f64) -> Result<f64, String>,
}
impl Queries<'_> {
    pub(super) fn reach(&self, zone: Zone, r: Rect, limit: f64) -> Result<f64, String> {
        checked_depth((self.reach)(zone, r, limit)?, limit)
    }
}
fn checked_depth(value: f64, limit: f64) -> Result<f64, String> {
    if !value.is_finite() || value < 0. || value > limit + 1e-8 {
        Err("Facade query returned invalid clearance.".into())
    } else {
        Ok(value)
    }
}
#[derive(Serialize)]
pub struct Column {
    pub kind: Kind,
    pub u0: f64,
    pub u1: f64,
}
#[derive(Serialize)]
pub struct PlannedCell {
    pub u: f64,
    pub z: f64,
    pub kind: Kind,
    pub opening: Option<facade_cells::Opening>,
    pub story: usize,
    pub flat: Flat,
    pub width: f64,
    pub height: f64,
    pub reach: f64,
    pub key: String,
    pub top: bool,
}
#[derive(Serialize)]
pub struct Room {
    pub x0: f64,
    pub x1: f64,
    pub floor: f64,
    pub head: f64,
    pub depth: f64,
    pub key: String,
}
#[derive(Serialize)]
pub struct Bay {
    pub index: usize,
    pub u0: f64,
    pub u1: f64,
    pub kind: ground::Kind,
    pub room: Option<Room>,
}
#[derive(Serialize)]
pub struct Recess {
    pub u0: f64,
    pub u1: f64,
    pub v0: f64,
    pub v1: f64,
    pub depth: f64,
}
#[derive(Serialize)]
pub struct Plan {
    pub columns: Vec<Column>,
    pub floors: Vec<f64>,
    pub flats: Vec<Vec<Flat>>,
    pub dado: Option<V3>,
    pub base: V3,
    pub seed: u32,
    pub balcony: f64,
    pub limit: f64,
    pub cells: Vec<PlannedCell>,
    pub recesses: Vec<Recess>,
    pub u_marks: Vec<f64>,
    pub z_marks: Vec<f64>,
    pub construction: facade_details::Plan,
    pub bays: Vec<Bay>,
    pub body_condition: wall::Settings,
    pub ground_condition: wall::Settings,
    #[serde(skip)]
    pub continuation: SeededRng,
}
pub(super) fn inside(walls: &[Rect], r: Rect) -> bool {
    let covered: f64 = walls
        .iter()
        .map(|w| {
            (w.u1.min(r.u1) - w.u0.max(r.u0)).max(0.) * (w.v1.min(r.v1) - w.v0.max(r.v0)).max(0.)
        })
        .sum();
    covered >= (r.u1 - r.u0) * (r.v1 - r.v0) - 1e-4
}
fn validate(r: &Request<'_>) -> Result<(), String> {
    let l = r.lot;
    if r.key.is_empty()
        || r.key.len() > 768
        || l.key.is_empty()
        || l.key.len() > 768
        || r.ground.len() > 64
        || r.body.len() > 64
        || [
            r.width,
            r.shift,
            r.body_span[0],
            r.body_span[1],
            r.shops,
            l.ground,
            l.top,
            l.roof,
            l.story_height,
            l.style.bay,
        ]
        .iter()
        .any(|v| !v.is_finite())
        || !(0.6..=40.).contains(&r.width)
        || !(-3. ..=1.5).contains(&r.shift)
        || r.body_span[0].abs() > 40.
        || !(if r.role == Role::Party { 0. } else { 0.6 }..=44.)
            .contains(&(r.body_span[1] - r.body_span[0]))
        || !(0. ..=1.).contains(&r.shops)
        || !(1..=16).contains(&l.stories)
        || !(2. ..=50.).contains(&l.ground)
        || l.roof < l.ground
        || l.top < l.roof
        || l.top > 50.
        || (l.stories > 1 && !(2. ..=50.).contains(&l.story_height))
        || !(1. ..=5.).contains(&l.style.bay)
        || !["tenement", "well"].contains(&l.kind)
    {
        return Err("Facade plan exceeds bounded lot geometry or identity.".into());
    }
    for walls in [r.ground, r.body] {
        for (i, a) in walls.iter().enumerate() {
            if [a.u0, a.u1, a.v0, a.v1]
                .iter()
                .any(|v| !v.is_finite() || v.abs() > 100.)
                || a.u0 >= a.u1
                || a.v0 >= a.v1
            {
                return Err("Invalid facade coverage rectangle.".into());
            }
            for b in &walls[..i] {
                if a.u0 < b.u1 && a.u1 > b.u0 && a.v0 < b.v1 && a.v1 > b.v0 {
                    return Err("Facade coverage rectangles must have disjoint interiors.".into());
                }
            }
        }
    }
    Ok(())
}
fn columns(r: &Request<'_>, rng: &mut SeededRng) -> Vec<Column> {
    let total = r.body_span[1] - r.body_span[0];
    let role = r.role;
    let l = r.lot;
    if role == Role::Party || role == Role::Well || l.kind == "well" {
        return vec![Column {
            kind: if role == Role::Well {
                Kind::Service
            } else {
                Kind::Blank
            },
            u0: r.body_span[0],
            u1: r.body_span[1],
        }];
    }
    let mut sequence = vec![];
    if role == Role::Side {
        let count = (total / 3.4).floor().max(1.) as usize;
        for _ in 0..count {
            sequence.push((
                if rng.chance(0.3) {
                    Kind::Blank
                } else {
                    Kind::Window
                },
                total / count as f64,
            ));
        }
    } else {
        let front = role == Role::Front;
        let stair = if total >= 5.2 && rng.chance(if front { 0.45 } else { 0.25 }) {
            rng.float(1.5, 2.)
        } else {
            0.
        };
        let service = if total - stair >= 4. && rng.chance(if front { 0.35 } else { 0.75 }) {
            rng.float(0.95, 1.25)
        } else {
            0.
        };
        let primary = total - stair - service;
        let count = (primary
            / if front {
                l.style.bay
            } else {
                l.style.bay * 1.15
            })
        .round()
        .max(1.) as usize;
        let plan = if front {
            l.style.plan
        } else {
            rng.pick(&["windows", "windows", "mixed"])
        };
        let offset = rng.integer(0, 1);
        for i in 0..count {
            sequence.push((
                match plan {
                    "windows" => Kind::Window,
                    "bands" => Kind::Band,
                    "balconies" => Kind::Balcony,
                    _ => {
                        if (i + offset) % 2 == 1 {
                            Kind::Balcony
                        } else {
                            Kind::Window
                        }
                    }
                },
                primary / count as f64,
            ));
        }
        if stair > 0. {
            let i = rng.pick(&[0, sequence.len(), sequence.len() / 2]);
            sequence.insert(i, (Kind::Stair, stair));
        }
        if service > 0. {
            let i = rng.pick(&[0, sequence.len()]);
            sequence.insert(i, (Kind::Service, service));
        }
    }
    let mut u = r.body_span[0];
    sequence
        .into_iter()
        .map(|(kind, width)| {
            let c = Column {
                kind,
                u0: u,
                u1: u + width,
            };
            u += width;
            c
        })
        .collect()
}
fn ground_bays(
    r: &Request<'_>,
    kit: &Kit,
    q: &Queries<'_>,
    rng: &mut SeededRng,
    budget: &mut Budget<'_>,
) -> Result<Vec<Bay>, String> {
    if r.role == Role::Party || r.role == Role::Well {
        return Ok(vec![]);
    }
    let count = (r.width / rng.float(2.6, 3.6)).round().max(1.) as usize;
    let entrance = if r.role == Role::Side {
        if rng.chance(0.3) {
            Some(rng.integer(0, count - 1))
        } else {
            None
        }
    } else {
        Some(rng.integer(0, count - 1))
    };
    let mut bays = vec![];
    for i in 0..count {
        budget.spend(r.ground.len() + 1)?;
        let u0 = r.width * i as f64 / count as f64;
        let u1 = r.width * (i + 1) as f64 / count as f64;
        if !inside(
            r.ground,
            Rect {
                u0,
                u1,
                v0: 0.,
                v1: r.lot.ground,
            },
        ) || q.reach(
            Zone::Ground,
            Rect {
                u0,
                u1,
                v0: 0.,
                v1: r.lot.ground.min(2.5),
            },
            0.15,
        )? == 0.
        {
            continue;
        }
        use ground::Kind::*;
        let kind = if entrance == Some(i) {
            Door
        } else if r.role == Role::Front {
            rng.pick(&[
                Shutter, Shutter, Shutter, Shutter, Shop, Shop, Gate, Gate, Service,
            ])
        } else {
            Service
        };
        let mut room = None;
        if matches!(kind, Shutter | Shop | Gate) {
            let key = format!("{}/unit-{i}", r.key);
            let mut rng = kit.rng(&format!("{key}/shop"))?;
            if rng.next_f64() < r.shops && u1 - u0 >= 2.4 && r.lot.ground >= 2.9 {
                let x0 = u0 + 0.3;
                let x1 = u1 - 0.3;
                let head = 2.6_f64.min(r.lot.ground - 0.7);
                let clear = checked_depth((q.hollow)(x0, x1, head, 7.2)?, 7.2)?;
                if clear >= 2.4 {
                    let choice = rng.next_f64();
                    let fraction = if choice < 0.15 {
                        choice * 1.5
                    } else {
                        0.65 + (choice - 0.15) * 0.35 / 0.85
                    };
                    room = Some(Room {
                        x0,
                        x1,
                        floor: 0.12,
                        head,
                        depth: 2.4 + (clear - 2.4) * fraction,
                        key,
                    });
                }
            }
        }
        bays.push(Bay {
            index: i,
            u0,
            u1,
            kind,
            room,
        });
    }
    Ok(bays)
}
/// Planning snapshots household state; queries are read-only and every result is range checked.
pub fn plan(
    r: &Request<'_>,
    kit: &Kit,
    q: &Queries<'_>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Plan, String> {
    validate(r)?;
    let mut budget = Budget::new(4 * 1024 * 1024, cancelled)?;
    budget.spend(1)?;
    let l = r.lot;
    let mut rng = kit.rng(r.key)?;
    let columns = columns(r, &mut rng);
    let floors: Vec<_> = (0..l.stories - 1)
        .map(|j| l.ground + j as f64 * l.story_height)
        .collect();
    let mut flats = vec![];
    for j in 0..floors.len() {
        let mut household = 0;
        let mut shared = 0;
        let mut row = vec![];
        for column in &columns {
            budget.spend(1)?;
            if matches!(column.kind, Kind::Stair | Kind::Service) || shared == 0 {
                household += 1;
                shared = if matches!(column.kind, Kind::Stair | Kind::Service) {
                    0
                } else {
                    rng.integer(1, 2)
                };
            }
            shared = shared.saturating_sub(1);
            row.push(palette::flat(
                &l.style,
                &mut kit.rng(&format!("{}/s{j}/h{household}", r.key))?,
            ));
        }
        flats.push(row);
    }
    let dado = if r.role == Role::Front && l.kind == "tenement" && rng.chance(0.45) {
        Some(shade(
            srgb(rng.pick(&palette::ACCENTS.split_whitespace().collect::<Vec<_>>())),
            0.8,
        ))
    } else {
        None
    };
    let base = shade(
        l.style.wall,
        match r.role {
            Role::Well => 0.6,
            Role::Party => 0.86,
            _ => 1.,
        },
    );
    let seed = rng.integer(1, 2147483647) as u32;
    let balcony = kit.rng(&format!("{}/balcony", l.key))?.float(0.7, 1.);
    let limit = 1.3_f64.min(1.6 - 0.05 - r.shift.max(0.));
    let mut cells = vec![];
    let mut thresholds = vec![];
    let mut balcony_windows = vec![];
    for (j, floor) in floors.iter().copied().enumerate() {
        for (ci, column) in columns.iter().enumerate() {
            budget.spend(r.body.len() + 1)?;
            if column.kind == Kind::Blank {
                continue;
            }
            let top = if j == floors.len() - 1 {
                l.roof
            } else {
                floor + l.story_height
            };
            let rect = Rect {
                u0: column.u0,
                u1: column.u1,
                v0: floor,
                v1: top,
            };
            if !inside(r.body, rect) {
                continue;
            }
            let reach = q.reach(
                Zone::Body,
                Rect {
                    v0: if column.kind == Kind::Balcony {
                        floor - 0.18
                    } else {
                        floor
                    },
                    ..rect
                },
                limit,
            )?;
            if reach == 0. {
                continue;
            }
            let kind = if column.kind == Kind::Balcony && reach < 0.6 {
                Kind::Window
            } else {
                column.kind
            };
            let key = format!("{}/s{j}/c{ci}", r.key);
            let cell = Cell {
                kind,
                role: r.role,
                story: j,
                width: column.u1 - column.u0,
                height: top - floor,
                reach,
                balcony,
                ground: l.ground,
                story_height: l.story_height,
                top: j == floors.len() - 1,
                lot_key: &l.key,
                key: &key,
                wash: base,
                style: &l.style,
                flat: &flats[j][ci],
                kit,
                plants: true,
                cloth: true,
            };
            let opening = cell.opening()?;
            if kind == Kind::Balcony {
                let fit = facade_cells::balcony::fit(&cell)?;
                thresholds.push(Rect {
                    u0: column.u0 + fit.door.x - 0.06,
                    u1: column.u0 + fit.door.x + 0.81,
                    v0: floor + 0.02,
                    v1: floor + fit.door.top,
                });
                if let Some(w) = fit.window {
                    balcony_windows.push(Rect {
                        u0: column.u0 + w.x,
                        u1: column.u0 + w.x + w.w,
                        v0: floor + w.z,
                        v1: floor + w.z + w.h,
                    });
                }
            }
            cells.push(PlannedCell {
                u: column.u0,
                z: floor,
                kind,
                opening,
                story: j,
                flat: cell.flat.clone(),
                width: cell.width,
                height: cell.height,
                reach,
                key,
                top: j == floors.len() - 1,
            });
        }
    }
    if cells.len() > 256 {
        return Err("Facade cell plan exceeds its opening allowance.".into());
    }
    let recesses: Vec<_> = cells
        .iter()
        .filter_map(|c| {
            c.opening.map(|o| Recess {
                u0: c.u + o.x,
                u1: c.u + o.x + o.w,
                v0: c.z + o.z,
                v1: c.z + o.z + o.h,
                depth: o.depth,
            })
        })
        .collect();
    let u_marks = columns
        .iter()
        .flat_map(|c| [c.u0, c.u1])
        .chain(recesses.iter().flat_map(|r| [r.u0, r.u1]))
        .collect();
    let z_marks = [1.15, l.roof]
        .into_iter()
        .chain(floors.iter().flat_map(|z| [*z, *z + 0.95]))
        .chain(recesses.iter().flat_map(|r| [r.v0, r.v1]))
        .collect();
    let mut all_openings: Vec<_> = recesses
        .iter()
        .map(|r| Rect {
            u0: r.u0,
            u1: r.u1,
            v0: r.v0,
            v1: r.v1,
        })
        .collect();
    all_openings.extend(thresholds);
    let construction = facade_details::plan(
        &facade_details::Request {
            key: r.key.into(),
            ground: l.ground,
            roof: l.roof,
            top: l.top,
            stories: l.stories,
            story_height: l.story_height,
            body_span: r.body_span,
            walls: r.body.to_vec(),
            openings: all_openings.clone(),
        },
        &kit.settings().weathering.seed,
        &|rect, limit| q.reach(Zone::Body, rect, limit),
        cancelled,
    )?;
    let mut ledges: Vec<_> = recesses
        .iter()
        .map(|r| wall::Ledge {
            u0: r.u0,
            u1: r.u1,
            z: r.v0,
            flow: 0.6,
        })
        .collect();
    ledges.extend(
        cells
            .iter()
            .filter(|c| c.kind == Kind::Balcony)
            .map(|c| wall::Ledge {
                u0: c.u + 0.12,
                u1: c.u + c.width - 0.12,
                z: c.z - 0.1,
                flow: 0.7,
            }),
    );
    ledges.push(wall::Ledge {
        u0: r.body_span[0],
        u1: r.body_span[1],
        z: l.top,
        flow: 1.,
    });
    ledges.extend(construction.ledges.iter().cloned());
    all_openings.extend(balcony_windows);
    let body_condition = wall::Settings {
        base: None,
        ledges,
        openings: all_openings
            .iter()
            .map(|r| wall::Opening {
                u0: r.u0,
                u1: r.u1,
                z0: r.v0,
                z1: r.v1,
            })
            .collect(),
        grime: kit.settings().weathering.grime,
        wear: kit.settings().weathering.wear,
        seed: f64::from(seed),
    };
    let bays = ground_bays(r, kit, q, &mut rng, &mut budget)?;
    let mut openings = vec![];
    for bay in &bays {
        if let Some(room) = &bay.room {
            openings.push(wall::Opening {
                u0: room.x0,
                u1: room.x1,
                z0: room.floor,
                z1: room.head,
            });
        } else {
            let key = format!("{}/unit-{}", r.key, bay.index);
            if let Some(o) = ground::opening(
                bay.kind,
                &ground::Unit {
                    key: key.clone(),
                    u0: bay.u0,
                    u1: bay.u1,
                    height: l.ground,
                },
                &mut kit.rng(&key)?,
            )? {
                openings.push(o);
            }
        }
    }
    let flow = if l.top - l.ground <= 0.05 {
        0.9
    } else if r.shift < 0. {
        0.8
    } else {
        0.2
    };
    let ground_condition = wall::Settings {
        base: Some(0.),
        ledges: vec![wall::Ledge {
            u0: 0.,
            u1: r.width,
            z: l.ground,
            flow,
        }],
        openings,
        grime: kit.settings().weathering.grime,
        wear: kit.settings().weathering.wear,
        seed: f64::from(seed),
    };
    Ok(Plan {
        columns,
        floors,
        flats,
        dado,
        base,
        seed,
        balcony,
        limit,
        cells,
        recesses,
        u_marks,
        z_marks,
        construction,
        bays,
        body_condition,
        ground_condition,
        continuation: rng,
    })
}
