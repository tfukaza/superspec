use serde::Deserialize;
use std::collections::BTreeMap;
use superspec_engine::worldspec::themes::kowloon::{
    facade_material::Facade,
    fixture::Fixture,
    mesh::{Mesh, Mode},
    metal::Metal,
    pen::Pen,
    roof_access::{self, Plot, Settings},
    skin::Deposited,
    weathered::{self, Recipe},
};
use superspec_geometry::Matrix;
#[path = "support/roof_access_scene.rs"]
mod scene_fixture;
#[derive(Deserialize)]
struct Chart {
    triangles: Vec<usize>,
    point: [f64; 2],
    sample: superspec_engine::materialspec::surface::Sample,
}
#[derive(Deserialize)]
struct Case {
    settings: Settings,
    plot: Plot,
    height: f64,
    reflected: bool,
    indices: Vec<u32>,
    positions: Vec<f64>,
    normals: Vec<f64>,
    uv: Vec<f64>,
    colors: Vec<f64>,
    slots: Vec<String>,
    charts: Vec<Chart>,
}
#[derive(Deserialize)]
struct Golden {
    repeats: BTreeMap<String, f64>,
    cases: Vec<Case>,
}
fn fixture() -> Golden {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/roof-access.json"
    ))
    .unwrap()
}
fn draw(mesh: &mut Mesh<'_>, case: &Case, repeats: &BTreeMap<String, f64>) -> Result<(), String> {
    roof_access::stair_hut(
        &mut Fixture {
            pen: Pen::new(
                if case.reflected {
                    Matrix([[-1., 0., 0.], [0., -1., 0.], [0., 0., -1.], [4., 3., 5.]])
                } else {
                    Matrix::default()
                },
                "roof",
            )
            .unwrap(),
            mesh,
            repeats: &|s| repeats[s],
        },
        case.plot,
        case.height,
        &case.settings,
    )
}
fn close(a: &serde_json::Value, b: &serde_json::Value) {
    use serde_json::Value;
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => assert!(
            (a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 2e-10,
            "{a} != {b}"
        ),
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len());
            for (a, b) in a.iter().zip(b) {
                close(a, b);
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            for (k, b) in b {
                close(&a[k], b);
            }
        }
        _ => assert_eq!(a, b),
    }
}
pub fn check_reference_roof_access() {
    let golden = fixture();
    for (ci, case) in golden.cases.iter().enumerate() {
        let mut mesh = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
        draw(&mut mesh, case, &golden.repeats).unwrap();
        assert_eq!(
            mesh.indices.len(),
            case.indices.len(),
            "roof {ci} triangle count"
        );
        assert_eq!(
            mesh.indices
                .iter()
                .zip(&case.indices)
                .position(|(a, b)| a != b),
            None,
            "roof {ci} indices"
        );
        for (actual, expected, label) in [
            (
                mesh.vertices
                    .iter()
                    .flat_map(|v| v.position)
                    .collect::<Vec<_>>(),
                &case.positions,
                "positions",
            ),
            (
                mesh.vertices.iter().flat_map(|v| v.normal).collect(),
                &case.normals,
                "normals",
            ),
            (
                mesh.vertices.iter().flat_map(|v| v.color).collect(),
                &case.colors,
                "colors",
            ),
            (
                mesh.vertices.iter().flat_map(|v| v.uv).collect(),
                &case.uv,
                "uv",
            ),
        ] {
            assert_eq!(actual.len(), expected.len(), "roof {ci} {label} count");
            if let Some(i) = actual
                .iter()
                .zip(expected)
                .position(|(a, b)| (a - b).abs() > 2e-10)
            {
                panic!("roof {ci} {label} {i}: {} != {}", actual[i], expected[i]);
            }
        }
        let slots: Vec<_> = mesh
            .triangle_surfaces
            .iter()
            .map(|i| mesh.surfaces[*i].slot.clone())
            .collect();
        assert_eq!(slots, case.slots);
        let plan = weathered::finish(&mut mesh, &|| false).unwrap();
        let mut received = vec![false; case.indices.len() / 3];
        for receiver in plan.receivers() {
            let chart = case
                .charts
                .iter()
                .find(|c| c.triangles.contains(&receiver.first_triangle))
                .unwrap();
            for (offset, seen) in received
                [receiver.first_triangle..receiver.first_triangle + receiver.triangle_count]
                .iter_mut()
                .enumerate()
            {
                let t = receiver.first_triangle + offset;
                assert!(chart.triangles.contains(&t));
                assert!(!*seen);
                *seen = true;
            }
            let job = &plan.jobs()[receiver.job];
            let [u, v] = chart.point;
            let runoff = job.runoff_recipe().map(|r| r.build(&|| false).unwrap());
            let sample_runoff = |u, v| runoff.as_ref().unwrap().sample(u, v);
            let sample = match &job.recipe {
                Recipe::Facade(r) => Facade::new(r).unwrap().sample(u, v),
                Recipe::FacadeDeposits(r) => Deposited::new(r).unwrap().sample(u, v),
                Recipe::Metal { settings, .. } => Metal::new(
                    settings,
                    runoff
                        .as_ref()
                        .map(|_| &sample_runoff as &dyn Fn(f64, f64) -> Result<f64, String>),
                )
                .unwrap()
                .sample(u, v),
                _ => panic!("Unexpected roof recipe"),
            }
            .unwrap();
            close(
                &serde_json::to_value(sample).unwrap(),
                &serde_json::to_value(&chart.sample).unwrap(),
            );
        }
        assert_eq!(
            received.iter().filter(|v| **v).count(),
            case.charts.iter().map(|c| c.triangles.len()).sum::<usize>()
        );
        let mut survey = Mesh::new(Mode::Survey, 100000, &|| false).unwrap();
        draw(&mut survey, case, &golden.repeats).unwrap();
        assert_eq!(survey.triangle_count(), case.indices.len() / 3);
    }
}
#[test]
fn complete_roof_access_matches_reference_geometry_and_finish_receivers() {
    check_reference_roof_access();
}
#[test]
fn complete_roof_access_bakes_appearance_without_changing_host_collision() {
    let scene = scene_fixture::scene();
    assert_eq!(scene.manifest.colliders.len(), 1);
    assert!(!scene.manifest.assets.is_empty());
    assert!(scene.manifest.meshes.iter().filter(|m| m.visible).count() > 4);
}
#[test]
fn malformed_roof_access_inputs_do_not_emit_and_next_build_succeeds() {
    let mut g = fixture();
    let case = &mut g.cases[0];
    for error in 0..4 {
        let s = case.settings.clone();
        let h = case.height;
        let p = case.plot;
        match error {
            0 => case.height = 2.,
            1 => case.plot.x1 = case.plot.x0 + 1.,
            2 => case.settings.weathering.grime = f64::NAN,
            _ => {
                case.settings.masonry = Some(roof_access::Masonry {
                    grime: 0.5,
                    wear: 0.5,
                    density: 0.,
                })
            }
        }
        let mut mesh = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
        assert!(draw(&mut mesh, case, &g.repeats).is_err());
        assert!(mesh.indices.is_empty());
        case.settings = s;
        case.height = h;
        case.plot = p;
    }
    let mut mesh = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
    draw(&mut mesh, case, &g.repeats).unwrap();
    assert!(!mesh.indices.is_empty());
}
