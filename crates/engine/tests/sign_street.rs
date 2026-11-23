use serde_json::json;
use superspec_engine::worldspec::themes::kowloon::{
    kit::Kit,
    massing::Rect,
    pen::Pen,
    signs::{
        copy::Business,
        mounts::Proposal,
        panel::Settings,
        street::{self, Queries},
    },
};
use superspec_geometry::Matrix;
use superspec_geometry::obstructions::Budget;
fn settings() -> Settings {
    serde_json::from_value(json!({
        "weathering": {"seed": "street/0", "wear": 0.25, "grime": 0.6, "rust": 0.4,
            "textureDensity": 32,
            "metal": {"material": "metal", "repeat": 1},
            "paint": {"material": "paint", "repeat": 1},
            "sheet": {"material": "sheet", "repeat": 1}},
        "lettering": true,
        "timber": {"textured": false, "timberRepeat": 1.2, "atlasRepeat": 1}
    }))
    .unwrap()
}
/// A three-bay, four-storey street front.
fn request() -> street::Request {
    serde_json::from_value(json!({
        "key": "front", "width": 9, "ground": 3.2, "roof": 12.8, "wash": [0.7, 0.9, 0.8],
        "body": [{"u0": 0, "u1": 9, "v0": 3.2, "v1": 12.8}],
        "bays": [{"kind": "window", "u0": 0, "u1": 3}, {"kind": "window", "u0": 3, "u1": 6},
                 {"kind": "window", "u0": 6, "u1": 9}],
        "floors": [3.2, 6.4, 9.6],
        "limit": 1.6
    }))
    .unwrap()
}
fn shop() -> Business {
    serde_json::from_value(json!({
        "name": "同益", "trade": "餅家", "english": "TUNG YICK BAKERY",
        "tagline": "馳名", "phone": "K-364497", "sector": "food"
    }))
    .unwrap()
}
fn plan(detail: u8, reach: &dyn Fn(Rect, f64) -> Result<f64, String>) -> Vec<Proposal> {
    let kit = Kit::new(settings(), detail).unwrap();
    let pen = Pen::new(Matrix::default(), "body").unwrap();
    let shop = shop();
    let near = |at: [f64; 3], _| Ok((at[0] < 5.).then(|| shop.clone()));
    let q = Queries {
        reach,
        shop_near: &near,
    };
    let request = request();
    let mut budget = Budget::new(100000, &|| false).unwrap();
    let mut proposals = street::street_signs(&request, &kit, &pen, &q, &mut budget).unwrap();
    proposals.extend(street::wall_advertisement(&request, &kit, &pen, &q, &mut budget).unwrap());
    proposals
}
#[test]
fn kit_options_respect_certain_chances_and_named_streams_repeat() {
    for detail in 0..3 {
        let kit = Kit::new(settings(), detail).unwrap();
        for i in 0..8 {
            let key = format!("part/{i}");
            assert!(!kit.option(&key, 0.).unwrap().planned);
            assert!(kit.option(&key, 1.).unwrap().planned);
            assert_eq!(
                kit.rng(&key).unwrap().next_f64(),
                kit.rng(&key).unwrap().next_f64()
            );
        }
    }
}
#[test]
fn street_choices_are_stable_across_detail_and_blocked_reach_withholds_signs() {
    let open = |_, limit| Ok(limit);
    let keys = |p: &[Proposal]| p.iter().map(|p| p.key.clone()).collect::<Vec<_>>();
    let reference = plan(0, &open);
    assert!(!reference.is_empty());
    for detail in 1..3 {
        assert_eq!(
            keys(&plan(detail, &open)),
            keys(&reference),
            "detail {detail}"
        );
    }
    // A pier with no projection room in the middle bay removes signs there but keeps the rest.
    let blocked = |r: Rect, limit: f64| Ok(if r.u0 < 5.5 && r.u1 > 4.5 { 0. } else { limit });
    let fewer = plan(0, &blocked);
    assert!(fewer.len() < reference.len());
    for key in keys(&fewer) {
        assert!(keys(&reference).contains(&key), "{key}");
    }
}
#[test]
fn street_planning_rejects_invalid_dependencies_and_cancellation_without_publication() {
    let request = request();
    let kit = Kit::new(settings(), 1).unwrap();
    let pen = Pen::new(Matrix::default(), "body").unwrap();
    assert!(Kit::new(settings(), 3).is_err());
    assert!(kit.option("x", f64::NAN).is_err());
    assert!(kit.rng(&"x".repeat(4097)).is_err());
    let valid = Queries {
        reach: &|_, limit| Ok(limit),
        shop_near: &|_, _| Ok(None),
    };
    assert!(
        street::street_signs(
            &request,
            &kit,
            &pen,
            &valid,
            &mut Budget::new(100000, &|| true).unwrap()
        )
        .is_err()
    );
    assert!(
        street::street_signs(
            &request,
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
                &request,
                &kit,
                &pen,
                &q,
                &mut Budget::new(100000, &|| false).unwrap()
            )
            .is_err()
        );
    }
    let mut bad = request.clone();
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
            &request,
            &kit,
            &pen,
            &valid,
            &mut Budget::new(100000, &|| false).unwrap()
        )
        .unwrap()
        .is_empty()
    );
}
