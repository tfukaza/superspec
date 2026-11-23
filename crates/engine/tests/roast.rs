use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    kit::Kit,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::{
        roast,
        room::{Assets, Room},
    },
};
use superspec_geometry::Matrix;
#[allow(dead_code)]
#[path = "support/shop_composition.rs"]
mod composition;
fn kit(seed: &str, detail: u8) -> Kit {
    Kit::new(serde_json::from_value(serde_json::json!({"weathering":{"seed":seed,"wear":0.25,"grime":0.6,"rust":0.4,"textureDensity":32,"metal":{"material":"metal","repeat":1.7},"sheet":{"material":"sheet","repeat":1.7},"paint":{"material":"paint","repeat":1.7}},"lettering":true,"timber":{"textured":true,"timberRepeat":1.7,"atlasRepeat":1.7}})).unwrap(),detail).unwrap()
}
/// Small shop rooms across widths, depths, heights, detail levels and reflected host frames.
fn rooms() -> Vec<(f64, f64, f64, String, u8, String, Matrix)> {
    let (c, s) = (0.9523335698857134, 0.3050586364434435);
    (0..8)
        .map(|i| {
            let x = if i % 2 == 0 { [c, s, 0.] } else { [-c, -s, 0.] };
            (
                [1.8, 2.2, 2.5, 3.2][i % 4],
                [2.4, 3.1][i / 4],
                [2.08, 2.4, 2.6][i % 3],
                format!("room/{i}"),
                (i % 3) as u8,
                format!("shop/{i}"),
                Matrix([x, [-s, c, 0.], [0., 0., 1.], [0.3, -0.2, 0.4]]),
            )
        })
        .collect()
}
#[test]
fn roast_rooms_survey_their_render_and_keep_walls_counter_and_food_display() {
    for (width, depth, height, seed, detail, key, matrix) in rooms() {
        let mut rendered = vec![];
        for mode in [Mode::Render, Mode::Survey] {
            let mut mesh = Mesh::new(mode, 400000, &|| false).unwrap();
            let kit = kit(&seed, detail);
            let room = Room {
                width,
                depth,
                height,
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
            roast::furnish(
                &mut Fixture {
                    pen: Pen::new(matrix, "room").unwrap(),
                    mesh: &mut mesh,
                    repeats: &|_| 1.7,
                },
                &room,
                roast::Assets {
                    hero_props: false,
                    food: true,
                    metal: true,
                    surfaces: true,
                    tiles: true,
                },
            )
            .unwrap();
            assert!(mesh.triangle_count() > 0);
            if mesh.surveying() {
                composition::same_positions(&mesh.survey, &rendered);
            } else {
                composition::valid(&mesh);
                rendered = composition::positions(&mesh);
                assert!(mesh.surfaces.iter().any(|s| s.slot == "kitchen-metal"));
                for side in ["left", "right"] {
                    let wall =
                        composition::part_points(&mesh, &format!("room/room-wall-{side}"), matrix);
                    composition::bounds(&wall, [0., 0., 0.], [width, depth, height]);
                    assert!(wall.iter().all(|p| (p[0] - 0.004).abs() < 0.00001
                        || (p[0] - (width - 0.004)).abs() < 0.00001));
                    assert!(wall.iter().any(|p| p[2] < 0.00001));
                    assert!(wall.iter().any(|p| p[2] > height - 0.07));
                }
                let register = composition::part_points(&mesh, "room/checkout/register", matrix);
                assert!(
                    register.iter().any(|p| (p[2] - 0.92).abs() < 0.00001),
                    "register must rest on kitchen counter"
                );
                let food = composition::part_points(&mesh, "room/display-food", matrix);
                assert!(
                    food.iter().any(|p| p[2] > 0.92),
                    "stocked food display must remain above work surface"
                );
            }
        }
    }
}
#[test]
fn deep_restaurant_tables_keep_floor_support_and_kitchen_clearance() {
    let build = |seed: &str| {
        let mut mesh = Mesh::new(Mode::Render, 400000, &|| false).unwrap();
        let kit = kit(seed, 1);
        let room = Room {
            width: 5.,
            depth: 6.,
            height: 2.8,
            kit: &kit,
            key: "dining",
            assets: Assets {
                posters: true,
                grocer_props: true,
                snack_packets: true,
                market_apple: true,
                shop_register: false,
                market_trays: false,
            },
        };
        roast::furnish(
            &mut Fixture {
                pen: Pen::new(Matrix::default(), "room").unwrap(),
                mesh: &mut mesh,
                repeats: &|_| 1.7,
            },
            &room,
            roast::Assets {
                hero_props: false,
                food: true,
                metal: true,
                surfaces: true,
                tiles: true,
            },
        )
        .unwrap();
        mesh
    };
    let mesh = build("dining-contract");
    let repeated = build("dining-contract");
    composition::same_mesh(&mesh, &repeated);
    let changed = build("other-menu");
    composition::different_detail(&mesh, &changed);
    composition::valid(&mesh);
    for (index, y) in [(0, 1.6), (1, 3.2)] {
        let table = composition::part_points(
            &mesh,
            &format!("room/dining/table-{index}"),
            Matrix::default(),
        );
        composition::bounds(&table, [4.2, y - 0.01, 0.], [4.9, y + 0.64, 1.3]);
        assert!(
            table.iter().any(|p| p[2].abs() < 1e-8),
            "dining table must stand on floor"
        );
        for x in [4.228, 4.862] {
            for dy in [0.008, 0.612] {
                composition::contains(&mesh, [x, y + dy, 0.766]);
                composition::contains(&changed, [x, y + dy, 0.766]);
            }
        }
    }
    assert!(
        !mesh
            .surfaces
            .iter()
            .any(|s| s.part == "room/dining/table-2")
    );
}
#[test]
fn stoa_roast_bay_seats_four_beside_a_clear_aisle() {
    let mut mesh = Mesh::new(Mode::Render, 400000, &|| false).unwrap();
    let kit = kit("stoa-roast-seating", 1);
    let room = Room {
        width: 3.975,
        depth: 3.10,
        height: 2.80,
        kit: &kit,
        key: "stoa-roast",
        assets: Assets {
            posters: true,
            grocer_props: true,
            snack_packets: true,
            market_apple: true,
            shop_register: true,
            market_trays: true,
        },
    };
    roast::furnish(
        &mut Fixture {
            pen: Pen::new(Matrix::default(), "room").unwrap(),
            mesh: &mut mesh,
            repeats: &|_| 1.7,
        },
        &room,
        roast::Assets {
            hero_props: true,
            food: true,
            metal: true,
            surfaces: true,
            tiles: true,
        },
    )
    .unwrap();
    composition::valid(&mesh);
    for index in 0..2 {
        let top = composition::part_points(
            &mesh,
            &format!("room/dining/table-{index}"),
            Matrix::default(),
        );
        assert!(top.iter().any(|p| (p[2] - 0.766).abs() < 1e-5));
        for side in [-1, 1] {
            let seat = composition::part_points(
                &mesh,
                &format!("room/dining/seat-{index}-{side}"),
                Matrix::default(),
            );
            assert!(seat.iter().any(|p| p[2].abs() < 1e-5));
            composition::bounds(&seat, [3.45, 0.61, 0.], [3.82, 2.89, 0.43]);
        }
    }
    // Check triangle bounds conservatively against the main walking strip
    // between the customer seating and the kitchen, below the hanging food.
    let lower = [2.43, 0.09, 0.07];
    let upper = [3.34, 2.95, 1.12];
    for (triangle, &surface) in mesh.indices.chunks_exact(3).zip(&mesh.triangle_surfaces) {
        let intersects = (0..3).all(|axis| {
            let values = triangle
                .iter()
                .map(|&i| mesh.vertices[i as usize].position[axis]);
            let min = values.clone().fold(f64::INFINITY, f64::min);
            let max = values.fold(f64::NEG_INFINITY, f64::max);
            max > lower[axis] && min < upper[axis]
        });
        assert!(
            !intersects,
            "{} crosses customer aisle",
            mesh.surfaces[surface].part
        );
    }
}
