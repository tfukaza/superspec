use serde::Deserialize;
use serde_json::Value;
use superspec_engine::worldspec::themes::kowloon::spans::{
    anchors::{self, Face, Plane, Rect, Surroundings},
    neighbors::Index,
    walking::{Surface, Walking},
};
use superspec_geometry::V3;
use superspec_geometry::obstructions::{Budget, Obstructions, Triangle};
#[path = "support/sign_reference.rs"]
#[allow(dead_code)]
mod reference;
#[derive(Deserialize)]
struct InputFace {
    host: usize,
    glazing: bool,
    points: Triangle,
    stored_normal: V3,
}
#[derive(Deserialize)]
struct Case {
    seed: String,
    hosts: Vec<String>,
    faces: Vec<InputFace>,
    physical: Vec<Triangle>,
    expected: Vec<Value>,
}
#[derive(Deserialize)]
struct Neighbors {
    points: Vec<V3>,
    radius: f64,
    rise: f64,
    expected: Vec<Vec<usize>>,
}
#[derive(Deserialize)]
struct Golden {
    cases: Vec<Case>,
    neighbors: Neighbors,
}
pub fn check_reference_span_anchors() {
    let g: Golden = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/span-anchors.json"
    ))
    .unwrap();
    for c in g.cases {
        let mut budget = Budget::new(16_000_000, &|| false).unwrap();
        let faces = c
            .faces
            .into_iter()
            .map(|f| Face {
                host: f.host,
                glazing: f.glazing,
                points: f.points,
                stored_normal: f.stored_normal,
            })
            .collect::<Vec<_>>();
        let planes = anchors::planes(&c.hosts, &faces, &mut budget).unwrap();
        assert_eq!(planes.len(), c.expected.len(), "{} plane count", c.seed);
        let obstacles = Obstructions::new(
            &faces.iter().map(|f| f.points).collect::<Vec<_>>(),
            &mut budget,
        )
        .unwrap();
        let floors = Walking::new(
            &c.physical
                .into_iter()
                .map(|points| Surface {
                    points,
                    floor_or_stairs: true,
                    collidable: true,
                })
                .collect::<Vec<_>>(),
            &mut budget,
        )
        .unwrap();
        let surroundings = Surroundings {
            obstructions: &obstacles,
            floors: &floors,
        };
        for (i, (p, expected)) in planes.iter().zip(c.expected).enumerate() {
            let mut metadata = expected.clone();
            for key in ["queries", "utility", "anchors"] {
                metadata.as_object_mut().unwrap().remove(key);
            }
            reference::close_at(
                &serde_json::to_value(p).unwrap(),
                &metadata,
                &format!("{}/{i}", c.seed),
            );
            for query in expected["queries"].as_array().unwrap() {
                let rect: Rect = serde_json::from_value(query["rect"].clone()).unwrap();
                assert_eq!(
                    p.covers(rect, &mut budget).unwrap(),
                    query["covers"].as_bool().unwrap(),
                    "{}/{i} {rect:?}",
                    c.seed
                );
            }
            let seed = format!("{}/{}/{i}", c.seed, p.host());
            for _ in 0..2 {
                reference::close_at(
                    &serde_json::to_value(
                        anchors::utility(p, i, &seed, &surroundings, &mut budget).unwrap(),
                    )
                    .unwrap(),
                    &expected["utility"],
                    &format!("{}/{i}/utility", c.seed),
                );
                reference::close_at(
                    &serde_json::to_value(
                        anchors::windows(p, i, &seed, &surroundings, &mut budget).unwrap(),
                    )
                    .unwrap(),
                    &expected["anchors"],
                    &format!("{}/{i}/windows", c.seed),
                );
            }
        }
    }
    let n = g.neighbors;
    let mut budget = Budget::new(1_000_000, &|| false).unwrap();
    let index = Index::new(&n.points, n.radius, n.rise, &mut budget).unwrap();
    for (i, expected) in n.expected.iter().enumerate() {
        assert_eq!(
            index.near(i, &mut budget).unwrap(),
            *expected,
            "neighbor {i}"
        );
    }
}
#[test]
fn wall_planes_window_fixings_and_ordered_neighbors_match_source() {
    check_reference_span_anchors();
}
#[test]
fn coverage_tolerance_boundaries_and_failure_recovery_are_explicit() {
    let mut budget = Budget::new(1_000_000, &|| false).unwrap();
    let make = |end: f64| {
        [
            [[0., 0., 0.], [end, 0., 0.], [end, 0., 2.]],
            [[0., 0., 0.], [end, 0., 2.], [0., 0., 2.]],
            [[1., 0., 0.], [2., 0., 0.], [2., 0., 2.]],
            [[1., 0., 0.], [2., 0., 2.], [1., 0., 2.]],
        ]
    };
    let query = Rect {
        u0: -2.,
        u1: 0.,
        z0: 0.,
        z1: 2.,
    };
    for (end, covers) in [(0.991, true), (0.989, false)] {
        let p = Plane::new("host", [0., -1., 0.], 0., &make(end), &mut budget).unwrap();
        assert_eq!(p.point(-1., 3., 0.1), [1., -0.1, 3.]);
        assert_eq!(p.covers(query, &mut budget).unwrap(), covers);
        assert!(
            p.covers(
                Rect {
                    u0: -2.001,
                    ..query
                },
                &mut budget
            )
            .is_ok_and(|v| !v)
        );
        assert!(
            p.covers(
                Rect {
                    u0: f64::NAN,
                    ..query
                },
                &mut budget
            )
            .is_err()
        );
        assert!(
            p.covers(query, &mut Budget::new(100, &|| true).unwrap())
                .is_err()
        );
        assert_eq!(p.covers(query, &mut budget).unwrap(), covers);
    }
    assert!(Plane::new("host", [0., 0., 1.], 0., &make(1.), &mut budget).is_err());
    assert!(Index::new(&[], 0., 1., &mut budget).is_err());
    assert!(Index::new(&[[f64::NAN, 0., 0.]], 1., 1., &mut budget).is_err());
    assert!(Index::new(&[[0.; 3]], 1., 1., &mut Budget::new(1, &|| false).unwrap()).is_err());
    let index = Index::new(&[[0.; 3], [0.; 3]], 1., 1., &mut budget).unwrap();
    assert!(
        index
            .near(0, &mut Budget::new(100, &|| true).unwrap())
            .is_err()
    );
    assert_eq!(index.near(0, &mut budget).unwrap(), [1]);
    assert!(
        Index::new(&[], 1., 1., &mut budget)
            .unwrap()
            .near(0, &mut budget)
            .is_err()
    );
}

