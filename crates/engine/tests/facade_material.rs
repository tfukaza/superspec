use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use superspec_aml::material::Material;
use superspec_engine::{
    materialspec::surface::{Inputs, RasterSettings, Sample, rasterize},
    worldspec::themes::kowloon::{
        facade_material::{self, Facade, Finish, PERIOD, Settings},
        fixture::Fixture,
        mesh::{Mesh, Mode},
        pen::Pen,
        weathered,
    },
};
use superspec_geometry::Matrix;
#[path = "support/facade_scene.rs"]
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
    settings: Settings,
    points: Vec<[f64; 2]>,
    finishes: Vec<Finish>,
    samples: Vec<Sample>,
    raster: Option<Pixels>,
    textured: bool,
}
#[derive(Deserialize)]
struct SourceImage {
    width: usize,
    height: usize,
    data: Vec<u8>,
}
#[derive(Deserialize)]
struct Golden {
    materials: Vec<Material>,
    #[serde(rename = "texturedMaterials")]
    textured_materials: Vec<Material>,
    images: BTreeMap<String, SourceImage>,
    cases: Vec<Case>,
}
fn fixture() -> Golden {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/facade-material.json"
    ))
    .unwrap()
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
fn bake_case(case: &Case, inputs: &Inputs<'_>) {
    let expected = case.raster.as_ref().unwrap();
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
        &case.settings,
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
        .bake(inputs, "facade", &|| false)
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
    for (id, bytes) in [
        (material.texture.unwrap(), &expected.color),
        (material.normal_texture.unwrap(), &expected.normal),
        (
            material.metallic_roughness_texture.unwrap(),
            &expected.packed,
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
pub fn check_reference_facade() {
    let golden = fixture();
    let materials = golden
        .materials
        .into_iter()
        .map(|m| (m.id.clone(), m))
        .collect();
    let textured_materials = golden
        .textured_materials
        .into_iter()
        .map(|m| (m.id.clone(), m))
        .collect();
    let images = golden
        .images
        .into_iter()
        .map(|(id, i)| {
            (
                id,
                superspec_geometry::image::Image {
                    width: i.width,
                    height: i.height,
                    rgba: i.data,
                    mime: "image/png",
                },
            )
        })
        .collect();
    for case in golden.cases {
        let inputs = Inputs {
            materials: if case.textured {
                &textured_materials
            } else {
                &materials
            },
            images: &images,
        };
        let field = Facade::new(&case.settings).unwrap();
        assert_eq!(case.points.len(), case.finishes.len());
        assert_eq!(case.points.len(), case.samples.len());
        for ((p, f), sample) in case.points.iter().zip(&case.finishes).zip(&case.samples) {
            close(
                &serde_json::to_value(field.finish(p[0], p[1]).unwrap()).unwrap(),
                &serde_json::to_value(f).unwrap(),
            );
            let actual = field.sample(p[0], p[1]).unwrap();
            assert_eq!(actual.height, 0.);
            close(
                &serde_json::to_value(actual).unwrap(),
                &serde_json::to_value(sample).unwrap(),
            );
        }
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
        }
        if let Some(expected) = &case.raster {
            let raster = rasterize(
                RasterSettings {
                    bounds: [0., 0., PERIOD, PERIOD],
                    texels_per_meter: case.settings.density,
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
            assert_eq!(
                (raster.layout.width, raster.layout.height),
                (expected.width, expected.height)
            );
            assert_eq!(raster.layers[0].color.rgba, expected.color);
            assert_eq!(
                raster.layers[0].normal.as_ref().unwrap().rgba,
                expected.normal
            );
            assert_eq!(raster.layers[0].metallic_roughness.rgba, expected.packed);
            bake_case(&case, &inputs);
        }
    }
}
#[test]
fn periodic_fields_and_delivered_raster_pixels_match_reference() {
    check_reference_facade();
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
    let mut s = fixture().cases.remove(2).settings;
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
        let mut s = fixture().cases.remove(2).settings;
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
