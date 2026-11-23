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
use superspec_geometry::Matrix;
fn kit(seed: &str, detail: u8) -> Kit {
    Kit::new(serde_json::from_value(serde_json::json!({"weathering":{"seed":seed,"wear":0.25,"grime":0.6,"rust":0.4,"textureDensity":32,"metal":{"material":"metal","repeat":1.7},"sheet":{"material":"sheet","repeat":1.7},"paint":{"material":"paint","repeat":1.7}},"lettering":true,"timber":{"textured":true,"timberRepeat":1.7,"atlasRepeat":1.7}})).unwrap(),detail).unwrap()
}
fn frame(index: usize) -> Matrix {
    let (c, s) = (0.9523335698857134, 0.3050586364434435);
    let x = if index % 2 == 0 {
        [c, s, 0.]
    } else {
        [-c, -s, 0.]
    };
    Matrix([x, [-s, c, 0.], [0., 0., 1.], [0.3, -0.2, 0.4]])
}
#[test]
fn grocer_layouts_are_seeded_and_cover_front_recessed_and_side_counters() {
    let mut rooms: Vec<_> = (0..8)
        .map(|i| {
            (
                [1.8, 2.2, 2.5, 3.2][i % 4],
                [2.4, 3.1][i / 4],
                [2.08, 2.4, 2.6][i % 3],
                format!("room/{i}"),
            )
        })
        .collect();
    rooms.push((4.4, 4.2, 2.6, "large-room/3".into()));
    let mut kinds = std::collections::BTreeSet::new();
    for (width, depth, height, seed) in rooms {
        let mut rng = SeededRng::from_text(&seed);
        let plan = serde_json::json!(grocer_layout::plan(width, depth, height, &mut rng).unwrap());
        let mut again = SeededRng::from_text(&seed);
        let repeated =
            serde_json::json!(grocer_layout::plan(width, depth, height, &mut again).unwrap());
        assert_eq!(plan, repeated, "{seed}");
        assert_eq!(rng.next_f64(), again.next_f64(), "{seed}");
        kinds.insert(plan["kind"].as_str().unwrap().to_string());
    }
    assert_eq!(
        kinds.into_iter().collect::<Vec<_>>(),
        ["front-counter", "recessed-counter", "side-counter"]
    );
}
#[test]
fn room_amenities_survey_the_triangles_they_render() {
    for i in 0..12 {
        let depth = [2.4, 3.1, 3.6][i / 4];
        let kit = kit(&format!("room/{i}"), (i % 3) as u8);
        let key = format!("shop/{i}");
        let room = Room {
            width: [1.8, 2.2, 2.5, 3.2][i % 4],
            depth,
            height: [2.08, 2.4, 2.6][i % 3],
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
        let y = [0.84, 1.085, 1.26][i / 4];
        let wall_from = (i % 4 != 0).then_some(depth - 0.95);
        let mut counts = vec![];
        for mode in [Mode::Render, Mode::Survey] {
            let mut mesh = Mesh::new(mode, 10000, &|| false).unwrap();
            room::amenities(
                &mut Fixture {
                    pen: Pen::new(frame(i), "room").unwrap(),
                    mesh: &mut mesh,
                    repeats: &|_| 1.7,
                },
                &room,
                y,
                wall_from,
            )
            .unwrap();
            counts.push(mesh.triangle_count());
        }
        assert_eq!(counts[0], counts[1], "{key}");
    }
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
