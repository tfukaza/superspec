use serde::Deserialize;
use superspec_engine::worldspec::themes::kowloon::conform::Edges;
use superspec_geometry::obstructions::{Budget, Triangle};
use superspec_geometry::{
    V3,
    math::{cross, sub},
};

#[derive(Deserialize)]
struct Case {
    vertices: Vec<V3>,
    triangle: Triangle,
    result: Vec<Triangle>,
}
#[derive(Deserialize)]
struct Reference {
    cases: Vec<Case>,
}
pub fn check_reference_receiver_edges() {
    let reference: Reference = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/receiver-edges.json"
    ))
    .unwrap();
    for case in reference.cases {
        let mut budget = Budget::new(1_000_000, &|| false).unwrap();
        let edges = Edges::new(case.vertices, &mut budget).unwrap();
        let result = edges.triangles(case.triangle, &mut budget).unwrap();
        assert_eq!(result, case.result);
        assert_eq!(result.len(), 4);
        let area = |face: Triangle| cross(sub(face[1], face[0]), sub(face[2], face[0]));
        let expected = area(case.triangle);
        let actual: V3 = std::array::from_fn(|axis| result.iter().map(|t| area(*t)[axis]).sum());
        for axis in 0..3 {
            assert!((expected[axis] - actual[axis]).abs() < 1e-12);
        }
        assert!(result.iter().all(|t| {
            area(*t)
                .iter()
                .zip(expected)
                .map(|(a, b)| a * b)
                .sum::<f64>()
                > 0.
        }));
    }
}
#[test]
fn shared_boundary_vertices_match_source_without_welding_thin_rims() {
    check_reference_receiver_edges();
}
#[test]
fn candidate_work_cancellation_and_long_edges_are_bounded() {
    let points = [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]];
    let mut budget = Budget::new(10000, &|| false).unwrap();
    let edges = Edges::new(points, &mut budget).unwrap();
    assert!(
        edges
            .triangles(
                [[0., 0., 0.], [1e6, 0., 0.], [0., 1., 0.]],
                &mut Budget::new(10000, &|| false).unwrap()
            )
            .is_err()
    );
    let calls = std::cell::Cell::new(0);
    let cancelled = || {
        calls.set(calls.get() + 1);
        calls.get() > 4
    };
    assert!(
        edges
            .triangles(points, &mut Budget::new(10000, &cancelled).unwrap())
            .is_err()
    );
    assert_eq!(edges.triangles(points, &mut budget).unwrap(), [points]);
    assert!(Edges::new([[f64::NAN, 0., 0.]], &mut budget).is_err());
}
