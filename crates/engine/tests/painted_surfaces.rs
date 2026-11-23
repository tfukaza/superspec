use std::{cell::Cell, collections::BTreeMap};
use superspec_aml::material::Material;
use superspec_engine::{
    materialspec::surface::{Inputs, RasterSettings, rasterize},
    worldspec::themes::kowloon::{
        fixture::Fixture,
        mesh::{Mesh, Mode},
        metal::Binding,
        painted::{self, Paint, Recipe, Settings, Surface},
        pen::Pen,
        weathered,
    },
};
use superspec_geometry::Matrix;
#[path = "support/paint_scene.rs"]
mod scene_fixture;
fn recipe() -> Recipe {
    Recipe {
        finish: Settings {
            bounds: [-0.3, 0., 0.7, 1.2],
            paint: [0.12, 0.31, 0.24],
            substrate: [0.17, 0.12, 0.08],
            seed: 37.,
            wear: 0.6,
            grime: 0.7,
            handle: Some([0.61, 0.7]),
            panels: vec![[-0.2, 0.15, 0.1, 0.6], [0.2, 0.15, 0.6, 0.6]],
        },
        binding: Binding {
            material: "paint".into(),
            repeat: 1.25,
        },
    }
}
fn materials() -> BTreeMap<String, Material> {
    BTreeMap::from([(
        "paint".into(),
        Material {
            id: "paint".into(),
            color: "#e3dfd3".into(),
            repeat: 1.25,
            roughness: 0.7,
            metalness: 0.,
            ..Material::default()
        },
    )])
}
fn surface() -> Surface {
    Surface {
        recipe: recipe(),
        group: "leaf".into(),
        density: 48.,
    }
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
#[test]
fn paint_samples_are_single_flat_layers_and_bake_relief_rasters() {
    let materials = materials();
    let inputs = Inputs {
        materials: &materials,
        images: &BTreeMap::new(),
    };
    let mut worn = recipe();
    worn.finish.wear = 0.9;
    let mut clean = recipe();
    clean.finish.wear = 0.;
    clean.finish.grime = 0.;
    clean.finish.handle = None;
    for recipe in [recipe(), worn, clean] {
        let paint = Paint::new(&recipe).unwrap();
        let [u0, v0, u1, v1] = recipe.finish.bounds;
        for i in 0..=10 {
            for j in 0..=10 {
                let u = u0 + (u1 - u0) * i as f64 / 10.;
                let v = v0 + (v1 - v0) * j as f64 / 10.;
                let sample = paint.sample(u, v).unwrap();
                assert_eq!(sample.height, 0.);
                assert!(sample.height_weight.is_none());
                assert!(sample.layer_elevation_weight.is_none());
                assert!(sample.bump.unwrap().is_finite());
                assert!((0. ..=1.).contains(&sample.roughness.unwrap()));
                assert_eq!(sample.layers.len(), 1);
                let layer = &sample.layers[0];
                assert_eq!(layer.weight, 1.);
                assert!(layer.coatings.is_empty());
                assert_eq!(layer.material, "paint");
                assert!(layer.tint.unwrap().iter().all(|c| c.is_finite()));
            }
        }
        let raster = rasterize(
            RasterSettings {
                bounds: recipe.finish.bounds,
                texels_per_meter: 48.,
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
        assert_eq!(raster.layers.len(), 1);
        assert!(raster.layers[0].normal.is_some());
    }
}

#[test]
fn painted_faces_plan_one_receiver_per_face_and_surveys_match_render() {
    for frame_name in ["identity", "reflected", "tilted"] {
        for operation in ["panel", "bevel", "box", "plain"] {
            let s = surface();
            let pen = Pen::new(frame(frame_name), "fixture").unwrap();
            let mut mesh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
            draw(
                &mut Fixture {
                    pen: pen.clone(),
                    mesh: &mut mesh,
                    repeats: &|_| 1.25,
                },
                operation,
                &s,
            )
            .unwrap();
            let triangles = mesh.triangle_count();
            let corners: Vec<_> = mesh
                .indices
                .iter()
                .map(|i| mesh.vertices[*i as usize].position)
                .collect();
            let plan = weathered::finish(&mut mesh, &|| false).unwrap();
            if operation == "plain" {
                assert!(plan.jobs().is_empty() && plan.receivers().is_empty());
            } else {
                // Only the painted face receives the finish; the rest of a box stays authored.
                assert_eq!(plan.jobs().len(), 1, "{frame_name} {operation}");
                assert_eq!(plan.receivers().len(), 1, "{frame_name} {operation}");
                let r = &plan.receivers()[0];
                assert_eq!(r.triangle_count, 2, "{frame_name} {operation}");
                assert!(r.first_triangle + r.triangle_count <= triangles);
            }
            drop(plan);
            let mut survey = Mesh::new(Mode::Survey, 10000, &|| false).unwrap();
            draw(
                &mut Fixture {
                    pen,
                    mesh: &mut survey,
                    repeats: &|_| panic!("survey material lookup"),
                },
                operation,
                &s,
            )
            .unwrap();
            assert_eq!(survey.survey, corners, "{frame_name} {operation}");
        }
    }
}
#[test]
fn admitted_coordinate_and_identity_boundaries_bake_and_reject_before_mutation() {
    let materials = materials();
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
    let materials = materials();
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
