use serde::Deserialize;
use serde_json::{Value, json};
use superspec_engine::worldspec::themes::kowloon::pavement::{
    context::Wall,
    layout::{Anchor, Layout},
    plan::{Pigment, Planner, Settings},
    surface::Finish,
    terrain,
};
use superspec_geometry::obstructions::Budget;
use superspec_geometry::{Matrix, V3};
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
struct Color {
    finish: Finish,
    value: V3,
}
#[derive(Deserialize)]
struct Sample {
    point: V3,
    height: f64,
    colors: Vec<Color>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TerrainCase {
    index: usize,
    sampling: f64,
    fine_marks: bool,
    receiving: Vec<Vec<V3>>,
    result: Value,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    options: Settings,
    seed: u32,
    grime: f64,
    wear: f64,
    tile_size: f64,
    faces: Vec<Vec<V3>>,
    walls: Vec<Wall>,
    layout: Grid,
    patches: Value,
    samples: Vec<Vec<Sample>>,
    terrains: Vec<TerrainCase>,
}
#[derive(Deserialize)]
struct Golden {
    cases: Vec<Case>,
}
fn fixture() -> Golden {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/pavement-plan.json"
    ))
    .unwrap()
}
pub fn check_reference_pavement_plans() {
    for (i, c) in fixture().cases.into_iter().enumerate() {
        let mut budget = Budget::new(32_000_000, &|| false).unwrap();
        let grid = c.layout.native();
        let planner = Planner {
            layout: &grid,
            settings: &c.options,
            seed: c.seed,
            walls: &c.walls,
        };
        let patches = planner.patches(&c.faces, &mut budget).unwrap();
        reference::close(&json!(patches), &c.patches);
        assert_eq!(patches.len(), c.samples.len());
        for (n, (patch, samples)) in patches.iter().zip(c.samples).enumerate() {
            let pigment = Pigment {
                patch,
                settings: &c.options,
                seed: c.seed,
                grime: c.grime,
                wear: c.wear,
                tile_size: c.tile_size,
            };
            for s in samples {
                assert!(
                    (pigment.height(s.point) - s.height).abs() < 2e-10,
                    "case {i} patch {n}"
                );
                for color in s.colors {
                    reference::close(
                        &json!(pigment.color(s.point, color.finish)),
                        &json!(color.value),
                    );
                }
            }
        }
        for t in c.terrains {
            let patch = &patches[t.index];
            let pigment = Pigment {
                patch,
                settings: &c.options,
                seed: c.seed,
                grime: c.grime,
                wear: c.wear,
                tile_size: c.tile_size,
            };
            let actual = terrain::build(
                &grid,
                patch,
                &pigment,
                &t.receiving,
                t.sampling,
                t.fine_marks,
                &mut budget,
            )
            .unwrap();
            reference::close(&json!(actual), &t.result);
            let original = actual
                .patch_faces
                .iter()
                .map(|p| {
                    superspec_engine::worldspec::themes::kowloon::pavement::field::area(p).abs()
                })
                .sum::<f64>();
            let pieces = actual
                .pieces
                .iter()
                .map(|p| {
                    superspec_engine::worldspec::themes::kowloon::pavement::field::area(&p.points)
                        .abs()
                })
                .sum::<f64>();
            assert!(
                (original - pieces).abs() < 1e-8,
                "manhole pieces retain the receiving footprint"
            );
        }
    }
}
#[test]
fn seeded_ground_pours_keep_supported_covers_wall_damage_and_damp_pigment() {
    check_reference_pavement_plans();
}
#[test]
fn invalid_or_cancelled_plans_fail_before_output_and_recover() {
    let c = fixture().cases.remove(0);
    let grid = c.layout.native();
    let mut settings = c.options;
    let mut budget = Budget::new(4_000_000, &|| false).unwrap();
    settings.tile_share = 2.;
    assert!(settings.validate().is_err());
    settings.tile_share = 0.1;
    let planner = Planner {
        layout: &grid,
        settings: &settings,
        seed: c.seed,
        walls: &c.walls,
    };
    assert!(
        planner
            .patches(&c.faces, &mut Budget::new(1000, &|| true).unwrap())
            .is_err()
    );
    assert!(
        planner
            .patches(&c.faces, &mut Budget::new(1, &|| false).unwrap())
            .is_err()
    );
    assert!(
        planner
            .patches(&[vec![[f64::NAN; 3]; 3]], &mut budget)
            .is_err()
    );
    assert!(!planner.patches(&c.faces, &mut budget).unwrap().is_empty());
}
