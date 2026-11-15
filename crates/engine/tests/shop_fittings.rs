use serde::Deserialize;
use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::fittings,
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
    index: usize,
    at: V3,
    tint: V3,
    seed: String,
    textured: bool,
    matrix: [V3; 4],
    result: Option<V3>,
    next: f64,
    geometry: assembly::Geometry,
}
#[derive(Deserialize)]
struct Golden {
    cases: Vec<Case>,
}
pub fn check_reference_shop_fittings() {
    let g: Golden = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/shop-fittings.json"
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
            let i = c.index as f64;
            let at = c.at;
            let tint = c.tint;
            let mut result = None;
            match c.kind.as_str() {
                "bottleCrate" => {
                    fittings::bottle_crate(d, tint, [0.1, 0.3, 0.15], [0.8, 0.3, 0.1], c.index != 0)
                }
                "rail" => fittings::rail(d, at, [1.2, 0.3, 0.7], 1.3, tint),
                "hook" => fittings::hook(d, at, 0.04 + i * 0.04, tint).map(|p| result = Some(p)),
                "pendant" => fittings::pendant(
                    d,
                    at,
                    0.3 + i * 0.2,
                    if c.index == 1 { None } else { Some(tint) },
                ),
                "tubeLight" => fittings::tube_light(d, at, [1.2, at[1], at[2]]),
                "ceilingFan" => fittings::ceiling_fan(d, at, 0.28, 0.8 + i * 0.2, &mut rng),
                "stool" => fittings::stool(d, at, tint),
                "priceCard" => fittings::price_card(d, at, &mut rng),
                "counterScale" => fittings::counter_scale(d, at, tint),
                "hangingScale" => fittings::hanging_scale(d, at, 0.7 + i * 0.2, tint),
                "calendar" => fittings::calendar(d, at, &mut rng, c.textured),
                "wallClock" => fittings::wall_clock(d, at, &mut rng),
                _ => panic!("unknown shop fitting"),
            }
            .unwrap();
            reference::close(&serde_json::json!(result), &serde_json::json!(c.result));
            assert_eq!(rng.next_f64(), c.next);
            assert_eq!(mesh.triangle_count() * 3, c.geometry.indices.len());
            if !mesh.surveying() {
                assembly::check(
                    &mut mesh,
                    &c.geometry,
                    &[],
                    &format!("{} {} textured={}", c.kind, c.index, c.textured),
                );
            }
        }
    }
}
#[test]
fn shop_lights_scales_and_displays_preserve_geometry_hooks_and_seeded_variation() {
    check_reference_shop_fittings();
}
#[test]
fn invalid_or_cancelled_fittings_preserve_randomness_and_next_job_recovers() {
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
    let d = &mut Fixture {
        pen: Pen::new(Matrix::default(), "equipment").unwrap(),
        mesh: &mut mesh,
        repeats: &|_| 1.,
    };
    let mut rng = SeededRng::from_text("fittings");
    let mut expected = rng.clone();
    assert!(fittings::ceiling_fan(d, [0.; 3], 0., 1., &mut rng).is_err());
    assert!(fittings::tube_light(d, [0.; 3], [0., 1., 0.]).is_err());
    assert!(fittings::pendant(d, [0.; 3], f64::INFINITY, None).is_err());
    assert_eq!(d.mesh.triangle_count(), 0);
    assert_eq!(rng.next_f64(), expected.next_f64());
    let mut cancelled = Mesh::new(Mode::Render, 10000, &|| true).unwrap();
    assert!(
        fittings::price_card(
            &mut Fixture {
                pen: d.pen.clone(),
                mesh: &mut cancelled,
                repeats: d.repeats
            },
            [0.; 3],
            &mut rng
        )
        .is_err()
    );
    assert_eq!(rng.next_f64(), expected.next_f64());
    fittings::ceiling_fan(d, [0., 0., 2.4], 0.28, 1.1, &mut rng).unwrap();
    assert!(d.mesh.triangle_count() > 0);
}
