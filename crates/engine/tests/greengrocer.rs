use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    kit::Kit,
    mesh::{Mesh, Mode},
    pen::Pen,
    shop::{
        greengrocer,
        room::{Assets, Room},
    },
};
use superspec_geometry::Matrix;
#[path = "support/shop_composition.rs"]
#[allow(dead_code)]
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
fn furnished_greengrocers_survey_their_render_and_support_checkout_and_back_stock() {
    for (width, depth, height, seed, detail, key, matrix) in rooms() {
        let c_matrix = matrix.0;
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
            greengrocer::furnish(
                &mut Fixture {
                    pen: Pen::new(Matrix(c_matrix), "room").unwrap(),
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
                let checkout = composition::part_points(&mesh, "room/checkout", Matrix(c_matrix));
                assert!(
                    checkout.iter().any(|p| p[2].abs() < 1e-8),
                    "checkout must stand on the floor"
                );
                assert!(
                    checkout.iter().any(|p| p[2] >= 0.9),
                    "checkout needs its counter and register"
                );
                for bank in ["rear-timber", "rear-steel"] {
                    let points = composition::part_points(
                        &mesh,
                        &format!("room/back-stock/{bank}"),
                        Matrix(c_matrix),
                    );
                    assert!(
                        points.iter().any(|p| p[2] < 0.02),
                        "back stock must have floor supports"
                    );
                    assert!(
                        points.iter().any(|p| p[2] > 1.7),
                        "back stock must retain upper shelves"
                    );
                }
            }
        }
    }
}

fn draw_room(
    seed: &str,
    width: f64,
    depth: f64,
    height: f64,
    frame: Matrix,
    mode: Mode,
) -> Mesh<'static> {
    let mut mesh = Mesh::new(mode, 400000, &|| false).unwrap();
    let kit = kit(seed, 1);
    let room = Room {
        width,
        depth,
        height,
        kit: &kit,
        key: "market",
        assets: Assets {
            posters: true,
            grocer_props: true,
            snack_packets: true,
            market_apple: true,
            shop_register: false,
            market_trays: false,
        },
    };
    greengrocer::furnish(
        &mut Fixture {
            pen: Pen::new(frame, "room").unwrap(),
            mesh: &mut mesh,
            repeats: &|_| 1.7,
        },
        &room,
    )
    .unwrap();
    mesh
}

#[test]
fn compact_rack_supports_and_deep_market_aisle_have_authored_dimensions() {
    let compact = draw_room(
        "composition",
        3.2,
        3.1,
        2.6,
        Matrix::default(),
        Mode::Render,
    );
    composition::valid(&compact);
    // The compact market reserves a 0.95 m aisle and 0.65 m front checkout.
    // The three upper tier decks span 0.04 .. 1.6 m and rise to 1.25 m.
    for (front, height) in [(0.53, 0.81667), (1., 1.03333), (1.47, 1.25)] {
        for x in [0.04, 1.6] {
            for y in [front, front + 0.47] {
                composition::contains(&compact, [x, y, height]);
            }
        }
    }
    let checkout = composition::part_points(&compact, "room/checkout", Matrix::default());
    composition::bounds(&checkout, [1.62, 0.02, 0.], [2.23, 0.49, 1.5]);
    let again = draw_room(
        "composition",
        3.2,
        3.1,
        2.6,
        Matrix::default(),
        Mode::Render,
    );
    composition::same_mesh(&compact, &again);
    let changed = draw_room(
        "other-stock",
        3.2,
        3.1,
        2.6,
        Matrix::default(),
        Mode::Render,
    );
    composition::different_detail(&compact, &changed);
    for (front, height) in [(0.53, 0.81667), (1., 1.03333), (1.47, 1.25)] {
        for x in [0.04, 1.6] {
            for y in [front, front + 0.47] {
                composition::contains(&changed, [x, y, height]);
            }
        }
    }

    let market = draw_room("composition", 6., 6., 2.8, Matrix::default(), Mode::Render);
    for side in ["left", "right"] {
        let bank = composition::part_points(
            &market,
            &format!("room/produce-bank-{side}"),
            Matrix::default(),
        );
        let (low, high) = if side == "left" {
            (-0.05, 2.45)
        } else {
            (3.55, 6.05)
        };
        composition::bounds(&bank, [low, 1.4, -0.01], [high, 4.75, 1.6]);
        assert!(
            bank.iter().any(|p| p[2].abs() < 1e-8),
            "produce bank must be floor-supported"
        );
    }
    let storage = composition::part_points(&market, "upper-storage-left", Matrix::default());
    assert!(storage.iter().any(|p| (p[2] - 1.98).abs() < 1e-8));
    let frame = Matrix([[0., 1., 0.], [-1., 0., 0.], [0., 0., 1.], [9., -3., 2.]]);
    let placed = draw_room("composition", 6., 6., 2.8, frame, Mode::Render);
    let expected = composition::positions(&market)
        .into_iter()
        .map(|p| frame.point(p))
        .collect::<Vec<_>>();
    let actual = composition::positions(&placed);
    assert_eq!(actual.len(), expected.len());
    // Both paths quantize once; placing an already quantized local corner may
    // differ by the sum of the two half-quantum errors at decimal ties.
    for (a, b) in actual.iter().zip(expected) {
        assert!(a.iter().zip(b).all(|(a, b)| (*a - b).abs() <= 0.0000101));
    }
}
