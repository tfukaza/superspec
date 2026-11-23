use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::restaurant_food,
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
fn restaurant_food_surveys_the_triangles_it_renders_in_every_variant_and_reflection() {
    let at = [0.1, 0.2, 0.3];
    for chicken in [true, false] {
        for textured in [false, true] {
            for i in 0..3 {
                let mut counts = vec![];
                for mode in [Mode::Render, Mode::Survey] {
                    let mut mesh = Mesh::new(mode, 100000, &|| false).unwrap();
                    let d = &mut Fixture {
                        pen: Pen::new(frame(i), "furniture").unwrap(),
                        mesh: &mut mesh,
                        repeats: &|_| 1.7,
                    };
                    if chicken {
                        let scale = [0.8, 0.9, 1.][i];
                        restaurant_food::hanging_chicken(d, at, scale, i, textured).unwrap();
                    } else {
                        restaurant_food::sliced_meat(d, at, [0.3, 0.2, 0.075], textured).unwrap();
                    }
                    assert!(mesh.triangle_count() > 0);
                    counts.push(mesh.triangle_count());
                }
                assert_eq!(
                    counts[0], counts[1],
                    "chicken={chicken} {i} textured={textured}"
                );
            }
        }
    }
}
#[test]
fn food_rejects_invalid_variants_and_envelopes_cancels_and_recovers() {
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
    let d = &mut Fixture {
        pen: Pen::new(Matrix::default(), "food").unwrap(),
        mesh: &mut mesh,
        repeats: &|_| 1.,
    };
    assert!(restaurant_food::hanging_chicken(d, [0.; 3], 1., 3, true).is_err());
    assert!(restaurant_food::sliced_meat(d, [0.; 3], [1., f64::NAN, 1.], true).is_err());
    assert_eq!(mesh.triangle_count(), 0);
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| true).unwrap();
    assert!(
        restaurant_food::hanging_chicken(
            &mut Fixture {
                pen: Pen::new(Matrix::default(), "food").unwrap(),
                mesh: &mut mesh,
                repeats: &|_| 1.
            },
            [0.; 3],
            1.,
            0,
            true
        )
        .is_err()
    );
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
    restaurant_food::hanging_chicken(
        &mut Fixture {
            pen: Pen::new(Matrix::default(), "food").unwrap(),
            mesh: &mut mesh,
            repeats: &|_| 1.,
        },
        [0.; 3],
        1.,
        0,
        true,
    )
    .unwrap();
    assert!(mesh.triangle_count() > 0);
}
