use serde::Deserialize;
use superspec_aml::rng::SeededRng;
use superspec_engine::{
    materialspec::surface::Sample,
    worldspec::themes::kowloon::{
        aircon::{self, Bounds},
        fixture::Fixture,
        mesh::{Mesh, Mode},
        metal::Metal,
        pen::Pen,
        weathered,
    },
};
use superspec_geometry::Matrix;
#[path = "support/aircon_scene.rs"]
mod scene_fixture;
#[derive(Deserialize)]
struct Receiver {
    first: usize,
    count: usize,
    point: [f64; 2],
    sample: Sample,
}
#[derive(Deserialize)]
struct Case {
    operation: String,
    seed: String,
    frame: String,
    span: Vec<f64>,
    settings: Option<weathered::Settings>,
    bounds: Bounds,
    next: u32,
    indices: Vec<u32>,
    positions: Vec<f64>,
    normals: Vec<f64>,
    uv: Vec<f64>,
    colors: Vec<f64>,
    slots: Vec<String>,
    receivers: Vec<Receiver>,
}
#[derive(Deserialize)]
struct Cases {
    cases: Vec<Case>,
}
fn repeat(slot: &str) -> f64 {
    match slot {
        "metal" => 1.,
        "paint" => 1.25,
        "aircon" => 1.5,
        "sheet" => 1.75,
        _ => panic!("unknown slot"),
    }
}
fn matrix(frame: &str) -> Matrix {
    if frame == "identity" {
        Matrix::default()
    } else {
        Matrix([[-1., 0., 0.], [0., -1., 0.], [0., 0., -1.], [4., 3., 5.]])
    }
}
fn draw(drawing: &mut Fixture<'_, '_>, case: &Case, rng: &mut SeededRng) -> Result<Bounds, String> {
    if case.operation == "window" {
        aircon::air_conditioner(
            drawing,
            case.span[0],
            case.span[1],
            case.span[2],
            rng,
            case.settings.as_ref(),
        )
    } else {
        aircon::condenser(
            drawing,
            case.span.clone().try_into().unwrap(),
            rng,
            case.settings.as_ref(),
        )
    }
}
fn close_json(a: &serde_json::Value, b: &serde_json::Value, label: &str) {
    match (a, b) {
        (serde_json::Value::Number(a), serde_json::Value::Number(b)) => assert!(
            (a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 2e-10,
            "{label}: {a} != {b}"
        ),
        (serde_json::Value::Array(a), serde_json::Value::Array(b)) => {
            assert_eq!(a.len(), b.len(), "{label}");
            for (a, b) in a.iter().zip(b) {
                close_json(a, b, label);
            }
        }
        (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
            assert_eq!(a.len(), b.len(), "{label}");
            for (k, b) in b {
                close_json(&a[k], b, &format!("{label}/{k}"));
            }
        }
        _ => assert_eq!(a, b, "{label}"),
    }
}
pub fn check_reference_aircon() {
    let cases: Cases =
        serde_json::from_str(include_str!("../../../tests/fixtures/rust/aircon.json")).unwrap();
    for case in cases.cases {
        let label = format!("{}/{}/{}", case.operation, case.seed, case.frame);
        let pen = Pen::new(matrix(&case.frame), "fixture").unwrap();
        let mut mesh = Mesh::new(Mode::Render, 50000, &|| false).unwrap();
        let mut rng = SeededRng::from_text(&case.seed);
        let bounds = draw(
            &mut Fixture {
                pen: pen.clone(),
                mesh: &mut mesh,
                repeats: &repeat,
            },
            &case,
            &mut rng,
        )
        .unwrap();
        close_json(
            &serde_json::to_value(bounds).unwrap(),
            &serde_json::to_value(&case.bounds).unwrap(),
            &label,
        );
        assert_eq!(rng.next_u32(), case.next, "{label} RNG");
        assert_eq!(
            mesh.indices.len(),
            case.indices.len(),
            "{label} triangle count"
        );
        assert!(
            mesh.indices == case.indices,
            "{label} index mismatch {:?}",
            mesh.indices
                .iter()
                .zip(&case.indices)
                .position(|(a, b)| a != b)
        );
        for (name, actual, expected) in [
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
                "uv",
                mesh.vertices.iter().flat_map(|v| v.uv).collect(),
                &case.uv,
            ),
            (
                "color",
                mesh.vertices.iter().flat_map(|v| v.color).collect(),
                &case.colors,
            ),
        ] {
            assert_eq!(actual.len(), expected.len(), "{label}/{name} count");
            for (i, (a, b)) in actual.iter().zip(expected).enumerate() {
                assert!((a - b).abs() < 2e-10, "{label}/{name}/{i}: {a} != {b}");
            }
        }
        assert_eq!(mesh.triangle_surfaces.len(), case.slots.len());
        for (s, slot) in mesh.triangle_surfaces.iter().zip(&case.slots) {
            assert_eq!(&mesh.surfaces[*s].slot, slot, "{label} slot");
            assert_eq!(mesh.surfaces[*s].part, "fixture");
        }
        let plan = weathered::finish(&mut mesh, &|| false).unwrap();
        assert_eq!(
            plan.receivers().len(),
            case.receivers.len(),
            "{label} receivers"
        );
        for (r, e) in plan.receivers().iter().zip(&case.receivers) {
            assert_eq!((r.first_triangle, r.triangle_count), (e.first, e.count));
            let job = &plan.jobs()[r.job];
            assert!(job.runoff_recipe().is_none());
            let metal = Metal::new(job.metal_settings().unwrap(), None).unwrap();
            close_json(
                &serde_json::to_value(metal.sample(e.point[0], e.point[1]).unwrap()).unwrap(),
                &serde_json::to_value(&e.sample).unwrap(),
                &label,
            );
        }
        drop(plan);
        let mut survey = Mesh::new(Mode::Survey, 50000, &|| false).unwrap();
        let mut rng = SeededRng::from_text(&case.seed);
        let bounds = draw(
            &mut Fixture {
                pen,
                mesh: &mut survey,
                repeats: &|_| panic!("survey resolved appearance"),
            },
            &case,
            &mut rng,
        )
        .unwrap();
        close_json(
            &serde_json::to_value(bounds).unwrap(),
            &serde_json::to_value(&case.bounds).unwrap(),
            &label,
        );
        assert_eq!(rng.next_u32(), case.next);
        assert_eq!(survey.survey.len(), mesh.indices.len());
        for (p, i) in survey.survey.iter().zip(&mesh.indices) {
            assert_eq!(*p, mesh.vertices[*i as usize].position, "{label} survey");
        }
    }
}
#[test]
fn aircon_geometry_atlas_receivers_and_rng_match_reference() {
    check_reference_aircon();
}

