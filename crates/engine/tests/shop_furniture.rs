use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::furniture::{self, Finish},
    timber,
};
use superspec_geometry::Matrix;
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
fn furniture_reports_the_same_stock_surfaces_inside_its_footprint_in_render_and_survey() {
    let tint = [0.36, 0.28, 0.17];
    for kind in [
        "shelving-timber",
        "shelving-metal",
        "counter-timber",
        "counter-tiled",
        "counter-steel",
        "crate",
    ] {
        for textured in [false, true] {
            for index in 0..3 {
                let size = if kind == "crate" {
                    [[0.4, 0.3, 0.24], [0.55, 0.34, 0.31], [0.7, 0.38, 0.38]][index]
                } else if kind.starts_with("counter") {
                    [[1.2, 0.3, 0.8], [1.9, 0.45, 0.92], [2.6, 0.6, 1.04]][index]
                } else {
                    [[1.2, 0.3, 1.7], [1.9, 0.45, 1.8], [2.6, 0.6, 1.9]][index]
                };
                let levels = 3 + index;
                let above = if index == 1 { 0. } else { 0.35 };
                let mut outcomes = vec![];
                for mode in [Mode::Render, Mode::Survey] {
                    let mut mesh = Mesh::new(mode, 100000, &|| false).unwrap();
                    let mut rng = SeededRng::from_text(&format!("furniture-{index}"));
                    let timber = timber::Settings {
                        textured,
                        timber_repeat: 1.7,
                        atlas_repeat: 1.7,
                        ..Default::default()
                    };
                    let d = &mut Fixture {
                        pen: Pen::new(frame(index), "furniture").unwrap(),
                        mesh: &mut mesh,
                        repeats: &|_| 1.7,
                    };
                    let surfaces = match kind {
                        "shelving-timber" | "shelving-metal" => furniture::shelving(
                            d,
                            size,
                            levels,
                            kind.strip_prefix("shelving-").unwrap(),
                            tint,
                            above,
                            &timber,
                        )
                        .unwrap(),
                        "crate" => {
                            vec![furniture::crate_box(d, size, tint, &mut rng, &timber).unwrap()]
                        }
                        _ => vec![
                            furniture::counter(
                                d,
                                size,
                                match kind {
                                    "counter-timber" => Finish::Timber,
                                    "counter-tiled" => Finish::Tiled,
                                    _ => Finish::Steel,
                                },
                                tint,
                                &mut rng,
                                &timber,
                            )
                            .unwrap(),
                        ],
                    };
                    // Survey skips finish-driven timber splits, so only emission is compared.
                    assert!(mesh.triangle_count() > 0, "{kind} {index}");
                    assert!(!surfaces.is_empty());
                    for s in &surfaces {
                        assert!(
                            0. <= s.x0 && s.x0 < s.x1 && s.x1 <= size[0],
                            "{kind} {index}"
                        );
                        assert!(0. <= s.y && s.depth > 0. && s.y + s.depth <= size[1]);
                        assert!(0. < s.z && s.z <= size[2] && s.clear > 0.);
                    }
                    outcomes.push((serde_json::json!(surfaces), rng.next_f64()));
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
