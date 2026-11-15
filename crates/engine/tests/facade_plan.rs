use serde::Deserialize;
use serde_json::{Value, json};
use std::cell::RefCell;
use superspec_engine::worldspec::themes::kowloon::{
    facade_plan::{self, Queries},
    kit::Kit,
    massing::Lot,
    signs::panel::Settings,
};
#[path = "support/facade_input.rs"]
mod input;
#[allow(dead_code)]
#[path = "support/sign_reference.rs"]
mod reference;
use input::{Input, LotData};
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    group: usize,
    style_seed: String,
    lot: LotData,
    request: Input,
    detail: u8,
    settings: Settings,
    reach: Value,
    hollow: Value,
    plan: Value,
}
impl Case {
    fn lot(&self) -> Lot {
        self.lot.build(&self.style_seed, self.request.width)
    }
}
#[derive(Deserialize)]
struct Golden {
    cases: Vec<Case>,
}
fn fixture() -> Golden {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/facade-plan.json"
    ))
    .unwrap()
}
pub fn check_reference_facade_plans() {
    for c in fixture().cases {
        let lot = c.lot();
        let kit = Kit::new(c.settings, c.detail).unwrap();
        let reach = RefCell::new(vec![]);
        let hollow = RefCell::new(vec![]);
        let q = Queries {
            reach: &|zone, r, limit| {
                reach
                    .borrow_mut()
                    .push(json!([zone, r.u0, r.u1, r.v0, r.v1, limit]));
                Ok(if c.group % 3 == 0 && r.u0 > 3. {
                    0.
                } else if c.group % 3 == 1 {
                    limit.min(0.4)
                } else {
                    limit
                })
            },
            hollow: &|u0, u1, h, limit| {
                hollow.borrow_mut().push(json!([u0, u1, h, limit]));
                Ok(if c.group % 4 == 0 { 0. } else { 3.1 })
            },
        };
        let plan = facade_plan::plan(&c.request.request(&lot), &kit, &q, &|| false).unwrap();
        reference::close(&json!(*reach.borrow()), &c.reach);
        reference::close(&json!(*hollow.borrow()), &c.hollow);
        let mut value = json!(plan);
        value["next"] = json!(plan.continuation.clone().next_f64());
        reference::close(&value, &c.plan);
    }
}
#[test]
fn facade_plans_match_bays_households_openings_conditions_queries_and_streams() {
    check_reference_facade_plans()
}
#[test]
fn invalid_queries_coverage_and_cancellation_fail_and_next_plan_recovers() {
    let c = fixture().cases.remove(30);
    let lot = c.lot();
    let kit = Kit::new(c.settings, c.detail).unwrap();
    let clear = Queries {
        reach: &|_, _, limit| Ok(limit),
        hollow: &|_, _, _, limit| Ok(limit),
    };
    for bad in [f64::NAN, -0.1, 200.] {
        let query = Queries {
            reach: &|_, _, _| Ok(bad),
            hollow: clear.hollow,
        };
        assert!(facade_plan::plan(&c.request.request(&lot), &kit, &query, &|| false).is_err());
    }
    let mut bad = c.request.clone();
    bad.body.push(bad.body[0]);
    assert!(facade_plan::plan(&bad.request(&lot), &kit, &clear, &|| false).is_err());
    assert!(facade_plan::plan(&c.request.request(&lot), &kit, &clear, &|| true).is_err());
    assert!(
        !facade_plan::plan(&c.request.request(&lot), &kit, &clear, &|| false)
            .unwrap()
            .cells
            .is_empty()
    );
}
