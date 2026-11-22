use serde::Deserialize;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    kit::Kit,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::{
        roast,
        room::{Assets, Room},
        shell,
    },
};
use superspec_geometry::{Matrix, V3};
#[allow(dead_code)]
#[path = "support/assembly_reference.rs"]
mod assembly;
#[allow(dead_code)]
#[path = "support/shop_composition.rs"]
mod composition;
#[allow(dead_code)]
#[path = "support/sign_reference.rs"]
mod reference;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    trade: shell::Trade,
    style_seed: String,
    lot: input::LotData,
    plan: Plan,
    proposals: Vec<serde_json::Value>,
    calls: serde_json::Value,
    width: f64,
    depth: f64,
    height: f64,
    seed: String,
    detail: u8,
    key: String,
    matrix: [V3; 4],
    independent_structure: std::collections::BTreeMap<String, assembly::Geometry>,
    independent_charts: std::collections::BTreeMap<String, Vec<assembly::Chart>>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Plan {
    x0: f64,
    x1: f64,
    floor: f64,
    head: f64,
    depth: f64,
    key: String,
    shutter_color: Option<V3>,
}
#[allow(dead_code)]
#[path = "support/facade_input.rs"]
mod input;
#[derive(Deserialize)]
struct Golden {
    rooms: Vec<Case>,
}
fn kit(seed: &str, detail: u8) -> Kit {
    Kit::new(serde_json::from_value(serde_json::json!({"weathering":{"seed":seed,"wear":0.25,"grime":0.6,"rust":0.4,"textureDensity":32,"metal":{"material":"metal","repeat":1.7},"sheet":{"material":"sheet","repeat":1.7},"paint":{"material":"paint","repeat":1.7}},"lettering":true,"timber":{"textured":true,"timberRepeat":1.7,"atlasRepeat":1.7}})).unwrap(),detail).unwrap()
}
pub fn check_reference_shop_shell() {
    let g: Golden =
        serde_json::from_str(include_str!("../../../tests/fixtures/rust/shop-shell.json")).unwrap();
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
            let lot = c.lot.build(&c.style_seed, 4.);
            let p = superspec_engine::worldspec::themes::kowloon::facade_plan::Room {
                x0: c.plan.x0,
                x1: c.plan.x1,
                floor: c.plan.floor,
                head: c.plan.head,
                depth: c.plan.depth,
                key: c.plan.key.clone(),
            };
            let calls = std::cell::RefCell::new(vec![]);
            let available = |a, b, depth, h| {
                calls.borrow_mut().push([a, b, depth, h]);
                Ok(c.width < 3. || a < 2.)
            };
            let proposals = shell::draw(
                &mut Fixture {
                    pen: Pen::new(Matrix(c.matrix), "room").unwrap(),
                    mesh: &mut mesh,
                    repeats: &|_| 1.7,
                },
                &shell::Request {
                    plan: &p,
                    lot: &lot,
                    coat: "wall",
                    kit: &kit,
                    assets: shell::Assets {
                        room: room.assets,
                        restaurant: roast::Assets {
                            hero_props: false,
                            food: true,
                            metal: true,
                            surfaces: true,
                            tiles: true,
                        },
                    },
                    trade: Some(c.trade),
                    shutter_color: c.plan.shutter_color,
                    frontage: Some(&available),
                },
            )
            .unwrap();
            reference::close(&serde_json::to_value(calls.into_inner()).unwrap(), &c.calls);
            assert_eq!(proposals.len(), c.proposals.len());
            for (p, e) in proposals.iter().zip(&c.proposals) {
                reference::close(&reference::metadata(p), e);
            }
            assert!(mesh.triangle_count() > 0);
            if mesh.surveying() {
                composition::same_positions(&mesh.survey, &rendered);
            } else {
                composition::valid(&mesh);
                rendered = composition::positions(&mesh);
                composition::open_room(
                    &mesh,
                    "room",
                    Matrix(c.matrix),
                    [p.x0, p.x1],
                    p.depth,
                    [p.floor, p.head],
                );
                for part in ["openings", "shop-frontage"] {
                    assembly::check_part(
                        &mesh,
                        part,
                        &c.independent_structure[part],
                        &format!("{}/{part}", c.key),
                    );
                }
                assembly::check_part_finishes(&mut mesh, &c.independent_charts, &c.key);
                let stock = composition::part_points(
                    &mesh,
                    &format!("shop-{}", c.trade.name()),
                    Matrix(c.matrix),
                );
                assert!(
                    stock.iter().any(|p| p[1] < -0.1),
                    "shop must contain interior fittings"
                );
            }
        }
    }
}
#[test]
fn complete_shops_preserve_shells_trades_frontage_and_sign_proposals() {
    check_reference_shop_shell();
}
