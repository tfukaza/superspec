use serde::Deserialize;
use std::{cell::Cell, collections::BTreeMap};
use superspec_aml::material::Material;
use superspec_engine::{
    materialspec::surface::{Inputs, RasterSettings, Sample, rasterize},
    worldspec::themes::kowloon::{
        fixture::Fixture,
        mesh::{Mesh, Mode},
        painted::{self, Paint, Recipe, Surface},
        pen::Pen,
        weathered,
    },
};
use superspec_geometry::Matrix;
#[path = "support/paint_scene.rs"]
mod scene_fixture;
#[derive(Deserialize)]
struct Case {
    recipe: Recipe,
    density: f64,
    points: Vec<[f64; 2]>,
    samples: Vec<Sample>,
    width: usize,
    height: usize,
    color: Vec<u8>,
    normal: Vec<u8>,
    packed: Vec<u8>,
}
#[derive(Deserialize)]
struct Receiver {
    first: usize,
    count: usize,
}
#[derive(Deserialize)]
struct Geometry {
    frame: String,
    operation: String,
    indices: Vec<u32>,
    positions: Vec<f64>,
    normals: Vec<f64>,
    uv: Vec<f64>,
    colors: Vec<f64>,
    receivers: Vec<Receiver>,
}
#[derive(Deserialize)]
struct Golden {
    materials: Vec<Material>,
    cases: Vec<Case>,
    meshes: Vec<Geometry>,
}
fn fixture() -> Golden {
    serde_json::from_str(include_str!("../../../tests/fixtures/rust/painted.json")).unwrap()
}
fn surface() -> Surface {
    Surface {
        recipe: fixture().cases.remove(0).recipe,
        group: "leaf".into(),
        density: 48.,
    }
}
fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 2e-12, "{a} != {b}");
}
fn frame(name: &str) -> Matrix {
    match name {
        "identity" => Matrix::default(),
        "reflected" => Matrix([[-1., 0., 0.], [0., -1., 0.], [0., 0., -1.], [4., 3., 5.]]),
        _ => Matrix([[0.8, 0., -0.6], [0., 1., 0.], [0.6, 0., 0.8], [2., -1., 3.]]),
    }
}
fn draw(f: &mut Fixture<'_, '_>, operation: &str, s: &Surface) -> Result<(), String> {
    let tint = s.recipe.finish.paint;
    match operation {
        "bevel" => painted::face(
            f,
            "paint",
            &[
                [-0.2, 0.1, 0.2],
                [0.5, 0.1, 0.2],
                [0.48, 0.08, 0.22],
                [-0.18, 0.08, 0.22],
            ],
            tint,
            Some(s),
        ),
        "box" => painted::cuboid(
            f,
            "paint",
            [-0.2, 0., 0.1],
            [0.5, 0.1, 1.1],
            tint,
            "b",
            Some(s),
        ),
        _ => painted::panel(
            f,
            "paint",
            [-0.2, 0.1, 0.5, 1.1],
            0.1,
            tint,
            if operation == "plain" { None } else { Some(s) },
        ),
    }
}
pub fn check_reference_paint() {
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
    for (i, case) in golden.cases.into_iter().enumerate() {
        let paint = Paint::new(&case.recipe).unwrap();
        assert_eq!(case.points.len(), case.samples.len());
        for (point, expected) in case.points.iter().zip(&case.samples) {
            let actual = paint.sample(point[0], point[1]).unwrap();
            assert!(
                (actual.bump.unwrap() - expected.bump.unwrap()).abs() < 2e-12,
                "case {i} at {point:?}: {:?} != {:?}",
                actual.bump,
                expected.bump
            );
            close(actual.roughness.unwrap(), expected.roughness.unwrap());
            assert_eq!(actual.height, 0.);
            assert_eq!(actual.height, expected.height);
            assert_eq!(actual.height_weight, expected.height_weight);
            assert_eq!(
                actual.layer_elevation_weight,
                expected.layer_elevation_weight
            );
            assert_eq!(actual.layers.len(), 1);
            assert_eq!(actual.layers.len(), expected.layers.len());
            assert_eq!(actual.layers[0].weight, 1.);
            assert_eq!(actual.layers[0].weight, expected.layers[0].weight);
            assert!(actual.layers[0].coatings.is_empty());
            assert!(expected.layers[0].coatings.is_empty());
            assert_eq!(actual.layers[0].roughness, expected.layers[0].roughness);
            assert_eq!(actual.layers[0].material, expected.layers[0].material);
            for (a, b) in actual.layers[0]
                .tint
                .unwrap()
                .into_iter()
                .chain(actual.layers[0].uv)
                .zip(
                    expected.layers[0]
                        .tint
                        .unwrap()
                        .into_iter()
                        .chain(expected.layers[0].uv),
                )
            {
                close(a, b);
            }
        }
        let raster = rasterize(
            RasterSettings {
                bounds: case.recipe.finish.bounds,
                texels_per_meter: case.density,
                preserve: true,
                repeat: false,
                uv_coordinates: true,
                bump: true,
            },
            &|u, v| paint.sample(u, v),
            &inputs,
            16 * 1024 * 1024,
            &|| false,
        )
        .unwrap();
        assert_eq!(
            (raster.layout.width, raster.layout.height),
            (case.width, case.height)
        );
        assert_eq!(raster.layers[0].color.rgba, case.color, "paint {i} color");
        assert_eq!(
            raster.layers[0].normal.as_ref().unwrap().rgba,
            case.normal,
            "paint {i} normal"
        );
        assert_eq!(
            raster.layers[0].metallic_roughness.rgba, case.packed,
            "paint {i} data"
        );
    }
    for case in golden.meshes {
        let s = surface();
        let pen = Pen::new(frame(&case.frame), "fixture").unwrap();
        let mut mesh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
        draw(
            &mut Fixture {
                pen: pen.clone(),
                mesh: &mut mesh,
                repeats: &|_| 1.25,
            },
            &case.operation,
            &s,
        )
        .unwrap();
        assert_eq!(
            mesh.indices, case.indices,
            "{} {}",
            case.frame, case.operation
        );
        for (actual, expected) in [
            (
                mesh.vertices
                    .iter()
                    .flat_map(|v| v.position)
                    .collect::<Vec<_>>(),
                case.positions,
            ),
            (
                mesh.vertices.iter().flat_map(|v| v.normal).collect(),
                case.normals,
            ),
            (mesh.vertices.iter().flat_map(|v| v.uv).collect(), case.uv),
            (
                mesh.vertices.iter().flat_map(|v| v.color).collect(),
                case.colors,
            ),
        ] {
            assert_eq!(actual.len(), expected.len());
            for (a, b) in actual.into_iter().zip(expected) {
                close(a, b);
            }
        }
        let plan = weathered::finish(&mut mesh, &|| false).unwrap();
        assert_eq!(plan.jobs().len(), usize::from(!case.receivers.is_empty()));
        assert_eq!(plan.receivers().len(), case.receivers.len());
        for (a, b) in plan.receivers().iter().zip(&case.receivers) {
            assert_eq!((a.first_triangle, a.triangle_count), (b.first, b.count));
        }
        drop(plan);
        let mut survey = Mesh::new(Mode::Survey, 10000, &|| false).unwrap();
        draw(
            &mut Fixture {
                pen,
                mesh: &mut survey,
                repeats: &|_| panic!("survey material lookup"),
            },
            &case.operation,
            &s,
        )
        .unwrap();
        assert_eq!(survey.survey.len(), mesh.indices.len());
        for (a, i) in survey.survey.iter().zip(&mesh.indices) {
            assert_eq!(*a, mesh.vertices[*i as usize].position);
        }
    }
}
#[test]
fn admitted_coordinate_and_identity_boundaries_bake_and_reject_before_mutation() {
    let materials = fixture()
        .materials
        .into_iter()
        .map(|m| (m.id.clone(), m))
        .collect();
    let inputs = Inputs {
        materials: &materials,
        images: &BTreeMap::new(),
    };
    for (bounds, density) in [
        ([9999., 0., 10000., 1.], 128.),
        ([-10000., -10000., 10000., 10000.], 0.00001),
    ] {
        let mut s = surface();
        s.recipe.finish.bounds = bounds;
        s.density = density;
        let paint = Paint::new(&s.recipe).unwrap();
        rasterize(
            RasterSettings {
                bounds,
                texels_per_meter: density,
                preserve: true,
                repeat: false,
                uv_coordinates: true,
                bump: true,
            },
            &|u, v| paint.sample(u, v),
            &inputs,
            16 * 1024 * 1024,
            &|| false,
        )
        .unwrap();
    }
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
    let pen = Pen::new(Matrix::default(), "paint").unwrap();
    let mut s = surface();
    s.group = "x".repeat(4090); // plus /paint is exactly 4096 bytes.
    draw(
        &mut Fixture {
            pen: pen.clone(),
            mesh: &mut mesh,
            repeats: &|_| 1.,
        },
        "box",
        &s,
    )
    .unwrap();
    let before = format!(
        "{:?}{:?}{:?}{:?}",
        mesh.vertices, mesh.indices, mesh.surfaces, mesh.triangle_surfaces
    );
    for length in [4091, 4096] {
        s.group = "x".repeat(length);
        assert!(
            draw(
                &mut Fixture {
                    pen: pen.clone(),
                    mesh: &mut mesh,
                    repeats: &|_| 1.
                },
                "box",
                &s
            )
            .is_err()
        );
        assert_eq!(
            before,
            format!(
                "{:?}{:?}{:?}{:?}",
                mesh.vertices, mesh.indices, mesh.surfaces, mesh.triangle_surfaces
            )
        );
    }
    let plan = weathered::finish(&mut mesh, &|| false).unwrap();
    assert_eq!(plan.jobs().len(), 1);
    plan.bake(&inputs, "boundaries", &|| false).unwrap();
}
#[test]
fn paint_fields_rasters_and_receiver_geometry_match_reference() {
    check_reference_paint();
}
#[test]
fn mixed_recipes_share_one_runner_and_preserve_geometry_and_collision() {
    let serial = scene_fixture::scene(false);
    let parallel = scene_fixture::scene(true);
    assert_eq!(serial.packet(), parallel.packet());
    assert_eq!(serial.manifest.colliders.len(), 1);
    assert!(!serial.manifest.assets.is_empty());
    assert!(serial.manifest.meshes.iter().any(|m| {
        m.material
            .as_deref()
            .is_some_and(|m| m.starts_with("$surface/raster/painted/"))
    }));
}

