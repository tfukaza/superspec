use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    facade_cells::Role,
    facade_plan::{self, Queries, Request},
    kit::Kit,
    massing::{Lot, Rect},
    palette,
};
fn lot() -> Lot {
    Lot {
        index: 0,
        kind: "tenement",
        x0: 0.,
        x1: 9.8,
        ground: 3.4,
        story_height: 3.,
        stories: 3,
        roof: 9.4,
        top: 10.05,
        front: 0.,
        back: 0.,
        style: palette::lot_style(&mut SeededRng::from_text("facade-style/0")),
        key: "lot-2".into(),
    }
}
fn request<'a>(lot: &'a Lot, ground: &'a [Rect], body: &'a [Rect]) -> Request<'a> {
    Request {
        lot,
        role: Role::Front,
        key: "front-2",
        width: 9.8,
        shift: -0.2,
        body_span: [-0.1, 10.],
        ground,
        body,
        shops: 0.,
    }
}
#[test]
fn invalid_queries_coverage_and_cancellation_fail_and_next_plan_recovers() {
    let lot = lot();
    let ground = [Rect {
        u0: 0.,
        u1: 9.8,
        v0: 0.,
        v1: 3.4,
    }];
    let body = vec![Rect {
        u0: -0.1,
        u1: 10.,
        v0: 3.4,
        v1: 10.05,
    }];
    let kit=Kit::new(serde_json::from_value(serde_json::json!({"weathering":{"seed":"facade/2","wear":0.25,"grime":0.6,"rust":0.4,"textureDensity":32,"metal":{"material":"metal","repeat":1},"sheet":{"material":"sheet","repeat":1},"paint":{"material":"paint","repeat":1}},"lettering":true,"timber":{"textured":true,"timberRepeat":1,"atlasRepeat":1}})).unwrap(),2).unwrap();
    let clear = Queries {
        reach: &|_, _, limit| Ok(limit),
        hollow: &|_, _, _, limit| Ok(limit),
    };
    for bad in [f64::NAN, -0.1, 200.] {
        let query = Queries {
            reach: &|_, _, _| Ok(bad),
            hollow: clear.hollow,
        };
        assert!(
            facade_plan::plan(&request(&lot, &ground, &body), &kit, &query, &|| false).is_err()
        );
    }
    let mut overlapping = body.clone();
    overlapping.push(overlapping[0]);
    assert!(
        facade_plan::plan(&request(&lot, &ground, &overlapping), &kit, &clear, &|| {
            false
        })
        .is_err()
    );
    assert!(facade_plan::plan(&request(&lot, &ground, &body), &kit, &clear, &|| true).is_err());
    assert!(
        !facade_plan::plan(&request(&lot, &ground, &body), &kit, &clear, &|| false)
            .unwrap()
            .cells
            .is_empty()
    );
}
