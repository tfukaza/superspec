use serde::Deserialize;
use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    balcony_props::{self, Garment, Plant, Room, Support},
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::Pen,
};
use superspec_geometry::{Matrix, V3};
#[path = "support/balcony_props_scene.rs"]
mod scene_fixture;
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
struct PlantCase {
    name: Plant,
    textured: bool,
    reflected: bool,
    at: V3,
    scale: f64,
    rotation: f64,
    #[serde(flatten)]
    geometry: Geometry,
}
#[derive(Deserialize)]
struct GarmentCase {
    seed: String,
    room: Room,
    support: Support,
    textured: bool,
    reflected: bool,
    garment: Option<Garment>,
    left: V3,
    right: V3,
    sway: f64,
    next: u32,
    #[serde(flatten)]
    geometry: Geometry,
}
#[derive(Deserialize)]
struct Small {
    support: Support,
    room: Room,
    next: u32,
}
#[derive(Deserialize)]
struct Golden {
    plants: Vec<PlantCase>,
    garments: Vec<GarmentCase>,
    small: Vec<Small>,
}
fn fixture() -> Golden {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/balcony-props.json"
    ))
    .unwrap()
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
pub fn check_reference_balcony_props() {
    let golden = fixture();
    for (i, c) in golden.plants.iter().enumerate() {
        for mode in [Mode::Render, Mode::Survey] {
            let mut mesh = Mesh::new(mode, 100000, &|| false).unwrap();
            balcony_props::plant(
                &mut Fixture {
                    pen: pen(c.reflected),
                    mesh: &mut mesh,
                    repeats: &|_| 1.,
                },
                c.name,
                c.at,
                c.scale,
                c.rotation,
                c.textured,
            )
            .unwrap();
            if mode == Mode::Render {
                check(&mesh, &c.geometry, &format!("plant {i}"));
            } else {
                assert_eq!(mesh.triangle_count() * 3, c.geometry.indices.len());
            }
        }
    }
    for (i, c) in golden.garments.iter().enumerate() {
        let mut rng = SeededRng::from_text(&c.seed);
        let garment = balcony_props::choose(&mut rng, c.room, c.support).unwrap();
        assert_eq!(
            serde_json::to_value(&garment).unwrap(),
            serde_json::to_value(&c.garment).unwrap()
        );
        assert_eq!(rng.next_u32(), c.next);
        for mode in [Mode::Render, Mode::Survey] {
            let mut mesh = Mesh::new(mode, 100000, &|| false).unwrap();
            if let Some(g) = &garment {
                balcony_props::hang(
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
    for c in golden.small {
        let mut rng = SeededRng::from_text("small");
        assert!(
            balcony_props::choose(&mut rng, c.room, c.support)
                .unwrap()
                .is_none()
        );
        assert_eq!(rng.next_u32(), c.next);
    }
}
#[test]
fn balcony_assets_and_fitting_rng_match_reference() {
    check_reference_balcony_props();
}
#[test]
fn balcony_props_deliver_shared_pbr_atlases_and_preserve_collision() {
    let scene = scene_fixture::scene();
    assert_eq!(scene.manifest.assets.len(), 6);
    assert_eq!(scene.manifest.colliders.len(), 1);
    for slot in ["balcony-plants", "balcony-cloth"] {
        assert_eq!(
            scene
                .manifest
                .meshes
                .iter()
                .filter(|m| m.material.as_deref() == Some(slot))
                .count(),
            3
        );
    }
}
#[test]
fn invalid_fits_are_rejected_without_geometry_or_rng_changes() {
    let g = fixture();
    let c = &g.garments[0];
    let mut garment = c.garment.clone().unwrap();
    for error in 0..3 {
        let before = garment.clone();
        match error {
            0 => garment.scale = f64::NAN,
            1 => garment.width += 1.,
            _ => garment.tint[0] = f64::INFINITY,
        }
        let mut mesh = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
        assert!(
            balcony_props::hang(
                &mut Fixture {
                    pen: pen(false),
                    mesh: &mut mesh,
                    repeats: &|_| 1.
                },
                &garment,
                c.left,
                c.right,
                0.,
                true
            )
            .is_err()
        );
        assert!(mesh.indices.is_empty());
        garment = before;
    }
    let mut rng = SeededRng::from_text("invalid");
    let mut unchanged = rng.clone();
    assert!(
        balcony_props::choose(
            &mut rng,
            Room {
                width: f64::NAN,
                drop: 1.
            },
            Support::Line
        )
        .is_err()
    );
    assert_eq!(rng.next_u32(), unchanged.next_u32());
    let mut mesh = Mesh::new(Mode::Render, 100000, &|| true).unwrap();
    assert!(
        balcony_props::plant(
            &mut Fixture {
                pen: pen(false),
                mesh: &mut mesh,
                repeats: &|_| 1.
            },
            Plant::RubberPlant,
            [0.; 3],
            1.,
            0.,
            true
        )
        .is_err()
    );
    assert!(mesh.indices.is_empty());
    let mut recovered = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
    balcony_props::hang(
        &mut Fixture {
            pen: pen(false),
            mesh: &mut recovered,
            repeats: &|_| 1.,
        },
        &garment,
        c.left,
        c.right,
        0.,
        true,
    )
    .unwrap();
    assert!(!recovered.indices.is_empty());
}
