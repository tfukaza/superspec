use serde::Deserialize;
use std::collections::BTreeMap;
use superspec_engine::{
    materialspec::surface::Sample,
    worldspec::themes::kowloon::{
        balconies::{self, Outline, Settings},
        facade_material::Facade,
        fixture::Fixture,
        mesh::{Mesh, Mode},
        metal::Metal,
        pen::Pen,
        skin::Deposited,
        weathered::{self, Recipe},
    },
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
struct Chart {
    triangles: Vec<usize>,
    point: [f64; 2],
    sample: Sample,
}
#[derive(Deserialize)]
struct Case {
    outline: Outline,
    tint: V3,
    trim: V3,
    settings: Settings,
    reflected: bool,
    charts: Vec<Chart>,
    #[serde(flatten)]
    geometry: Geometry,
}
#[derive(Deserialize)]
struct Golden {
    repeats: BTreeMap<String, f64>,
    cases: Vec<Case>,
}
fn pen(reflected: bool) -> Pen {
    Pen::new(
        if reflected {
            Matrix([[-1., 0., 0.], [0., -1., 0.], [0., 0., -1.], [4., 3., 5.]])
        } else {
            Matrix::default()
        },
        "balcony",
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

fn draw(mesh: &mut Mesh<'_>, c: &Case, repeats: &BTreeMap<String, f64>) -> Result<(), String> {
    let mut d = Fixture {
        pen: pen(c.reflected),
        mesh,
        repeats: &|s| repeats[s],
    };
    balconies::slab(&mut d, &c.outline, -0.12, 0., c.tint, "concrete")?;
    balconies::parapet(
        &mut d,
        &c.outline,
        0.85,
        c.tint,
        c.trim,
        &c.settings,
        "balcony",
    )?;
    balconies::anti_climb(&mut d, 0., 0.8, 0.5, c.trim, &c.settings.weathering)
}
pub fn check_reference_balconies() {
    let golden: Golden =
        serde_json::from_str(include_str!("../../../tests/fixtures/rust/balconies.json")).unwrap();
    for (i, c) in golden.cases.iter().enumerate() {
        let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
        draw(&mut mesh, c, &golden.repeats).unwrap();
        check(&mesh, &c.geometry, &format!("balcony {i}"));
        let plan = weathered::finish(&mut mesh, &|| false).unwrap();
        let mut covered = vec![false; c.geometry.indices.len() / 3];
        for receiver in plan.receivers() {
            let chart = c
                .charts
                .iter()
                .find(|c| c.triangles.contains(&receiver.first_triangle))
                .unwrap();
            for (t, seen) in covered
                .iter_mut()
                .enumerate()
                .skip(receiver.first_triangle)
                .take(receiver.triangle_count)
            {
                assert!(chart.triangles.contains(&t));
                assert!(!*seen);
                *seen = true;
            }
            let job = &plan.jobs()[receiver.job];
            let runoff = job.runoff_recipe().map(|r| r.build(&|| false).unwrap());
            let sample_runoff = |u, v| runoff.as_ref().unwrap().sample(u, v);
            let [u, v] = chart.point;
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
                _ => panic!("Unexpected balcony finish"),
            }
            .unwrap();
            close(
                &serde_json::to_value(sample).unwrap(),
                &serde_json::to_value(&chart.sample).unwrap(),
            );
        }
        assert_eq!(
            covered.iter().filter(|x| **x).count(),
            c.charts.iter().map(|c| c.triangles.len()).sum::<usize>()
        );
        let mut survey = Mesh::new(Mode::Survey, 150000, &|| false).unwrap();
        draw(&mut survey, c, &golden.repeats).unwrap();
        assert_eq!(
            survey.triangle_count() * 3,
            c.geometry.indices.len(),
            "survey {i}"
        );
    }
}

#[test]
fn balconies_match_reference() {
    check_reference_balconies();
}
#[test]
fn invalid_balcony_outline_does_not_emit_and_next_draw_recovers() {
    let g: Golden =
        serde_json::from_str(include_str!("../../../tests/fixtures/rust/balconies.json")).unwrap();
    let c = &g.cases[0];
    let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
    let mut bad = c.outline;
    bad.swap(1, 2);
    let mut d = Fixture {
        pen: pen(false),
        mesh: &mut mesh,
        repeats: &|_| 1.,
    };
    assert!(balconies::slab(&mut d, &bad, -0.12, 0., c.tint, "concrete").is_err());
    assert!(d.mesh.indices.is_empty());
    balconies::slab(&mut d, &c.outline, -0.12, 0., c.tint, "concrete").unwrap();
    assert!(!mesh.indices.is_empty());
}
