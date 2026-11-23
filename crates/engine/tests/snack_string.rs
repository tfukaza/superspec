use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::snack_string,
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
fn packet_poses_hang_from_their_clips_within_the_drop_and_displays_survey_like_render() {
    let at = [0.1, 0.2, 2.2];
    for i in 0..4 {
        let drop = [0.1, 0.52, 0.9, 1.5][i];
        let span = [0.03, [0.06, 0.4, 0.9, 1.4][i], 0.1, 2.2];
        let seed = format!("packets/{i}");
        let mirrored = frame(i).determinant() < 0.;
        let poses = snack_string::plan(drop, &mut SeededRng::from_text(&seed), mirrored).unwrap();
        for p in &poses {
            assert!(p.min[2] >= -drop, "{seed}");
            assert!((0..3).all(|axis| p.min[axis] <= p.max[axis]));
            let anchor = p.matrix.point(p.seam);
            for (a, b) in anchor.iter().zip(p.clip) {
                assert!((a - b).abs() < 1e-12, "{seed} seam must meet its clip");
            }
        }
        for string in [true, false] {
            for textured in [false, true] {
                let mut outcomes = vec![];
                for mode in [Mode::Render, Mode::Survey] {
                    let mut mesh = Mesh::new(mode, 100000, &|| false).unwrap();
                    let mut rng = SeededRng::from_text(&seed);
                    let mut d = Fixture {
                        pen: Pen::new(frame(i), "furniture").unwrap(),
                        mesh: &mut mesh,
                        repeats: &|_| 1.7,
                    };
                    if string {
                        snack_string::string(&mut d, at, drop, &mut rng, textured).unwrap();
                    } else {
                        snack_string::row(&mut d, span, &mut rng, textured).unwrap();
                    }
                    outcomes.push((mesh.triangle_count(), rng.next_f64()));
                }
                assert_eq!(outcomes[0], outcomes[1], "{seed} string={string}");
            }
        }
    }
}
#[test]
fn invalid_and_cancelled_displays_preserve_rng_and_recover() {
    let mut rng = SeededRng::from_text("packets");
    let mut expected = rng.clone();
    assert!(snack_string::plan(f64::NAN, &mut rng, false).is_err());
    assert!(snack_string::plan(11., &mut rng, false).is_err());
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| true).unwrap();
    assert!(
        snack_string::string(
            &mut Fixture {
                pen: Pen::new(Matrix::default(), "packet").unwrap(),
                mesh: &mut mesh,
                repeats: &|_| 1.
            },
            [0.; 3],
            0.8,
            &mut rng,
            true
        )
        .is_err()
    );
    assert_eq!(mesh.triangle_count(), 0);
    assert_eq!(rng.next_f64(), expected.next_f64());
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
    let mut d = Fixture {
        pen: Pen::new(Matrix::default(), "packet").unwrap(),
        mesh: &mut mesh,
        repeats: &|_| 1.,
    };
    assert!(snack_string::row(&mut d, [1., 0., 0., 0.], &mut rng, true).is_err());
    snack_string::string(&mut d, [0.; 3], 0.8, &mut rng, true).unwrap();
    assert!(mesh.triangle_count() > 0);
}
