use serde::Deserialize;
use std::collections::BTreeMap;
use superspec_engine::{
    materialspec::surface::Sample,
    worldspec::themes::kowloon::{
        facade_details::{self, Plan, Request, Settings},
        facade_material::Facade,
        fixture::Fixture,
        massing::Rect,
        mesh::{Mesh, Mode},
        metal::Metal,
        pen::Pen,
        skin::Deposited,
        weathered::{self, Recipe},
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
struct Threshold {
    u0: f64,
    u1: f64,
}
#[derive(Deserialize)]
struct Case {
    request: Request,
    #[serde(rename = "groundRects")]
    ground_rects: Vec<Rect>,
    thresholds: Vec<Threshold>,
    blockers: Vec<Rect>,
    miters: [Option<f64>; 2],
    plan: Plan,
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
        "facade",
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
    let reach = |r: Rect, limit: f64| {
        Ok(
            if c.blockers
                .iter()
                .any(|b| r.u0 < b.u1 && r.u1 > b.u0 && r.v0 < b.v1 && r.v1 > b.v0)
            {
                0.
            } else {
                limit
            },
        )
    };
    let miters = |r: Rect| {
        Ok([
            if r.u0 == 0. { c.miters[0] } else { None },
            if r.u1 == 6. { c.miters[1] } else { None },
        ])
    };
    let plan = facade_details::plan(&c.request, &c.settings.weathering.seed, &reach, &|| false)?;
    close(
        &serde_json::to_value(&plan).unwrap(),
        &serde_json::to_value(&c.plan).unwrap(),
    );
    let mut d = Fixture {
        pen: pen(c.reflected),
        mesh,
        repeats: &|s| repeats[s],
    };
    facade_details::draw(&mut d, &plan, &c.settings, &c.request.key, &miters)?;
    facade_details::plinth(
        &mut d,
        &c.ground_rects,
        &c.thresholds
            .iter()
            .map(|t| [t.u0, t.u1])
            .collect::<Vec<_>>(),
        c.request.ground,
        &c.settings,
        &c.request.key,
        &reach,
        &miters,
    )
}
pub fn check_reference_facade_details() {
    check_corners();
    let golden: Golden = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/facade-details.json"
    ))
    .unwrap();
    for (i, c) in golden.cases.iter().enumerate() {
        let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
        draw(&mut mesh, c, &golden.repeats).unwrap();
        check(&mesh, &c.geometry, &format!("facade {i}"));
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
fn facade_construction_and_runoff_plans_match_reference() {
    check_reference_facade_details();
}
#[test]
fn cancelled_or_invalid_planning_recovers_without_emission() {
    let mut g: Golden = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/facade-details.json"
    ))
    .unwrap();
    let c = &mut g.cases[0];
    assert!(facade_details::plan(&c.request, "test", &|_, limit| Ok(limit), &|| true).is_err());
    assert!(facade_details::plan(&c.request, "test", &|_, _| Ok(f64::NAN), &|| false).is_err());
    let ground = c.request.ground;
    c.request.ground = f64::NAN;
    let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
    assert!(draw(&mut mesh, c, &g.repeats).is_err());
    assert!(mesh.indices.is_empty());
    c.request.ground = ground;
    draw(&mut mesh, c, &g.repeats).unwrap();
    assert!(!mesh.indices.is_empty());
}

#[test]
fn corner_query_failure_aborts_the_detached_drawing() {
    let g: Golden = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/facade-details.json"
    ))
    .unwrap();
    let c = g.cases.iter().find(|c| !c.plan.bands.is_empty()).unwrap();
    let mut mesh = Mesh::new(Mode::Render, 150_000, &|| false).unwrap();
    let error = facade_details::draw(
        &mut Fixture {
            pen: pen(c.reflected),
            mesh: &mut mesh,
            repeats: &|slot| g.repeats[slot],
        },
        &c.plan,
        &c.settings,
        &c.request.key,
        &|_| Err("Clearance cancelled.".into()),
    )
    .unwrap_err();
    assert_eq!(error, "Clearance cancelled.");
    let mut fresh = Mesh::new(Mode::Render, 150_000, &|| false).unwrap();
    draw(&mut fresh, c, &g.repeats).unwrap();
    assert!(!fresh.indices.is_empty());
}
#[derive(Deserialize)]
struct CornerFace {
    matrix: [[f64; 3]; 4],
    shift: f64,
    ground: Vec<Rect>,
    body: Vec<Rect>,
    blocked: bool,
}
#[derive(Deserialize)]
struct Query {
    index: usize,
    zone: superspec_engine::worldspec::themes::kowloon::facade_details::corners::Zone,
    u: f64,
    heights: [f64; 2],
    result: Option<f64>,
}
#[derive(Deserialize)]
struct CornerCase {
    faces: Vec<CornerFace>,
    queries: Vec<Query>,
}
fn check_corners() {
    use superspec_engine::worldspec::themes::kowloon::facade_details::corners::{Face, Joins};
    #[derive(Deserialize)]
    struct Golden {
        cases: Vec<CornerCase>,
    }
    let g: Golden = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/facade-corners.json"
    ))
    .unwrap();
    for (i, c) in g.cases.iter().enumerate() {
        let joins = Joins::new(
            c.faces
                .iter()
                .map(|f| Face {
                    pen: Pen::new(Matrix(f.matrix), "corner").unwrap(),
                    shift: f.shift,
                    ground: f.ground.clone(),
                    body: f.body.clone(),
                })
                .collect(),
        )
        .unwrap();
        for (j, q) in c.queries.iter().enumerate() {
            let actual = joins
                .miter(q.index, q.zone, q.u, q.heights, &|index, _, _, depth| {
                    Ok(if c.faces[index].blocked { 0. } else { depth })
                })
                .unwrap();
            match (actual, q.result) {
                (Some(a), Some(b)) => {
                    assert!((a - b).abs() < 2e-10, "corner {i} query {j}: {a} != {b}")
                }
                (None, None) => (),
                _ => panic!("corner {i} query {j} presence"),
            }
        }
    }
}
