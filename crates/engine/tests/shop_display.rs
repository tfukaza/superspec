use serde::Deserialize;
use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::{display, frontage},
    timber,
};
use superspec_geometry::{Matrix, V3};
#[allow(dead_code)]
#[path = "support/assembly_reference.rs"]
mod assembly;
#[allow(dead_code)]
#[path = "support/sign_reference.rs"]
mod reference;
#[derive(Deserialize)]
struct Case {
    kind: display::Kind,
    trade: display::Trade,
    seed: String,
    textured: bool,
    next: f64,
    matrix: [V3; 4],
    geometry: assembly::Geometry,
}
#[derive(Deserialize)]
struct Planning {
    width: f64,
    seed: String,
    mode: u8,
    plans: serde_json::Value,
    calls: serde_json::Value,
    next: f64,
}
#[derive(Deserialize)]
struct Golden {
    cases: Vec<Case>,
    plans: Vec<Planning>,
}
pub fn check_reference_shop_display() {
    let g: Golden = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/shop-display.json"
    ))
    .unwrap();
    for c in g.cases {
        for mode in [Mode::Render, Mode::Survey] {
            let mut mesh = Mesh::new(mode, 100000, &|| false).unwrap();
            let mut rng = SeededRng::from_text(&c.seed);
            display::draw(
                &mut Fixture {
                    pen: Pen::new(Matrix(c.matrix), "furniture").unwrap(),
                    mesh: &mut mesh,
                    repeats: &|_| 1.7,
                },
                c.kind,
                c.trade,
                &mut rng,
                &display::Settings {
                    timber: &timber::Settings {
                        textured: c.textured,
                        timber_repeat: 1.7,
                        atlas_repeat: 1.7,
                        ..Default::default()
                    },
                    props: c.textured,
                    apples: c.textured,
                },
            )
            .unwrap();
            assert_eq!(rng.next_f64(), c.next);
            if !mesh.surveying() {
                assembly::check_triangles(
                    &mesh,
                    &c.geometry,
                    &format!("{:?} {:?}", c.kind, c.trade),
                );
            }
        }
    }
    for p in g.plans {
        let mut rng = SeededRng::from_text(&p.seed);
        let calls = std::cell::RefCell::new(vec![]);
        let plans = frontage::plan(p.width, &mut rng, &|a, b, d, h| {
            calls.borrow_mut().push([a, b, d, h]);
            Ok(match p.mode {
                0 => true,
                1 => false,
                2 => a >= p.width / 2.,
                3 => h < 1.,
                _ => a < p.width / 2.,
            })
        })
        .unwrap();
        reference::close(&serde_json::to_value(&plans).unwrap(), &p.plans);
        reference::close(&serde_json::to_value(calls.into_inner()).unwrap(), &p.calls);
        assert_eq!(rng.next_f64(), p.next);
        for plan in plans {
            assert!(plan.depth + 0.04 <= 0.65);
            assert!(
                plan.center[0] + plan.width / 2. <= p.width / 2. - 0.5 + 1e-10
                    || plan.center[0] - plan.width / 2. >= p.width / 2. + 0.5 - 1e-10
            );
        }
    }
}
#[test]
fn supported_displays_preserve_stock_and_clearance_queries() {
    check_reference_shop_display();
}
#[test]
fn failed_clearance_and_cancellation_do_not_emit() {
    assert!(
        frontage::plan(f64::NAN, &mut SeededRng::from_u32(1), &|_, _, _, _| Ok(
            true
        ))
        .is_err()
    );
    assert!(
        frontage::plan(3.2, &mut SeededRng::from_u32(1), &|_, _, _, _| Err(
            "cancelled".into()
        ))
        .is_err()
    );
    let mut mesh = Mesh::new(Mode::Render, 100000, &|| true).unwrap();
    assert!(
        display::draw(
            &mut Fixture {
                pen: Pen::new(Matrix::default(), "display").unwrap(),
                mesh: &mut mesh,
                repeats: &|_| 1.
            },
            display::Kind::Cart,
            display::Trade::Grocer,
            &mut SeededRng::from_u32(1),
            &display::Settings {
                timber: &Default::default(),
                props: true,
                apples: true
            }
        )
        .is_err()
    );
    assert_eq!(mesh.triangle_count(), 0);
}
