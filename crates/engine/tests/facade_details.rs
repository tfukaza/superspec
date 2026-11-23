use superspec_engine::worldspec::themes::kowloon::{
    facade_details::{self, Request, Settings},
    fixture::Fixture,
    massing::Rect,
    mesh::{Mesh, Mode},
    pen::Pen,
    weathered,
};
use superspec_geometry::Matrix;
struct Case {
    request: Request,
    settings: Settings,
    ground_rects: Vec<Rect>,
    thresholds: Vec<[f64; 2]>,
    blockers: Vec<Rect>,
    miters: [Option<f64>; 2],
    reflected: bool,
}
fn rect(u0: f64, u1: f64, v0: f64, v1: f64) -> Rect {
    Rect { u0, u1, v0, v1 }
}
/// A three-storey front with two windows, a doorway threshold and optional blocked trim.
fn case(reflected: bool, blocked: bool, mitered: bool) -> Case {
    Case {
        request: Request {
            key: "lot/front".into(),
            ground: 2.8,
            roof: 8.8,
            top: 9.2,
            stories: 3,
            story_height: 3.,
            body_span: [0., 6.],
            walls: vec![rect(0., 6., 2.8, 9.2)],
            openings: vec![rect(1., 2.3, 3.4, 4.8), rect(3.5, 4.8, 6.1, 7.8)],
        },
        settings: serde_json::from_value(serde_json::json!({"weathering":{"seed":"facade/0","wear":0.25,"grime":0.6,"rust":0.4,"textureDensity":64,"metal":{"material":"metal","repeat":1.5},"sheet":{"material":"sheet","repeat":1.5},"paint":{"material":"paint","repeat":1}},"concrete":{"material":"concrete","repeat":2.7},"detail":1,"trim":[0.8,0.77,0.65]})).unwrap(),
        ground_rects: vec![rect(0., 6., 0., 2.8)],
        thresholds: vec![[2.5, 3.5]],
        blockers: if blocked {
            vec![rect(3., 6., 0., 9.2)]
        } else {
            vec![]
        },
        miters: if mitered {
            [Some(0.2), Some(0.3)]
        } else {
            [None; 2]
        },
        reflected,
    }
}
fn repeat(slot: &str) -> f64 {
    match slot {
        "wall" => 1.3,
        "concrete" => 2.7,
        "metal" | "sheet" => 1.5,
        _ => 1.,
    }
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
fn draw(mesh: &mut Mesh<'_>, c: &Case) -> Result<(), String> {
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
    let mut d = Fixture {
        pen: pen(c.reflected),
        mesh,
        repeats: &|s| repeat(s),
    };
    facade_details::draw(&mut d, &plan, &c.settings, &c.request.key, &miters)?;
    facade_details::plinth(
        &mut d,
        &c.ground_rects,
        &c.thresholds,
        c.request.ground,
        &c.settings,
        &c.request.key,
        &reach,
        &miters,
    )
}
#[test]
fn detail_receivers_partition_triangles_and_surveys_match_render() {
    for (reflected, blocked, mitered) in [
        (false, false, false),
        (true, false, false),
        (false, true, false),
        (false, false, true),
    ] {
        let c = case(reflected, blocked, mitered);
        let label = format!("reflected={reflected} blocked={blocked} mitered={mitered}");
        let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
        draw(&mut mesh, &c).unwrap();
        assert!(mesh.triangle_count() > 0, "{label}");
        let mut survey = Mesh::new(Mode::Survey, 150000, &|| false).unwrap();
        draw(&mut survey, &c).unwrap();
        assert_eq!(survey.triangle_count(), mesh.triangle_count(), "{label}");
        for (corner, (actual, &index)) in survey.survey.iter().zip(&mesh.indices).enumerate() {
            for axis in 0..3 {
                assert!(
                    (actual[axis] - mesh.vertices[index as usize].position[axis]).abs() <= 2e-10,
                    "{label} corner {corner}"
                );
            }
        }
        let triangles = mesh.triangle_count();
        let plan = weathered::finish(&mut mesh, &|| false).unwrap();
        let mut covered = vec![false; triangles];
        for receiver in plan.receivers() {
            for seen in covered
                .iter_mut()
                .skip(receiver.first_triangle)
                .take(receiver.triangle_count)
            {
                assert!(!*seen, "{label}: receivers overlap");
                *seen = true;
            }
            assert!(receiver.first_triangle + receiver.triangle_count <= triangles);
        }
        assert!(covered.iter().any(|c| *c), "{label}");
    }
}
#[test]
fn cancelled_or_invalid_planning_recovers_without_emission() {
    let mut c = case(false, false, false);
    assert!(facade_details::plan(&c.request, "test", &|_, limit| Ok(limit), &|| true).is_err());
    assert!(facade_details::plan(&c.request, "test", &|_, _| Ok(f64::NAN), &|| false).is_err());
    let ground = c.request.ground;
    c.request.ground = f64::NAN;
    let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
    assert!(draw(&mut mesh, &c).is_err());
    assert!(mesh.indices.is_empty());
    c.request.ground = ground;
    draw(&mut mesh, &c).unwrap();
    assert!(!mesh.indices.is_empty());
}

#[test]
fn corner_query_failure_aborts_the_detached_drawing() {
    let c = case(false, false, false);
    let plan = facade_details::plan(
        &c.request,
        &c.settings.weathering.seed,
        &|_, limit| Ok(limit),
        &|| false,
    )
    .unwrap();
    assert!(!plan.bands.is_empty());
    let mut mesh = Mesh::new(Mode::Render, 150_000, &|| false).unwrap();
    let error = facade_details::draw(
        &mut Fixture {
            pen: pen(c.reflected),
            mesh: &mut mesh,
            repeats: &|slot| repeat(slot),
        },
        &plan,
        &c.settings,
        &c.request.key,
        &|_| Err("Clearance cancelled.".into()),
    )
    .unwrap_err();
    assert_eq!(error, "Clearance cancelled.");
    let mut fresh = Mesh::new(Mode::Render, 150_000, &|| false).unwrap();
    draw(&mut fresh, &c).unwrap();
    assert!(!fresh.indices.is_empty());
}
