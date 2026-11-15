use serde::Deserialize;
use serde_json::Value;
use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    signs::{
        Construction,
        copy::{self, Brief, Business},
        designs::{self, Look},
        panel::Design,
    },
    weathered,
};
fn close(a: &Value, b: &Value) {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => assert!(
            (a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 2e-10,
            "{a} != {b}"
        ),
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len());
            for (a, b) in a.iter().zip(b) {
                close(a, b)
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.len(), b.len());
            for (k, b) in b {
                close(&a[k], b)
            }
        }
        _ => assert_eq!(a, b),
    }
}
#[derive(Deserialize)]
struct CopyCase {
    seed: String,
    brief: Brief,
    business: Business,
    title: String,
    floor: Option<String>,
    phone: String,
    up: Option<String>,
    company: String,
    next: f64,
}
#[derive(Deserialize)]
struct Copies {
    cases: Vec<CopyCase>,
}
pub fn check_reference_copy() {
    let golden: Copies =
        serde_json::from_str(include_str!("../../../tests/fixtures/rust/sign-copy.json")).unwrap();
    for c in golden.cases {
        let mut rng = SeededRng::from_text(&c.seed);
        let b = copy::invent(&mut rng, &c.brief).unwrap();
        close(
            &serde_json::to_value(&b).unwrap(),
            &serde_json::to_value(c.business).unwrap(),
        );
        assert_eq!(copy::title(&b), c.title);
        assert_eq!(copy::floor_line(&b), c.floor);
        assert_eq!(copy::phone_line(&b), c.phone);
        assert_eq!(copy::way_up(&b), c.up);
        assert_eq!(copy::company_title(&b, &mut rng), c.company);
        assert_eq!(rng.next_f64(), c.next);
    }
}
#[derive(Deserialize)]
struct DesignCase {
    seed: String,
    construction: Construction,
    settings: weathered::Settings,
    look: Look,
    business: Business,
    size: [f64; 3],
    title: Design,
    tiles: Vec<Design>,
    painted: Design,
    next: f64,
}
#[derive(Deserialize)]
struct Designs {
    cases: Vec<DesignCase>,
}
pub fn check_reference_designs() {
    let golden: Designs = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/sign-designs.json"
    ))
    .unwrap();
    for (i, c) in golden.cases.into_iter().enumerate() {
        let mut rng = SeededRng::from_text(&c.seed);
        let look = designs::draw_look(c.construction, &c.settings, &mut rng).unwrap();
        close(
            &serde_json::to_value(&look).unwrap(),
            &serde_json::to_value(c.look).unwrap(),
        );
        let title = designs::title_panel(&c.business, c.size, &look, &mut rng).unwrap();
        close(
            &serde_json::to_value(title).unwrap(),
            &serde_json::to_value(c.title).unwrap(),
        );
        let tiles = designs::tile_panels(
            &c.business.name,
            0.6,
            0.04,
            &Look {
                construction: Construction::Tile,
                ..look.clone()
            },
            &mut rng,
        )
        .unwrap();
        close(
            &serde_json::to_value(tiles).unwrap(),
            &serde_json::to_value(c.tiles).unwrap(),
        );
        let painted = designs::painted_field(
            &copy::title(&c.business),
            if i % 2 == 0 { [1.2, 4.] } else { [4., 1.2] },
            &Look {
                construction: Construction::Painted,
                ..look
            },
            &mut rng,
        )
        .unwrap();
        close(
            &serde_json::to_value(painted).unwrap(),
            &serde_json::to_value(c.painted).unwrap(),
        );
        assert_eq!(rng.next_f64(), c.next, "design {i} continuation");
    }
}
#[test]
fn seeded_business_copy_matches_reference() {
    check_reference_copy()
}
#[test]
fn sign_compositions_match_reference() {
    check_reference_designs()
}
#[test]
fn empty_trade_pool_rejects_without_consuming_random_stream() {
    let mut rng = SeededRng::from_text("invalid");
    let mut fresh = rng.clone();
    assert!(
        copy::invent(
            &mut rng,
            &Brief {
                sectors: Some(vec![]),
                ..Default::default()
            }
        )
        .is_err()
    );
    assert_eq!(rng.next_u32(), fresh.next_u32());
    assert!(copy::invent(&mut rng, &Brief::default()).is_ok());
}
