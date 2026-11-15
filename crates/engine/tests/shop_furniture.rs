use serde::Deserialize;
use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::furniture::{self, Finish},
    timber,
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
    levels: usize,
    above: f64,
    tint: V3,
    seed: String,
    textured: bool,
    matrix: [V3; 4],
    surfaces: serde_json::Value,
    next: f64,
    geometry: assembly::Geometry,
}
#[derive(Deserialize)]
struct Golden {
    cases: Vec<Case>,
}
pub fn check_reference_shop_furniture() {
    let g: Golden = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/shop-furniture.json"
    ))
    .unwrap();
    for c in g.cases {
        for mode in [Mode::Render, Mode::Survey] {
            let mut mesh = Mesh::new(mode, 100000, &|| false).unwrap();
            let mut rng = SeededRng::from_text(&c.seed);
            let timber = timber::Settings {
                textured: c.textured,
                timber_repeat: 1.7,
                atlas_repeat: 1.7,
                ..Default::default()
            };
            let d = &mut Fixture {
                pen: Pen::new(Matrix(c.matrix), "furniture").unwrap(),
                mesh: &mut mesh,
                repeats: &|_| 1.7,
            };
            let surfaces = match c.kind.as_str() {
                "shelving-timber" | "shelving-metal" => furniture::shelving(
                    d,
                    c.size,
                    c.levels,
                    c.kind.strip_prefix("shelving-").unwrap(),
                    c.tint,
                    c.above,
                    &timber,
                )
                .unwrap(),
                "counter-timber" | "counter-tiled" | "counter-steel" => vec![
                    furniture::counter(
                        d,
                        c.size,
                        match c.kind.as_str() {
                            "counter-timber" => Finish::Timber,
                            "counter-tiled" => Finish::Tiled,
                            _ => Finish::Steel,
                        },
                        c.tint,
                        &mut rng,
                        &timber,
                    )
                    .unwrap(),
                ],
                "crate" => {
                    vec![furniture::crate_box(d, c.size, c.tint, &mut rng, &timber).unwrap()]
                }
                _ => panic!("unknown furniture"),
            };
            reference::close(&serde_json::json!(surfaces), &c.surfaces);
            assert_eq!(rng.next_f64(), c.next);
            // Survey skips finish-driven timber splits, retaining the same physical surfaces.
            if !mesh.surveying() {
                assembly::check(
                    &mut mesh,
                    &c.geometry,
                    &[],
                    &format!("{} {} textured={}", c.kind, c.seed, c.textured),
                );
            } else {
                assert!(mesh.triangle_count() > 0);
            }
        }
    }
}
#[test]
fn furniture_keeps_stock_surfaces_timber_charts_geometry_and_rng() {
    check_reference_shop_furniture();
}
#[test]
fn invalid_shelves_and_cancelled_crates_do_not_consume_stock_randomness() {
    let timber = timber::Settings::default();
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
    let d = &mut Fixture {
        pen: Pen::new(Matrix::default(), "furniture").unwrap(),
        mesh: &mut mesh,
        repeats: &|_| 1.,
    };
    assert!(furniture::shelving(d, [1., 0.3, 0.2], 30, "timber", [1.; 3], 0., &timber).is_err());
    assert_eq!(d.mesh.triangle_count(), 0);
    let mut rng = SeededRng::from_text("stock");
    let mut expected = rng.clone();
    assert!(
        furniture::counter(
            d,
            [f64::NAN, 0.3, 1.],
            Finish::Tiled,
            [1.; 3],
            &mut rng,
            &timber
        )
        .is_err()
    );
    assert_eq!(rng.next_f64(), expected.next_f64());
    let mut cancelled = Mesh::new(Mode::Render, 10000, &|| true).unwrap();
    assert!(
        furniture::crate_box(
            &mut Fixture {
                pen: d.pen.clone(),
                mesh: &mut cancelled,
                repeats: d.repeats
            },
            [0.4, 0.3, 0.2],
            [1.; 3],
            &mut rng,
            &timber
        )
        .is_err()
    );
    assert_eq!(rng.next_f64(), expected.next_f64());
    furniture::crate_box(d, [0.4, 0.3, 0.2], [1.; 3], &mut rng, &timber).unwrap();
    assert!(d.mesh.triangle_count() > 0);
}
