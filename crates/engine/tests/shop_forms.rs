use serde::Deserialize;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::{Pen, Tint},
    shop::forms::{self, Ball, Tints},
};
use superspec_geometry::{Matrix, V3};
#[path = "support/assembly_reference.rs"]
mod assembly;
#[allow(dead_code)]
#[path = "support/sign_reference.rs"]
mod reference;
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Input {
    Lathe {
        base: V3,
        profile: Vec<[f64; 2]>,
        sides: usize,
        #[serde(rename = "textureRepeat")]
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
#[derive(Deserialize)]
struct Shape {
    #[serde(flatten)]
    input: Input,
    field: bool,
}
#[derive(Deserialize)]
struct Rotation {
    origin: V3,
    axis: usize,
    angle: f64,
}
#[derive(Deserialize)]
struct Case {
    input: Shape,
    rotation: Rotation,
    matrix: [V3; 4],
    geometry: assembly::Geometry,
}
#[derive(Deserialize)]
struct Golden {
    cases: Vec<Case>,
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
pub fn check_reference_shop_forms() {
    let golden: Golden =
        serde_json::from_str(include_str!("../../../tests/fixtures/rust/shop-forms.json")).unwrap();
    for (i, c) in golden.cases.iter().enumerate() {
        for mode in [Mode::Render, Mode::Survey] {
            let mut mesh = Mesh::new(mode, 30000, &|| false).unwrap();
            let pen = Pen::new(Matrix(c.matrix), "stock").unwrap();
            let pen =
                forms::rotated(&pen, c.rotation.origin, c.rotation.axis, c.rotation.angle).unwrap();
            draw(
                &mut Fixture {
                    pen,
                    mesh: &mut mesh,
                    repeats: &|_| 1.7,
                },
                &c.input,
            )
            .unwrap();
            assert_eq!(
                mesh.triangle_count() * 3,
                c.geometry.indices.len(),
                "form {i}"
            );
            if !mesh.surveying() {
                assembly::check(&mut mesh, &c.geometry, &[], &format!("form {i}"));
            }
        }
    }
}
#[test]
fn smooth_shop_forms_preserve_geometry_charts_colors_and_reflected_survey() {
    check_reference_shop_forms();
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
