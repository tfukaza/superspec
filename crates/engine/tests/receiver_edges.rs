use superspec_engine::worldspec::themes::kowloon::conform::Edges;
use superspec_geometry::obstructions::{Budget, Triangle};
use superspec_geometry::{
    V3,
    math::{add, cross, scale, sub},
};

/// A receiver triangle whose edges carry three neighbouring midpoints, plus decoys: duplicates,
/// a vertex lifted off the plane by 1e-10, one lifted by 1 mm (a thin rim) and an interior point.
fn case(origin: V3, u: V3, v: V3, size: f64) -> (Vec<V3>, Triangle) {
    let at = |a: f64, b: f64, lift: f64| {
        let n = cross(u, v);
        add(
            add(origin, add(scale(u, a * size), scale(v, b * size))),
            scale(n, lift),
        )
    };
    let vertices = vec![
        at(0., 0., 0.),
        at(2., 0., 0.),
        at(0., 2., 0.),
        at(1., 0., 0.),
        at(1., 1., 0.),
        at(0., 1., 0.),
        at(1., 0., 0.),
        at(0., 1., 0.),
        at(1., 0., 1e-10),
        at(1., 1., 0.001),
        at(0.5, 0.5, 0.),
    ];
    (vertices, [at(0., 0., 0.), at(2., 0., 0.), at(0., 2., 0.)])
}

#[test]
fn shared_boundary_vertices_split_receivers_preserving_area_and_orientation() {
    for (origin, u, v, size) in [
        ([0., 0., 0.], [1., 0., 0.], [0., 1., 0.], 1.),
        ([100., -70., 0.4], [1., 0., 0.], [0., 1., 0.], 1.),
        ([0., 3., 0.], [1., 0., 0.], [0., 0., 1.], 1.),
        ([0., 0., 0.], [1., 0., 0.], [0., 1., 0.], 0.02),
    ] {
        let (vertices, triangle) = case(origin, u, v, size);
        let mut budget = Budget::new(1_000_000, &|| false).unwrap();
        let edges = Edges::new(vertices, &mut budget).unwrap();
        let result = edges.triangles(triangle, &mut budget).unwrap();
        // The three edge midpoints split the triangle into four; the lifted rim is not welded.
        assert_eq!(result.len(), 4);
        let rim = case(origin, u, v, size).0[9];
        assert!(result.iter().flatten().all(|p| *p != rim));
        let area = |face: Triangle| cross(sub(face[1], face[0]), sub(face[2], face[0]));
        let expected = area(triangle);
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
