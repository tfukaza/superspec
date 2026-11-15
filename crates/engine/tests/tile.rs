use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use superspec_aml::material::Material;
use superspec_engine::{
    materialspec::surface::Inputs,
    worldspec::themes::kowloon::{
        fixture::Fixture,
        mesh::{Mesh, Mode},
        pen::Pen,
        tile::{self, Channels, Recipe, Surface, Tile},
        weathered,
    },
};
use superspec_geometry::Matrix;
#[path = "support/tile_scene.rs"]
mod scene_fixture;
#[derive(Deserialize)]
struct Pixels {
    width: usize,
    height: usize,
    color: Vec<u8>,
    normal: Vec<u8>,
    packed: Vec<u8>,
}
#[derive(Deserialize)]
struct Case {
    recipe: Recipe,
    points: Vec<[f64; 3]>,
    channels: Vec<Channels>,
    surfaces: Vec<Surface>,
    raster: Option<Pixels>,
}
#[derive(Deserialize)]
struct Golden {
    materials: Vec<Material>,
    cases: Vec<Case>,
}
fn fixture() -> Golden {
    serde_json::from_str(include_str!("../../../tests/fixtures/rust/tile.json")).unwrap()
}
fn close(a: &Value, b: &Value) {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => assert!(
            (a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 2e-10,
            "{a} != {b}"
        ),
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len());
            for (a, b) in a.iter().zip(b) {
                close(a, b);
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            for (k, b) in b {
                close(&a[k], b);
            }
        }
        _ => assert_eq!(a, b),
    }
}
fn draw(mesh: &mut Mesh<'_>, recipe: &Recipe) -> Result<(), String> {
    tile::panel(
        &mut Fixture {
            pen: Pen::new(Matrix::default(), "ceramic").unwrap(),
            mesh,
            repeats: &|_| 1.,
        },
        "ceramic",
        recipe,
        "tiles",
    )
}
pub fn check_reference_tiles() {
    let golden = fixture();
    let materials = golden
        .materials
        .into_iter()
        .map(|m| (m.id.clone(), m))
        .collect();
    let inputs = Inputs {
        materials: &materials,
        images: &BTreeMap::new(),
    };
    for (ci, case) in golden.cases.iter().enumerate() {
        let field = Tile::new(&case.recipe.tiles).unwrap();
        for (([u, v, f], channels), surface) in
            case.points.iter().zip(&case.channels).zip(&case.surfaces)
        {
            close(
                &serde_json::to_value(field.channels(*u, *v, *f).unwrap()).unwrap(),
                &serde_json::to_value(channels).unwrap(),
            );
            close(
                &serde_json::to_value(
                    field
                        .surface(*u, *v, *f, &case.recipe.materials, case.recipe.bed_tint)
                        .unwrap(),
                )
                .unwrap(),
                &serde_json::to_value(surface).unwrap(),
            );
        }
        // Revisit points in reverse to cross neighborhood cache boundaries without changing samples.
        for ([u, v, f], channels) in case.points.iter().zip(&case.channels).rev().step_by(19) {
            close(
                &serde_json::to_value(field.channels(*u, *v, *f).unwrap()).unwrap(),
                &serde_json::to_value(channels).unwrap(),
            );
        }
        if let Some(expected) = &case.raster {
            let mut mesh = Mesh::new(Mode::Render, 100, &|| false).unwrap();
            draw(&mut mesh, &case.recipe).unwrap();
            assert_eq!(mesh.triangle_count(), 2);
            let before: Vec<_> = mesh
                .vertices
                .iter()
                .map(|v| (v.position, v.normal, v.color))
                .collect();
            let plan = weathered::finish(&mut mesh, &|| false).unwrap();
            assert_eq!(plan.jobs().len(), 1);
            let bakes = plan.bake(&inputs, "tile", &|| false).unwrap();
            assert_eq!(
                (bakes[0].layout().width, bakes[0].layout().height),
                (expected.width, expected.height)
            );
            assert_eq!(
                before,
                mesh.vertices
                    .iter()
                    .map(|v| (v.position, v.normal, v.color))
                    .collect::<Vec<_>>()
            );
            for v in &mesh.vertices {
                assert_eq!(v.uv, bakes[0].layout().uv([v.position[0], v.position[2]]));
            }
            let mut scene=superspec_engine::compile(r#"<architecture version="1" id="tile"><box id="host" width="1" depth=".1" height="1" visible="false"/></architecture>"#,0).unwrap();
            let material = bakes[0].material().clone();
            scene
                .append_baked_visual(
                    "host",
                    &mesh,
                    &BTreeMap::from([(material.id.clone(), material.id.clone())]),
                    &bakes,
                )
                .unwrap();
            for (id, bytes) in [
                (material.texture.unwrap(), &expected.color),
                (material.normal_texture.unwrap(), &expected.normal),
                (
                    material.metallic_roughness_texture.unwrap(),
                    &expected.packed,
                ),
            ] {
                let a = scene.manifest.assets.iter().find(|a| a.id == id).unwrap();
                let decoded = superspec_geometry::image::decode(
                    &scene.asset_bytes[a.byte_start..a.byte_start + a.byte_length],
                )
                .unwrap();
                assert_eq!(decoded.rgba.len(), bytes.len());
                if let Some(index) = decoded.rgba.iter().zip(bytes).position(|(a, b)| a != b) {
                    panic!(
                        "ceramic {ci} {id} byte {index}: {} != {}",
                        decoded.rgba[index], bytes[index]
                    );
                }
            }
            assert_eq!(scene.manifest.colliders.len(), 1);
        }
    }
}
#[test]
fn ceramic_fields_and_baked_pixels_match_reference() {
    check_reference_tiles();
}
#[test]
fn ceramic_finish_adds_no_physical_relief() {
    let scene = scene_fixture::scene();
    assert_eq!(scene.manifest.colliders.len(), 1);
    assert_eq!(scene.manifest.assets.len(), 9);
    assert_eq!(
        scene.manifest.meshes.iter().filter(|m| m.visible).count(),
        3
    );
}
#[test]
fn ceramic_admission_survey_and_failed_bake_keep_physical_geometry_intact() {
    let mut recipe = fixture().cases.remove(7).recipe;
    for error in 0..6 {
        let original = recipe.clone();
        match error {
            0 => recipe.tiles.size = Some(0.),
            1 => recipe.tiles.grime = f64::NAN,
            2 => recipe.materials.bed_repeat = 0.,
            3 => recipe.density = 0.,
            4 => recipe.materials.grime = "x".repeat(4097),
            _ => recipe.bounds[2] = f64::INFINITY,
        }
        let mut mesh = Mesh::new(Mode::Render, 100, &|| false).unwrap();
        assert!(draw(&mut mesh, &recipe).is_err());
        assert_eq!(mesh.triangle_count(), 0);
        recipe = original;
    }
    let field = Tile::new(&recipe.tiles).unwrap();
    for p in [
        [f64::NAN, 0., 0.1],
        [0., f64::INFINITY, 0.1],
        [0., 0., 0.],
        [0., 0., f64::INFINITY],
    ] {
        assert!(field.channels(p[0], p[1], p[2]).is_err());
    }
    let mut survey = Mesh::new(Mode::Survey, 100, &|| false).unwrap();
    recipe.tiles.grime = f64::NAN;
    draw(&mut survey, &recipe).unwrap();
    assert_eq!(survey.triangle_count(), 2);
    assert!(survey.vertices.is_empty());
    recipe.tiles.grime = 0.6;
    let mut mesh = Mesh::new(Mode::Render, 100, &|| false).unwrap();
    draw(&mut mesh, &recipe).unwrap();
    let before: Vec<_> = mesh.vertices.iter().map(|v| v.uv).collect();
    let plan = weathered::finish(&mut mesh, &|| false).unwrap();
    assert!(
        plan.bake(
            &Inputs {
                materials: &BTreeMap::new(),
                images: &BTreeMap::new()
            },
            "tile",
            &|| false
        )
        .is_err()
    );
    assert_eq!(
        before,
        mesh.vertices.iter().map(|v| v.uv).collect::<Vec<_>>()
    );
    // A failed detached build is discarded; the next one remains useful.
    let mut recovered = Mesh::new(Mode::Render, 100, &|| false).unwrap();
    draw(&mut recovered, &recipe).unwrap();
    assert_eq!(recovered.triangle_count(), 2);
}
