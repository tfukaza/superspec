use serde_json::{Value, json};
use std::collections::BTreeMap;
use superspec_aml::material::Material;
use superspec_engine::{
    materialspec::surface::{Inputs, RasterSettings, rasterize},
    worldspec::themes::kowloon::{
        facade_material::{self, Facade, PERIOD, Settings},
        fixture::Fixture,
        mesh::{Mesh, Mode},
        pen::Pen,
        weathered,
    },
};
use superspec_geometry::Matrix;
#[path = "support/facade_scene.rs"]
mod scene_fixture;
fn settings() -> Settings {
    serde_json::from_value(json!({
        "concrete": false,
        "coat": {"material": "coat", "repeat": 1.3, "tint": [0.7, 0.9, 0.8]},
        "backing": {"material": "backing", "repeat": 2.7},
        "wear": 0.65,
        "density": 32,
        "variant": 0
    }))
    .unwrap()
}
fn materials() -> BTreeMap<String, Material> {
    serde_json::from_value::<Vec<Material>>(json!([
        {"id": "coat", "color": "#caaf92", "repeat": 1.3, "roughness": 0.8, "metalness": 0},
        {"id": "backing", "color": "#807a70", "repeat": 2.7, "roughness": 0.95, "metalness": 0.05}
    ]))
    .unwrap()
    .into_iter()
    .map(|m| (m.id.clone(), m))
    .collect()
}
fn close(a: &Value, b: &Value) {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => assert!(
            (a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 2e-11,
            "{a} != {b}"
        ),
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len());
            for (a, b) in a.iter().zip(b) {
                close(a, b);
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.len(), b.len());
            for (k, a) in a {
                close(a, &b[k]);
            }
        }
        _ => assert_eq!(a, b),
    }
}
#[test]
fn facade_fields_repeat_with_the_period_and_stay_flat() {
    for (concrete, variant, wear) in [(false, 0, 0.65), (false, 2, 0.), (true, 1, 0.65)] {
        let mut s = settings();
        s.concrete = concrete;
        s.variant = variant;
        s.wear = wear;
        let field = Facade::new(&s).unwrap();
        for [u, v] in [
            [0., 0.],
            [0., 1.7],
            [PERIOD, PERIOD],
            [0.17, 2.8],
            [-1.2, 5.4],
        ] {
            close(
                &serde_json::to_value(field.finish(u, v).unwrap()).unwrap(),
                &serde_json::to_value(field.finish(u + PERIOD, v - PERIOD).unwrap()).unwrap(),
            );
            assert_eq!(field.sample(u, v).unwrap().height, 0.);
        }
    }
}
#[test]
fn baked_facade_keeps_geometry_and_maps_world_position_to_periodic_uv() {
    let s = settings();
    let materials = materials();
    let images = BTreeMap::new();
    let inputs = Inputs {
        materials: &materials,
        images: &images,
    };
    let field = Facade::new(&s).unwrap();
    let raster = rasterize(
        RasterSettings {
            bounds: [0., 0., PERIOD, PERIOD],
            texels_per_meter: s.density,
            preserve: true,
            repeat: true,
            uv_coordinates: false,
            bump: true,
        },
        &|u, v| field.sample(u, v),
        &inputs,
        32 * 1024 * 1024,
        &|| false,
    )
    .unwrap();
    let mut mesh = Mesh::new(Mode::Render, 100, &|| false).unwrap();
    let pen = Pen::new(Matrix::default(), "wall").unwrap();
    facade_material::panel(
        &mut Fixture {
            pen,
            mesh: &mut mesh,
            repeats: &|_| 1.,
        },
        "wall",
        [-1., 0., PERIOD + 1., PERIOD],
        0.,
        &s,
        "facade",
    )
    .unwrap();
    let before: Vec<_> = mesh
        .vertices
        .iter()
        .map(|v| (v.position, v.normal, v.color))
        .collect();
    let bakes = weathered::finish(&mut mesh, &|| false)
        .unwrap()
        .bake(&inputs, "facade", &|| false)
        .unwrap();
    assert_eq!(bakes.len(), 1);
    assert_eq!(bakes[0].layout().uv([0., 0.]), [0., 0.]);
    assert_eq!(bakes[0].layout().uv([PERIOD, PERIOD]), [1., 1.]);
    assert_eq!(
        before,
        mesh.vertices
            .iter()
            .map(|v| (v.position, v.normal, v.color))
            .collect::<Vec<_>>()
    );
    assert!(mesh.vertices.iter().any(|v| v.uv[0] < 0.));
    assert!(mesh.vertices.iter().any(|v| v.uv[0] > 1.));
    for vertex in &mesh.vertices {
        assert!((vertex.uv[0] - vertex.position[0] / PERIOD).abs() < 1e-12);
        assert!((vertex.uv[1] - vertex.position[2] / PERIOD).abs() < 1e-12);
    }
    let mut scene=superspec_engine::compile(r#"<architecture version="1" id="facade"><box id="host" width="8" depth=".1" height="5" visible="false"/></architecture>"#,0).unwrap();
    let material = bakes[0].material().clone();
    scene
        .append_baked_visual(
            "host",
            &mesh,
            &BTreeMap::from([(material.id.clone(), material.id.clone())]),
            &bakes,
        )
        .unwrap();
    // The delivered textures are the standalone raster of the same periodic field.
    for (id, bytes) in [
        (material.texture.unwrap(), &raster.layers[0].color.rgba),
        (
            material.normal_texture.unwrap(),
            &raster.layers[0].normal.as_ref().unwrap().rgba,
        ),
        (
            material.metallic_roughness_texture.unwrap(),
            &raster.layers[0].metallic_roughness.rgba,
        ),
    ] {
        let a = scene.manifest.assets.iter().find(|a| a.id == id).unwrap();
        let image = superspec_geometry::image::decode(
            &scene.asset_bytes[a.byte_start..a.byte_start + a.byte_length],
        )
        .unwrap();
        assert_eq!(&image.rgba, bytes, "facade {id}");
    }
}
#[test]
fn adjacent_receivers_share_one_material_and_keep_host_collision_separate() {
    for concrete in [false, true] {
        let scene = scene_fixture::scene(concrete);
        assert_eq!(scene.manifest.assets.len(), 3);
        assert_eq!(scene.manifest.colliders.len(), 1);
        let walls: Vec<_> = scene.manifest.meshes.iter().filter(|m| m.visible).collect();
        assert_eq!(walls.len(), 2);
        assert_eq!(walls[0].material, walls[1].material);
    }
}
#[test]
fn facade_sharing_tracks_normalized_variant_and_material_inputs() {
    let mut s = settings();
    let mut mesh = Mesh::new(Mode::Render, 100, &|| false).unwrap();
    let pen = Pen::new(Matrix::default(), "wall").unwrap();
    for i in 0..5 {
        match i {
            1 => s.variant = 2,
            2 => s.coat.repeat += 0.1,
            3 => s.coat.tint[0] += 0.1,
            4 => s.wear = 0.,
            _ => {}
        }
        facade_material::panel(
            &mut Fixture {
                pen: pen.moved([i as f64 * 2., 0., 0.]).unwrap(),
                mesh: &mut mesh,
                repeats: &|_| 1.,
            },
            "wall",
            [0., 0., 1., 1.],
            0.,
            &s,
            "facade",
        )
        .unwrap();
    }
    let plan = weathered::finish(&mut mesh, &|| false).unwrap();
    assert_eq!(plan.jobs().len(), 4);
    assert_eq!(
        plan.receivers().iter().map(|r| r.job).collect::<Vec<_>>(),
        [0, 0, 1, 2, 3]
    );
}
#[test]
fn invalid_facade_inputs_fail_before_drawing_and_surveys_do_not_sample_materials() {
    let mut mesh = Mesh::new(Mode::Render, 100, &|| false).unwrap();
    let pen = Pen::new(Matrix::default(), "wall").unwrap();
    for i in 0..6 {
        let mut s = settings();
        match i {
            0 => s.wear = f64::NAN,
            1 => s.density = 0.,
            2 => s.coat.repeat = 0.,
            3 => s.backing.material = "x".repeat(4097),
            4 => s.coat.tint[0] = f64::INFINITY,
            _ => s.density = 4096.,
        };
        assert!(
            facade_material::panel(
                &mut Fixture {
                    pen: pen.clone(),
                    mesh: &mut mesh,
                    repeats: &|_| 1.
                },
                "wall",
                [0., 0., 1., 1.],
                0.,
                &s,
                "facade"
            )
            .is_err()
        );
        assert_eq!(mesh.triangle_count(), 0);
        let mut survey = Mesh::new(Mode::Survey, 100, &|| false).unwrap();
        facade_material::panel(
            &mut Fixture {
                pen: pen.clone(),
                mesh: &mut survey,
                repeats: &|_| panic!("survey lookup"),
            },
            "wall",
            [0., 0., 1., 1.],
            0.,
            &s,
            "facade",
        )
        .unwrap();
        assert_eq!(survey.triangle_count(), 2);
    }
}
