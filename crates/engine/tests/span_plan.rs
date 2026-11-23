use superspec_engine::worldspec::themes::kowloon::spans::{
    anchors::Anchor,
    plan::{self, Rule},
    walking::Walking,
};
use superspec_geometry::obstructions::{Budget, Obstructions};
#[path = "support/span_street.rs"]
mod street;
use street::{RULE, street};
#[test]
fn span_plans_are_seeded_repeatable_and_reference_their_anchors() {
    let mut budget = Budget::new(32_000_000, &|| false).unwrap();
    let s = street(4., &mut budget);
    let anchors = s.utility.len() + s.windows.len();
    for seed in ["span-plan/1", "span-plan/4", "span-plan/8"] {
        let first = s.plan(RULE, seed, &mut budget).unwrap();
        assert_eq!(
            serde_json::to_value(&first).unwrap(),
            serde_json::to_value(s.plan(RULE, seed, &mut budget).unwrap()).unwrap(),
            "{seed}"
        );
        for cable in &first.cables {
            assert!(cable.from < anchors && cable.to < anchors, "{seed}");
            for strand in &cable.strands {
                assert!(strand.points.len() >= 2);
                assert!(strand.points.iter().flatten().all(|v| v.is_finite()));
            }
        }
        for rack in &first.racks {
            assert!(rack.anchor < anchors, "{seed}");
        }
        for line in &first.clotheslines {
            assert!(line.from < anchors && line.to < anchors, "{seed}");
        }
        for sign in &first.signs {
            assert!(sign.from < anchors && sign.to < anchors, "{seed}");
            assert!(sign.length > 0.);
        }
    }
}
#[test]
fn invalid_cancelled_and_exhausted_span_jobs_leave_inputs_reusable() {
    let mut budget = Budget::new(32_000_000, &|| false).unwrap();
    let s = street(6., &mut budget);
    let seed = "span-plan/1";
    let rule = Rule {
        clotheslines: 0.,
        ..RULE
    };
    let floors = Walking::new(&[], &mut budget).unwrap();
    for (rule, anchors, cancel, work) in [
        (
            Rule {
                reach: f64::NAN,
                ..rule
            },
            s.utility.clone(),
            false,
            1_000_000,
        ),
        (
            rule,
            vec![Anchor {
                plane: 99,
                ..s.utility[0].clone()
            }],
            false,
            1_000_000,
        ),
        (rule, s.utility.clone(), true, 1_000_000),
        (rule, s.utility.clone(), false, 10),
    ] {
        let obstacles = Obstructions::new(&s.triangles, &mut budget).unwrap();
        let predicate = || cancel;
        let mut work = Budget::new(work, &predicate).unwrap();
        assert!(
            plan::build(
                rule, seed, &s.planes, &anchors, &s.windows, &floors, obstacles, &mut work
            )
            .is_err()
        );
    }
    let obstacles = Obstructions::new(&s.triangles, &mut budget).unwrap();
    let result = plan::build(
        rule,
        seed,
        &s.planes,
        &s.utility,
        &s.windows,
        &floors,
        obstacles,
        &mut budget,
    )
    .unwrap();
    assert!(!result.cables.is_empty());
}
