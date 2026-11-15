use serde::Deserialize;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::restaurant_food,
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
    scale: f64,
    size: V3,
    i: usize,
    textured: bool,
    matrix: [V3; 4],
    geometry: assembly::Geometry,
}
#[derive(Deserialize)]
struct Golden {
    cases: Vec<Case>,
}
pub fn check_reference_restaurant_food() {
    let g: Golden = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/restaurant-food.json"
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
            if c.kind == "chicken" {
                restaurant_food::hanging_chicken(d, c.at, c.scale, c.i, c.textured).unwrap();
            } else {
                restaurant_food::sliced_meat(d, c.at, c.size, c.textured).unwrap();
            }
            assert_eq!(mesh.triangle_count() * 3, c.geometry.indices.len());
            if !mesh.surveying() {
                assembly::check(
                    &mut mesh,
                    &c.geometry,
                    &[],
                    &format!("{} {} textured={}", c.kind, c.i, c.textured),
                );
            }
        }
    }
}
#[test]
fn restaurant_food_preserves_real_anchor_uniform_fit_labels_and_vertex_tints() {
    check_reference_restaurant_food();
}
#[test]
fn food_rejects_invalid_variants_and_envelopes_cancels_and_recovers() {
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
    let d = &mut Fixture {
        pen: Pen::new(Matrix::default(), "food").unwrap(),
        mesh: &mut mesh,
        repeats: &|_| 1.,
    };
    assert!(restaurant_food::hanging_chicken(d, [0.; 3], 1., 3, true).is_err());
    assert!(restaurant_food::sliced_meat(d, [0.; 3], [1., f64::NAN, 1.], true).is_err());
    assert_eq!(mesh.triangle_count(), 0);
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| true).unwrap();
    assert!(
        restaurant_food::hanging_chicken(
            &mut Fixture {
                pen: Pen::new(Matrix::default(), "food").unwrap(),
                mesh: &mut mesh,
                repeats: &|_| 1.
            },
            [0.; 3],
            1.,
            0,
            true
        )
        .is_err()
    );
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
    restaurant_food::hanging_chicken(
        &mut Fixture {
            pen: Pen::new(Matrix::default(), "food").unwrap(),
            mesh: &mut mesh,
            repeats: &|_| 1.,
        },
        [0.; 3],
        1.,
        0,
        true,
    )
    .unwrap();
    assert!(mesh.triangle_count() > 0);
}
