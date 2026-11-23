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
struct Case {
    operation: &'static str,
    seed: String,
    frame: &'static str,
    span: Vec<f64>,
    settings: Option<weathered::Settings>,
}
fn settings() -> weathered::Settings {
    serde_json::from_value(serde_json::json!({
        "seed": "aircon/finish",
        "wear": 0.25,
        "grime": 0.6,
        "rust": 0.4,
        "textureDensity": 128,
        "metal": {"material": "metal", "repeat": 1},
        "sheet": {"material": "sheet", "repeat": 1.75},
        "paint": {"material": "paint", "repeat": 1.25}
    }))
    .unwrap()
}
/// Window units and condensers across seeds, mirrored frames, short spans and bare finishes.
fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    let mut add = |operation, seed: u32, frame, span: &[f64], weathered: bool| {
        cases.push(Case {
            operation,
            seed: format!("aircon/{seed}"),
            frame,
            span: span.to_vec(),
            settings: weathered.then(settings),
        })
    };
    for seed in 0..6 {
        add("window", seed, "identity", &[0.7, 1.2, 0.7], true);
    }
    for seed in [0, 2] {
        add("window", seed, "reflection", &[0.7, 1.2, 0.1], true);
    }
    add("window", 4, "identity", &[0.7, 1.2, 0.7], false);
    for seed in 0..4 {
        add("condenser", seed, "identity", &[0., 1.4, 0.8, 1.6], true);
    }
    add("condenser", 7, "identity", &[0., 0.91, 0.8, 0.5], true);
    add("condenser", 8, "identity", &[0., 1.4, 0.8, 0.9], true);
    add("condenser", 0, "reflection", &[0., 1.4, 0.8, 1.6], true);
    add("condenser", 1, "identity", &[0., 1.4, 0.8, 1.6], false);
    cases
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
#[test]
fn aircon_survey_matches_render_bounds_rng_and_corners_with_receivers_inside_the_unit() {
    for case in cases() {
        let label = format!("{}/{}/{}", case.operation, case.seed, case.frame);
        let pen = Pen::new(matrix(case.frame), "fixture").unwrap();
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
        assert!(mesh.triangle_count() > 0, "{label}");
        assert_eq!(mesh.triangle_surfaces.len(), mesh.triangle_count());
        for s in &mesh.triangle_surfaces {
            assert_eq!(mesh.surfaces[*s].part, "fixture");
        }
        let triangles = mesh.triangle_count();
        let plan = weathered::finish(&mut mesh, &|| false).unwrap();
        if case.settings.is_none() {
            assert!(plan.receivers().is_empty(), "{label}");
        }
        for r in plan.receivers() {
            assert!(r.triangle_count > 0 && r.first_triangle + r.triangle_count <= triangles);
            let job = &plan.jobs()[r.job];
            assert!(job.runoff_recipe().is_none());
            let metal = Metal::new(job.metal_settings().unwrap(), None).unwrap();
            let sample: Sample = metal.sample(0.3, 0.7).unwrap();
            let _ = sample;
        }
        drop(plan);
        let mut survey = Mesh::new(Mode::Survey, 50000, &|| false).unwrap();
        let mut survey_rng = SeededRng::from_text(&case.seed);
        let surveyed = draw(
            &mut Fixture {
                pen,
                mesh: &mut survey,
                repeats: &|_| panic!("survey resolved appearance"),
            },
            &case,
            &mut survey_rng,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(surveyed).unwrap(),
            serde_json::to_value(bounds).unwrap(),
            "{label} bounds"
        );
        assert_eq!(survey_rng.next_u32(), rng.next_u32(), "{label} RNG");
        assert_eq!(survey.survey.len(), mesh.indices.len());
        for (p, i) in survey.survey.iter().zip(&mesh.indices) {
            assert_eq!(*p, mesh.vertices[*i as usize].position, "{label} survey");
        }
    }
}
#[test]
fn condenser_geometry_stays_inside_its_declared_installation_allowance() {
    for case in cases().into_iter().filter(|c| c.operation == "condenser") {
        let frame = matrix(case.frame);
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
