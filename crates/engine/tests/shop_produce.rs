use serde::Deserialize;
use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::produce,
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
    at: V3,
    radius: f64,
    height: f64,
    seed: String,
    i: usize,
    next: f64,
    matrix: [V3; 4],
    geometry: assembly::Geometry,
}
#[derive(Deserialize)]
struct Golden {
    cases: Vec<Case>,
}
pub fn check_reference_shop_produce() {
    let g: Golden = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/shop-produce.json"
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
            let mut rng = SeededRng::from_text(&c.seed);
            match c.kind.as_str() {
                "heap" => {
                    let kind = produce::fruit_kind("lemon").unwrap();
                    produce::heap(d, c.at, [0.2, 0.14], 0.028, 3, &mut rng, |d, at, rng| {
                        produce::fruit(d, at, &kind, rng)
                    })
                }
                "melon" => produce::melon(d, c.at, c.height, &mut rng),
                "melonHalf" => produce::melon_half(d, c.at, c.radius),
                "bananas" => produce::bananas(d, c.at, &mut rng),
                "pineapple" => produce::pineapple(d, c.at, &mut rng),
                "cabbage" => produce::cabbage(d, c.at, c.radius, &mut rng),
                "greens" => produce::greens(d, c.at, c.height, &mut rng),
                "root" => produce::root(d, c.at, c.height, c.i % 2 == 1, &mut rng),
                "garlicString" => produce::garlic_string(d, c.at, c.height, &mut rng),
                "sausages" => produce::sausages(d, c.at, &mut rng),
                "saltedFish" => produce::salted_fish(d, c.at, c.height, &mut rng),
                _ => produce::fruit(d, c.at, &produce::fruit_kind(&c.kind).unwrap(), &mut rng),
            }
            .unwrap();
            assert_eq!(rng.next_f64(), c.next, "{} {}", c.kind, c.seed);
            assert_eq!(mesh.triangle_count() * 3, c.geometry.indices.len());
            if !mesh.surveying() {
                assembly::check(
                    &mut mesh,
                    &c.geometry,
                    &[],
                    &format!("{} {}", c.kind, c.seed),
                );
            }
        }
    }
}
#[test]
fn produce_preserves_shape_colors_leaf_sheets_visible_courses_and_random_streams() {
    check_reference_shop_produce();
}
#[test]
fn produce_rejects_invalid_input_bounds_empty_heap_work_and_recovers_after_cancellation() {
    let mut rng = SeededRng::from_text("produce");
    let mut expected = rng.clone();
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
    let d = &mut Fixture {
        pen: Pen::new(Matrix::default(), "produce").unwrap(),
        mesh: &mut mesh,
        repeats: &|_| 1.,
    };
    assert!(produce::root(d, [0.; 3], -1., true, &mut rng).is_err());
    let mut invalid = produce::fruit_kind("apple").unwrap();
    invalid.colors.clear();
    assert!(produce::fruit(d, [0.; 3], &invalid, &mut rng).is_err());
    assert_eq!(rng.next_f64(), expected.next_f64());
    assert_eq!(mesh.triangle_count(), 0);
    let mut mesh = Mesh::new(Mode::Render, 100, &|| false).unwrap();
    let d = &mut Fixture {
        pen: Pen::new(Matrix::default(), "produce").unwrap(),
        mesh: &mut mesh,
        repeats: &|_| 1.,
    };
    assert!(produce::heap(d, [0.; 3], [10., 10.], 0.001, 1, &mut rng, |_, _, _| Ok(())).is_err());
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| true).unwrap();
    let d = &mut Fixture {
        pen: Pen::new(Matrix::default(), "produce").unwrap(),
        mesh: &mut mesh,
        repeats: &|_| 1.,
    };
    assert!(produce::bananas(d, [0.; 3], &mut rng).is_err());
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
    let d = &mut Fixture {
        pen: Pen::new(Matrix::default(), "produce").unwrap(),
        mesh: &mut mesh,
        repeats: &|_| 1.,
    };
    produce::bananas(d, [0.; 3], &mut rng).unwrap();
    assert!(mesh.triangle_count() > 0);
}
