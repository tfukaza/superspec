use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use superspec_aml::material::Material;
use superspec_engine::worldspec::themes::kowloon::{
    building::{Plan, Request, Shape, draw, finish, footprint::Footprint},
    clearance::RenderTriangle,
    deck,
    kit::Kit,
    mesh::{Mesh, Mode},
    metal::Binding,
    pavement::materials,
    shop,
    signs::panel::Settings,
    tile, timber, weathered,
};
use superspec_geometry::obstructions::{Budget, Obstructions};
use superspec_geometry::{Matrix, V3};
#[path = "support/assembly_reference.rs"]
#[allow(dead_code)]
mod assembly;
#[allow(dead_code)]
#[path = "support/shop_composition.rs"]
mod composition;
#[path = "support/building_native_shop.rs"]
mod native_shop;
#[path = "support/sign_reference.rs"]
#[allow(dead_code)]
mod reference;
#[derive(Deserialize)]
struct Case {
    seed: String,
    detail: u8,
    width: f64,
    depth: f64,
    height: f64,
    stories: usize,
    shops: f64,
    #[serde(rename = "loop")]
    loop_: Option<Vec<V3>>,
    matrix: [V3; 4],
    geometry: assembly::Geometry,
    charts: Vec<assembly::Chart>,
    plans: Value,
    #[serde(rename = "nativeShop")]
    native_shop: Option<native_shop::Shop>,
}
#[derive(Deserialize)]
struct Golden {
    initial: Vec<Material>,
    cases: Vec<Case>,
}
pub fn check_reference_buildings() {
    let g: Golden =
        serde_json::from_str(include_str!("../../../tests/fixtures/rust/building.json")).unwrap();
    for c in g.cases {
        let mut budget = Budget::new(128_000_000, &|| false).unwrap();
        let shape = if let Some(p) = &c.loop_ {
            Shape::Polygon(Footprint::new(p, c.height).unwrap())
        } else {
            Shape::Box {
                width: c.width,
                depth: c.depth,
                height: c.height,
                bottom_center: false,
            }
        };
        let ground = [
            RenderTriangle {
                points: [[-30., -30., 0.], [30., -30., 0.], [30., 30., 0.]],
                visible: true,
            },
            RenderTriangle {
                points: [[-30., -30., 0.], [30., 30., 0.], [-30., 30., 0.]],
                visible: true,
            },
        ];
        let plan = Plan::new(
            vec![Request {
                host: "host".into(),
                matrix: Matrix(c.matrix),
                shape,
                stories: Some(c.stories),
                shops: Some(c.shops),
                base_wall: None,
            }],
            &c.seed,
            vec![],
            &ground,
            &mut budget,
        )
        .unwrap();
        let identity = format!("{}/host", c.seed);
        let binding = |id: &str| Binding {
            material: id.into(),
            repeat: 1.,
        };
        let kit = Kit::new(
            Settings {
                weathering: weathered::Settings {
                    seed: identity.clone(),
                    wear: 0.25,
                    grime: 0.6,
                    rust: 0.4,
                    texture_density: 32.,
                    metal: binding("metal"),
                    sheet: binding("sheet"),
                    paint: binding("paint"),
                },
                lettering: true,
                timber: timber::Settings {
                    textured: true,
                    timber_repeat: 1.,
                    atlas_repeat: 1.,
                    ..timber::Settings::default()
                },
            },
            c.detail,
        )
        .unwrap();
        let materials = finish::Materials {
            wall: binding("wall"),
            concrete: binding("concrete"),
            substrates: vec![],
            tiles: tile::Materials {
                ceramic: "$kowloon/wall-ceramic".into(),
                bed: "$kowloon/wall-tile-bed".into(),
                grime: "$kowloon/wall-tile-grime".into(),
                repeat: 1.,
                bed_repeat: 1.,
                bed_roughness: 1.,
                roughness: 0.7,
            },
            repair: binding("render-repair"),
            density: 32.,
        };
        let finishes = plan.sites()[0]
            .lots
            .iter()
            .map(|l| finish::choose(&kit, l, &materials).unwrap())
            .collect::<Vec<_>>();
        let authored = g.initial.iter().map(|m| m.id.clone()).collect();
        let mut catalog = g
            .initial
            .iter()
            .cloned()
            .map(|m| (m.id.clone(), m))
            .collect();
        let assets = materials::assets(
            &materials::Settings {
                scope: "$deck/concrete",
                base: "concrete",
                declared: &BTreeMap::from([("asphalt".into(), "roofing".into())]),
                colors: &BTreeMap::from([("tile".into(), "#a8674c".into())]),
                authored: &authored,
            },
            &mut catalog,
        )
        .unwrap();
        let deck = deck::Settings {
            host: "host",
            seed: &identity,
            grime: 0.6,
            wear: 0.25,
            fine_marks: c.detail > 0,
        };
        let mut rendered = vec![];
        for mode in [Mode::Render, Mode::Survey] {
            let mut mesh = Mesh::new(mode, 500_000, &|| false).unwrap();
            let book = draw::site(
                &plan,
                0,
                &mut mesh,
                &draw::Settings {
                    kit: &kit,
                    finishes: &finishes,
                    concrete: binding("concrete"),
                    wall: binding("wall"),
                    brushed: binding("brushed-metal"),
                    plants: true,
                    cloth: true,
                    posters: true,
                    shops: Some(shop::shell::Assets {
                        room: shop::room::Assets {
                            posters: true,
                            grocer_props: true,
                            snack_packets: true,
                            market_apple: true,
                            shop_register: false,
                            market_trays: false,
                        },
                        restaurant: shop::roast::Assets {
                            hero_props: false,
                            food: true,
                            metal: false,
                            surfaces: true,
                            tiles: true,
                        },
                    }),
                    deck: &deck,
                    pavement: &assets,
                },
                &|_| 1.,
                &mut budget,
                &|| false,
            )
            .unwrap_or_else(|error| panic!("{}: {error}", c.seed));
            let empty = Obstructions::new(&[], &mut budget).unwrap();
            let plans = book
                .settle(&empty, &mut budget, |_, p, s| {
                    let first = mesh.triangle_count();
                    p.draw(&mut mesh, &|_| 1., s)?;
                    Ok(mesh.triangle_count() - first)
                })
                .unwrap();
            reference::close_at(
                &serde_json::to_value(plans).unwrap(),
                &c.plans,
                &format!("{} sign plans", c.seed),
            );
            if !mesh.surveying() {
                rendered = composition::positions(&mesh);
                if let Some(shop) = &c.native_shop {
                    let (structure, selected) = native_shop::structural(&mesh, shop);
                    assembly::check_triangles(&structure, &c.geometry, &c.seed);
                    assembly::check_selected_finishes(&mut mesh, &selected, &c.charts, &c.seed);
                } else {
                    assembly::check_triangles(&mesh, &c.geometry, &c.seed);
                    assembly::check_finishes(&mut mesh, &c.geometry, &c.charts, &c.seed);
                }
            } else {
                assert_eq!(mesh.survey.len(), rendered.len());
                for (actual, expected) in mesh.survey.iter().zip(&rendered) {
                    for (a, b) in actual.iter().zip(expected) {
                        // Survey delivery quantizes final deck/sign corners to 10 micrometers.
                        assert!((a - b).abs() <= 0.0000051, "{} survey position", c.seed);
                    }
                }
            }
            if c.native_shop.is_none() {
                assert_eq!(
                    mesh.triangle_count() * 3,
                    c.geometry.indices.len(),
                    "{}",
                    c.seed
                );
            }
        }
    }
}
#[test]
fn complete_buildings_preserve_connected_facades_roofs_shops_and_signs() {
    check_reference_buildings();
}
