use serde::Deserialize;
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
#[path = "support/assembly_reference.rs"]
mod assembly;
#[allow(dead_code)]
#[path = "support/shop_composition.rs"]
mod composition;
#[path = "support/facade_input.rs"]
mod input;
#[allow(dead_code)]
#[path = "support/sign_reference.rs"]
mod reference;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    group: usize,
    style_seed: String,
    lot: input::LotData,
    request: input::Input,
    detail: u8,
    settings: Settings,
    finish: Finish,
    geometry: assembly::Geometry,
    charts: Vec<assembly::Chart>,
    parts: Vec<String>,
    room_plans: Vec<Room>,
    proposals: Vec<serde_json::Value>,
}
#[derive(Deserialize)]
struct Room {
    x0: f64,
    x1: f64,
    floor: f64,
    head: f64,
    depth: f64,
    key: String,
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
#[derive(Deserialize)]
struct Golden {
    cases: Vec<Case>,
}
pub fn check_reference_open_facades() {
    let g: Golden = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/open-facade.json"
    ))
    .unwrap();
    for c in g.cases {
        let lot = c.lot.build(&c.style_seed, c.request.width);
        let kit = Kit::new(c.settings, c.detail).unwrap();
        let r = c.request.request(&lot);
        let q = Queries {
            reach: &|_, _, limit| Ok(limit),
            hollow: &|_, _, _, _| Ok(3.1),
        };
        let plan = facade_plan::plan(&r, &kit, &q, &|| false).unwrap();
        let rooms: Vec<_> = plan
            .bays
            .iter()
            .filter_map(|bay| bay.room.as_ref())
            .collect();
        assert_eq!(rooms.len(), c.room_plans.len());
        assert!(!rooms.is_empty());
        for (actual, expected) in rooms.iter().zip(&c.room_plans) {
            reference::close(
                &serde_json::to_value(actual).unwrap(),
                &serde_json::json!({
                    "x0": expected.x0, "x1": expected.x1, "floor": expected.floor,
                    "head": expected.head, "depth": expected.depth, "key": expected.key,
                }),
            );
        }
        let mut rendered = vec![];
        for mode in [Mode::Render, Mode::Survey] {
            let mut mesh = Mesh::new(mode, 400000, &|| false).unwrap();
            let proposals = facade::draw(
                &mut Fixture {
                    pen: Pen::new(Matrix::default(), "envelope").unwrap(),
                    mesh: &mut mesh,
                    repeats: &|_| 1.,
                },
                &r,
                &facade::Settings {
                    kit: &kit,
                    finish: &c.finish,
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
            assert_eq!(
                proposals.len(),
                c.proposals.len(),
                "facade {} proposals",
                c.group
            );
            for (p, e) in proposals.iter().zip(&c.proposals) {
                reference::close(&reference::metadata(p), e);
            }
            if mesh.surveying() {
                composition::same_positions(&mesh.survey, &rendered);
            } else {
                composition::valid(&mesh);
                rendered = composition::positions(&mesh);
                let mut room_triangles = std::collections::BTreeSet::new();
                for room in &c.room_plans {
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
                        stock.iter().any(|p| p[1] < -0.1),
                        "each selected trade must furnish its room"
                    );
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
                let expected_parts: std::collections::BTreeSet<_> = c.parts.iter().collect();
                for surface in &mesh.surfaces {
                    assert!(
                        expected_parts.contains(&surface.part)
                            || c.room_plans
                                .iter()
                                .any(|room| native_owner(&surface.part, room)),
                        "unclassified facade owner {}",
                        surface.part
                    );
                }
                let selected: Vec<_> = mesh
                    .triangle_surfaces
                    .iter()
                    .enumerate()
                    .filter_map(|(t, &s)| {
                        let part = &mesh.surfaces[s].part;
                        ((!part.starts_with("shop-")
                            || part == "shop-frontage"
                            || part == "shop-herbalist")
                            && !room_triangles.contains(&t))
                        .then_some(t)
                    })
                    .collect();
                let strict = selected_mesh(&mesh, &selected);
                assert_eq!(
                    strict
                        .triangle_surfaces
                        .iter()
                        .map(|&s| &strict.surfaces[s].part)
                        .collect::<Vec<_>>(),
                    c.parts.iter().collect::<Vec<_>>()
                );
                assembly::check_triangles(
                    &strict,
                    &c.geometry,
                    &format!("facade {} retained owners", c.group),
                );
                assembly::check_selected_finishes(
                    &mut mesh,
                    &selected,
                    &c.charts,
                    &format!("facade {}", c.group),
                );
            }
        }
    }
}
#[test]
fn open_facades_match_stocked_rooms_frontage_and_deferred_signs() {
    check_reference_open_facades()
}
