use serde::Deserialize;
use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::snack_string,
};
use superspec_geometry::{Matrix, V3};
#[path = "support/assembly_reference.rs"]
mod assembly;
#[allow(dead_code)]
#[path = "support/sign_reference.rs"]
mod reference;
#[derive(Deserialize)]
struct Pose {
    matrix: [V3; 4],
    clip: V3,
    seam: V3,
    min: V3,
    max: V3,
}
#[derive(Deserialize)]
struct Case {
    kind: String,
    drop: f64,
    span: [f64; 4],
    at: V3,
    seed: String,
    textured: bool,
    poses: Vec<Pose>,
    next: f64,
    matrix: [V3; 4],
    geometry: assembly::Geometry,
}
#[derive(Deserialize)]
struct Golden {
    cases: Vec<Case>,
}
pub fn check_reference_snack_strings() {
    let g: Golden = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/snack-string.json"
    ))
    .unwrap();
    for c in g.cases {
        let mirrored = Matrix(c.matrix).determinant() < 0.;
        let poses =
            snack_string::plan(c.drop, &mut SeededRng::from_text(&c.seed), mirrored).unwrap();
        assert_eq!(poses.len(), c.poses.len());
        for (p, expected) in poses.iter().zip(&c.poses) {
            for (a, b) in p
                .matrix
                .0
                .iter()
                .chain([&p.clip, &p.seam, &p.min, &p.max])
                .flatten()
                .zip(
                    expected
                        .matrix
                        .iter()
                        .chain([&expected.clip, &expected.seam, &expected.min, &expected.max])
                        .flatten(),
                )
            {
                assert!((a - b).abs() < 1e-12, "{} {} != {}", c.seed, a, b);
            }
            assert!(p.min[2] >= -c.drop);
            let anchor = p.matrix.point(p.seam);
            for (a, b) in anchor.iter().zip(p.clip) {
                assert!((a - b).abs() < 1e-12);
            }
        }
        for mode in [Mode::Render, Mode::Survey] {
            let mut mesh = Mesh::new(mode, 100000, &|| false).unwrap();
            let mut rng = SeededRng::from_text(&c.seed);
            let mut d = Fixture {
                pen: Pen::new(Matrix(c.matrix), "furniture").unwrap(),
                mesh: &mut mesh,
                repeats: &|_| 1.7,
            };
            if c.kind == "string" {
                snack_string::string(&mut d, c.at, c.drop, &mut rng, c.textured).unwrap();
            } else {
                snack_string::row(&mut d, c.span, &mut rng, c.textured).unwrap();
            }
            assert_eq!(rng.next_f64(), c.next, "{} {}", c.kind, c.seed);
            assert_eq!(mesh.triangle_count() * 3, c.geometry.indices.len());
            if !mesh.surveying() {
                assembly::check(
                    &mut mesh,
                    &c.geometry,
                    &[],
                    &format!("{} {} textured={}", c.kind, c.seed, c.textured),
                );
            }
        }
    }
}
#[test]
fn packet_displays_preserve_real_bounds_attachments_reflected_labels_and_random_streams() {
    check_reference_snack_strings();
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
