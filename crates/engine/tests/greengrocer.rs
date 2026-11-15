use serde::Deserialize;
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
use superspec_geometry::{Matrix, V3};
#[path = "support/shop_composition.rs"]
#[allow(dead_code)]
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
}
#[derive(Deserialize)]
struct Golden {
    rooms: Vec<Case>,
}
fn kit(seed: &str, detail: u8) -> Kit {
    Kit::new(serde_json::from_value(serde_json::json!({"weathering":{"seed":seed,"wear":0.25,"grime":0.6,"rust":0.4,"textureDensity":32,"metal":{"material":"metal","repeat":1.7},"sheet":{"material":"sheet","repeat":1.7},"paint":{"material":"paint","repeat":1.7}},"lettering":true,"timber":{"textured":true,"timberRepeat":1.7,"atlasRepeat":1.7}})).unwrap(),detail).unwrap()
}
pub fn check_reference_greengrocer() {
    let g: Golden = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/greengrocer.json"
    ))
    .unwrap();
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
            greengrocer::furnish(
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
                let checkout = composition::part_points(&mesh, "room/checkout", Matrix(c.matrix));
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
                        Matrix(c.matrix),
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
#[test]
fn complete_greengrocer_preserves_stock_transforms_materials_and_named_streams() {
    check_reference_greengrocer();
}
