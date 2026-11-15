use serde::Deserialize;
use superspec_engine::worldspec::themes::kowloon::spans::{
    catenary::hanging,
    walking::{Surface, Walking},
};
use superspec_geometry::V3;
use superspec_geometry::obstructions::Budget;

#[derive(Deserialize)]
struct Curve {
    from: V3,
    to: V3,
    slack: f64,
    segments: usize,
    points: Vec<V3>,
}
#[derive(Deserialize)]
struct Face {
    points: [V3; 3],
    kind: String,
    collidable: bool,
}
#[derive(Deserialize)]
struct Query {
    point: V3,
    clearance: f64,
    below: Option<f64>,
    clears: bool,
}
#[derive(Deserialize)]
struct Golden {
    curves: Vec<Curve>,
    surfaces: Vec<Face>,
    queries: Vec<Query>,
}

pub fn check_reference_span_foundations() {
    let golden: Golden = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/span-foundations.json"
    ))
    .unwrap();
    let mut budget = Budget::new(10_000_000, &|| false).unwrap();
    for (i, c) in golden.curves.iter().enumerate() {
        let actual = hanging(c.from, c.to, c.slack, c.segments, &mut budget).unwrap();
        assert_eq!(actual.len(), c.points.len());
        assert_eq!(actual[0], c.from);
        assert_eq!(actual[c.segments], c.to);
        for (j, (a, b)) in actual.iter().zip(&c.points).enumerate() {
            for k in 0..3 {
                assert!(
                    (a[k] - b[k]).abs() < 1e-7,
                    "curve {i} sample {j} axis {k}: {} vs {}",
                    a[k],
                    b[k]
                );
            }
        }
    }
    let faces = golden
        .surfaces
        .into_iter()
        .map(|f| Surface {
            points: f.points,
            floor_or_stairs: matches!(f.kind.as_str(), "floor" | "stairs"),
            collidable: f.collidable,
        })
        .collect::<Vec<_>>();
    let walking = Walking::new(&faces, &mut budget).unwrap();
    for (i, q) in golden.queries.iter().enumerate() {
        let actual = walking.below(q.point, &mut budget).unwrap();
        match (actual, q.below) {
            (Some(a), Some(b)) => assert!((a - b).abs() < 1e-10, "height query {i}: {a} vs {b}"),
            _ => assert_eq!(actual, q.below, "height query {i}"),
        }
        assert_eq!(
            walking.clears(q.point, q.clearance, &mut budget).unwrap(),
            q.clears,
            "clearance query {i}"
        );
    }
}
#[test]
fn hanging_curves_and_physical_floor_heights_match_source() {
    check_reference_span_foundations();
}

#[test]
fn invalid_cancelled_and_exhausted_queries_recover() {
    let mut budget = Budget::new(100_000, &|| false).unwrap();
    let a = [0., 0., 4.];
    let b = [5., 0., 5.];
    for (point, slack, segments) in [
        ([f64::NAN, 0., 0.], 0.1, 8),
        (b, f64::INFINITY, 8),
        (b, 0.1, 0),
        (b, 0.1, 4097),
    ] {
        assert!(hanging(a, point, slack, segments, &mut budget).is_err());
    }
    let mut cancelled = Budget::new(100_000, &|| true).unwrap();
    assert!(hanging(a, b, 0.1, 8, &mut cancelled).is_err());
    assert!(Walking::new(&[], &mut cancelled).is_err());
    assert!(hanging(a, b, 0.1, 8, &mut Budget::new(1, &|| false).unwrap()).is_err());
    let curve = hanging(a, b, 0.1, 8, &mut budget).unwrap();
    assert_eq!(curve[0], a);
    assert_eq!(curve[8], b);
    let walking = Walking::new(&[], &mut budget).unwrap();
    assert!(walking.below([f64::NAN, 0., 0.], &mut budget).is_err());
    assert!(walking.clears(a, -1., &mut budget).is_err());
    assert!(walking.below(a, &mut cancelled).is_err());
    assert_eq!(walking.below(a, &mut budget).unwrap(), None);
    assert!(walking.clears(a, 2.6, &mut budget).unwrap());
}

#[test]
fn walking_rejects_downward_degenerate_and_exactly_steep_surfaces() {
    let mut budget = Budget::new(100_000, &|| false).unwrap();
    let faces = [
        [[0., 0., 0.], [2., 0., 0.], [0., 2., 0.]],
        [[0., 0., 1.], [0., 2., 1.], [2., 0., 1.]],
        [[0., 0., 2.], [2., 0., 2.], [0., 2., 4.]],
        [[0., 0., 3.], [1., 0., 3.], [2., 0., 3.]],
    ]
    .map(|points| Surface {
        points,
        floor_or_stairs: true,
        collidable: true,
    });
    let walking = Walking::new(&faces, &mut budget).unwrap();
    assert_eq!(
        walking.below([0.5, 0.5, 10.], &mut budget).unwrap(),
        Some(0.)
    );
    assert_eq!(
        walking.below([0.5, 0.5, -0.05], &mut budget).unwrap(),
        Some(0.)
    );
    assert_eq!(
        walking.below([0.5, 0.5, -0.050001], &mut budget).unwrap(),
        None
    );
    assert!(walking.clears([0.5, 0.5, 2.6], 2.6, &mut budget).unwrap());
    assert!(
        !walking
            .clears([0.5, 0.5, 2.599999], 2.6, &mut budget)
            .unwrap()
    );
}

#[test]
fn admitted_slack_and_segment_bounds_keep_exact_supports() {
    let a = [0., 0., 4.];
    let b = [8., 0., 4.];
    let mut budget = Budget::new(100_000, &|| false).unwrap();
    let taut = hanging(a, b, -1., 8, &mut budget).unwrap();
    assert!(taut.iter().all(|p| p[2] == 4.));
    let slack = hanging(a, b, 1., 4096, &mut budget).unwrap();
    assert_eq!(slack.len(), 4097);
    assert_eq!(slack[0], a);
    assert_eq!(slack[4096], b);
    assert!(slack[2048][2] < 4.);
    for j in 0..=4096 {
        assert!((slack[j][2] - slack[4096 - j][2]).abs() < 1e-10);
    }
    assert!(hanging(a, [1_000_001., 0., 4.], 0.1, 8, &mut budget).is_err());
}
