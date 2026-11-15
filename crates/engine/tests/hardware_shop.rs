use serde::Deserialize;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    kit::Kit,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::{
        hardware,
        room::{Assets, Room},
    },
};
use superspec_geometry::{Matrix, V3};
#[allow(dead_code)]
#[path = "support/assembly_reference.rs"]
mod assembly;
#[allow(dead_code)]
#[path = "support/sign_reference.rs"]
mod reference;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    width: f64,
    depth: f64,
    height: f64,
    seed: String,
    detail: u8,
    key: String,
    matrix: [V3; 4],
    geometry: assembly::Geometry,
}
#[derive(Deserialize)]
struct Golden {
    rooms: Vec<Case>,
}
fn kit(seed: &str, detail: u8) -> Kit {
    Kit::new(serde_json::from_value(serde_json::json!({"weathering":{"seed":seed,"wear":0.25,"grime":0.6,"rust":0.4,"textureDensity":32,"metal":{"material":"metal","repeat":1.7},"sheet":{"material":"sheet","repeat":1.7},"paint":{"material":"paint","repeat":1.7}},"lettering":true,"timber":{"textured":true,"timberRepeat":1.7,"atlasRepeat":1.7}})).unwrap(),detail).unwrap()
}
pub fn check_reference_hardware_shop() {
    let g: Golden = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/hardware-shop.json"
    ))
    .unwrap();
    for c in g.rooms {
        for mode in [Mode::Render, Mode::Survey] {
            let mut mesh = Mesh::new(mode, 400000, &|| false).unwrap();
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
            hardware::furnish(
                &mut Fixture {
                    pen: Pen::new(Matrix(c.matrix), "room").unwrap(),
                    mesh: &mut mesh,
                    repeats: &|_| 1.7,
                },
                &room,
            )
            .unwrap();
            assert!(mesh.triangle_count() > 0);
            if !mesh.surveying() {
                assembly::check_triangles(&mesh, &c.geometry, &c.key);
            }
        }
    }
}
#[test]
fn complete_hardware_shop_preserves_stock_transforms_materials_and_named_streams() {
    check_reference_hardware_shop();
}
