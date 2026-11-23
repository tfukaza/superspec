use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::Pen,
    windows::{Guard, Kind, Look, security_bars, window},
};
use superspec_geometry::Matrix;
#[path = "support/window_scene.rs"]
mod scene_fixture;

#[test]
fn window_preview_scene_preserves_source_host_and_separate_collision() {
    let scene = scene_fixture::scene();
    assert!(scene.manifest.meshes.iter().filter(|m| m.visible).count() > 10);
    assert!(
        scene
            .manifest
            .colliders
            .iter()
            .all(|id| !id.contains("dressing"))
    );
}

#[test]
fn weathered_window_binds_maps_without_changing_physics_or_glass() {
    let scene = scene_fixture::weathered_scene();
    assert!(scene.manifest.assets.len() > 3);
    assert!(scene.manifest.meshes.iter().any(|m| {
        m.material
            .as_deref()
            .is_some_and(|s| s.starts_with("$surface/raster/window/"))
    }));
    assert!(
        scene
            .manifest
            .colliders
            .iter()
            .all(|id| !id.contains("dressing"))
    );
    assert_eq!(
        scene
            .manifest
            .materials
            .iter()
            .find(|m| m.id == "glass")
            .unwrap()
            .opacity,
        Some(0.28)
    );
    assert!(
        scene
            .manifest
            .meshes
            .iter()
            .any(|m| m.visible && m.vertex_count > 0 && m.material.as_deref() == Some("glass"))
    );
}

/// A lit, hooded casement with optional window weathering.
fn look(kind: Kind, weathered: bool) -> Look {
    let mut look: Look = serde_json::from_value(serde_json::json!({
        "kind": "casement",
        "frame": [0.2, 0.5, 0.8],
        "trim": [0.7, 0.6, 0.5],
        "curtain": [0.3, 0.8, 0.2],
        "reach": 1.5,
        "hood": true,
        "lit": true,
        "boarded": false,
        "fixed": false,
        "recess": 0.3,
        "weathering": {
            "seed": "window-weathering/地",
            "wear": 0.25,
            "grime": 0.6,
            "rust": 0.4,
            "textureDensity": 128,
            "metal": {"material": "metal", "repeat": 1.5},
            "sheet": {"material": "metal", "repeat": 1.5},
            "paint": {"material": "paint", "repeat": 1.25}
        }
    }))
    .unwrap();
    look.kind = kind;
    if !weathered {
        look.weathering = None;
    }
    look
}
const SPAN: [f64; 4] = [0.2, 2.2, 0.4, 2.1];
const SEED: &str = "window-fixture/7";

#[test]
fn scoped_guard_and_bolt_finishes_restore_the_callers_timber_style() {
    use superspec_engine::worldspec::themes::kowloon::{fasteners::bolt, timber, weathered};
    let weathered_look = look(Kind::Casement, true);
    let settings = weathered_look.weathering.as_ref().unwrap();
    for operation in ["guard", "bolt"] {
        for success in [true, false] {
            let base = Pen::new(matrix("reflection"), "owner")
                .unwrap()
                .with_timber(timber::Settings {
                    slots: vec!["paint".into()],
                    textured: false,
                    ..Default::default()
                })
                .unwrap();
            let mut mesh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
            let mut drawing = Fixture {
                pen: base.clone(),
                mesh: &mut mesh,
                repeats: &repeat,
            };
            let result = if operation == "guard" {
                security_bars(
                    &mut drawing,
                    [0., if success { 1. } else { -1. }, 0., 1.],
                    Guard::Grid,
                    [0.3, 0.5, 0.7],
                    Some(settings),
                )
            } else {
                bolt(
                    &mut drawing,
                    "metal",
                    [0.; 3],
                    [0., 1., 0.],
                    if success { 0.01 } else { -1. },
                    [0.3, 0.5, 0.7],
                    Some(settings),
                )
            };
            assert_eq!(result.is_ok(), success);
            let first = drawing.mesh.triangle_count();
            drawing
                .panel("paint", [0., 1., 0., 1.], 0., [0.3, 0.5, 0.7])
                .unwrap();
            let actual = expanded(drawing.mesh, first);
            let mut control = Mesh::new(Mode::Render, 100, &|| false).unwrap();
            Fixture {
                pen: base,
                mesh: &mut control,
                repeats: &repeat,
            }
            .panel("paint", [0., 1., 0., 1.], 0., [0.3, 0.5, 0.7])
            .unwrap();
            assert_eq!(
                actual,
                expanded(&control, 0),
                "{operation} success={success}"
            );
            assert!(
                weathered::finish(drawing.mesh, &|| false)
                    .unwrap()
                    .receivers()
                    .iter()
                    .all(|r| r.first_triangle + r.triangle_count <= first)
            );
        }
    }
}

