use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use superspec_engine::worldspec::themes::kowloon::{
    mesh::{Mesh, Mode},
    pavement::{
        context::Wall,
        layout::{Anchor, Layout},
        plan::Settings,
        render::{self, Assets, Site},
        surface::{Finish, Source},
    },
};
use superspec_geometry::obstructions::Budget;
use superspec_geometry::{Matrix, V3};
#[path = "support/assembly_reference.rs"]
#[allow(dead_code)]
mod assembly;
#[allow(dead_code)]
#[path = "support/sign_reference.rs"]
mod reference;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Grid {
    world: [V3; 4],
    columns: Vec<f64>,
    rows: Vec<f64>,
    zones_x: Vec<f64>,
    zones_y: Vec<f64>,
    construction_x: Vec<f64>,
    construction_y: Vec<f64>,
    anchors: Vec<Anchor>,
}
impl Grid {
    fn native(self) -> Layout {
        Layout {
            world: Matrix(self.world),
            columns: self.columns,
            rows: self.rows,
            zones_x: self.zones_x,
            zones_y: self.zones_y,
            construction_x: self.construction_x,
            construction_y: self.construction_y,
            anchors: self.anchors,
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    host: String,
    seed: String,
    layout: Grid,
    faces: Vec<Vec<V3>>,
    walls: Vec<Wall>,
    options: Settings,
    sources: BTreeMap<String, Source>,
    sampling: f64,
    fine_marks: bool,
    flush: bool,
    geometry: assembly::Geometry,
    charts: Vec<assembly::Chart>,
    regions: Value,
}
#[derive(Deserialize)]
struct Golden {
    cases: Vec<Case>,
}
pub fn check_reference_pavement_render() {
    let g: Golden = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/pavement-render.json"
    ))
    .unwrap();
    for c in g.cases {
        let grid = c.layout.native();
        let sources = c
            .sources
            .iter()
            .filter_map(|(k, s)| {
                serde_json::from_value::<Finish>(json!(k))
                    .ok()
                    .map(|f| (f, s.clone()))
            })
            .collect();
        let assets = Assets {
            sources,
            tile_bed: c.sources["tile-bed"].clone(),
            edge: c.sources["edge"].clone(),
            water: c.sources["water-film"].clone(),
            paper: c.sources["paper"].clone(),
            brick: c.sources["brick"].clone(),
            can: c.sources["can"].clone(),
        };
        let seams = c.walls.iter().map(|w| w.seam).collect::<Vec<_>>();
        let site = Site {
            host: &c.host,
            part: &c.host,
            seed: &c.seed,
            layout: &grid,
            faces: &c.faces,
            seams: &seams,
            walls: &c.walls,
            settings: &c.options,
            grime: 0.6,
            wear: 0.7,
            sampling: c.sampling,
            tile_size: 0.4,
            tile_aspect: None,
            tile_wear: None,
            tile_grime: None,
            density: Some(32.),
            asphalt_density: None,
            fine_marks: c.fine_marks,
            flush: c.flush,
        };
        for mode in [Mode::Render, Mode::Survey] {
            let mut mesh = Mesh::new(mode, 500_000, &|| false).unwrap();
            let mut budget = Budget::new(32_000_000, &|| false).unwrap();
            let regions = render::draw_preserved(&mut mesh, &site, &assets, &mut budget).unwrap();
            reference::close(&json!(regions.iter().map(|r|json!({"id":r.id,"kind":r.kind,"bounds":r.bounds,"footprint":r.footprint})).collect::<Vec<_>>()),&c.regions);
            assert_eq!(
                mesh.triangle_count() * 3,
                c.geometry.indices.len(),
                "{} triangle count",
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
fn complete_pavement_preserves_holes_water_rims_litter_and_intrinsic_material_receivers() {
    check_reference_pavement_render();
}
