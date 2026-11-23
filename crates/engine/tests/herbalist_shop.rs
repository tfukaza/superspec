use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    kit::Kit,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::{
        herbalist,
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
fn furnished_herbalist_shops_emit_valid_render_and_survey_for_every_room_size() {
    for (width, depth, height, seed, detail, key, matrix) in rooms() {
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
            herbalist::furnish(
                &mut Fixture {
                    pen: Pen::new(matrix, "room").unwrap(),
                    mesh: &mut mesh,
                    repeats: &|_| 1.7,
                },
                &room,
            )
            .unwrap();
            assert!(mesh.triangle_count() > 0, "{key}");
            if !mesh.surveying() {
                composition::valid(&mesh);
            }
        }
    }
}