#[test]
fn invalid_paint_is_rejected_before_emission_and_survey_skips_appearance() {
    let pen = Pen::new(Matrix::default(), "paint").unwrap();
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
    for i in 0..8 {
        let mut s = surface();
        match i {
            0 => s.recipe.finish.paint[0] = f64::NAN,
            1 => s.recipe.finish.wear = -0.1,
            2 => s.recipe.finish.panels = vec![[0., 0., 1., 1.]; 257],
            3 => s.recipe.finish.handle = Some([f64::INFINITY, 0.]),
            4 => s.recipe.binding.repeat = f64::NAN,
            5 => s.recipe.finish.bounds[2] = s.recipe.finish.bounds[0],
            6 => s.group = "x".repeat(4097),
            _ => s.density = f64::INFINITY,
        }
        assert!(
            draw(
                &mut Fixture {
                    pen: pen.clone(),
                    mesh: &mut mesh,
                    repeats: &|_| 1.
                },
                "box",
                &s
            )
            .is_err()
        );
        assert_eq!(mesh.triangle_count(), 0);
        let mut survey = Mesh::new(Mode::Survey, 1000, &|| false).unwrap();
        draw(
            &mut Fixture {
                pen: pen.clone(),
                mesh: &mut survey,
                repeats: &|_| panic!("survey lookup"),
            },
            "box",
            &s,
        )
        .unwrap();
        assert!(survey.triangle_count() > 0);
    }
    draw(
        &mut Fixture {
            pen,
            mesh: &mut mesh,
            repeats: &|_| 1.,
        },
        "box",
        &surface(),
    )
    .unwrap();
    assert!(mesh.triangle_count() > 0);
}

