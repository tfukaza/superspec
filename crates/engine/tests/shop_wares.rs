use serde::Deserialize;
use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::wares,
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
    tint: V3,
    seed: String,
    i: usize,
    count: usize,
    top: V3,
    next: f64,
    matrix: [V3; 4],
    geometry: assembly::Geometry,
}
#[derive(Deserialize)]
struct Golden {
    cases: Vec<Case>,
}
pub fn check_reference_shop_wares() {
    let g: Golden =
        serde_json::from_str(include_str!("../../../tests/fixtures/rust/shop-wares.json")).unwrap();
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
                "wok" => wares::wok(d, c.at, c.radius, c.tint),
                "hangingWok" => wares::hanging_wok(d, c.at, c.radius, c.tint, c.i % 2 == 1),
                "pot" => wares::pot(
                    d,
                    c.at,
                    c.radius,
                    c.height,
                    c.tint,
                    (c.i % 2 == 1).then_some([0.12, 0.19, 0.08]),
                ),
                "kettle" => wares::kettle(d, c.at, c.radius, c.tint),
                "bucket" => wares::bucket(d, c.at, c.radius, c.height, c.tint),
                "basin" => wares::basin(d, c.at, c.radius, c.tint),
                "basket" => wares::basket(d, c.at, c.radius, c.height, c.tint),
                "clayPot" => wares::clay_pot(d, c.at, c.radius, &mut rng),
                "stoolStack" => wares::stool_stack(d, c.at, c.count, &[c.tint, [0.1, 0.2, 0.3]]),
                "broom" => wares::broom(d, c.at, c.top, &mut rng),
                "mop" => wares::mop(d, c.at, c.top, &mut rng),
                "duster" => wares::duster(d, c.at, &mut rng),
                "coil" => wares::coil(d, c.at, c.radius, 0.012, c.count, c.tint),
                "ladles" => wares::ladles(d, c.at, &mut rng),
                "ladder" => wares::ladder(d, c.at, c.top, 0.42, &mut rng),
                _ => panic!("unknown ware"),
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
fn wares_preserve_bowls_handles_reflections_surface_charts_and_random_streams() {
    check_reference_shop_wares();
}
#[test]
fn wares_reject_unbounded_work_cancel_and_recover() {
    let mut rng = SeededRng::from_text("wares");
    let mut expected = rng.clone();
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
    let d = &mut Fixture {
        pen: Pen::new(Matrix::default(), "wares").unwrap(),
        mesh: &mut mesh,
        repeats: &|_| 1.,
    };
    assert!(wares::coil(d, [0.; 3], 0.2, 0.01, usize::MAX, [1.; 3]).is_err());
    assert!(wares::stool_stack(d, [0.; 3], 1, &[]).is_err());
    assert!(wares::clay_pot(d, [0.; 3], f64::NAN, &mut rng).is_err());
    assert!(wares::ladder(d, [0.; 3], [0., 0., 100.], 0.4, &mut rng).is_err());
    assert_eq!(mesh.triangle_count(), 0);
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| true).unwrap();
    assert!(
        wares::mop(
            &mut Fixture {
                pen: Pen::new(Matrix::default(), "wares").unwrap(),
                mesh: &mut mesh,
                repeats: &|_| 1.
            },
            [0.; 3],
            [0.1, 0., 1.2],
            &mut rng
        )
        .is_err()
    );
    assert_eq!(rng.next_f64(), expected.next_f64());
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
    wares::clay_pot(
        &mut Fixture {
            pen: Pen::new(Matrix::default(), "wares").unwrap(),
            mesh: &mut mesh,
            repeats: &|_| 1.,
        },
        [0.; 3],
        0.15,
        &mut rng,
    )
    .unwrap();
    assert!(mesh.triangle_count() > 0);
}