fn expanded(mesh: &Mesh<'_>, first: usize) -> Vec<(String, String, Vec<u64>)> {
    mesh.indices
        .as_chunks::<3>()
        .0
        .iter()
        .enumerate()
        .skip(first)
        .map(|(t, tri)| {
            let s = &mesh.surfaces[mesh.triangle_surfaces[t]];
            (
                s.part.clone(),
                s.slot.clone(),
                tri.iter()
                    .flat_map(|i| {
                        let v = mesh.vertices[*i as usize];
                        v.position
                            .into_iter()
                            .chain(v.normal)
                            .chain(v.uv)
                            .chain(v.color)
                            .map(f64::to_bits)
                    })
                    .collect(),
            )
        })
        .collect()
}

#[test]
fn timber_window_fixture_delivers_real_library_atlas_channels_and_own_geometry() {
    let scene = scene_fixture::timber_scene();
    assert_eq!(scene.manifest.assets.len(), 3);
    assert!(
        scene
            .manifest
            .meshes
            .iter()
            .any(|m| m.material.as_deref() == Some("shop-timber") && m.vertex_count > 0)
    );
    assert!(
        scene
            .manifest
            .colliders
            .iter()
            .all(|id| !id.contains("dressing"))
    );
    for asset in scene.manifest.assets {
        let image = superspec_geometry::image::decode(
            &scene.asset_bytes[asset.byte_start..asset.byte_start + asset.byte_length],
        )
        .unwrap();
        assert_eq!((image.width, image.height), (1024, 1792));
    }
}

const SLOTS: [&str; 8] = [
    "concrete",
    "paint",
    "metal",
    "timber",
    "pane",
    "interior-lit",
    "glass",
    "shop-timber",
];
fn repeat(slot: &str) -> f64 {
    1. + SLOTS.iter().position(|s| *s == slot).unwrap() as f64 * 0.25
}
fn matrix(frame: &str) -> Matrix {
    match frame {
        "identity" => Matrix::default(),
        "reflection" => Matrix([[-1., 0., 0.], [0., -1., 0.], [0., 0., -1.], [4., 3., 5.]]),
        _ => panic!("unknown frame"),
    }
}

#[test]
fn every_window_kind_surveys_its_rendered_corners_with_identical_seed_use() {
    for kind in [Kind::Casement, Kind::Slider, Kind::Louver, Kind::Timber] {
        for weathered in [false, true] {
            for frame in ["identity", "reflection"] {
                let look = look(kind, weathered);
                let pen = Pen::new(matrix(frame), "fixture").unwrap();
                let mut mesh = Mesh::new(Mode::Render, 20000, &|| false).unwrap();
                let mut rng = SeededRng::from_text(SEED);
                window(
                    &mut Fixture {
                        pen: pen.clone(),
                        mesh: &mut mesh,
                        repeats: &repeat,
                    },
                    SPAN,
                    &look,
                    &mut rng,
                )
                .unwrap();
                assert!(mesh.triangle_count() > 0);
                let mut survey = Mesh::new(Mode::Survey, 20000, &|| false).unwrap();
                let mut survey_rng = SeededRng::from_text(SEED);
                window(
                    &mut Fixture {
                        pen,
                        mesh: &mut survey,
                        repeats: &|_| panic!("survey must not resolve appearance"),
                    },
                    SPAN,
                    &look,
                    &mut survey_rng,
                )
                .unwrap();
                assert_eq!(survey_rng.next_u32(), rng.next_u32(), "{kind:?} seed use");
                assert_eq!(survey.survey.len(), mesh.indices.len());
                for (point, index) in survey.survey.iter().zip(&mesh.indices) {
                    assert_eq!(*point, mesh.vertices[*index as usize].position);
                }
            }
        }
    }
}

