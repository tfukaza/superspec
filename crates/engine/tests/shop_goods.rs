use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::goods,
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
fn goods_lines_have_positive_sizes_and_survey_like_render_with_the_same_randomness() {
    const KINDS: [&str; 13] = [
        "tins",
        "bottles",
        "jars",
        "bigJars",
        "cartons",
        "packets",
        "rolls",
        "thermoses",
        "mugs",
        "bowls",
        "paintTins",
        "ceramicJars",
        "sack",
    ];
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
                    let mut rng = SeededRng::from_text(&format!("goods/{index}"));
                    if kind == "sack" {
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
                        let kind = serde_json::from_value(serde_json::json!(kind)).unwrap();
                        let mut line = goods::line(kind, &mut rng, textured).unwrap();
                        assert!(line.width > 0. && line.depth > 0. && line.height > 0.);
                        assert!(line.stack >= 1);
                        (line.draw)(d, [0.1, 0.2, 0.3]).unwrap();
                        // A repeated placement stacked on the first draws again.
                        (line.draw)(d, [0.4, 0.2, 0.3 + line.height]).unwrap();
                    }
                    assert!(mesh.triangle_count() > 0, "{kind} {index}");
                    outcomes.push((mesh.triangle_count(), rng.next_f64()));
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
