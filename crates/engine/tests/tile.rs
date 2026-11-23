use serde_json::{Value, json};
use std::collections::BTreeMap;
use superspec_aml::material::Material;
use superspec_engine::{
    materialspec::surface::Inputs,
    worldspec::themes::kowloon::{
        fixture::Fixture,
        mesh::{Mesh, Mode},
        pen::Pen,
        tile::{self, Recipe, Tile},
        weathered,
    },
};
use superspec_geometry::Matrix;
#[path = "support/tile_scene.rs"]
mod scene_fixture;
fn recipe() -> Recipe {
    serde_json::from_value(json!({
        "bounds": [-0.4, -0.3, 0.4, 0.6],
        "tiles": {"seed": 22, "wear": 0.7, "share": 1, "grime": 0.6, "size": 0.18, "aspect": 1},
        "materials": {"ceramic": "ceramic", "bed": "bed", "grime": "grime", "repeat": 1.3,
            "bedRepeat": 2.4, "bedRoughness": 0.9, "roughness": 0.65},
        "bedTint": [0.9, 0.8, 0.7],
        "density": 256
    }))
    .unwrap()
}
fn materials() -> BTreeMap<String, Material> {
    serde_json::from_value::<Vec<Material>>(json!([
        {"id": "ceramic", "color": "#b4c4a2", "roughness": 0.65, "metalness": 0},
        {"id": "bed", "color": "#737373", "roughness": 0.9, "metalness": 0},
        {"id": "grime", "color": "#725139", "roughness": 1, "metalness": 0}
    ]))
    .unwrap()
    .into_iter()
    .map(|m| (m.id.clone(), m))
    .collect()
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
#[test]
fn ceramic_samples_do_not_depend_on_visit_order() {
    for tiles in [
        json!({"seed": 0, "wear": 0.7, "share": 1, "grime": 0.6}),
        json!({"seed": 4294967295u32, "wear": 0.7, "share": 1, "grime": 0.6}),
        json!({"seed": 14, "wear": 0.7, "share": 0.46, "grime": 0.6}),
        json!({"seed": 19, "wear": 0, "share": 1, "grime": 0}),
        json!({"seed": 22, "wear": 0.7, "share": 1, "grime": 0.6, "size": 0.18, "aspect": 1}),
        json!({"seed": 32, "wear": 0.7, "share": 1, "grime": 0.6, "size": 0.09, "aspect": 2}),
    ] {
        let mut recipe = recipe();
        recipe.tiles = serde_json::from_value(tiles).unwrap();
        let field = Tile::new(&recipe.tiles).unwrap();
        let points: Vec<[f64; 3]> = (0..120)
            .map(|i| {
                [
                    -1.379 + (i % 12) as f64 * 0.197,
                    -1.384 + (i / 12) as f64 * 0.311,
                    [0.0009765625, 0.00390625, 0.015625][i % 3],
                ]
            })
            .collect();
        let forward: Vec<Value> = points
            .iter()
            .map(|&[u, v, f]| {
                json!([
                    field.channels(u, v, f).unwrap(),
                    field
                        .surface(u, v, f, &recipe.materials, recipe.bed_tint)
                        .unwrap()
                ])
            })
            .collect();
        // Revisit points in reverse to cross neighborhood cache boundaries without changing samples.
        let fresh = Tile::new(&recipe.tiles).unwrap();
        for (&[u, v, f], expected) in points.iter().zip(&forward).rev() {
            assert_eq!(
                json!([
                    fresh.channels(u, v, f).unwrap(),
                    fresh
                        .surface(u, v, f, &recipe.materials, recipe.bed_tint)
                        .unwrap()
                ]),
                *expected
            );
        }
    }
}
#[test]
fn baked_ceramic_keeps_one_flat_quad_and_maps_uv_from_position() {
    let recipe = recipe();
    let materials = materials();
    let inputs = Inputs {
        materials: &materials,
        images: &BTreeMap::new(),
    };
    let mut mesh = Mesh::new(Mode::Render, 100, &|| false).unwrap();
    draw(&mut mesh, &recipe).unwrap();
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
    for id in [
        material.texture.unwrap(),
        material.normal_texture.unwrap(),
        material.metallic_roughness_texture.unwrap(),
    ] {
        let a = scene.manifest.assets.iter().find(|a| a.id == id).unwrap();
        let decoded = superspec_geometry::image::decode(
            &scene.asset_bytes[a.byte_start..a.byte_start + a.byte_length],
        )
        .unwrap();
        assert_eq!(
            (decoded.width, decoded.height),
            (bakes[0].layout().width, bakes[0].layout().height)
        );
    }
    assert_eq!(scene.manifest.colliders.len(), 1);
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
    let mut recipe = recipe();
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
