use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::{display, frontage},
};
use superspec_geometry::Matrix;
/// Whatever clearance the street grants, planned displays stay shallow and beside the doorway.
pub fn check_frontage_plans() {
    let mut planned = 0;
    for mode in 0..5 {
        for (i, width) in [0.9, 1.8, 2.2, 2.8, 3.2, 3.6, 4.2, 5.].into_iter().enumerate() {
            let mut rng = SeededRng::from_text(&format!("frontage/{}", mode * 8 + i));
            let plans = frontage::plan(width, &mut rng, &|a, _, _, h| {
                Ok(match mode {
                    0 => true,
                    1 => false,
                    2 => a >= width / 2.,
                    3 => h < 1.,
                    _ => a < width / 2.,
                })
            })
            .unwrap();
            planned += plans.len();
            for plan in plans {
                assert!(plan.depth + 0.04 <= 0.65);
                assert!(
                    plan.center[0] + plan.width / 2. <= width / 2. - 0.5 + 1e-10
                        || plan.center[0] - plan.width / 2. >= width / 2. + 0.5 - 1e-10
                );
            }
        }
    }
    assert!(planned > 0, "open frontages must plan displays");
}
#[test]
fn planned_displays_stay_shallow_and_clear_of_the_doorway() {
    check_frontage_plans();
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
