use serde::Deserialize;
use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    laundry::{self, Room, Support},
    mesh::{Mesh, Mode},
    pen::Pen,
};
use superspec_geometry::{Matrix, V3};
#[derive(Deserialize)]
struct Geometry {
    indices: Vec<u32>,
    positions: Vec<f64>,
    normals: Vec<f64>,
    uv: Vec<f64>,
    colors: Vec<f64>,
    slots: Vec<String>,
}
#[derive(Deserialize)]
struct Fitted {
    kind: String,
    width: f64,
    drop: f64,
}
#[derive(Deserialize)]
struct Case {
    seed: String,
    room: Room,
    support: Support,
    textured: bool,
    reflected: bool,
    garment: Option<Fitted>,
    left: V3,
    right: V3,
    sway: f64,
    next: u32,
    #[serde(flatten)]
    geometry: Geometry,
}
#[derive(Deserialize)]
struct Row {
    seed: String,
    support: Support,
    textured: bool,
    reflected: bool,
    next: u32,
    #[serde(flatten)]
    geometry: Geometry,
}
#[derive(Deserialize)]
struct Golden {
    garments: Vec<Case>,
    rows: Vec<Row>,
}
fn pen(reflected: bool) -> Pen {
    Pen::new(
        if reflected {
            Matrix([[-1., 0., 0.], [0., -1., 0.], [0., 0., -1.], [4., 3., 5.]])
        } else {
            Matrix::default()
        },
        "prop",
    )
    .unwrap()
}
fn check(mesh: &Mesh<'_>, expected: &Geometry, label: &str) {
    assert_eq!(
        mesh.indices.len(),
        expected.indices.len(),
        "{label} triangle count"
    );
    assert_eq!(
        mesh.indices
            .iter()
            .zip(&expected.indices)
            .position(|(a, b)| a != b),
        None,
        "{label} indexed order"
    );
    for (a, b, name) in [
        (
            mesh.vertices
                .iter()
                .flat_map(|v| v.position)
                .collect::<Vec<_>>(),
            &expected.positions,
            "positions",
        ),
        (
            mesh.vertices.iter().flat_map(|v| v.normal).collect(),
            &expected.normals,
            "normals",
        ),
        (
            mesh.vertices.iter().flat_map(|v| v.color).collect(),
            &expected.colors,
            "colors",
        ),
        (
            mesh.vertices.iter().flat_map(|v| v.uv).collect(),
            &expected.uv,
            "uv",
        ),
    ] {
        assert_eq!(a.len(), b.len(), "{label} {name}");
        if let Some(i) = a.iter().zip(b).position(|(a, b)| (a - b).abs() > 2e-10) {
            panic!("{label} {name} {i}: {} != {}", a[i], b[i]);
        }
    }
    assert_eq!(
        mesh.triangle_surfaces
            .iter()
            .map(|i| &mesh.surfaces[*i].slot)
            .collect::<Vec<_>>(),
        expected.slots.iter().collect::<Vec<_>>()
    );
}

pub fn check_reference_laundry() {
    let golden: Golden =
        serde_json::from_str(include_str!("../../../tests/fixtures/rust/laundry.json")).unwrap();
    for (i, c) in golden.garments.iter().enumerate() {
        let mut rng = SeededRng::from_text(&c.seed);
        let garment = laundry::choose(&mut rng, c.room, c.support).unwrap();
        assert_eq!(rng.next_u32(), c.next, "garment {i} RNG");
        match (&garment, &c.garment) {
            (Some(a), Some(b)) => {
                assert_eq!(a.kind(), b.kind);
                assert!((a.width() - b.width).abs() < 1e-12);
                assert!((a.drop() - b.drop).abs() < 1e-12);
            }
            (None, None) => (),
            _ => panic!("garment {i} fit"),
        }
        for mode in [Mode::Render, Mode::Survey] {
            let mut mesh = Mesh::new(mode, 100000, &|| false).unwrap();
            if let Some(g) = &garment {
                laundry::hang(
                    &mut Fixture {
                        pen: pen(c.reflected),
                        mesh: &mut mesh,
                        repeats: &|_| 1.,
                    },
                    g,
                    c.left,
                    c.right,
                    c.sway,
                    c.textured,
                )
                .unwrap();
            }
            if mode == Mode::Render {
                check(&mesh, &c.geometry, &format!("garment {i}"));
            } else {
                assert_eq!(mesh.triangle_count() * 3, c.geometry.indices.len());
            }
        }
    }
    for (i, c) in golden.rows.iter().enumerate() {
        for mode in [Mode::Render, Mode::Survey] {
            let mut rng = SeededRng::from_text(&c.seed);
            let mut mesh = Mesh::new(mode, 100000, &|| false).unwrap();
            let mut draw = Fixture {
                pen: pen(c.reflected),
                mesh: &mut mesh,
                repeats: &|_| 1.,
            };
            if c.support == Support::Hanger {
                laundry::hangers(
                    &mut draw,
                    &laundry::HangerRow {
                        span: [-0.4, 2.4],
                        z: 2.5,
                        slope: 0.1,
                        reach: 0.6,
                        drop: 1.5,
                    },
                    &mut rng,
                    c.textured,
                )
                .unwrap();
            } else {
                laundry::washing(
                    &mut draw,
                    [-0.4, 2.4],
                    2.5,
                    &mut rng,
                    &laundry::WashingOptions {
                        fill: 0.85,
                        drop: 1.5,
                        support: c.support,
                    },
                    c.textured,
                )
                .unwrap();
            }
            assert_eq!(rng.next_u32(), c.next, "row {i} RNG");
            if mode == Mode::Render {
                check(&mesh, &c.geometry, &format!("row {i}"));
            } else {
                assert_eq!(mesh.triangle_count() * 3, c.geometry.indices.len());
            }
        }
    }
}
#[test]
fn fitted_laundry_and_rows_match_reference() {
    check_reference_laundry();
}
#[test]
fn invalid_laundry_preserves_rng_and_next_draw_recovers() {
    let mut rng = SeededRng::from_text("recovery");
    let mut untouched = rng.clone();
    assert!(
        laundry::choose(
            &mut rng,
            Room {
                width: f64::NAN,
                drop: 1.
            },
            Support::Line
        )
        .is_err()
    );
    assert_eq!(rng.next_u32(), untouched.next_u32());
    let garment = laundry::choose(
        &mut rng,
        Room {
            width: 1.5,
            drop: 1.6,
        },
        Support::Line,
    )
    .unwrap()
    .unwrap();
    let mut mesh = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
    let mut draw = Fixture {
        pen: pen(false),
        mesh: &mut mesh,
        repeats: &|_| 1.,
    };
    assert!(
        laundry::hang(
            &mut draw,
            &garment,
            [f64::NAN, 0., 0.],
            [1., 0., 0.],
            0.,
            true
        )
        .is_err()
    );
    assert_eq!(draw.mesh.triangle_count(), 0);
    laundry::hang(&mut draw, &garment, [0., 0., 2.], [1., 0., 2.], 0., true).unwrap();
    assert!(mesh.triangle_count() > 0);
    let mut cancelled = Mesh::new(Mode::Render, 100000, &|| true).unwrap();
    assert!(
        laundry::hang(
            &mut Fixture {
                pen: pen(false),
                mesh: &mut cancelled,
                repeats: &|_| 1.
            },
            &garment,
            [0., 0., 2.],
            [1., 0., 2.],
            0.,
            true
        )
        .is_err()
    );
}
