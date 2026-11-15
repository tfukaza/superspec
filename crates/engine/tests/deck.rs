use serde::Deserialize;
use std::collections::BTreeMap;
use superspec_aml::material::Material;
use superspec_engine::worldspec::themes::kowloon::{
    deck::{self, Settings},
    mesh::{Mesh, Mode},
    pavement::materials,
};
use superspec_geometry::V3;
use superspec_geometry::obstructions::Budget;
#[path = "support/assembly_reference.rs"]
#[allow(dead_code)]
mod assembly;
#[path = "support/sign_reference.rs"]
#[allow(dead_code)]
mod reference;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    corners: Vec<V3>,
    initial: Vec<Material>,
    seed: String,
    felt: Option<String>,
    fine_marks: bool,
    geometry: assembly::Geometry,
    charts: Vec<assembly::Chart>,
}
#[derive(Deserialize)]
struct Golden {
    cases: Vec<Case>,
}
pub fn check_reference_decks() {
    let golden: Golden =
        serde_json::from_str(include_str!("../../../tests/fixtures/rust/deck.json")).unwrap();
    for c in golden.cases {
        let authored = c.initial.iter().map(|m| m.id.clone()).collect();
        let mut catalog = c.initial.into_iter().map(|m| (m.id.clone(), m)).collect();
        let declared = c
            .felt
            .map(|id| BTreeMap::from([("asphalt".into(), id)]))
            .unwrap_or_default();
        let colors = BTreeMap::from([("tile".into(), "#a8674c".into())]);
        let assets = materials::assets(
            &materials::Settings {
                scope: "$deck/screed",
                base: "screed",
                declared: &declared,
                colors: &colors,
                authored: &authored,
            },
            &mut catalog,
        )
        .unwrap();
        let settings = Settings {
            host: "host",
            seed: &c.seed,
            grime: 0.6,
            wear: 0.7,
            fine_marks: c.fine_marks,
        };
        for mode in [Mode::Render, Mode::Survey] {
            let mut mesh = Mesh::new(mode, 500_000, &|| false).unwrap();
            let mut budget = Budget::new(64_000_000, &|| false).unwrap();
            deck::lay(
                &mut mesh,
                &settings,
                &assets,
                "terrace",
                &c.corners,
                &mut budget,
            )
            .unwrap();
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
fn roof_terraces_preserve_winding_scale_felt_and_quarry_tiles() {
    check_reference_decks();
}
