use serde::Deserialize;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    kit::Kit,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::{
        grocer,
        room::{Assets, Room},
    },
};
use superspec_geometry::{Matrix, V3};
#[allow(dead_code)]
#[path = "support/shop_composition.rs"]
mod composition;
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
    layout: serde_json::Value,
}
#[derive(Deserialize)]
struct Golden {
    rooms: Vec<Case>,
}
fn kit(seed: &str, detail: u8) -> Kit {
    Kit::new(serde_json::from_value(serde_json::json!({"weathering":{"seed":seed,"wear":0.25,"grime":0.6,"rust":0.4,"textureDensity":32,"metal":{"material":"metal","repeat":1.7},"sheet":{"material":"sheet","repeat":1.7},"paint":{"material":"paint","repeat":1.7}},"lettering":true,"timber":{"textured":true,"timberRepeat":1.7,"atlasRepeat":1.7}})).unwrap(),detail).unwrap()
}
pub fn check_reference_grocer() {
    let g: Golden =
        serde_json::from_str(include_str!("../../../tests/fixtures/rust/grocer.json")).unwrap();
    for c in g.rooms {
        let mut rendered = vec![];
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
            grocer::furnish(
                &mut Fixture {
                    pen: Pen::new(Matrix(c.matrix), "room").unwrap(),
                    mesh: &mut mesh,
                    repeats: &|_| 1.7,
                },
                &room,
            )
            .unwrap();
            assert!(mesh.triangle_count() > 0);
            if mesh.surveying() {
                composition::same_positions(&mesh.survey, &rendered);
            } else {
                composition::valid(&mesh);
                rendered = composition::positions(&mesh);
                let mut frame = Matrix(c.matrix);
                if c.layout["mirrored"].as_bool().unwrap() {
                    frame = frame.then(Matrix([
                        [-1., 0., 0.],
                        [0., 1., 0.],
                        [0., 0., 1.],
                        [c.width, 0., 0.],
                    ]));
                }
                let placement = |value: &serde_json::Value| {
                    let turn = value["turn"].as_f64().unwrap();
                    let origin: V3 = serde_json::from_value(value["origin"].clone()).unwrap();
                    frame.then(Matrix([
                        [turn.cos(), turn.sin(), 0.],
                        [-turn.sin(), turn.cos(), 0.],
                        [0., 0., 1.],
                        origin,
                    ]))
                };
                let counter = placement(&c.layout["counter"]);
                let register = composition::part_points(&mesh, "room/checkout/register", counter);
                composition::bounds(&register, [0.015, 0.11, 0.89], [0.335, 0.41, 1.18]);
                assert!(
                    register.iter().any(|p| p[2] < 0.91),
                    "register must sit on the counter"
                );
                // The shipped register's highest authored corner is 0.18684932 m
                // above its base, independently of its conservative SIZE envelope.
                assert!(
                    register.iter().any(|p| (p[2] - 1.08684932).abs() < 0.00001),
                    "register display must be present"
                );
                // The independently authored plan reserves full-height timber backing
                // and a supported top board, including the mirror of the whole room.
                for shelf in c.layout["shelves"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|s| s["family"] == "timber")
                {
                    let shelf_frame = placement(shelf);
                    let size: V3 = serde_json::from_value(shelf["size"].clone()).unwrap();
                    // The preserved timber fixture leaves a 6 mm rear setback.
                    for x in [0., size[0]] {
                        for y in [0., size[1] - 0.006] {
                            composition::contains(&mesh, shelf_frame.point([x, y, size[2]]));
                        }
                    }
                }
            }
        }
    }
}
#[test]
fn complete_grocer_preserves_stock_transforms_materials_and_named_streams() {
    check_reference_grocer();
}
