use serde_json::{Value, json};
use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    signs::{
        Construction,
        copy::{self, Brief},
        designs::{self, Look},
    },
    weathered,
};
#[test]
fn briefs_steer_invented_copy_and_derived_lines_follow_the_business() {
    for (seed, wanted) in [
        ("business/0", json!({})),
        ("business/1", json!({"sectors": ["food"], "floor": 1})),
        (
            "business/3",
            json!({"sectors": ["medical", "retail"], "floor": 2}),
        ),
    ] {
        let brief: Brief = serde_json::from_value(wanted.clone()).unwrap();
        let b = copy::invent(&mut SeededRng::from_text(seed), &brief).unwrap();
        let again = copy::invent(&mut SeededRng::from_text(seed), &brief).unwrap();
        let business = serde_json::to_value(&b).unwrap();
        assert_eq!(
            business,
            serde_json::to_value(&again).unwrap(),
            "seeded copy"
        );
        if let Some(sectors) = wanted.get("sectors").and_then(Value::as_array) {
            assert!(sectors.contains(&business["sector"]), "{business}");
        }
        let floor = wanted.get("floor").filter(|f| !f.is_null());
        assert_eq!(business.get("floor").filter(|f| !f.is_null()), floor);
        let name = business["name"].as_str().unwrap();
        let title = copy::title(&b);
        assert!(title.starts_with(name), "{title}");
        assert!(
            copy::phone_line(&b).contains(business["phone"].as_str().unwrap()),
            "phone line carries the number"
        );
        assert_eq!(copy::floor_line(&b).is_some(), floor.is_some());
        assert_eq!(copy::way_up(&b).is_some(), floor.is_some());
        assert!(copy::company_title(&b, &mut SeededRng::from_text(seed)).starts_with(name));
    }
}
#[test]
fn sign_compositions_fill_the_requested_sizes_and_tile_each_character() {
    let settings: weathered::Settings = serde_json::from_value(json!({
        "seed": "sign-design/0", "wear": 0.25, "grime": 0.6, "rust": 0.4, "textureDensity": 0,
        "metal": {"material": "metal", "repeat": 1},
        "sheet": {"material": "sheet", "repeat": 1},
        "paint": {"material": "paint", "repeat": 1}
    }))
    .unwrap();
    for (i, (construction, size)) in [
        (Construction::Sheet, [0.7, 3.2, 0.08]),
        (Construction::Lightbox, [3.6, 0.8, 0.08]),
        (Construction::Timber, [5.4, 0.8, 0.08]),
        (Construction::Tile, [2.1, 1.1, 0.08]),
    ]
    .into_iter()
    .enumerate()
    {
        let seed = format!("design/{i}");
        let mut rng = SeededRng::from_text(&seed);
        let business = copy::invent(&mut rng, &Brief::default()).unwrap();
        let look = designs::draw_look(construction, &settings, &mut rng).unwrap();
        let mut replay = SeededRng::from_text(&seed);
        copy::invent(&mut replay, &Brief::default()).unwrap();
        assert_eq!(
            serde_json::to_value(&look).unwrap(),
            serde_json::to_value(designs::draw_look(construction, &settings, &mut replay).unwrap())
                .unwrap(),
            "seeded look"
        );
        let title =
            serde_json::to_value(designs::title_panel(&business, size, &look, &mut rng).unwrap())
                .unwrap();
        for (key, want) in ["width", "height", "depth"].into_iter().zip(size) {
            assert!((title[key].as_f64().unwrap() - want).abs() < 1e-12, "{key}");
        }
        let name = serde_json::to_value(&business).unwrap()["name"]
            .as_str()
            .unwrap()
            .to_owned();
        let tiles = designs::tile_panels(
            &name,
            0.6,
            0.04,
            &Look {
                construction: Construction::Tile,
                ..look.clone()
            },
            &mut rng,
        )
        .unwrap();
        assert_eq!(tiles.len(), name.chars().count());
        for tile in serde_json::to_value(tiles).unwrap().as_array().unwrap() {
            assert!((tile["width"].as_f64().unwrap() - 0.6).abs() < 1e-12);
            assert!((tile["height"].as_f64().unwrap() - 0.6).abs() < 1e-12);
        }
        let field = if i % 2 == 0 { [1.2, 4.] } else { [4., 1.2] };
        let painted = serde_json::to_value(
            designs::painted_field(
                &copy::title(&business),
                field,
                &Look {
                    construction: Construction::Painted,
                    ..look
                },
                &mut rng,
            )
            .unwrap(),
        )
        .unwrap();
        assert!((painted["width"].as_f64().unwrap() - field[0]).abs() < 1e-12);
        assert!((painted["height"].as_f64().unwrap() - field[1]).abs() < 1e-12);
    }
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
