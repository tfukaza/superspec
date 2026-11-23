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
const LEFT: V3 = [-0.4, 0.2, 2.4];
const RIGHT: V3 = [1.1, 0.8, 2.7];
fn linen() -> Garment {
    balcony_props::choose(
        &mut SeededRng::from_text("linen/1"),
        Room {
            width: 1.5,
            drop: 1.8,
        },
        Support::Line,
    )
    .unwrap()
    .unwrap()
}
fn hang(mode: Mode, garment: &Garment, reflected: bool, textured: bool) -> usize {
    let mut mesh = Mesh::new(mode, 100000, &|| false).unwrap();
    balcony_props::hang(
        &mut Fixture {
            pen: pen(reflected),
            mesh: &mut mesh,
            repeats: &|_| 1.,
        },
        garment,
        LEFT,
        RIGHT,
        0.05,
        textured,
    )
    .unwrap();
    mesh.triangle_count()
}
#[test]
fn chosen_garments_fit_their_room_and_tiny_rooms_hang_nothing() {
    for support in [Support::Line, Support::Pole, Support::Hanger] {
        for i in 0..16 {
            let room = Room {
                width: 0.6 + 0.1 * i as f64,
                drop: 0.8 + 0.1 * i as f64,
            };
            let mut rng = SeededRng::from_text(&format!("fit/{i}"));
            if let Some(g) = balcony_props::choose(&mut rng, room, support).unwrap() {
                assert!(
                    g.width <= room.width + 1e-12,
                    "width {} > {}",
                    g.width,
                    room.width
                );
                assert!(
                    g.drop <= room.drop + 1e-12,
                    "drop {} > {}",
                    g.drop,
                    room.drop
                );
                assert!(g.scale > 0.);
            }
        }
        assert!(
            balcony_props::choose(
                &mut SeededRng::from_text("small"),
                Room {
                    width: 0.05,
                    drop: 0.1
                },
                support
            )
            .unwrap()
            .is_none()
        );
    }
}
#[test]
fn plants_and_garments_survey_like_render_and_mirror_without_changing_topology() {
    for plant in [Plant::RubberPlant, Plant::TrailingPothos, Plant::SnakePlant] {
        for textured in [false, true] {
            let count = |mode, reflected| {
                let mut mesh = Mesh::new(mode, 100000, &|| false).unwrap();
                balcony_props::plant(
                    &mut Fixture {
                        pen: pen(reflected),
                        mesh: &mut mesh,
                        repeats: &|_| 1.,
                    },
                    plant,
                    [0.3, -0.4, 0.7],
                    0.8,
                    0.37,
                    textured,
                )
                .unwrap();
                mesh.triangle_count()
            };
            let render = count(Mode::Render, false);
            assert!(render > 0);
            assert_eq!(count(Mode::Survey, false), render);
            assert_eq!(count(Mode::Render, true), render);
        }
    }
    let garment = linen();
    for textured in [false, true] {
        let render = hang(Mode::Render, &garment, false, textured);
        assert!(render > 0);
        assert_eq!(hang(Mode::Survey, &garment, false, textured), render);
        assert_eq!(hang(Mode::Render, &garment, true, textured), render);
    }
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
    let mut garment = linen();
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
                LEFT,
                RIGHT,
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
    assert!(hang(Mode::Render, &garment, false, true) > 0);
}
