use serde::Deserialize;
use serde_json::Value;
use superspec_engine::worldspec::themes::kowloon::spans::{
    anchors::{Anchor, Plane},
    plan::{self, Rule},
    walking::{Surface, Walking},
};
use superspec_geometry::V3;
use superspec_geometry::obstructions::{Budget, Obstructions, Triangle};
#[path = "support/sign_reference.rs"]
#[allow(dead_code)]
mod reference;
#[derive(Deserialize)]
struct PlaneInput {
    host: String,
    normal: V3,
    offset: f64,
    faces: Vec<Triangle>,
}
#[derive(Deserialize)]
struct Case {
    seed: String,
    rule: Rule,
    planes: Vec<PlaneInput>,
    physical: Vec<Triangle>,
    triangles: Vec<Triangle>,
    utility: Vec<Anchor>,
    windows: Vec<Anchor>,
    expected: Value,
}
#[derive(Deserialize)]
struct Golden {
    cases: Vec<Case>,
}
pub fn check_reference_span_plans() {
    let golden: Golden =
        serde_json::from_str(include_str!("../../../tests/fixtures/rust/span-plan.json")).unwrap();
    for c in golden.cases {
        let mut budget = Budget::new(32_000_000, &|| false).unwrap();
        let planes = c
            .planes
            .iter()
            .map(|p| Plane::new(&p.host, p.normal, p.offset, &p.faces, &mut budget).unwrap())
            .collect::<Vec<_>>();
        let floors = Walking::new(
            &c.physical
                .iter()
                .map(|&points| Surface {
                    points,
                    floor_or_stairs: true,
                    collidable: true,
                })
                .collect::<Vec<_>>(),
            &mut budget,
        )
        .unwrap();
        for _ in 0..2 {
            let obstacles = Obstructions::new(&c.triangles, &mut budget).unwrap();
            let plan = plan::build(
                c.rule,
                &c.seed,
                &planes,
                &c.utility,
                &c.windows,
                &floors,
                obstacles,
                &mut budget,
            )
            .unwrap();
            reference::close_at(&serde_json::to_value(plan).unwrap(), &c.expected, &c.seed);
        }
    }
}
#[test]
fn signs_washing_cables_and_racks_preserve_ordered_scene_placement() {
    check_reference_span_plans();
}
#[test]
fn invalid_cancelled_and_exhausted_span_jobs_leave_inputs_reusable() {
    let g: Golden =
        serde_json::from_str(include_str!("../../../tests/fixtures/rust/span-plan.json")).unwrap();
    let c = &g.cases[1];
    let mut budget = Budget::new(32_000_000, &|| false).unwrap();
    let planes = c
        .planes
        .iter()
        .map(|p| Plane::new(&p.host, p.normal, p.offset, &p.faces, &mut budget).unwrap())
        .collect::<Vec<_>>();
    let floors = Walking::new(&[], &mut budget).unwrap();
    for (rule, anchors, cancel, work) in [
        (
            Rule {
                reach: f64::NAN,
                ..c.rule
            },
            c.utility.clone(),
            false,
            1_000_000,
        ),
        (
            c.rule,
            vec![Anchor {
                plane: 99,
                ..c.utility[0].clone()
            }],
            false,
            1_000_000,
        ),
        (c.rule, c.utility.clone(), true, 1_000_000),
        (c.rule, c.utility.clone(), false, 10),
    ] {
        let obstacles = Obstructions::new(&c.triangles, &mut budget).unwrap();
        let predicate = || cancel;
        let mut work = Budget::new(work, &predicate).unwrap();
        assert!(
            plan::build(
                rule, &c.seed, &planes, &anchors, &c.windows, &floors, obstacles, &mut work
            )
            .is_err()
        );
    }
    let obstacles = Obstructions::new(&c.triangles, &mut budget).unwrap();
    let result = plan::build(
        c.rule,
        &c.seed,
        &planes,
        &c.utility,
        &c.windows,
        &floors,
        obstacles,
        &mut budget,
    )
    .unwrap();
    assert!(!result.cables.is_empty());
}