#[test]
fn extraction_and_generation_enforce_limits_and_recover_from_interrupted_work() {
    use std::cell::Cell;
    let mut budget = Budget::new(4_000_000, &|| false).unwrap();
    let patch = |width, height| {
        [
            [[0., 0., 0.], [width, 0., 0.], [width, 0., height]],
            [[0., 0., 0.], [width, 0., height], [0., 0., height]],
        ]
    };
    let face = |host, points| Face {
        host,
        points,
        glazing: false,
        stored_normal: [0., -1., 0.],
    };
    assert!(anchors::planes(&["a".into()], &[face(1, patch(8., 9.)[0])], &mut budget).is_err());
    assert!(anchors::planes(&["a".into(), "a".into()], &[], &mut budget).is_err());
    assert!(anchors::planes(&["a".into()], &[face(0, [[f64::NAN; 3]; 3])], &mut budget).is_err());
    assert!(Plane::new("a", [0., -1., 0.], 0., &patch(2000., 2000.), &mut budget).is_err());
    let large = Plane::new("a", [0., -1., 0.], 0., &patch(2000., 60.), &mut budget).unwrap();
    let obstacles = Obstructions::new(&[], &mut budget).unwrap();
    let floors = Walking::new(&[], &mut budget).unwrap();
    let surroundings = Surroundings {
        obstructions: &obstacles,
        floors: &floors,
    };
    assert!(anchors::utility(&large, 0, "test", &surroundings, &mut budget).is_err());
    let small = Plane::new("a", [0., -1., 0.], 0., &patch(8., 9.), &mut budget).unwrap();
    let calls = Cell::new(0);
    let cancel = || {
        calls.set(calls.get() + 1);
        calls.get() > 12
    };
    assert!(
        anchors::utility(
            &small,
            0,
            "test",
            &surroundings,
            &mut Budget::new(100_000, &cancel).unwrap()
        )
        .is_err()
    );
    assert!(calls.get() > 12);
    assert!(
        anchors::utility(
            &small,
            0,
            "test",
            &surroundings,
            &mut Budget::new(1, &|| false).unwrap()
        )
        .is_err()
    );
    assert!(
        anchors::windows(
            &small,
            0,
            "test",
            &surroundings,
            &mut Budget::new(100_000, &|| true).unwrap()
        )
        .is_err()
    );
    assert!(
        !anchors::utility(&small, 0, "test", &surroundings, &mut budget)
            .unwrap()
            .is_empty()
    );
    assert!(
        anchors::windows(&small, 0, "test", &surroundings, &mut budget)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn extraction_bounds_the_sum_of_retained_plane_indexes() {
    let mut faces = vec![];
    for i in 0..20 {
        let y = i as f64 * 2.;
        for points in [
            [[0., y, 0.], [100., y, 0.], [100., y, 100.]],
            [[0., y, 0.], [100., y, 100.], [0., y, 100.]],
        ] {
            faces.push(Face {
                host: 0,
                glazing: false,
                points,
                stored_normal: [0., -1., 0.],
            });
        }
    }
    let mut budget = Budget::new(4_000_000, &|| false).unwrap();
    let result = anchors::planes(&["a".into()], &faces, &mut budget);
    assert!(result.is_err_and(|e| e.contains("allowance")));
    let planes = anchors::planes(&["a".into()], &faces[..4], &mut budget).unwrap();
    assert_eq!(planes.len(), 2);
}