#[test]
fn shared_paint_jobs_keep_recipe_identity_and_bind_only_after_success() {
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
    let pen = Pen::new(Matrix::default(), "paint").unwrap();
    let s = surface();
    for x in [0., 2.] {
        draw(
            &mut Fixture {
                pen: pen.moved([x, 0., 0.]).unwrap(),
                mesh: &mut mesh,
                repeats: &|_| 1.,
            },
            "panel",
            &s,
        )
        .unwrap();
    }
    let mut altered = surface();
    altered.recipe.finish.seed += 1.;
    draw(
        &mut Fixture {
            pen,
            mesh: &mut mesh,
            repeats: &|_| 1.,
        },
        "bevel",
        &altered,
    )
    .unwrap();
    let before = format!(
        "{:?}{:?}{:?}{:?}",
        mesh.vertices, mesh.indices, mesh.surfaces, mesh.triangle_surfaces
    );
    let plan = weathered::finish(&mut mesh, &|| false).unwrap();
    assert_eq!(plan.jobs().len(), 2);
    assert_eq!(
        plan.receivers().iter().map(|r| r.job).collect::<Vec<_>>(),
        [0, 0, 1]
    );
    let materials = fixture()
        .materials
        .into_iter()
        .map(|m| (m.id.clone(), m))
        .collect();
    let inputs = Inputs {
        materials: &materials,
        images: &BTreeMap::new(),
    };
    let calls = Cell::new(0);
    assert!(
        plan.bake(&inputs, "paint", &|| {
            calls.set(calls.get() + 1);
            calls.get() > 10
        })
        .is_err()
    );
    assert!(calls.get() > 10);
    assert_eq!(
        before,
        format!(
            "{:?}{:?}{:?}{:?}",
            mesh.vertices, mesh.indices, mesh.surfaces, mesh.triangle_surfaces
        )
    );
}