#[test]
fn condenser_geometry_stays_inside_its_declared_installation_allowance() {
    let cases: Cases =
        serde_json::from_str(include_str!("../../../tests/fixtures/rust/aircon.json")).unwrap();
    for case in cases
        .cases
        .into_iter()
        .filter(|c| c.operation == "condenser")
    {
        let frame = matrix(&case.frame);
        let inverse = frame.inverse().unwrap();
        let mut mesh = Mesh::new(Mode::Render, 50000, &|| false).unwrap();
        draw(
            &mut Fixture {
                pen: Pen::new(frame, "fixture").unwrap(),
                mesh: &mut mesh,
                repeats: &repeat,
            },
            &case,
            &mut SeededRng::from_text(&case.seed),
        )
        .unwrap();
        for (corner, index) in mesh.indices.iter().enumerate() {
            let [x, y, z] = inverse.point(mesh.vertices[*index as usize].position);
            let slot = &mesh.surfaces[mesh.triangle_surfaces[corner / 3]].slot;
            assert!(
                x >= case.span[0] - 1e-5 && x <= case.span[1] + 1e-5,
                "{} x {x}",
                case.seed
            );
            // The rain sheet oscillates across its mounting plane by pitch * 0.14.
            let back = if slot == "sheet" { -0.09 * 0.14 } else { 0. };
            assert!(
                (back - 1e-5..=0.45001).contains(&y),
                "{} {slot} y {y}",
                case.seed
            );
            assert!(
                z >= case.span[2] - 0.36001 && z <= case.span[2] + case.span[3] + 1e-5,
                "{} z {z}",
                case.seed
            );
        }
    }
}

