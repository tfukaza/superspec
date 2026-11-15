use serde::Deserialize;
use serde_json::{Value, json};
use superspec_engine::worldspec::themes::kowloon::{
    building::{Plan, Request, Shape, footprint::Footprint},
    clearance::{Collision, RenderTriangle},
    facade_plan::Zone,
    massing::Rect,
};
use superspec_geometry::obstructions::Budget;
use superspec_geometry::{Matrix, V3};
#[path = "support/sign_reference.rs"]
#[allow(dead_code)]
mod reference;
#[derive(Deserialize)]
struct Physical {
    points: [V3; 3],
    object: String,
    solid: Option<String>,
}
#[derive(Deserialize)]
struct Render {
    points: [V3; 3],
    visible: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Input {
    host: String,
    matrix: [V3; 4],
    width: Option<f64>,
    depth: Option<f64>,
    height: f64,
    #[serde(default)]
    bottom_center: bool,
    stories: usize,
    shops: f64,
    base_wall: Option<String>,
    #[serde(rename = "loop")]
    loop_: Option<Vec<V3>>,
}
impl Input {
    fn request(&self) -> Request {
        Request {
            host: self.host.clone(),
            matrix: Matrix(self.matrix),
            shape: if let Some(p) = &self.loop_ {
                Shape::Polygon(Footprint::new(p, self.height).unwrap())
            } else {
                Shape::Box {
                    width: self.width.unwrap(),
                    depth: self.depth.unwrap(),
                    height: self.height,
                    bottom_center: self.bottom_center,
                }
            },
            stories: Some(self.stories),
            shops: Some(self.shops),
            base_wall: self.base_wall.clone(),
        }
    }
}
#[derive(Deserialize)]
struct Case {
    seed: String,
    requests: Vec<Input>,
    sites: Vec<Value>,
    warnings: Value,
}
#[derive(Deserialize)]
struct Golden {
    physical: Vec<Physical>,
    render: Vec<Render>,
    cases: Vec<Case>,
}
pub fn check_reference_building_placement() {
    let g: Golden = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/building-placement.json"
    ))
    .unwrap();
    let render: Vec<_> = g
        .render
        .iter()
        .map(|t| RenderTriangle {
            points: t.points,
            visible: t.visible,
        })
        .collect();
    for c in g.cases {
        let mut budget = Budget::new(16_000_000, &|| false).unwrap();
        let plan = Plan::new(
            c.requests.iter().map(Input::request).collect(),
            &c.seed,
            g.physical
                .iter()
                .map(|t| Collision {
                    points: t.points,
                    object: t.object.clone(),
                    solid: t.solid.clone(),
                })
                .collect(),
            &render,
            &mut budget,
        )
        .unwrap();
        let warnings: Vec<_> = plan
            .warnings
            .iter()
            .map(|w| {
                let min: V3 = std::array::from_fn(|a| {
                    w.corners.iter().map(|p| p[a]).fold(f64::INFINITY, f64::min)
                });
                let max: V3 = std::array::from_fn(|a| {
                    w.corners
                        .iter()
                        .map(|p| p[a])
                        .fold(f64::NEG_INFINITY, f64::max)
                });
                json!({"code":w.code,"host":w.host,"bounds":{"min":min,"max":max}})
            })
            .collect();
        reference::close(&json!(warnings), &c.warnings);
        assert_eq!(plan.sites().len(), c.sites.len());
        for (i, (s, expected)) in plan.sites().iter().zip(&c.sites).enumerate() {
            reference::close(&json!(s.matrix.0), &expected["matrix"]);
            reference::close(&json!(s.lots), &expected["lots"]);
            reference::close(&json!(s.blocks), &expected["blocks"]);
            reference::close(&json!(s.foreign), &expected["foreign"]);
            reference::close(&json!(s.shops), &expected["shops"]);
            reference::close(&json!(s.footprint), &expected["footprint"]);
            if let Some(f) = &s.footprint {
                reference::close(&json!(f.inset(0.14).unwrap()), &expected["inner"]);
            }
            let mut count = 0;
            for lot in 0..s.lots.len() {
                let faces = plan.facades(i, lot, &mut budget).unwrap();
                for f in &faces {
                    let e = &expected["faces"][count];
                    count += 1;
                    assert_eq!(lot, e["lot"].as_u64().unwrap() as usize);
                    let actual = json!({"matrix":f.pen.matrix.0,"mirrored":f.pen.mirrored,"role":f.role,"key":f.key,"width":f.width,"shift":f.shift,"bodySpan":f.body_span,"ground":f.ground,"body":f.body});
                    for (key, value) in actual.as_object().unwrap() {
                        reference::close(value, &e[key]);
                    }
                    for q in e["probes"].as_array().unwrap() {
                        let zone = if q["zone"] == "ground" {
                            Zone::Ground
                        } else {
                            Zone::Body
                        };
                        let rect: Rect = serde_json::from_value(q["rect"].clone()).unwrap();
                        let result = plan
                            .reach(f, zone, rect, q["limit"].as_f64().unwrap(), &mut budget)
                            .unwrap();
                        reference::close(&json!(result), &q["result"]);
                    }
                    let span = [0.2, (f.width - 0.2).min(2.5)];
                    for q in e["hollow"].as_array().unwrap() {
                        reference::close(
                            &json!(
                                plan.hollow(
                                    f,
                                    span,
                                    2.6,
                                    q["limit"].as_f64().unwrap(),
                                    &mut budget
                                )
                                .unwrap()
                            ),
                            &q["result"],
                        );
                    }
                    for q in e["frontage"].as_array().unwrap() {
                        assert_eq!(
                            plan.frontage(f, span, q["depth"].as_f64().unwrap(), 1.4, &mut budget)
                                .unwrap(),
                            q["result"].as_bool().unwrap()
                        );
                    }
                }
            }
            assert_eq!(count, expected["faces"].as_array().unwrap().len());
        }
    }
}
#[test]
fn neighboring_buildings_preserve_exposure_clearance_frontage_and_polygon_edges() {
    check_reference_building_placement();
}
#[test]
fn invalid_footprints_and_cancelled_plans_leave_inputs_reusable() {
    let square = [[0., 0., 0.], [6., 0., 0.], [6., 6., 0.], [0., 6., 0.]];
    let good = Footprint::new(&square, 9.5).unwrap();
    assert!(Footprint::new(&square, 60.).is_err());
    assert!(Footprint::new(&square.map(|p| [p[0] * 10., p[1], p[2]]), 9.5).is_err());
    assert!(good.lot(16, "test").is_err());
    assert!(good.inset(4.).is_err());
    assert!(
        Footprint::new(
            &[[0., 0., 0.], [6., 0., 0.], [2., 2., 0.], [0., 6., 0.]],
            9.5
        )
        .is_err()
    );
    assert!(
        Footprint::new(
            &[[0., 0., 0.], [6., 0., 0.], [6., 6., 1.], [0., 6., 0.]],
            9.5
        )
        .is_err()
    );
    let request = || Request {
        host: "home".into(),
        matrix: Matrix::default(),
        shape: Shape::Polygon(good.clone()),
        stories: Some(3),
        shops: Some(0.),
        base_wall: None,
    };
    assert!(
        Plan::new(
            vec![request()],
            "test",
            vec![],
            &[],
            &mut Budget::new(1_000_000, &|| true).unwrap()
        )
        .is_err()
    );
    assert!(
        Plan::new(
            vec![request(), request()],
            "test",
            vec![],
            &[],
            &mut Budget::new(1_000_000, &|| false).unwrap()
        )
        .is_err()
    );
    assert!(
        Plan::new(
            vec![request()],
            "test",
            vec![],
            &[],
            &mut Budget::new(1_000_000, &|| false).unwrap()
        )
        .is_ok()
    );
    assert_eq!(good.points(), square);
}
