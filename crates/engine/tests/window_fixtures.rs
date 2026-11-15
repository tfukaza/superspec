use serde::Deserialize;
use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::Pen,
    windows::{Guard, Look, security_bars, window},
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

#[test]
fn scoped_guard_and_bolt_finishes_restore_the_callers_timber_style() {
    use superspec_engine::worldspec::themes::kowloon::{fasteners::bolt, timber, weathered};
    let cases: Cases =
        serde_json::from_str(include_str!("../../../tests/fixtures/rust/windows.json")).unwrap();
    let settings = cases
        .cases
        .iter()
        .find_map(|c| c.look.weathering.as_ref())
        .unwrap();
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
#[derive(Deserialize)]
struct Case {
    #[serde(rename = "receiverEvidence", default)]
    receiver_evidence: Vec<ReceiverEvidence>,
    #[serde(default)]
    receivers: usize,
    frame: String,
    operation: String,
    span: [f64; 4],
    look: Look,
    seed: String,
    next: u32,
    indices: Vec<u32>,
    positions: Vec<f64>,
    normals: Vec<f64>,
    uv: Vec<f64>,
    colors: Vec<f64>,
    slots: Vec<String>,
}
#[derive(Deserialize)]
struct ReceiverEvidence {
    first: usize,
    count: usize,
    contacts: Vec<[f64; 2]>,
    gravity: [f64; 2],
    materials: Vec<String>,
}
#[derive(Deserialize)]
struct Cases {
    cases: Vec<Case>,
}
fn matrix(frame: &str) -> Matrix {
    match frame {
        "identity" => Matrix::default(),
        "reflection" => Matrix([[-1., 0., 0.], [0., -1., 0.], [0., 0., -1.], [4., 3., 5.]]),
        _ => panic!("unknown frame"),
    }
}
fn draw(drawing: &mut Fixture<'_, '_>, case: &Case, rng: &mut SeededRng) -> Result<(), String> {
    match case.operation.as_str() {
        "bars" => security_bars(
            drawing,
            case.span,
            Guard::Bars,
            case.look.frame,
            case.look.weathering.as_ref(),
        ),
        "grid" => security_bars(
            drawing,
            case.span,
            Guard::Grid,
            case.look.frame,
            case.look.weathering.as_ref(),
        ),
        "diamond" => security_bars(
            drawing,
            case.span,
            Guard::Diamond,
            case.look.frame,
            case.look.weathering.as_ref(),
        ),
        _ => window(drawing, case.span, &case.look, rng),
    }
}
/// The same reference comparisons execute in the native and actual browser Wasm suites.
pub fn check_reference_windows() {
    let cases: Cases =
        serde_json::from_str(include_str!("../../../tests/fixtures/rust/windows.json")).unwrap();
    for case in cases.cases {
        let pen = Pen::new(matrix(&case.frame), "fixture").unwrap();
        let mut mesh = Mesh::new(Mode::Render, 20000, &|| false).unwrap();
        let mut rng = SeededRng::from_text(&case.seed);
        draw(
            &mut Fixture {
                pen: pen.clone(),
                mesh: &mut mesh,
                repeats: &repeat,
            },
            &case,
            &mut rng,
        )
        .unwrap();
        let plan =
            superspec_engine::worldspec::themes::kowloon::weathered::finish(&mut mesh, &|| false)
                .unwrap();
        assert_eq!(
            plan.receivers().len(),
            case.receivers,
            "{} receiver count",
            case.operation
        );
        if !case.receiver_evidence.is_empty() {
            assert_eq!(case.receiver_evidence.len(), plan.receivers().len());
            for (expected, range) in case.receiver_evidence.iter().zip(plan.receivers()) {
                let job = &plan.jobs()[range.job];
                assert_eq!(
                    (range.first_triangle, range.triangle_count),
                    (expected.first, expected.count)
                );
                assert_eq!(
                    vec![
                        job.metal_settings().unwrap().substrate.material.clone(),
                        job.metal_settings().unwrap().coating.material.clone()
                    ],
                    expected.materials
                );
                let contacts = job.metal_settings().unwrap().contacts.as_ref().unwrap();
                assert_eq!(contacts.len(), expected.contacts.len());
                for (a, b) in contacts
                    .iter()
                    .flatten()
                    .chain(
                        &job.metal_settings()
                            .unwrap()
                            .metric
                            .as_ref()
                            .unwrap()
                            .gravity,
                    )
                    .zip(expected.contacts.iter().flatten().chain(&expected.gravity))
                {
                    assert!(
                        (a - b).abs() < 2e-10,
                        "{} exposure: {a} != {b}",
                        case.operation
                    );
                }
            }
        }
        drop(plan);
        assert_eq!(
            rng.next_u32(),
            case.next,
            "seed consumption: {}",
            case.operation
        );
        assert_eq!(
            mesh.indices.len(),
            case.indices.len(),
            "{}/{} triangle count",
            case.frame,
            case.operation
        );
        assert_eq!(
            mesh.vertices.len() * 3,
            case.positions.len(),
            "{}/{} sharing",
            case.frame,
            case.operation
        );
        assert!(
            mesh.indices == case.indices,
            "{}/{} sharing indices",
            case.frame,
            case.operation
        );
        for (label, actual, expected) in [
            (
                "position",
                mesh.vertices
                    .iter()
                    .flat_map(|v| v.position)
                    .collect::<Vec<_>>(),
                &case.positions,
            ),
            (
                "normal",
                mesh.vertices.iter().flat_map(|v| v.normal).collect(),
                &case.normals,
            ),
            (
                "UV",
                mesh.vertices.iter().flat_map(|v| v.uv).collect(),
                &case.uv,
            ),
            (
                "color",
                mesh.vertices.iter().flat_map(|v| v.color).collect(),
                &case.colors,
            ),
        ] {
            assert_eq!(actual.len(), expected.len());
            for (i, (a, b)) in actual.iter().zip(expected).enumerate() {
                // Same numerical policy as the primitive suite: far below any quantization step.
                assert!(
                    (a - b).abs() < 2e-12,
                    "{}/{} {label}[{i}]: {a} != {b}",
                    case.frame,
                    case.operation
                );
            }
        }
        assert_eq!(mesh.triangle_surfaces.len(), case.slots.len());
        for (surface, slot) in mesh.triangle_surfaces.iter().zip(&case.slots) {
            assert_eq!(&mesh.surfaces[*surface].slot, slot);
        }
        let mut survey = Mesh::new(Mode::Survey, 20000, &|| false).unwrap();
        let mut rng = SeededRng::from_text(&case.seed);
        draw(
            &mut Fixture {
                pen,
                mesh: &mut survey,
                repeats: &|_| panic!("survey must not resolve appearance"),
            },
            &case,
            &mut rng,
        )
        .unwrap();
        assert_eq!(rng.next_u32(), case.next);
        assert_eq!(survey.survey.len(), mesh.indices.len());
        for (point, index) in survey.survey.iter().zip(&mesh.indices) {
            assert_eq!(*point, mesh.vertices[*index as usize].position);
        }
    }
}
#[test]
fn complete_windows_and_guards_match_reference_geometry_and_seed_state() {
    check_reference_windows();
}

#[test]
fn window_admission_and_cancelled_frames_allow_subsequent_work() {
    use std::cell::Cell;
    let cases: Cases =
        serde_json::from_str(include_str!("../../../tests/fixtures/rust/windows.json")).unwrap();
    let case = cases
        .cases
        .iter()
        .find(|c| c.operation == "casement" && c.look.weathering.is_some())
        .unwrap();
    let pen = Pen::new(matrix("reflection"), "fixture").unwrap();
    let cancelled = Cell::new(false);
    let check = || cancelled.get();
    let mut mesh = Mesh::new(Mode::Render, 20000, &check).unwrap();
    let mut rng = SeededRng::from_text(&case.seed);
    let untouched = rng.clone().next_u32();
    let mut slider = case.look.clone();
    slider.kind = superspec_engine::worldspec::themes::kowloon::windows::Kind::Slider;
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
        let mut look = case.look.clone();
        look.transom_ratio = Some(ratio);
        assert!(
            window(
                &mut Fixture {
                    pen: pen.clone(),
                    mesh: &mut mesh,
                    repeats: &repeat
                },
                case.span,
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
        let mut look = case.look.clone();
        look.lower_pane_bars = Some(bars);
        assert!(
            window(
                &mut Fixture {
                    pen: pen.clone(),
                    mesh: &mut mesh,
                    repeats: &repeat
                },
                case.span,
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
        window(&mut drawing, case.span, &case.look, &mut rng)
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
    let mut fresh = Mesh::new(Mode::Render, 20000, &check).unwrap();
    window(
        &mut Fixture {
            pen,
            mesh: &mut fresh,
            repeats: &repeat,
        },
        case.span,
        &case.look,
        &mut SeededRng::from_text(&case.seed),
    )
    .unwrap();
    assert_eq!(fresh.indices.len(), case.indices.len());
}

#[test]
fn guard_patterns_keep_their_perimeter_and_positive_winding() {
    use superspec_geometry::math::{cross, dot, sub};
    for (pattern, triangles) in [(Guard::Bars, 96), (Guard::Grid, 88), (Guard::Diamond, 160)] {
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
        assert_eq!(mesh.triangle_count(), triangles);
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
    use superspec_engine::worldspec::themes::kowloon::windows::{Kind, window_sized};
    let cases: Cases =
        serde_json::from_str(include_str!("../../../tests/fixtures/rust/windows.json")).unwrap();
    let mut look = cases
        .cases
        .iter()
        .find(|c| c.look.kind == Kind::Slider)
        .unwrap()
        .look
        .clone();
    look.weathering = None;
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
