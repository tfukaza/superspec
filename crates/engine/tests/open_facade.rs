use superspec_engine::worldspec::themes::kowloon::{
    facade,
    facade_envelope::Finish,
    facade_plan::{self, Queries},
    fixture::Fixture,
    kit::Kit,
    mesh::{Mesh, Mode},
    pen::Pen,
    signs::panel::Settings,
};
use superspec_geometry::Matrix;
#[allow(dead_code)]
#[path = "support/shop_composition.rs"]
mod composition;
#[path = "support/facade_input.rs"]
mod input;
struct Room {
    x0: f64,
    x1: f64,
    floor: f64,
    head: f64,
    depth: f64,
    trade: String,
}
fn native_owner(part: &str, room: &Room) -> bool {
    let Some(suffix) = part.strip_prefix(&format!("shop-{}", room.trade)) else {
        return false;
    };
    if matches!(suffix, "" | "/services" | "/room-detail") {
        return true;
    }
    match room.trade.as_str() {
        "grocer" => suffix == "/checkout/register",
        "greengrocer" => matches!(
            suffix,
            "/checkout" | "/back-stock/rear-timber" | "/back-stock/rear-steel" | "/back-stock/side"
        ),
        "roast" => matches!(
            suffix,
            "/room-wall-left"
                | "/room-wall-right"
                | "/room-wall-back"
                | "/checkout/register"
                | "/chopping-board"
                | "/board-scoring"
                | "/cleaver"
                | "/display"
                | "/work-towel"
                | "/display-food"
                | "/task-lamp-0"
                | "/task-lamp-1"
                | "/order-slips"
                | "/tableware"
                | "/posters"
                | "/dining/table-0"
                | "/dining/table-1"
                | "/dining/table-2"
        ),
        _ => false,
    }
}
fn selected_mesh(mesh: &Mesh<'_>, triangles: &[usize]) -> Mesh<'static> {
    let mut selected = Mesh::new(Mode::Render, mesh.triangle_count() + 1, &|| false).unwrap();
    selected.vertices = mesh.vertices.clone();
    selected.surfaces = mesh.surfaces.clone();
    for &t in triangles {
        selected
            .indices
            .extend_from_slice(&mesh.indices[t * 3..t * 3 + 3]);
        selected.triangle_surfaces.push(mesh.triangle_surfaces[t]);
    }
    selected
}
fn in_room(mesh: &Mesh<'_>, triangle: usize, room: &Room) -> bool {
    if mesh.surfaces[mesh.triangle_surfaces[triangle]].part != "envelope" {
        return false;
    }
    let low = [room.x0, -room.depth, room.floor];
    let high = [room.x1, 0., room.head];
    mesh.indices[triangle * 3..triangle * 3 + 3]
        .iter()
        .all(|&i| {
            mesh.vertices[i as usize]
                .position
                .iter()
                .enumerate()
                .all(|(axis, value)| {
                    // Delivered coordinates are quantized to ten micrometers.
                    *value >= low[axis] - 0.0000051 && *value <= high[axis] + 0.0000051
                })
        })
}
/// Whether `part` stands inside the planned room, behind its frontage.
fn furnishes(mesh: &Mesh<'_>, part: &str, room: &facade_plan::Room) -> bool {
    mesh.indices
        .chunks_exact(3)
        .zip(&mesh.triangle_surfaces)
        .filter(|(_, surface)| mesh.surfaces[**surface].part == part)
        .any(|(triangle, _)| {
            triangle.iter().any(|&i| {
                let p = mesh.vertices[i as usize].position;
                p[0] >= room.x0 && p[0] <= room.x1 && p[1] < -0.1
            })
        })
}
/// Shopfront facades on three lots: every planned room is an open enclosure of its own that one
/// trade furnishes within its bounds, with services under the head, and the survey agrees with
/// the render.
pub fn check_open_facades() {
    for (group, shift, concrete) in [(31, 0.3, false), (32, -0.2, true), (33, 0., false)] {
        let top = 7.050000000000001;
        let request: input::Input = serde_json::from_value(serde_json::json!({
            "role": "front", "key": format!("front-{group}"), "width": 9.8, "shift": shift,
            "body_span": [-0.1, 10.],
            "ground": [{"u0": 0., "u1": 9.8, "v0": 0., "v1": 3.4}],
            "body": [{"u0": -0.1, "u1": 10., "v0": 3.4, "v1": top}],
            "shops": 1.,
        }))
        .unwrap();
        let settings: Settings = serde_json::from_value(serde_json::json!({
            "weathering": {
                "seed": format!("facade/{group}"), "wear": 0.25, "grime": 0.6, "rust": 0.4,
                "textureDensity": 32,
                "metal": {"material": "metal", "repeat": 1.},
                "paint": {"material": "paint", "repeat": 1.},
                "sheet": {"material": "sheet", "repeat": 1.},
            },
            "lettering": true,
            "timber": {"textured": true, "timberRepeat": 1., "atlasRepeat": 1.},
        }))
        .unwrap();
        let finish: Finish = serde_json::from_value(serde_json::json!({
            "coat": {"material": if concrete { "concrete" } else { "wall" }, "repeat": 1.},
            "bare": {"material": "concrete", "repeat": 1.},
            "concrete": concrete, "seed": 43 + group, "density": 32, "tiles": null,
        }))
        .unwrap();
        let lot = input::LotData {
            kind: "tenement".into(),
            ground: 3.4,
            roof: 6.4,
            top,
            stories: 2,
            story_height: 3.,
            key: format!("lot-{group}"),
        }
        .build("facade-style/6", request.width);
        let kit = Kit::new(settings, 2).unwrap();
        let r = request.request(&lot);
        let q = Queries {
            reach: &|_, _, limit| Ok(limit),
            hollow: &|_, _, _, _| Ok(3.1),
        };
        let plan = facade_plan::plan(&r, &kit, &q, &|| false).unwrap();
        let planned: Vec<_> = plan
            .bays
            .iter()
            .filter_map(|bay| bay.room.as_ref())
            .collect();
        assert!(!planned.is_empty());
        let mut rendered = vec![];
        for mode in [Mode::Render, Mode::Survey] {
            let mut mesh = Mesh::new(mode, 400000, &|| false).unwrap();
            facade::draw(
                &mut Fixture {
                    pen: Pen::new(Matrix::default(), "envelope").unwrap(),
                    mesh: &mut mesh,
                    repeats: &|_| 1.,
                },
                &r,
                &facade::Settings {
                    kit: &kit,
                    finish: &finish,
                    concrete: superspec_engine::worldspec::themes::kowloon::metal::Binding {
                        material: "concrete".into(),
                        repeat: 1.,
                    },
                    plants: true,
                    cloth: true,
                    posters: true,
                    shops: Some(
                        superspec_engine::worldspec::themes::kowloon::shop::shell::Assets {
                            room:
                                superspec_engine::worldspec::themes::kowloon::shop::room::Assets {
                                    posters: true,
                                    grocer_props: true,
                                    snack_packets: true,
                                    market_apple: true,
                                    shop_register: false,
                                    market_trays: false,
                                },
                            restaurant:
                                superspec_engine::worldspec::themes::kowloon::shop::roast::Assets {
                                    hero_props: false,
                                    food: true,
                                    metal: false,
                                    surfaces: true,
                                    tiles: true,
                                },
                        },
                    ),
                },
                &facade::Queries {
                    geometry: Queries {
                        reach: q.reach,
                        hollow: q.hollow,
                    },
                    miters: &|_, _| Ok([None; 2]),
                    shop_near: &|_, _, _| Ok(None),
                    frontage: Some(&|_, _, _, _| Ok(true)),
                },
                &|| false,
            )
            .unwrap();
            if mesh.surveying() {
                composition::same_positions(&mesh.survey, &rendered);
            } else {
                composition::valid(&mesh);
                rendered = composition::positions(&mesh);
                let mut room_triangles = std::collections::BTreeSet::new();
                for planned in &planned {
                    let trades: Vec<_> = ["grocer", "greengrocer", "roast", "hardware", "herbalist"]
                        .into_iter()
                        .filter(|trade| furnishes(&mesh, &format!("shop-{trade}"), planned))
                        .collect();
                    assert_eq!(
                        trades.len(),
                        1,
                        "one trade must furnish {}: {trades:?}",
                        planned.key
                    );
                    let room = &Room {
                        x0: planned.x0,
                        x1: planned.x1,
                        floor: planned.floor,
                        head: planned.head,
                        depth: planned.depth,
                        trade: trades[0].into(),
                    };
                    let selected: Vec<_> = (0..mesh.triangle_count())
                        .filter(|&t| in_room(&mesh, t, room))
                        .collect();
                    assert!(
                        selected.iter().all(|t| room_triangles.insert(*t)),
                        "enclosures must have disjoint ownership"
                    );
                    let enclosure = selected_mesh(&mesh, &selected);
                    composition::open_room(
                        &enclosure,
                        "envelope",
                        Matrix::default(),
                        [room.x0, room.x1],
                        room.depth,
                        [room.floor, room.head],
                    );
                    let trade = format!("shop-{}", room.trade);
                    let stock = composition::part_points(&mesh, &trade, Matrix::default());
                    assert!(
                        stock.iter().any(|p| (p[2] - room.floor).abs() < 0.00001),
                        "stock must retain floor support"
                    );
                    assert!(
                        stock.iter().any(|p| p[2] > room.floor + 0.6),
                        "stock must retain vertical extent"
                    );
                    for surface in &mesh.surfaces {
                        if native_owner(&surface.part, room) {
                            let points =
                                composition::part_points(&mesh, &surface.part, Matrix::default());
                            composition::bounds(
                                &points,
                                [room.x0, -room.depth, room.floor],
                                [room.x1, 0., room.head],
                            );
                        }
                    }
                    let services = composition::part_points(
                        &mesh,
                        &format!("{trade}/services"),
                        Matrix::default(),
                    );
                    composition::bounds(
                        &services,
                        [room.x0, -room.depth, room.floor],
                        [room.x1, 0., room.head],
                    );
                    assert!(services.iter().all(|p| p[2] >= room.head - 0.13));
                    assert!(services.iter().any(|p| p[0] < room.x0 + 0.1));
                    assert!(services.iter().any(|p| p[0] > room.x1 - 0.1));
                }
            }
        }
    }
}
#[test]
fn open_facades_enclose_and_furnish_each_planned_room_and_surveys_match() {
    check_open_facades()
}
