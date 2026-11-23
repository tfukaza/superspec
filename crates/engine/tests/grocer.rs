use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    kit::Kit,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::{
        grocer, grocer_layout,
        room::{Assets, Room},
    },
};
use superspec_geometry::{Matrix, V3};
#[allow(dead_code)]
#[path = "support/shop_composition.rs"]
mod composition;
fn kit(seed: &str, detail: u8) -> Kit {
    Kit::new(serde_json::from_value(serde_json::json!({"weathering":{"seed":seed,"wear":0.25,"grime":0.6,"rust":0.4,"textureDensity":32,"metal":{"material":"metal","repeat":1.7},"sheet":{"material":"sheet","repeat":1.7},"paint":{"material":"paint","repeat":1.7}},"lettering":true,"timber":{"textured":true,"timberRepeat":1.7,"atlasRepeat":1.7}})).unwrap(),detail).unwrap()
}
/// Small shop rooms across widths, depths, heights, detail levels and reflected host frames.
fn rooms() -> Vec<(f64, f64, f64, String, u8, String, Matrix)> {
    let (c, s) = (0.9523335698857134, 0.3050586364434435);
    (0..6)
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
fn furnished_grocers_survey_their_render_and_follow_their_planned_counter_and_shelves() {
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
            grocer::furnish(
                &mut Fixture {
                    pen: Pen::new(matrix, "room").unwrap(),
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
                // The furnished room follows the public layout plan drawn from its own stream.
                let layout = serde_json::json!(
                    grocer_layout::plan(width, depth, height, &mut room.stream("layout").unwrap())
                        .unwrap()
                );
                let mut frame = matrix;
                if layout["mirrored"].as_bool().unwrap() {
                    frame = frame.then(Matrix([
                        [-1., 0., 0.],
                        [0., 1., 0.],
                        [0., 0., 1.],
                        [width, 0., 0.],
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
                let counter = placement(&layout["counter"]);
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
                // The plan reserves full-height timber backing and a supported top
                // board, including the mirror of the whole room.
                for shelf in layout["shelves"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|s| s["family"] == "timber")
                {
                    let shelf_frame = placement(shelf);
                    let size: V3 = serde_json::from_value(shelf["size"].clone()).unwrap();
                    // The timber fixture leaves a 6 mm rear setback.
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
