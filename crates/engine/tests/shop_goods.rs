use serde::Deserialize;
use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::goods,
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
    seed: String,
    textured: bool,
    dimensions: Option<[f64; 4]>,
    next: f64,
    matrix: [V3; 4],
    geometry: assembly::Geometry,
}
#[derive(Deserialize)]
struct Golden {
    cases: Vec<Case>,
}
pub fn check_reference_shop_goods() {
    let g: Golden =
        serde_json::from_str(include_str!("../../../tests/fixtures/rust/shop-goods.json")).unwrap();
    for c in g.cases {
        for mode in [Mode::Render, Mode::Survey] {
            let mut mesh = Mesh::new(mode, 100000, &|| false).unwrap();
            let d = &mut Fixture {
                pen: Pen::new(Matrix(c.matrix), "furniture").unwrap(),
                mesh: &mut mesh,
                repeats: &|_| 1.7,
            };
            let mut rng = SeededRng::from_text(&c.seed);
            if c.kind == "sack" {
                goods::sack(
                    d,
                    [0.1, 0.2, 0.3],
                    0.2,
                    0.45,
                    [0.6, 0.4, 0.2],
                    [0.2, 0.3, 0.1],
                )
                .unwrap();
            } else {
                let kind = serde_json::from_value(serde_json::json!(c.kind)).unwrap();
                let mut line = goods::line(kind, &mut rng, c.textured).unwrap();
                for (a, b) in [line.width, line.depth, line.height, line.stack as f64]
                    .iter()
                    .zip(c.dimensions.unwrap())
                {
                    assert!((a - b).abs() < 1e-12);
                }
                (line.draw)(d, [0.1, 0.2, 0.3]).unwrap();
                (line.draw)(d, [0.4, 0.2, 0.3 + line.height]).unwrap();
            }
            assert_eq!(rng.next_f64(), c.next, "{} {}", c.kind, c.seed);
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
fn goods_preserve_product_sizes_brands_repeated_placements_and_surface_charts() {
    check_reference_shop_goods();
}
#[test]
fn invalid_and_cancelled_goods_do_not_emit_and_next_draw_recovers() {
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
    let d = &mut Fixture {
        pen: Pen::new(Matrix::default(), "goods").unwrap(),
        mesh: &mut mesh,
        repeats: &|_| 1.,
    };
    assert!(goods::jar(d, [0.; 3], f64::NAN, 0.2, [1.; 3], [1.; 3]).is_err());
    assert!(goods::sack(d, [0.; 3], 0.2, -0.3, [1.; 3], [1.; 3]).is_err());
    assert_eq!(mesh.triangle_count(), 0);
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| true).unwrap();
    assert!(
        goods::tin(
            &mut Fixture {
                pen: Pen::new(Matrix::default(), "goods").unwrap(),
                mesh: &mut mesh,
                repeats: &|_| 1.
            },
            [0.; 3],
            0.05,
            0.1,
            [1.; 3]
        )
        .is_err()
    );
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
    goods::tin(
        &mut Fixture {
            pen: Pen::new(Matrix::default(), "goods").unwrap(),
            mesh: &mut mesh,
            repeats: &|_| 1.,
        },
        [0.; 3],
        0.05,
        0.1,
        [1.; 3],
    )
    .unwrap();
    assert!(mesh.triangle_count() > 0);
}
