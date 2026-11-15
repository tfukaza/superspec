//! One planned facade's ground/body skins and recessed aperture linings.
use super::{
    facade_cells::{Kind, Role},
    facade_material,
    facade_plan::{Plan, Request, Zone},
    fixture::Fixture,
    massing::Rect,
    metal::Binding,
    palette::{blend, shade, srgb},
    pen::Pen,
    skin::{self, Wash},
    tiled_skin, wall,
};
use superspec_geometry::V3;
#[derive(Clone, serde::Deserialize)]
pub struct Finish {
    pub coat: Binding,
    pub bare: Binding,
    pub concrete: bool,
    pub seed: u32,
    pub density: f64,
    pub tiles: Option<tiled_skin::Settings>,
}
fn dressed(role: Role) -> bool {
    matches!(role, Role::Front | Role::Back | Role::Side)
}
fn wash(r: &Request<'_>, p: &Plan, zone: Zone, u: f64, z: f64) -> V3 {
    if matches!(zone, Zone::Ground) {
        return if z < 1.15 {
            p.dado.map_or(p.base, |color| blend(p.base, color))
        } else {
            p.base
        };
    }
    if !dressed(r.role) || p.floors.is_empty() || z >= r.lot.roof {
        return p.base;
    }
    let row = (((z - r.lot.ground) / r.lot.story_height).floor().max(0.) as usize)
        .min(p.floors.len() - 1);
    let Some(column) = p.columns.iter().position(|c| u >= c.u0 && u < c.u1) else {
        return p.base;
    };
    if matches!(p.columns[column].kind, Kind::Stair | Kind::Service) {
        return p.base;
    }
    let flat = &p.flats[row][column];
    if let Some(color) = flat.paint
        && (flat.band == "full" || z - p.floors[row] < 0.95)
    {
        blend(p.base, color)
    } else {
        p.base
    }
}
fn washes(r: &Request<'_>, p: &Plan, zone: Zone) -> Vec<Wash> {
    if matches!(zone, Zone::Ground) {
        return p
            .dado
            .map(|color| Wash {
                bounds: [-1000., -1000., 1000., 1.15],
                tint: blend(p.base, color),
                lower_inclusive: false,
            })
            .into_iter()
            .collect();
    }
    let mut result = vec![];
    if !dressed(r.role) {
        return result;
    }
    for (j, floor) in p.floors.iter().copied().enumerate() {
        for (i, c) in p.columns.iter().enumerate() {
            if matches!(c.kind, Kind::Stair | Kind::Service) {
                continue;
            }
            let flat = &p.flats[j][i];
            if let Some(color) = flat.paint {
                let bottom = if j == 0 { -1000. } else { floor };
                let top = if flat.band == "full" {
                    p.floors.get(j + 1).copied().unwrap_or(r.lot.roof)
                } else {
                    floor + 0.95
                }
                .min(r.lot.roof);
                if bottom < top {
                    result.push(Wash {
                        bounds: [c.u0, bottom, c.u1, top],
                        tint: blend(p.base, color),
                        lower_inclusive: true,
                    });
                }
            }
        }
    }
    result
}
fn on<'a, 'c>(d: &'a mut Fixture<'_, 'c>, pen: Pen) -> Fixture<'a, 'c> {
    Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    }
}
/// Skin and recess geometry stay visual; the building owner retains the physical envelope.
pub fn draw(d: &mut Fixture<'_, '_>, r: &Request<'_>, p: &Plan, f: &Finish) -> Result<(), String> {
    d.mesh.spend_work(1)?;
    if p.flats.len() != p.floors.len()
        || p.flats.iter().any(|row| row.len() != p.columns.len())
        || p.floors.len() > 16
        || p.columns.len() > 64
        || p.recesses.len() > 256
        || p.bays.len() > 32
    {
        return Err("Facade envelope requires a bounded complete household plan.".into());
    }
    let make_finish = |condition: &wall::Settings| facade_material::Settings {
        concrete: f.concrete,
        coat: facade_material::Coat {
            material: f.coat.material.clone(),
            repeat: f.coat.repeat,
            tint: p.base,
        },
        backing: f.bare.clone(),
        wear: condition.wear,
        density: f.density,
        variant: f.seed % 2,
    };
    let shops: Vec<_> = p.bays.iter().filter_map(|b| b.room.as_ref()).collect();
    let ground = skin::Settings {
        rects: r.ground.to_vec(),
        openings: shops
            .iter()
            .map(|s| Rect {
                u0: s.x0,
                u1: s.x1,
                v0: s.floor,
                v1: s.head,
            })
            .collect(),
        u_marks: p
            .u_marks
            .iter()
            .copied()
            .chain(shops.iter().flat_map(|s| [s.x0, s.x1]))
            .collect(),
        z_marks: p
            .z_marks
            .iter()
            .copied()
            .chain(shops.iter().flat_map(|s| [s.floor, s.head]))
            .collect(),
        finish: make_finish(&p.ground_condition),
        condition: p.ground_condition.clone(),
        washes: washes(r, p, Zone::Ground),
        tiles: f.tiles.clone().map(|mut tiles| {
            tiles.seed = p.seed;
            tiles
        }),
    };
    let body = skin::Settings {
        rects: r.body.to_vec(),
        openings: p
            .recesses
            .iter()
            .map(|s| Rect {
                u0: s.u0,
                u1: s.u1,
                v0: s.v0,
                v1: s.v1,
            })
            .collect(),
        u_marks: p.u_marks.clone(),
        z_marks: p.z_marks.clone(),
        finish: make_finish(&p.body_condition),
        condition: p.body_condition.clone(),
        washes: washes(r, p, Zone::Body),
        tiles: None,
    };
    let body_pen = d.pen.moved([0., r.shift, 0.])?;
    skin::draw(d, &f.coat.material, &ground, &format!("{}/ground", r.key))?;
    let d = &mut on(d, body_pen);
    skin::draw(d, &f.coat.material, &body, &format!("{}/body", r.key))?;
    let condition = wall::Condition::new(&p.body_condition)?;
    for s in &p.recesses {
        d.mesh.spend_work(1)?;
        let tint = shade(
            blend(
                wash(r, p, Zone::Body, (s.u0 + s.u1) / 2., s.v0 - 0.05),
                condition.sample(s.u0, s.v0)?.tint,
            ),
            0.82,
        );
        d.face(
            &f.coat.material,
            &[
                [s.u0, 0., s.v0],
                [s.u0, -s.depth, s.v0],
                [s.u0, -s.depth, s.v1],
                [s.u0, 0., s.v1],
            ],
            [1., 0., 0.],
            tint,
        )?;
        d.face(
            &f.coat.material,
            &[
                [s.u1, 0., s.v0],
                [s.u1, -s.depth, s.v0],
                [s.u1, -s.depth, s.v1],
                [s.u1, 0., s.v1],
            ],
            [-1., 0., 0.],
            tint,
        )?;
        d.face(
            &f.coat.material,
            &[
                [s.u0, 0., s.v0],
                [s.u1, 0., s.v0],
                [s.u1, -s.depth, s.v0],
                [s.u0, -s.depth, s.v0],
            ],
            [0., 0., 1.],
            tint,
        )?;
        d.face(
            &f.coat.material,
            &[
                [s.u0, 0., s.v1],
                [s.u1, 0., s.v1],
                [s.u1, -s.depth, s.v1],
                [s.u0, -s.depth, s.v1],
            ],
            [0., 0., -1.],
            shade(tint, 0.8),
        )?;
        d.panel("paint", [s.u0, s.u1, s.v0, s.v1], -s.depth, srgb("#2a2927"))?;
    }
    Ok(())
}
