use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::stock_mesh,
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
fn baked_stock_surveys_the_triangles_it_renders_in_every_variant_and_reflection() {
    let at = [0.1, -0.2, 0.3];
    let tint = [0.7, 0.3, 0.2];
    for apple in [true, false] {
        for textured in [false, true] {
            for index in 0..4 {
                let f = index as f64;
                let size = [0.42 + 0.02 * f, 0.31 + 0.01 * f, 0.12 + 0.02 * f];
                let mut counts = vec![];
                for mode in [Mode::Render, Mode::Survey] {
                    let mut mesh = Mesh::new(mode, 30000, &|| false).unwrap();
                    let d = &mut Fixture {
                        pen: Pen::new(frame(index), "furniture").unwrap(),
                        mesh: &mut mesh,
                        repeats: &|_| 1.7,
                    };
                    if apple {
                        stock_mesh::apple_pile(d, at, size, tint, textured)
                    } else {
                        stock_mesh::snack_packet(d, at, [size[0], size[2]], tint, textured, index)
                    }
                    .unwrap();
                    assert!(mesh.triangle_count() > 0);
                    counts.push(mesh.triangle_count());
                }
                assert_eq!(
                    counts[0], counts[1],
                    "apple={apple} {index} textured={textured}"
                );
            }
        }
    }
}
#[test]
fn invalid_and_cancelled_stock_rejects_before_drawing_then_recovers() {
    let mut mesh = Mesh::new(Mode::Render, 30000, &|| false).unwrap();
    let d = &mut Fixture {
        pen: Pen::new(Matrix::default(), "stock").unwrap(),
        mesh: &mut mesh,
        repeats: &|_| 1.,
    };
    assert!(stock_mesh::snack_packet(d, [0.; 3], [0.1, 0.2], [1.; 3], true, 4).is_err());
    assert!(stock_mesh::apple_pile(d, [0.; 3], [0.4, 0., 0.1], [1.; 3], true).is_err());
    assert_eq!(d.mesh.triangle_count(), 0);
    let mut cancelled = Mesh::new(Mode::Render, 30000, &|| true).unwrap();
    assert!(
        stock_mesh::apple_pile(
            &mut Fixture {
                pen: d.pen.clone(),
                mesh: &mut cancelled,
                repeats: d.repeats
            },
            [0.; 3],
            [0.4, 0.3, 0.1],
            [1.; 3],
            true
        )
        .is_err()
    );
    assert_eq!(cancelled.triangle_count(), 0);
    stock_mesh::apple_pile(d, [0.; 3], [0.4, 0.3, 0.1], [1.; 3], true).unwrap();
    assert!(d.mesh.triangle_count() > 0);
}
