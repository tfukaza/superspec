use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::{Pen, Tint},
    shop::forms::{self, Ball, Tints},
};
use superspec_geometry::{Matrix, V3};
enum Input {
    Lathe {
        base: V3,
        profile: Vec<[f64; 2]>,
        sides: usize,
        texture_repeat: Option<f64>,
        tints: Vec<V3>,
    },
    Limb {
        path: Vec<V3>,
        radii: Vec<f64>,
        sides: usize,
        flatten: f64,
    },
    Ball(Ball),
    Front {
        span: [f64; 4],
        y: f64,
    },
}
struct Shape {
    input: Input,
    field: bool,
}
fn draw(d: &mut Fixture<'_, '_>, s: &Shape) -> Result<(), String> {
    let field = |p: V3| Ok([0.5 + p[0] * 0.1, 0.7 + p[1] * 0.1, 0.3 + p[2] * 0.1]);
    let tint = if s.field {
        Tint::Field(&field)
    } else {
        Tint::Color([0.7, 0.6, 0.5])
    };
    match &s.input {
        Input::Lathe {
            base,
            profile,
            sides,
            texture_repeat,
            tints,
        } => {
            let bands: Vec<_> = tints.iter().copied().map(Tint::Color).collect();
            let shades = if s.field {
                Tints::Uniform(&tint)
            } else {
                Tints::Bands(&bands)
            };
            forms::lathe(d, "paint", *base, profile, &shades, *sides, *texture_repeat)
        }
        Input::Limb {
            path,
            radii,
            sides,
            flatten,
        } => forms::limb(d, "paint", path, radii, &tint, *sides, *flatten),
        Input::Ball(shape) => forms::ball(d, "paint", shape, &tint),
        Input::Front { span, y } => forms::front(d, "paint", *span, *y, [0.7, 0.6, 0.5]),
    }
}
#[test]
fn rotated_and_reflected_forms_survey_the_triangles_they_render() {
    let shapes = || {
        [
            Input::Lathe {
                base: [0.31, -0.22, 0.17],
                profile: vec![[0.25, 0.], [0.25, 0.6]],
                sides: 6,
                texture_repeat: None,
                tints: vec![[0.4, 0.5, 0.6], [0.8, 0.7, 0.5]],
            },
            Input::Lathe {
                base: [0.31, -0.22, 0.17],
                profile: vec![[0.1, 0.], [0.25, 0.2], [0.18, 0.5], [0.05, 0.6]],
                sides: 9,
                texture_repeat: Some(1.3),
                tints: vec![[0.4, 0.5, 0.6], [0.8, 0.7, 0.5], [0.2, 0.3, 0.1]],
            },
            Input::Limb {
                path: vec![[0., 0., 0.], [0., 0., 0.5]],
                radii: vec![0.2, 0.1],
                sides: 6,
                flatten: 1.,
            },
            Input::Limb {
                path: vec![[0., 0., 0.], [0.1, 0.05, 0.3], [0.15, 0.2, 0.6]],
                radii: vec![0.08, 0.06, 0.03],
                sides: 7,
                flatten: 0.6,
            },
            Input::Ball(Ball {
                center: [0.3, -0.2, 0.7],
                radius: 0.21,
                stretch: [1., 1., 1.],
                bands: 2,
                sides: 7,
            }),
            Input::Ball(Ball {
                center: [0.3, -0.2, 0.7],
                radius: 0.15,
                stretch: [1.4, 0.8, 1.1],
                bands: 5,
                sides: 10,
            }),
            Input::Front {
                span: [-0.2, 0.5, 0.1, 0.9],
                y: -0.13,
            },
        ]
    };
    let rotations = [(0, 0.31), (1, -0.6), (2, 0.5)];
    for mirror in [1., -1.] {
        let matrix = Matrix([[mirror, 0., 0.], [0., 1., 0.], [0., 0., 1.], [2., -1., 0.4]]);
        for (axis, angle) in rotations {
            for field in [false, true] {
                for (i, input) in shapes().into_iter().enumerate() {
                    let shape = Shape { input, field };
                    let mut counts = vec![];
                    for mode in [Mode::Render, Mode::Survey] {
                        let mut mesh = Mesh::new(mode, 30000, &|| false).unwrap();
                        let pen = Pen::new(matrix, "stock").unwrap();
                        let pen = forms::rotated(&pen, [0.2, 0.3, 0.1], axis, angle).unwrap();
                        draw(
                            &mut Fixture {
                                pen,
                                mesh: &mut mesh,
                                repeats: &|_| 1.7,
                            },
                            &shape,
                        )
                        .unwrap();
                        assert!(mesh.triangle_count() > 0, "form {i}");
                        counts.push(mesh.triangle_count());
                    }
                    assert_eq!(counts[0], counts[1], "form {i} axis {axis} mirror {mirror}");
                }
            }
        }
    }
}
#[test]
fn invalid_exhausted_and_cancelled_shapes_reject_then_valid_draws_recover() {
    let tint = Tint::Color([1.; 3]);
    let mut shape = Ball {
        center: [0.; 3],
        radius: 0.3,
        stretch: [1.; 3],
        sides: 8,
        bands: 4,
    };
    let mut mesh = Mesh::new(Mode::Render, 30000, &|| false).unwrap();
    let d = &mut Fixture {
        pen: Pen::new(Matrix::default(), "stock").unwrap(),
        mesh: &mut mesh,
        repeats: &|_| 1.,
    };
    shape.stretch[0] = 0.;
    assert!(forms::ball(d, "paint", &shape, &tint).is_err());
    shape.stretch[0] = 1.;
    assert!(
        forms::lathe(
            d,
            "paint",
            [0.; 3],
            &[[0.2, 0.], [0.3, 0.4]],
            &Tints::Uniform(&tint),
            8,
            Some(f64::NAN)
        )
        .is_err()
    );
    assert!(forms::limb(d, "paint", &[[0.; 3], [0.; 3]], &[0.1, 0.1], &tint, 6, 1.).is_err());
    assert!(forms::limb(d, "paint", &[[0.; 3], [0., 0., 1.]], &[0.1], &tint, 6, 1.).is_err());
    assert_eq!(d.mesh.triangle_count(), 0);
    forms::ball(d, "paint", &shape, &tint).unwrap();
    assert!(d.mesh.triangle_count() > 0);
    for cancelled in [false, true] {
        let callback = || cancelled;
        let mut mesh = Mesh::new(Mode::Render, 8, &callback).unwrap();
        assert!(
            forms::ball(
                &mut Fixture {
                    pen: Pen::new(Matrix::default(), "stock").unwrap(),
                    mesh: &mut mesh,
                    repeats: &|_| 1.
                },
                "paint",
                &shape,
                &tint
            )
            .is_err()
        );
        assert!(mesh.triangle_count() <= 8);
        if cancelled {
            assert_eq!(mesh.triangle_count(), 0);
        }
    }
    forms::ball(d, "paint", &shape, &tint).unwrap();
}
