use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::wares,
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
fn wares_survey_like_render_with_the_same_randomness() {
    const KINDS: [&str; 15] = [
        "wok",
        "hangingWok",
        "pot",
        "kettle",
        "bucket",
        "basin",
        "basket",
        "clayPot",
        "stoolStack",
        "broom",
        "mop",
        "duster",
        "coil",
        "ladles",
        "ladder",
    ];
    let at = [0.1, 0.2, 0.3];
    let top = [0.3, 0.2, 1.5];
    let tint = [0.36, 0.28, 0.17];
    for kind in KINDS {
        for i in 0..4 {
            let f = i as f64;
            let radius = 0.12 + 0.035 * f;
            let height = 0.2 + 0.05 * f;
            let count = i + 1;
            let mut outcomes = vec![];
            for mode in [Mode::Render, Mode::Survey] {
                let mut mesh = Mesh::new(mode, 100000, &|| false).unwrap();
                let d = &mut Fixture {
                    pen: Pen::new(frame(i), "furniture").unwrap(),
                    mesh: &mut mesh,
                    repeats: &|_| 1.7,
                };
                let mut rng = SeededRng::from_text(&format!("wares/{i}"));
                match kind {
                    "wok" => wares::wok(d, at, radius, tint),
                    "hangingWok" => wares::hanging_wok(d, at, radius, tint, i % 2 == 1),
                    "pot" => wares::pot(
                        d,
                        at,
                        radius,
                        height,
                        tint,
                        (i % 2 == 1).then_some([0.12, 0.19, 0.08]),
                    ),
                    "kettle" => wares::kettle(d, at, radius, tint),
                    "bucket" => wares::bucket(d, at, radius, height, tint),
                    "basin" => wares::basin(d, at, radius, tint),
                    "basket" => wares::basket(d, at, radius, height, tint),
                    "clayPot" => wares::clay_pot(d, at, radius, &mut rng),
                    "stoolStack" => wares::stool_stack(d, at, count, &[tint, [0.1, 0.2, 0.3]]),
                    "broom" => wares::broom(d, at, top, &mut rng),
                    "mop" => wares::mop(d, at, top, &mut rng),
                    "duster" => wares::duster(d, at, &mut rng),
                    "coil" => wares::coil(d, at, radius, 0.012, count, tint),
                    "ladles" => wares::ladles(d, at, &mut rng),
                    _ => wares::ladder(d, at, top, 0.42, &mut rng),
                }
                .unwrap();
                assert!(mesh.triangle_count() > 0, "{kind} {i}");
                outcomes.push((mesh.triangle_count(), rng.next_f64()));
            }
            assert_eq!(outcomes[0], outcomes[1], "{kind} {i}");
        }
    }
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
