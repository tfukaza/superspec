use serde::Deserialize;
use serde_json::{Value, json};
use std::cell::RefCell;
use superspec_engine::worldspec::themes::kowloon::{
    kit::Kit,
    pen::Pen,
    signs::{
        copy::Business,
        panel::Settings,
        street::{self, Queries},
    },
};
use superspec_geometry::Matrix;
use superspec_geometry::obstructions::Budget;
#[derive(Deserialize)]
struct Choice {
    key: String,
    chance: f64,
    first: f64,
    planned: bool,
    shown: bool,
}
#[derive(Deserialize)]
struct Case {
    request: street::Request,
    detail: u8,
    settings: Settings,
    shop: Business,
    reflected: bool,
    mode: usize,
    queries: Value,
    lookups: Value,
    proposals: Vec<Value>,
    options: Vec<Choice>,
}
#[derive(Deserialize)]
struct Golden {
    cases: Vec<Case>,
}
fn fixture() -> Golden {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/sign-street.json"
    ))
    .unwrap()
}
fn close(a: &Value, b: &Value) {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => assert!(
            (a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 2e-10,
            "{a} != {b}"
        ),
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len());
            for (a, b) in a.iter().zip(b) {
                close(a, b);
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in b {
                close(&a[k], v);
            }
        }
        _ => assert_eq!(a, b),
    }
}
pub fn check_reference_streets() {
    for (i, c) in fixture().cases.into_iter().enumerate() {
        let kit = Kit::new(c.settings, c.detail).unwrap();
        for choice in c.options {
            let got = kit.option(&choice.key, choice.chance).unwrap();
            assert_eq!((got.planned, got.shown), (choice.planned, choice.shown));
            assert_eq!(kit.rng(&choice.key).unwrap().next_f64(), choice.first);
        }
        let pen = Pen::new(
            if c.reflected {
                Matrix([[-1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [9., 3., 5.]])
            } else {
                Matrix::default()
            },
            "body",
        )
        .unwrap();
        let queries = RefCell::new(Vec::new());
        let lookups = RefCell::new(Vec::new());
        let reach = |r: superspec_engine::worldspec::themes::kowloon::massing::Rect, limit: f64| {
            queries
                .borrow_mut()
                .push(json!({"u0":r.u0,"u1":r.u1,"v0":r.v0,"v1":r.v1,"limit":limit}));
            Ok(if c.mode == 1 && r.u0 < 5.5 && r.u1 > 4.5 {
                0.
            } else if c.mode == 2 {
                limit.min(0.55)
            } else {
                limit
            })
        };
        let near = |at: [f64; 3], radius| {
            lookups.borrow_mut().push(json!({"at":at,"radius":radius}));
            Ok((at[0] < 5.).then(|| c.shop.clone()))
        };
        let q = Queries {
            reach: &reach,
            shop_near: &near,
        };
        let mut budget = Budget::new(100000, &|| false).unwrap();
        let mut proposals = street::street_signs(&c.request, &kit, &pen, &q, &mut budget).unwrap();
        proposals
            .extend(street::wall_advertisement(&c.request, &kit, &pen, &q, &mut budget).unwrap());
        assert_eq!(proposals.len(), c.proposals.len(), "street {i}");
        close(&json!(queries.into_inner()), &c.queries);
        close(&json!(lookups.into_inner()), &c.lookups);
        for (p, e) in proposals.iter().zip(&c.proposals) {
            close(
                &json!({"key":p.key,"mount":p.mount,"form":p.form,"shown":p.shown,"above":p.above,"business":p.business,"steel":p.steel,"panels":p.panels.iter().map(|p|json!({"design":p.design,"sides":p.sides,"matrix":p.pen.matrix.0})).collect::<Vec<_>>()}),
                e,
            );
        }
    }
}
#[test]
fn streets_keep_reference_choices_names_clearance_order_and_detail_stability() {
    check_reference_streets()
}
#[test]
fn street_planning_rejects_invalid_dependencies_and_cancellation_without_publication() {
    let c = fixture().cases.remove(0);
    let kit = Kit::new(c.settings.clone(), 1).unwrap();
    let pen = Pen::new(Matrix::default(), "body").unwrap();
    assert!(Kit::new(c.settings.clone(), 3).is_err());
    assert!(kit.option("x", f64::NAN).is_err());
    assert!(kit.rng(&"x".repeat(4097)).is_err());
    let valid = Queries {
        reach: &|_, limit| Ok(limit),
        shop_near: &|_, _| Ok(None),
    };
    assert!(
        street::street_signs(
            &c.request,
            &kit,
            &pen,
            &valid,
            &mut Budget::new(100000, &|| true).unwrap()
        )
        .is_err()
    );
    assert!(
        street::street_signs(
            &c.request,
            &kit,
            &pen,
            &valid,
            &mut Budget::new(1, &|| false).unwrap()
        )
        .is_err()
    );
    for depth in [f64::NAN, -1., 100.] {
        let q = Queries {
            reach: &|_, _| Ok(depth),
            shop_near: &|_, _| Ok(None),
        };
        assert!(
            street::street_signs(
                &c.request,
                &kit,
                &pen,
                &q,
                &mut Budget::new(100000, &|| false).unwrap()
            )
            .is_err()
        );
    }
    let mut bad = c.request.clone();
    bad.body.push(bad.body[0]);
    assert!(
        street::street_signs(
            &bad,
            &kit,
            &pen,
            &valid,
            &mut Budget::new(100000, &|| false).unwrap()
        )
        .is_err()
    );
    assert!(
        !street::street_signs(
            &c.request,
            &kit,
            &pen,
            &valid,
            &mut Budget::new(100000, &|| false).unwrap()
        )
        .unwrap()
        .is_empty()
    );
}