#[test]
fn invalid_clearance_and_cancelled_emission_publish_nothing_and_recover() {
    use std::cell::Cell;
    let checks = Cell::new(0);
    let cancelling = Cell::new(false);
    let cancelled = || {
        checks.set(checks.get() + 1);
        cancelling.get() && checks.get() > 40
    };
    for operation in ["window", "condenser"] {
        let pen = Pen::new(Matrix::default(), "fixture").unwrap();
        let mut mesh = Mesh::new(Mode::Render, 50000, &cancelled).unwrap();
        let mut rng = SeededRng::from_text("aircon/0");
        let original = rng.clone().next_u32();
        let mut drawing = Fixture {
            pen: pen.clone(),
            mesh: &mut mesh,
            repeats: &repeat,
        };
        let invalid = if operation == "window" {
            aircon::air_conditioner(&mut drawing, 0., 1., -1., &mut rng, None)
        } else {
            aircon::condenser(&mut drawing, [0., 0.3, 0.8, 0.5], &mut rng, None)
        };
        assert!(invalid.is_err());
        assert_eq!(mesh.triangle_count(), 0);
        assert_eq!(rng.next_u32(), original);
        let mut drawing = Fixture {
            pen: pen.clone(),
            mesh: &mut mesh,
            repeats: &repeat,
        };
        let emit = |drawing: &mut Fixture<'_, '_>| {
            let mut rng = SeededRng::from_text("aircon/0");
            if operation == "window" {
                aircon::air_conditioner(drawing, 0.7, 1.2, 0.7, &mut rng, None)
            } else {
                aircon::condenser(drawing, [0., 1.4, 0.8, 1.6], &mut rng, None)
            }
        };
        checks.set(0);
        cancelling.set(true);
        assert!(
            emit(&mut drawing)
                .unwrap_err()
                .to_lowercase()
                .contains("cancel")
        );
        assert!(mesh.triangle_count() > 0);
        cancelling.set(false);
        let mut fresh = Mesh::new(Mode::Render, 50000, &cancelled).unwrap();
        emit(&mut Fixture {
            pen,
            mesh: &mut fresh,
            repeats: &repeat,
        })
        .unwrap();
        assert!(fresh.triangle_count() > 500);
        assert!(fresh.surfaces.iter().all(|s| s.part == "fixture"));
    }
}

#[test]
fn real_atlas_and_baked_supports_keep_host_collision_separate() {
    let scene = scene_fixture::scene();
    assert_eq!(scene.manifest.colliders.len(), 1);
    assert!(
        scene
            .manifest
            .colliders
            .iter()
            .all(|id| !id.contains("dressing"))
    );
    assert!(
        scene
            .manifest
            .assets
            .iter()
            .any(|a| a.id == "superspec-material:aircon.png")
    );
    for part in ["window-unit", "condenser"] {
        assert!(
            scene.manifest.meshes.iter().any(|m| m.visible
                && m.id.contains(part)
                && m.material.as_deref() == Some("aircon"))
        );
    }
    assert!(scene.manifest.meshes.iter().any(|m| {
        m.material
            .as_deref()
            .is_some_and(|s| s.starts_with("$surface/raster/aircon/"))
    }));
}