#[test]
fn window_admission_and_cancelled_frames_allow_subsequent_work() {
    use std::cell::Cell;
    let casement = look(Kind::Casement, true);
    let pen = Pen::new(matrix("reflection"), "fixture").unwrap();
    let cancelled = Cell::new(false);
    let check = || cancelled.get();
    let mut mesh = Mesh::new(Mode::Render, 20000, &check).unwrap();
    let mut rng = SeededRng::from_text(SEED);
    let untouched = rng.clone().next_u32();
    let mut slider = casement.clone();
    slider.kind = Kind::Slider;
    for width in [0.2, 0.21] {
        assert!(
            window(
                &mut Fixture {
                    pen: pen.clone(),
                    mesh: &mut mesh,
                    repeats: &repeat
                },
                [0., width, 0., 1.],
                &slider,
                &mut rng
            )
            .unwrap_err()
            .contains("glazing clearance")
        );
        assert!(mesh.indices.is_empty());
        assert_eq!(rng.clone().next_u32(), untouched);
    }
    for ratio in [f64::NAN, 0., 1., 0.01, 0.99] {
        let mut look = casement.clone();
        look.transom_ratio = Some(ratio);
        assert!(
            window(
                &mut Fixture {
                    pen: pen.clone(),
                    mesh: &mut mesh,
                    repeats: &repeat
                },
                SPAN,
                &look,
                &mut rng
            )
            .is_err()
        );
        assert!(mesh.indices.is_empty());
        assert_eq!(rng.clone().next_u32(), untouched);
    }
    for bars in [
        vec![0.5, 0.5],
        vec![0.001],
        vec![0.5, 0.501],
        vec![f64::NAN],
        vec![0.5; 4097],
    ] {
        let mut look = casement.clone();
        look.lower_pane_bars = Some(bars);
        assert!(
            window(
                &mut Fixture {
                    pen: pen.clone(),
                    mesh: &mut mesh,
                    repeats: &repeat
                },
                SPAN,
                &look,
                &mut rng
            )
            .is_err()
        );
        assert!(mesh.indices.is_empty());
        assert_eq!(rng.clone().next_u32(), untouched);
    }
    // Cancellation during a translated frame must restore the owner's coordinate system.
    let reads = Cell::new(0);
    let repeats = |slot: &str| {
        let n = reads.get() + 1;
        reads.set(n);
        if n == 6 {
            cancelled.set(true);
        }
        repeat(slot)
    };
    let mut drawing = Fixture {
        pen: pen.clone(),
        mesh: &mut mesh,
        repeats: &repeats,
    };
    assert!(
        window(&mut drawing, SPAN, &casement, &mut rng)
            .unwrap_err()
            .contains("cancelled")
    );
    assert_eq!(drawing.pen.matrix.0, pen.matrix.0);
    assert_eq!(drawing.pen.mirrored, pen.mirrored);
    cancelled.set(false);
    let first_plain = drawing.mesh.triangle_count();
    drawing
        .panel("paint", [0., 1., 0., 1.], 0., [1.; 3])
        .unwrap();
    let tail = &drawing.mesh.indices[drawing.mesh.indices.len() - 6..];
    for index in tail {
        let point = drawing.mesh.vertices[*index as usize].position;
        assert_eq!(point[1], 3., "restored unshifted facade plane");
        assert_eq!(drawing.mesh.vertices[*index as usize].color, [1.; 3]);
    }
    assert!(
        superspec_engine::worldspec::themes::kowloon::weathered::finish(drawing.mesh, &|| false)
            .unwrap()
            .receivers()
            .iter()
            .all(|r| r.first_triangle + r.triangle_count <= first_plain)
    );
    // A fresh draw after the failures matches an undisturbed draw of the same window.
    let mut fresh = Mesh::new(Mode::Render, 20000, &check).unwrap();
    let mut control = Mesh::new(Mode::Render, 20000, &|| false).unwrap();
    for target in [&mut fresh, &mut control] {
        window(
            &mut Fixture {
                pen: pen.clone(),
                mesh: target,
                repeats: &repeat,
            },
            SPAN,
            &casement,
            &mut SeededRng::from_text(SEED),
        )
        .unwrap();
    }
    assert!(fresh.triangle_count() > 0);
    assert_eq!(expanded(&fresh, 0), expanded(&control, 0));
}

#[test]
fn guard_patterns_keep_their_perimeter_and_positive_winding() {
    use superspec_geometry::math::{cross, dot, sub};
    for pattern in [Guard::Bars, Guard::Grid, Guard::Diamond] {
        let mut mesh = Mesh::new(Mode::Render, 1000, &|| false).unwrap();
        security_bars(
            &mut Fixture {
                pen: Pen::new(Matrix::default(), "guard").unwrap(),
                mesh: &mut mesh,
                repeats: &repeat,
            },
            [0., 1., 0., 1.],
            pattern,
            [0.2, 0.3, 0.4],
            None,
        )
        .unwrap();
        assert!(mesh.triangle_count() > 0);
        for triangle in mesh.indices.as_chunks::<3>().0 {
            let [a, b, c] = triangle.map(|i| mesh.vertices[i as usize]);
            assert!(
                dot(
                    cross(sub(b.position, a.position), sub(c.position, a.position)),
                    a.normal
                ) > 0.
            );
        }
        for vertex in &mesh.vertices {
            assert!((-0.01001..=1.01001).contains(&vertex.position[0]));
            assert!((-0.01001..=1.01001).contains(&vertex.position[2]));
            assert!((0.08499..=0.10501).contains(&vertex.position[1]));
        }
    }
}

#[test]
fn exact_slider_width_keeps_leaf_count_independent_of_local_origin() {
    use superspec_engine::worldspec::themes::kowloon::windows::window_sized;
    let mut look = look(Kind::Slider, false);
    look.fixed = true;
    look.boarded = false;
    for (width, leaves) in [(1.7, 2), (1.70001, 3)] {
        for x in [0., 2.72, 5.11, 10.37] {
            let mut mesh = Mesh::new(Mode::Render, 20_000, &|| false).unwrap();
            window_sized(
                &mut Fixture {
                    pen: Pen::new(Matrix::default(), "window").unwrap(),
                    mesh: &mut mesh,
                    repeats: &repeat,
                },
                [x, 0.95, width, 1.3],
                &look,
                &mut SeededRng::from_text("leaf-count"),
            )
            .unwrap();
            let glazing = mesh
                .triangle_surfaces
                .iter()
                .filter(|surface| mesh.surfaces[**surface].slot == "glass")
                .count();
            assert_eq!(glazing, leaves * 2, "width {width} at local origin {x}");
        }
    }
}
