use serde::Deserialize;
use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::{chopping_block, restaurant_surfaces::Settings, restaurant_work_details as details},
};
use superspec_geometry::{Matrix, V3};
#[path = "support/assembly_reference.rs"]
mod assembly;
#[allow(dead_code)]
#[path = "support/sign_reference.rs"]
mod reference;
#[derive(Deserialize)]
struct Case {
    kind: String,
    size: V3,
    at: V3,
    seed: u32,
    textured: bool,
    next: f64,
    matrix: [V3; 4],
    geometry: assembly::Geometry,
}
#[derive(Deserialize)]
struct Golden {
    cases: Vec<Case>,
}
pub fn check_reference_restaurant_details() {
    let g: Golden = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/restaurant-details.json"
    ))
    .unwrap();
    for c in g.cases {
        for mode in [Mode::Render, Mode::Survey] {
            let mut mesh = Mesh::new(mode, 100000, &|| false).unwrap();
            let d = &mut Fixture {
                pen: Pen::new(Matrix(c.matrix), "furniture").unwrap(),
                mesh: &mut mesh,
                repeats: &|_| 1.7,
            };
            let mut rng = SeededRng::from_u32(c.seed);
            let settings = Settings {
                fields: c.textured,
                tiles: c.textured,
                metal_repeat: 1.7,
            };
            match c.kind.as_str() {
                "choppingBlock" => chopping_block::draw(d, c.at, c.textured, settings).unwrap(),
                "displayCabinet" => {
                    details::display_cabinet(d, c.size[0], c.size[2], 1.8, 1.7).unwrap()
                }
                "counterTowel" => details::counter_towel(d, 0.23, c.size[2]).unwrap(),
                "orderSlips" => details::order_slips(d, c.at, c.size[0], &mut rng, 1.7).unwrap(),
                "chopstickCup" => details::chopstick_cup(d, c.at, &mut rng).unwrap(),
                _ => panic!("Unknown detail"),
            }
            assert_eq!(rng.next_f64(), c.next, "{} seed {}", c.kind, c.seed);
            assert_eq!(mesh.triangle_count() * 3, c.geometry.indices.len());
            if !mesh.surveying() {
                assembly::check(
                    &mut mesh,
                    &c.geometry,
                    &[],
                    &format!("{} {} textured={}", c.kind, c.seed, c.textured),
                );
            }
        }
    }
}
#[test]
fn kitchen_details_preserve_closed_cloth_continuous_wood_and_supported_glazing() {
    check_reference_restaurant_details();
}
#[test]
fn invalid_and_cancelled_detail_work_emits_nothing() {
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
    let d = &mut Fixture {
        pen: Pen::new(Matrix::default(), "details").unwrap(),
        mesh: &mut mesh,
        repeats: &|_| 1.,
    };
    assert!(details::display_cabinet(d, -1., 0.8, 1.8, 1.).is_err());
    assert!(details::counter_towel(d, f64::NAN, 0.8).is_err());
    assert_eq!(mesh.triangle_count(), 0);
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| true).unwrap();
    assert!(
        details::counter_towel(
            &mut Fixture {
                pen: Pen::new(Matrix::default(), "details").unwrap(),
                mesh: &mut mesh,
                repeats: &|_| 1.
            },
            0.23,
            0.8
        )
        .is_err()
    );
}
