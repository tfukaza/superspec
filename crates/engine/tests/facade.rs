use serde::Deserialize;
use superspec_engine::worldspec::themes::kowloon::{
    facade,
    facade_envelope::Finish,
    facade_plan::Queries,
    fixture::Fixture,
    kit::Kit,
    mesh::{Mesh, Mode},
    pen::Pen,
    signs::panel::Settings,
};
use superspec_geometry::Matrix;
#[path = "support/assembly_reference.rs"]
mod assembly;
#[path = "support/facade_input.rs"]
mod input;
#[allow(dead_code)]
#[path = "support/sign_reference.rs"]
mod reference;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    group: usize,
    style_seed: String,
    lot: input::LotData,
    request: input::Input,
    detail: u8,
    settings: Settings,
    finish: Finish,
    geometry: assembly::Geometry,
    charts: Vec<assembly::Chart>,
    proposals: Vec<serde_json::Value>,
}
#[derive(Deserialize)]
struct Golden {
    cases: Vec<Case>,
}
pub fn check_reference_facades() {
    let g: Golden =
        serde_json::from_str(include_str!("../../../tests/fixtures/rust/facade.json")).unwrap();
    for c in g.cases {
        let lot = c.lot.build(&c.style_seed, c.request.width);
        let kit = Kit::new(c.settings, c.detail).unwrap();
        let r = c.request.request(&lot);
        let q = Queries {
            reach: &|_, r, limit| {
                Ok(if c.group % 3 == 0 && r.u0 > 3. {
                    0.
                } else if c.group % 3 == 1 {
                    limit.min(0.4)
                } else {
                    limit
                })
            },
            hollow: &|_, _, _, _| Ok(if c.group % 4 == 0 { 0. } else { 3.1 }),
        };

        for mode in [Mode::Render, Mode::Survey] {
            let mut mesh = Mesh::new(mode, 150000, &|| false).unwrap();
            let proposals = facade::draw(
                &mut Fixture {
                    pen: Pen::new(Matrix::default(), "envelope").unwrap(),
                    mesh: &mut mesh,
                    repeats: &|_| 1.,
                },
                &r,
                &facade::Settings {
                    kit: &kit,
                    finish: &c.finish,
                    concrete: superspec_engine::worldspec::themes::kowloon::metal::Binding {
                        material: "concrete".into(),
                        repeat: 1.,
                    },
                    plants: true,
                    cloth: true,
                    posters: true,
                    shops: None,
                },
                &facade::Queries {
                    geometry: Queries {
                        reach: q.reach,
                        hollow: q.hollow,
                    },
                    miters: &|_, _| Ok([None; 2]),
                    shop_near: &|_, _, _| Ok(None),
                    frontage: None,
                },
                &|| false,
            )
            .unwrap();
            assert_eq!(proposals.len(), c.proposals.len());
            for (p, e) in proposals.iter().zip(&c.proposals) {
                reference::close(&reference::metadata(p), e);
            }
            assert_eq!(mesh.triangle_count() * 3, c.geometry.indices.len());
            if !mesh.surveying() {
                assembly::check(
                    &mut mesh,
                    &c.geometry,
                    &c.charts,
                    &format!("envelope {}", c.group),
                );
            }
        }
    }
}
#[test]
fn complete_facades_match_walls_households_services_ground_and_deferred_signs() {
    check_reference_facades()
}
