use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use superspec_aml::material::Material;
use superspec_engine::worldspec::themes::kowloon::{
    building::{Plan, Request, Shape, footprint::Footprint, shell},
    deck,
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pavement::materials,
    pen::Pen,
};
use superspec_geometry::obstructions::Budget;
use superspec_geometry::{Matrix, V3};
#[path = "support/assembly_reference.rs"]
#[allow(dead_code)]
mod assembly;
#[path = "support/sign_reference.rs"]
#[allow(dead_code)]
mod reference;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Input {
    host: String,
    matrix: [V3; 4],
    width: Option<f64>,
    depth: Option<f64>,
    height: f64,
    #[serde(default)]
    bottom_center: bool,
    stories: usize,
    #[serde(rename = "loop")]
    loop_: Option<Vec<V3>>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    input: Input,
    seed: String,
    fine_marks: bool,
    roofs: Value,
    geometry: assembly::Geometry,
    charts: Vec<assembly::Chart>,
}
#[derive(Deserialize)]
struct Golden {
    initial: Vec<Material>,
    cases: Vec<Case>,
}
pub fn check_reference_building_shells() {
    let g: Golden = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/building-shell.json"
    ))
    .unwrap();
    for c in g.cases {
        let r = c.input;
        let shape = if let Some(p) = r.loop_ {
            Shape::Polygon(Footprint::new(&p, r.height).unwrap())
        } else {
            Shape::Box {
                width: r.width.unwrap(),
                depth: r.depth.unwrap(),
                height: r.height,
                bottom_center: r.bottom_center,
            }
        };
        let mut budget = Budget::new(64_000_000, &|| false).unwrap();
        let plan = Plan::new(
            vec![Request {
                host: r.host.clone(),
                matrix: Matrix(r.matrix),
                shape,
                stories: Some(r.stories),
                shops: Some(0.),
                base_wall: None,
            }],
            &c.seed,
            vec![],
            &[],
            &mut budget,
        )
        .unwrap();
        let site = &plan.sites()[0];
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
                declared: &BTreeMap::new(),
                colors: &BTreeMap::from([("tile".into(), "#a8674c".into())]),
                authored: &authored,
            },
            &mut catalog,
        )
        .unwrap();
        let identity = format!("{}/{}", c.seed, r.host);
        let deck = deck::Settings {
            host: &r.host,
            seed: &identity,
            grime: 0.6,
            wear: 0.7,
            fine_marks: c.fine_marks,
        };
        for mode in [Mode::Render, Mode::Survey] {
            let mut mesh = Mesh::new(mode, 500_000, &|| false).unwrap();
            let mut roofs = Vec::new();
            for lot in &site.lots {
                if let Some(roof) = shell::draw(
                    &mut Fixture {
                        pen: Pen::new(site.matrix, "envelope").unwrap(),
                        mesh: &mut mesh,
                        repeats: &|slot| catalog.get(slot).map_or(1., |m| m.repeat),
                    },
                    site,
                    lot,
                    &shell::Settings {
                        deck: &deck,
                        assets: &assets,
                        coat: "wall",
                    },
                    &mut budget,
                )
                .unwrap()
                {
                    let r = roof.request;
                    roofs.push(json!({"matrix":roof.pen.matrix.0,"request":{"lot_key":r.lot_key,"tenement":r.tenement,"wall_tint":r.wall_tint,"width":r.width,"depth":r.depth,"parapet":r.parapet,"ceiling":r.ceiling,"perimeter":r.perimeter}}));
                }
            }
            reference::close(&json!(roofs), &c.roofs);
            assert_eq!(
                mesh.triangle_count() * 3,
                c.geometry.indices.len(),
                "{}",
                c.seed
            );
            if !mesh.surveying() {
                assembly::check_triangles(&mesh, &c.geometry, &c.seed);
                assembly::check_finishes(&mut mesh, &c.geometry, &c.charts, &c.seed);
            }
        }
    }
}
#[test]
fn shells_close_stepped_lots_polygon_parapets_and_decks_with_stable_roof_reservations() {
    check_reference_building_shells();
}
