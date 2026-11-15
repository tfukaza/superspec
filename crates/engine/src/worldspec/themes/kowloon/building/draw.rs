//! Detached building output; shared placement remains immutable during each host job.
use super::{placement::Plan, shell};
use crate::worldspec::themes::kowloon::{
    deck, facade,
    facade_details::corners,
    facade_envelope::Finish,
    facade_plan::{self, Zone},
    fixture::Fixture,
    kit::Kit,
    mesh::Mesh,
    metal::Binding,
    pavement::render::Assets,
    pen::Pen,
    rooftop, shop,
    signs::book::Book,
};
use std::cell::RefCell;
use superspec_geometry::obstructions::Budget;
pub struct Settings<'a> {
    pub kit: &'a Kit,
    pub finishes: &'a [Finish],
    pub concrete: Binding,
    pub wall: Binding,
    pub brushed: Binding,
    pub plants: bool,
    pub cloth: bool,
    pub posters: bool,
    pub shops: Option<shop::shell::Assets>,
    pub deck: &'a deck::Settings<'a>,
    pub pavement: &'a Assets,
}
fn zone(z: corners::Zone) -> Zone {
    match z {
        corners::Zone::Ground => Zone::Ground,
        corners::Zone::Body => Zone::Body,
    }
}
/// The caller owns unpublished mesh and remaining work. Errors prevent that job from being committed.
pub fn site(
    plan: &Plan,
    index: usize,
    mesh: &mut Mesh<'_>,
    s: &Settings<'_>,
    repeats: &dyn Fn(&str) -> f64,
    budget: &mut Budget<'_>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Book, String> {
    let site = plan.sites().get(index).ok_or("Unknown building site.")?;
    if s.finishes.len() != site.lots.len() || s.deck.host != site.host {
        return Err("Building styles must belong to every lot of this host.".into());
    }
    let root = Pen::new(site.matrix, "envelope")?;
    let budget = RefCell::new(budget);
    let mut signs = Book::default();
    for (lot, finish) in site.lots.iter().zip(s.finishes) {
        let faces = plan.facades(index, lot.index, &mut budget.borrow_mut())?;
        let joins = corners::Joins::new(
            faces
                .iter()
                .map(|f| corners::Face {
                    pen: f.pen.clone(),
                    shift: f.shift,
                    ground: f.ground.clone(),
                    body: f.body.clone(),
                })
                .collect(),
        )?;
        for (face_index, f) in faces.iter().enumerate() {
            let reach = |z, r, depth| plan.reach(f, z, r, depth, &mut budget.borrow_mut());
            let hollow = |u0, u1, height, depth| {
                plan.hollow(f, [u0, u1], height, depth, &mut budget.borrow_mut())
            };
            let frontage = |u0, u1, depth, height| {
                plan.frontage(f, [u0, u1], depth, height, &mut budget.borrow_mut())
            };
            let miter = |z, r: crate::worldspec::themes::kowloon::massing::Rect| {
                let z = match z {
                    Zone::Ground => corners::Zone::Ground,
                    Zone::Body => corners::Zone::Body,
                };
                let reach = |i, z, r, depth| {
                    plan.reach(&faces[i], zone(z), r, depth, &mut budget.borrow_mut())
                };
                Ok([
                    joins.miter(face_index, z, r.u0, [r.v0, r.v1], &reach)?,
                    joins.miter(face_index, z, r.u1, [r.v0, r.v1], &reach)?,
                ])
            };
            let nearby = |at, radius, local: &[_]| {
                signs
                    .shop_near_with(&site.host, at, radius, local)
                    .map(|b| b.cloned())
            };
            let proposals = facade::draw(
                &mut Fixture {
                    pen: f.pen.clone(),
                    mesh,
                    repeats,
                },
                &f.request(site),
                &facade::Settings {
                    kit: s.kit,
                    finish,
                    concrete: s.concrete.clone(),
                    plants: s.plants,
                    cloth: s.cloth,
                    posters: s.posters,
                    shops: s.shops,
                },
                &facade::Queries {
                    geometry: facade_plan::Queries {
                        reach: &reach,
                        hollow: &hollow,
                    },
                    miters: &miter,
                    shop_near: &nearby,
                    frontage: Some(&frontage),
                },
                cancelled,
            )?;
            for p in proposals {
                signs.propose(&site.host, s.kit.settings(), p)?;
            }
        }
        let roof = shell::draw(
            &mut Fixture {
                pen: root.clone(),
                mesh,
                repeats,
            },
            site,
            lot,
            &shell::Settings {
                deck: s.deck,
                assets: s.pavement,
                coat: &finish.coat.material,
            },
            &mut budget.borrow_mut(),
        )?;
        if let Some(roof) = roof {
            let proposals = rooftop::draw(
                &mut Fixture {
                    pen: roof.pen,
                    mesh,
                    repeats,
                },
                &roof.request,
                &rooftop::Settings {
                    kit: s.kit,
                    wall: s.wall.clone(),
                    concrete: s.concrete.clone(),
                    brushed: s.brushed.clone(),
                    plants: s.plants,
                    cloth: s.cloth,
                },
            )?;
            for p in proposals {
                signs.propose(&site.host, s.kit.settings(), p)?;
            }
        }
    }
    Ok(signs)
}
