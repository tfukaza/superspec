use serde::Deserialize;
use std::collections::BTreeMap;
use superspec_aml::rng::SeededRng;
use superspec_engine::{
    materialspec::surface::Sample,
    worldspec::themes::kowloon::{
        fixture::Fixture,
        household,
        mesh::{Mesh, Mode},
        metal::Metal,
        palette::srgb,
        pen::Pen,
        weathered,
    },
};
use superspec_geometry::Matrix;
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
    kind: String,
    seed: String,
    settings: Option<weathered::Settings>,
    depth: f64,
    reflected: bool,
    next: u32,
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
        "household",
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

fn draw(mesh: &mut Mesh<'_>, c: &Case, repeats: &BTreeMap<String, f64>) -> Result<u32, String> {
    let mut rng = SeededRng::from_text(&c.seed);
    let mut d = Fixture {
        pen: pen(c.reflected),
        mesh,
        repeats: &|s| repeats[s],
    };
    match c.kind.as_str() {
        "rack" => household::drying_rack(
            &mut d,
            [-0.4, 1.5],
            2.,
            c.depth,
            &mut rng,
            c.settings.as_ref(),
            true,
        )?,
        "pole" => {
            household::drying_pole(&mut d, 0.3, 2., 1.2, &mut rng, c.settings.as_ref(), true)?
        }
        "plants" => household::plants(&mut d, [-0.4, 1.5], 0.3, 0.1, &mut rng, true)?,
        "cage" => household::cage(
            &mut d,
            &household::Cage {
                span: [-0.4, 1.5],
                heights: [0.2, 1.8],
                depth: c.depth,
                color: srgb("#7fb0c8"),
                stocked: true,
            },
            &mut rng,
            c.settings.as_ref(),
            true,
        )?,
        "lamp" => household::wall_lamp(&mut d, 0.3, 0.7, c.settings.as_ref().unwrap())?,
        "meters" => {
            household::meter_bank(&mut d, -0.4, 0.2, 3, &mut rng, c.settings.as_ref().unwrap())?
        }
        _ => panic!("Unknown fixture"),
    }
    Ok(rng.next_u32())
}
pub fn check_reference_household() {
    let golden: Golden =
        serde_json::from_str(include_str!("../../../tests/fixtures/rust/household.json")).unwrap();
    for (i, c) in golden.cases.iter().enumerate() {
        let mut mesh = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
        assert_eq!(
            draw(&mut mesh, c, &golden.repeats).unwrap(),
            c.next,
            "case {i} RNG"
        );
        check(&mesh, &c.geometry, &format!("household {i} {}", c.kind));
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
        let mut survey = Mesh::new(Mode::Survey, 100000, &|| false).unwrap();
        assert_eq!(draw(&mut survey, c, &golden.repeats).unwrap(), c.next);
        assert_eq!(
            survey.triangle_count() * 3,
            c.geometry.indices.len(),
            "survey {i}"
        );
    }
}
#[test]
fn household_geometry_and_weathering_match_reference() {
    check_reference_household();
}
#[test]
fn invalid_household_admission_preserves_seed_and_rebuild_recovers() {
    let mut g: Golden =
        serde_json::from_str(include_str!("../../../tests/fixtures/rust/household.json")).unwrap();
    let mut mesh = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
    let valid = g.cases[0].depth;
    g.cases[0].depth = f64::NAN;
    assert!(draw(&mut mesh, &g.cases[0], &g.repeats).is_err());
    assert!(mesh.indices.is_empty());
    g.cases[0].depth = valid;
    assert_eq!(
        draw(&mut mesh, &g.cases[0], &g.repeats).unwrap(),
        g.cases[0].next
    );
    assert!(!mesh.indices.is_empty());
    let mut cancelled = Mesh::new(Mode::Render, 100000, &|| true).unwrap();
    assert!(draw(&mut cancelled, &g.cases[0], &g.repeats).is_err());
}
