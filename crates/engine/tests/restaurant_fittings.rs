use serde::Deserialize;
use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::{restaurant_fittings as kitchen, restaurant_surfaces::Settings},
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
    count: usize,
    seed: u32,
    textured: bool,
    support: Option<serde_json::Value>,
    next: f64,
    matrix: [V3; 4],
    geometry: assembly::Geometry,
}
#[derive(Deserialize)]
struct Golden {
    cases: Vec<Case>,
}
pub fn check_reference_restaurant_fittings() {
    let g: Golden = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/restaurant-fittings.json"
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
                "restaurantSteelTop" => kitchen::steel_top(d, c.size, 0.04, 1.7).unwrap(),
                "restaurantWorkBench" => {
                    let shelf = kitchen::workbench(d, c.size, 1.7).unwrap();
                    let expected = c.support.as_ref().unwrap();
                    for (key, v) in [
                        ("x0", shelf.x0),
                        ("x1", shelf.x1),
                        ("y", shelf.y),
                        ("depth", shelf.depth),
                        ("z", shelf.z),
                        ("clear", shelf.clear),
                    ] {
                        assert!((v - expected[key].as_f64().unwrap()).abs() < 1e-12);
                    }
                }
                "restaurantCounter" => assert_eq!(
                    kitchen::counter(d, c.size, c.seed, settings).unwrap(),
                    c.support.as_ref().unwrap().as_f64().unwrap()
                ),
                "extractor" => kitchen::extractor(d, c.at, c.size[0], 2.6, 1.7).unwrap(),
                "bowlStack" => {
                    kitchen::bowl_stack(d, c.at, c.count, 0.07, [0.2, 0.3, 0.4]).unwrap()
                }
                "condimentGroup" => {
                    kitchen::condiment_group(d, c.at, &mut rng, c.textured).unwrap()
                }
                "takeawayStack" => kitchen::takeaway_stack(d, c.at, &mut rng).unwrap(),
                "meatTray" => assert_eq!(
                    kitchen::meat_tray(d, c.at, [c.size[0], c.size[1]], 1.7).unwrap(),
                    c.support.as_ref().unwrap().as_f64().unwrap()
                ),
                "sauceTray" => kitchen::sauce_tray(d, c.at, 1.7).unwrap(),
                _ => panic!("Unknown fitting"),
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
fn kitchen_fittings_preserve_support_planes_material_charts_and_rng() {
    check_reference_restaurant_fittings();
}
#[test]
fn invalid_and_cancelled_kitchen_work_is_bounded() {
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
    let d = &mut Fixture {
        pen: Pen::new(Matrix::default(), "kitchen").unwrap(),
        mesh: &mut mesh,
        repeats: &|_| 1.,
    };
    assert!(kitchen::workbench(d, [0.1, 0.4, 0.8], 1.).is_err());
    assert!(kitchen::steel_top(d, [0.8, 0.4, 0.8], 1., 1.).is_err());
    assert!(kitchen::bowl_stack(d, [0.; 3], usize::MAX, 0.07, [1.; 3]).is_err());
    assert_eq!(mesh.triangle_count(), 0);
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| true).unwrap();
    assert!(
        kitchen::sauce_tray(
            &mut Fixture {
                pen: Pen::new(Matrix::default(), "kitchen").unwrap(),
                mesh: &mut mesh,
                repeats: &|_| 1.
            },
            [0.; 3],
            1.
        )
        .is_err()
    );
}
