use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::produce,
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
fn produce_surveys_like_render_with_the_same_randomness() {
    const KINDS: [&str; 22] = [
        "orange",
        "apple",
        "greenApple",
        "pear",
        "tomato",
        "lemon",
        "pomelo",
        "plum",
        "potato",
        "onion",
        "egg",
        "heap",
        "melon",
        "melonHalf",
        "bananas",
        "pineapple",
        "cabbage",
        "greens",
        "root",
        "garlicString",
        "sausages",
        "saltedFish",
    ];
    let at = [0.1, 0.2, 0.3];
    for kind in KINDS {
        for i in 0..3 {
            let radius = [0.08, 0.095, 0.11][i];
            let height = [0.3, 0.35, 0.4][i];
            let mut outcomes = vec![];
            for mode in [Mode::Render, Mode::Survey] {
                let mut mesh = Mesh::new(mode, 100000, &|| false).unwrap();
                let d = &mut Fixture {
                    pen: Pen::new(frame(i), "furniture").unwrap(),
                    mesh: &mut mesh,
                    repeats: &|_| 1.7,
                };
                let mut rng = SeededRng::from_text(&format!("produce/{i}"));
                match kind {
                    "heap" => {
                        let kind = produce::fruit_kind("lemon").unwrap();
                        produce::heap(d, at, [0.2, 0.14], 0.028, 3, &mut rng, |d, at, rng| {
                            produce::fruit(d, at, &kind, rng)
                        })
                    }
                    "melon" => produce::melon(d, at, height, &mut rng),
                    "melonHalf" => produce::melon_half(d, at, radius),
                    "bananas" => produce::bananas(d, at, &mut rng),
                    "pineapple" => produce::pineapple(d, at, &mut rng),
                    "cabbage" => produce::cabbage(d, at, radius, &mut rng),
                    "greens" => produce::greens(d, at, height, &mut rng),
                    "root" => produce::root(d, at, height, i % 2 == 1, &mut rng),
                    "garlicString" => produce::garlic_string(d, at, height, &mut rng),
                    "sausages" => produce::sausages(d, at, &mut rng),
                    "saltedFish" => produce::salted_fish(d, at, height, &mut rng),
                    _ => produce::fruit(d, at, &produce::fruit_kind(kind).unwrap(), &mut rng),
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
fn produce_rejects_invalid_input_bounds_empty_heap_work_and_recovers_after_cancellation() {
    let mut rng = SeededRng::from_text("produce");
    let mut expected = rng.clone();
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
    let d = &mut Fixture {
        pen: Pen::new(Matrix::default(), "produce").unwrap(),
        mesh: &mut mesh,
        repeats: &|_| 1.,
    };
    assert!(produce::root(d, [0.; 3], -1., true, &mut rng).is_err());
    let mut invalid = produce::fruit_kind("apple").unwrap();
    invalid.colors.clear();
    assert!(produce::fruit(d, [0.; 3], &invalid, &mut rng).is_err());
    assert_eq!(rng.next_f64(), expected.next_f64());
    assert_eq!(mesh.triangle_count(), 0);
    let mut mesh = Mesh::new(Mode::Render, 100, &|| false).unwrap();
    let d = &mut Fixture {
        pen: Pen::new(Matrix::default(), "produce").unwrap(),
        mesh: &mut mesh,
        repeats: &|_| 1.,
    };
    assert!(produce::heap(d, [0.; 3], [10., 10.], 0.001, 1, &mut rng, |_, _, _| Ok(())).is_err());
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| true).unwrap();
    let d = &mut Fixture {
        pen: Pen::new(Matrix::default(), "produce").unwrap(),
        mesh: &mut mesh,
        repeats: &|_| 1.,
    };
    assert!(produce::bananas(d, [0.; 3], &mut rng).is_err());
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
    let d = &mut Fixture {
        pen: Pen::new(Matrix::default(), "produce").unwrap(),
        mesh: &mut mesh,
        repeats: &|_| 1.,
    };
    produce::bananas(d, [0.; 3], &mut rng).unwrap();
    assert!(mesh.triangle_count() > 0);
}
