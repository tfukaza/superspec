use serde::Deserialize;
use superspec_engine::worldspec::themes::kowloon::{
    facade_envelope::{self, Finish},
    facade_plan::{self, Queries},
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
}
#[derive(Deserialize)]
struct Golden {
    cases: Vec<Case>,
}
pub fn check_reference_facade_envelopes() {
    let g: Golden = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/facade-envelope.json"
    ))
    .unwrap();
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
        let plan = facade_plan::plan(&r, &kit, &q, &|| false).unwrap();
        for mode in [Mode::Render, Mode::Survey] {
            let mut mesh = Mesh::new(mode, 150000, &|| false).unwrap();
            facade_envelope::draw(
                &mut Fixture {
                    pen: Pen::new(Matrix::default(), "envelope").unwrap(),
                    mesh: &mut mesh,
                    repeats: &|_| 1.,
                },
                &r,
                &plan,
                &c.finish,
            )
            .unwrap();
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
fn facade_envelopes_match_washes_openings_skins_recesses_and_survey() {
    check_reference_facade_envelopes()
}
