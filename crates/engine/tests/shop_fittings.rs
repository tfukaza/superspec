use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::fittings,
};
use superspec_geometry::{Matrix, V3};
fn frame(index: usize) -> Matrix {
    let (c, s) = (0.9523335698857134, 0.3050586364434435);
    let x = if index % 2 == 0 {
        [c, s, 0.]
    } else {
        [-c, -s, 0.]
    };
    Matrix([x, [-s, c, 0.], [0., 0., 1.], [0.3, -0.2, 0.4]])
}
#[test]
fn fittings_survey_like_render_consume_the_same_randomness_and_hooks_hang_below_anchor() {
    const KINDS: [&str; 12] = [
        "bottleCrate",
        "rail",
        "hook",
        "pendant",
        "tubeLight",
        "ceilingFan",
        "stool",
        "priceCard",
        "counterScale",
        "hangingScale",
        "calendar",
        "wallClock",
    ];
    let at: V3 = [0.21, -0.12, 0.6];
    let tint: V3 = [0.36, 0.28, 0.17];
    for kind in KINDS {
        for textured in [false, true] {
            for index in 0..3 {
                let mut outcomes = vec![];
                for mode in [Mode::Render, Mode::Survey] {
                    let mut mesh = Mesh::new(mode, 100000, &|| false).unwrap();
                    let d = &mut Fixture {
                        pen: Pen::new(frame(index), "furniture").unwrap(),
                        mesh: &mut mesh,
                        repeats: &|_| 1.7,
                    };
                    let mut rng = SeededRng::from_text(&format!("furniture-{index}"));
                    let i = index as f64;
                    let mut result = None;
                    match kind {
                        "bottleCrate" => fittings::bottle_crate(
                            d,
                            tint,
                            [0.1, 0.3, 0.15],
                            [0.8, 0.3, 0.1],
                            index != 0,
                        ),
                        "rail" => fittings::rail(d, at, [1.2, 0.3, 0.7], 1.3, tint),
                        "hook" => {
                            fittings::hook(d, at, 0.04 + i * 0.04, tint).map(|p| result = Some(p))
                        }
                        "pendant" => fittings::pendant(
                            d,
                            at,
                            0.3 + i * 0.2,
                            if index == 1 { None } else { Some(tint) },
                        ),
                        "tubeLight" => fittings::tube_light(d, at, [1.2, at[1], at[2]]),
                        "ceilingFan" => fittings::ceiling_fan(d, at, 0.28, 0.8 + i * 0.2, &mut rng),
                        "stool" => fittings::stool(d, at, tint),
                        "priceCard" => fittings::price_card(d, at, &mut rng),
                        "counterScale" => fittings::counter_scale(d, at, tint),
                        "hangingScale" => fittings::hanging_scale(d, at, 0.7 + i * 0.2, tint),
                        "calendar" => fittings::calendar(d, at, &mut rng, textured),
                        _ => fittings::wall_clock(d, at, &mut rng),
                    }
                    .unwrap();
                    if kind == "hook" {
                        // The hook returns its hanging point, the anchor lowered by the drop.
                        let p = result.unwrap();
                        let want = [at[0], at[1], at[2] - (0.04 + i * 0.04)];
                        assert!(p.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-12));
                    } else {
                        assert!(result.is_none());
                    }
                    outcomes.push((mesh.triangle_count(), rng.next_f64(), result));
                }
                assert_eq!(
                    outcomes[0], outcomes[1],
                    "{kind} {index} textured={textured}"
                );
            }
        }
    }
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
