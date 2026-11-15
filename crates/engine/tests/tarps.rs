use serde::Deserialize;
use std::collections::BTreeMap;
use superspec_engine::{
    materialspec::surface::Sample,
    worldspec::themes::kowloon::{
        fixture::Fixture,
        mesh::{Mesh, Mode},
        metal::Metal,
        pen::Pen,
        tarps::{self, Envelope, Pattern},
        weathered,
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
    envelope: Envelope,
    tint: V3,
    hem: V3,
    pattern: Option<Pattern>,
    key: String,
    settings: Option<weathered::Settings>,
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
        "awning",
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
    tarps::canvas(
        &mut Fixture {
            pen: pen(c.reflected),
            mesh,
            repeats: &|s| repeats[s],
        },
        c.envelope,
        c.tint,
        c.hem,
        &c.key,
        c.settings.as_ref(),
        c.pattern,
    )
}
pub fn check_reference_tarps() {
    let golden: Golden =
        serde_json::from_str(include_str!("../../../tests/fixtures/rust/tarps.json")).unwrap();
    for (i, c) in golden.cases.iter().enumerate() {
        let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
        draw(&mut mesh, c, &golden.repeats).unwrap();
        check(&mesh, &c.geometry, &format!("canvas {i}"));
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
            let metal = Metal::new(
                job.metal_settings().unwrap(),
                runoff
                    .as_ref()
                    .map(|_| &sample_runoff as &dyn Fn(f64, f64) -> Result<f64, String>),
            )
            .unwrap();
            close(
                &serde_json::to_value(metal.sample(chart.point[0], chart.point[1]).unwrap())
                    .unwrap(),
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
fn fitted_canvas_matches_reference() {
    check_reference_tarps();
}
#[test]
fn invalid_canvas_does_not_emit_and_next_build_recovers() {
    let mut g: Golden =
        serde_json::from_str(include_str!("../../../tests/fixtures/rust/tarps.json")).unwrap();
    let c = &mut g.cases[0];
    let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
    let depth = c.envelope.depth;
    c.envelope.depth = f64::NAN;
    assert!(draw(&mut mesh, c, &g.repeats).is_err());
    assert!(mesh.indices.is_empty());
    c.envelope.depth = depth;
    draw(&mut mesh, c, &g.repeats).unwrap();
    assert!(!mesh.indices.is_empty());
    let mut cancelled = Mesh::new(Mode::Render, 150000, &|| true).unwrap();
    assert!(draw(&mut cancelled, c, &g.repeats).is_err());
}
