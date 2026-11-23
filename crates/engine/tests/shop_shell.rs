use superspec_engine::worldspec::themes::kowloon::{
    facade_plan,
    fixture::Fixture,
    kit::Kit,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::{
        roast,
        room::{Assets, Room},
        shell::{self, Trade},
    },
};
use superspec_geometry::Matrix;
#[allow(dead_code)]
#[path = "support/shop_composition.rs"]
mod composition;
#[allow(dead_code)]
#[path = "support/facade_input.rs"]
mod input;
fn kit(seed: &str, detail: u8) -> Kit {
    Kit::new(serde_json::from_value(serde_json::json!({"weathering":{"seed":seed,"wear":0.25,"grime":0.6,"rust":0.4,"textureDensity":32,"metal":{"material":"metal","repeat":1.7},"sheet":{"material":"sheet","repeat":1.7},"paint":{"material":"paint","repeat":1.7}},"lettering":true,"timber":{"textured":true,"timberRepeat":1.7,"atlasRepeat":1.7}})).unwrap(),detail).unwrap()
}
/// Every trade, in a small and a large room, turned and mirrored on the street: the shell is a
/// valid open room, its trade furnishes the interior, and the survey agrees with the render.
pub fn check_shop_shell_rooms() {
    let trades = [
        Trade::Grocer,
        Trade::Greengrocer,
        Trade::Roast,
        Trade::Hardware,
        Trade::Herbalist,
    ];
    for i in 0..10 {
        let trade = trades[i % 5];
        let (width, depth) = if i < 5 { (2.2, 2.4) } else { (3.2, 3.1) };
        let detail = (i % 3) as u8;
        let mirror = if i % 2 == 0 { 1. } else { -1. };
        let matrix = Matrix([
            [0.9523335698857134 * mirror, 0.3050586364434435 * mirror, 0.],
            [-0.3050586364434435, 0.9523335698857134, 0.],
            [0., 0., 1.],
            [0.3, -0.2, 0.4],
        ]);
        let key = format!("shop/{i}");
        let lot = input::LotData {
            kind: "tenement".into(),
            ground: 3.4,
            roof: 6.4,
            top: 7.05,
            stories: 2,
            story_height: 3.,
            key: format!("lot/{i}"),
        }
        .build(&format!("shop-style/{i}"), 4.);
        let p = facade_plan::Room {
            x0: 0.3,
            x1: 0.3 + width,
            floor: 0.12,
            head: 2.6,
            depth,
            key: key.clone(),
        };
        let mut rendered = vec![];
        for mode in [Mode::Render, Mode::Survey] {
            let mut mesh = Mesh::new(mode, 400000, &|| false).unwrap();
            let kit = kit(&format!("room/{i}"), detail);
            let room = Room {
                width,
                depth,
                height: 2.48,
                kit: &kit,
                key: &key,
                assets: Assets {
                    posters: true,
                    grocer_props: true,
                    snack_packets: true,
                    market_apple: true,
                    shop_register: false,
                    market_trays: false,
                },
            };
            let available = |a, _, _, _| Ok(width < 3. || a < 2.);
            shell::draw(
                &mut Fixture {
                    pen: Pen::new(matrix, "room").unwrap(),
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
                    trade: Some(trade),
                    shutter_color: (i == 7).then_some([0.2, 0.4, 0.1]),
                    frontage: Some(&available),
                },
            )
            .unwrap();
            assert!(mesh.triangle_count() > 0);
            if mesh.surveying() {
                composition::same_positions(&mesh.survey, &rendered);
            } else {
                composition::valid(&mesh);
                rendered = composition::positions(&mesh);
                composition::open_room(
                    &mesh,
                    "room",
                    matrix,
                    [p.x0, p.x1],
                    p.depth,
                    [p.floor, p.head],
                );
                let stock =
                    composition::part_points(&mesh, &format!("shop-{}", trade.name()), matrix);
                assert!(
                    stock.iter().any(|p| p[1] < -0.1),
                    "shop must contain interior fittings"
                );
            }
        }
    }
}
#[test]
fn complete_shops_are_valid_open_rooms_their_trades_furnish_and_surveys_match() {
    check_shop_shell_rooms();
}
