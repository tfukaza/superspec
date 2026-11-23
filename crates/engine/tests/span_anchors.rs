use superspec_engine::worldspec::themes::kowloon::spans::{
    anchors::{self, Face, Plane, Rect, Surroundings},
    neighbors::Index,
    walking::{Surface, Walking},
};
use superspec_geometry::obstructions::{Budget, Obstructions};
#[test]
fn wall_planes_window_fixings_and_neighbors_are_repeatable() {
    let mut budget = Budget::new(16_000_000, &|| false).unwrap();
    let quad = |x0: f64, x1: f64, y: f64, z0: f64, z1: f64| {
        let p = [[x0, y, z0], [x1, y, z0], [x1, y, z1], [x0, y, z1]];
        [[p[0], p[1], p[2]], [p[0], p[2], p[3]]]
    };
    let mut faces = vec![];
    for (glazing, points) in [
        (false, quad(0., 8., 0., 0., 9.)),
        (true, quad(2., 3., 0.02, 3.1, 4.6)),
    ] {
        for points in points {
            faces.push(Face {
                host: 0,
                glazing,
                points,
                stored_normal: [0., -1., 0.],
            });
        }
    }
    let planes = anchors::planes(&["tower".into()], &faces, &mut budget).unwrap();
    assert_eq!(planes.len(), 1);
    assert_eq!(planes[0].windows().len(), 1);
    let obstacles = Obstructions::new(
        &faces.iter().map(|f| f.points).collect::<Vec<_>>(),
        &mut budget,
    )
    .unwrap();
    let floors = Walking::new(
        &[
            [[-10., -10., 0.], [20., -10., 0.], [20., 20., 0.]],
            [[-10., -10., 0.], [20., 20., 0.], [-10., 20., 0.]],
        ]
        .map(|points| Surface {
            points,
            floor_or_stairs: true,
            collidable: true,
        }),
        &mut budget,
    )
    .unwrap();
    let surroundings = Surroundings {
        obstructions: &obstacles,
        floors: &floors,
    };
    let p = &planes[0];
    let run = |budget: &mut Budget<'_>| {
        (
            serde_json::to_value(anchors::utility(p, 0, "tower/0", &surroundings, budget).unwrap())
                .unwrap(),
            serde_json::to_value(anchors::windows(p, 0, "tower/0", &surroundings, budget).unwrap())
                .unwrap(),
        )
    };
    let first = run(&mut budget);
    assert_eq!(first, run(&mut budget), "seeded fixings are repeatable");
    assert!(!first.0.as_array().unwrap().is_empty());
    let windows = anchors::windows(p, 0, "tower/0", &surroundings, &mut budget).unwrap();
    assert!(!windows.is_empty());
    assert!(windows.iter().all(|a| a.plane == 0 && a.window.is_some()));
    let points = [[0., 0., 0.], [1., 0., 0.], [4., 0., 0.]];
    let index = Index::new(&points, 1.5, 1., &mut budget).unwrap();
    assert_eq!(index.near(0, &mut budget).unwrap(), [1]);
    assert_eq!(index.near(1, &mut budget).unwrap(), [0]);
    assert!(index.near(2, &mut budget).unwrap().is_empty());
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
