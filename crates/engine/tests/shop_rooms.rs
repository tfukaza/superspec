use serde::Deserialize;
use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    kit::Kit,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::{
        grocer_layout,
        room::{self, Assets, Room},
    },
};
use superspec_geometry::{Matrix, V3};
#[path = "support/assembly_reference.rs"]
mod assembly;
#[allow(dead_code)]
#[path = "support/sign_reference.rs"]
mod reference;
#[derive(Deserialize)]
struct Plan {
    width: f64,
    depth: f64,
    height: f64,
    seed: String,
    plan: serde_json::Value,
    next: f64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    width: f64,
    depth: f64,
    height: f64,
    seed: String,
    detail: u8,
    key: String,
    y: f64,
    wall_from: Option<f64>,
    matrix: [V3; 4],
    geometry: assembly::Geometry,
}
#[derive(Deserialize)]
struct Golden {
    plans: Vec<Plan>,
    rooms: Vec<Case>,
}
fn kit(seed: &str, detail: u8) -> Kit {
    Kit::new(serde_json::from_value(serde_json::json!({"weathering":{"seed":seed,"wear":0.25,"grime":0.6,"rust":0.4,"textureDensity":32,"metal":{"material":"metal","repeat":1.7},"sheet":{"material":"sheet","repeat":1.7},"paint":{"material":"paint","repeat":1.7}},"lettering":true,"timber":{"textured":true,"timberRepeat":1.7,"atlasRepeat":1.7}})).unwrap(),detail).unwrap()
}
pub fn check_reference_shop_rooms() {
    let g: Golden =
        serde_json::from_str(include_str!("../../../tests/fixtures/rust/shop-rooms.json")).unwrap();
    let mut kinds = std::collections::BTreeSet::new();
    for p in g.plans {
        let mut rng = SeededRng::from_text(&p.seed);
        let plan = grocer_layout::plan(p.width, p.depth, p.height, &mut rng).unwrap();
        let value = serde_json::json!(plan);
        kinds.insert(value["kind"].as_str().unwrap().to_string());
        reference::close(&value, &p.plan);
        assert_eq!(rng.next_f64(), p.next);
    }
    assert_eq!(kinds.len(), 3);
    for c in g.rooms {
        for mode in [Mode::Render, Mode::Survey] {
            let mut mesh = Mesh::new(mode, 10000, &|| false).unwrap();
            let kit = kit(&c.seed, c.detail);
            let room = Room {
                width: c.width,
                depth: c.depth,
                height: c.height,
                kit: &kit,
                key: &c.key,
                assets: Assets {
                    posters: true,
                    grocer_props: true,
                    snack_packets: true,
                    market_apple: true,
                    shop_register: false,
                    market_trays: false,
                },
            };
            room::amenities(
                &mut Fixture {
                    pen: Pen::new(Matrix(c.matrix), "room").unwrap(),
                    mesh: &mut mesh,
                    repeats: &|_| 1.7,
                },
                &room,
                c.y,
                c.wall_from,
            )
            .unwrap();
            assert_eq!(mesh.triangle_count() * 3, c.geometry.indices.len());
            if !mesh.surveying() {
                assembly::check(&mut mesh, &c.geometry, &[], &c.key);
            }
        }
    }
}
#[test]
fn grocer_layout_and_room_amenities_preserve_reservations_random_streams_and_detail() {
    check_reference_shop_rooms();
}
#[test]
fn invalid_room_dimensions_do_not_advance_layout_stream() {
    let mut rng = SeededRng::from_text("room");
    let mut expected = rng.clone();
    for dims in [[f64::NAN, 3., 2.5], [2., -1., 2.5], [2., 3., 0.]] {
        assert!(grocer_layout::plan(dims[0], dims[1], dims[2], &mut rng).is_err());
    }
    assert_eq!(rng.next_f64(), expected.next_f64());
    assert!(grocer_layout::plan(2.5, 3.1, 2.4, &mut rng).is_ok());
}
