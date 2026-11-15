use serde::Deserialize;
use superspec_engine::{
    materialspec::surface::Sample,
    worldspec::themes::kowloon::{
        fixture::Fixture,
        mesh::{Mesh, Mode},
        metal::{self, Metal},
        pen::Pen,
        pipes::{Options, PipeSystem},
        weathered,
    },
};
use superspec_geometry::Matrix;
#[path = "support/pipe_scene.rs"]
mod scene_fixture;

#[derive(Deserialize)]
struct Receiver {
    first: usize,
    count: usize,
    settings: Option<metal::Settings>,
    points: Vec<[f64; 2]>,
    samples: Vec<Sample>,
}
#[derive(Deserialize)]
struct Case {
    frame: String,
    operation: String,
    options: Options,
    receivers: Vec<Receiver>,
    indices: Vec<u32>,
    positions: Vec<f64>,
    normals: Vec<f64>,
    uv: Vec<f64>,
    colors: Vec<f64>,
    slots: Vec<String>,
    raster: Option<Raster>,
}
#[derive(Deserialize)]
struct Raster {
    width: usize,
    height: usize,
    color: Vec<u8>,
    properties: Vec<u8>,
    normal: Vec<u8>,
}
#[derive(Deserialize)]
struct Cases {
    cases: Vec<Case>,
    #[serde(rename = "rasterMaterials")]
    raster_materials: Vec<superspec_aml::material::Material>,
}
fn matrix(frame: &str) -> Matrix {
    match frame {
        "identity" => Matrix::default(),
        "reflection" => Matrix([[-1., 0., 0.], [0., -1., 0.], [0., 0., -1.], [4., 3., 5.]]),
        "tilted" => Matrix([[1., 0., 0.], [0., 0.8, 0.6], [0., -0.6, 0.8], [2., 3., 1.]]),
        _ => panic!("unknown frame"),
    }
}
fn repeat(slot: &str) -> f64 {
    if slot == "metal" { 1.5 } else { 1.25 }
}
fn draw(drawing: &mut Fixture<'_, '_>, case: &Case) -> Result<(), String> {
    let pipe = PipeSystem::new(case.options.clone())?;
    match case.operation.as_str() {
        "vertical" => pipe.straight(drawing, [0.2, 0.3, 0.1], [0.2, 0.3, 2.1], &[0.4, 1.4]),
        "diagonal" => pipe.straight(drawing, [0.2, 0.3, 0.1], [1.2, 1.3, 2.1], &[0.4, 1.4]),
        "elbow" => pipe.bend(
            drawing,
            [0., 0.3, 0.],
            [0., 0.3, 0.3],
            [0.3, 0.3, 0.3],
            true,
        ),
        "coupling" => pipe.coupling(drawing, [0.2, 0.3, 1.], [0., 0., 1.]),
        "mount" => pipe.mount(drawing, [0.2, 0.3, 1.], [0., 0., 1.], [0.2, 0., 1.]),
        "run" => pipe.run(
            drawing,
            &[
                [0.2, 0.3, 0.1],
                [0.2, 0.3, 0.1],
                [0.2, 0.3, 3.2],
                [1.8, 0.3, 3.2],
            ],
            Some(0.),
        ),
        _ => panic!("unknown operation"),
    }
}
fn close_json(actual: &serde_json::Value, expected: &serde_json::Value, context: &str) {
    match (actual, expected) {
        (serde_json::Value::Number(a), serde_json::Value::Number(b)) => assert!(
            (a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 2e-10,
            "{context}: {a} != {b}"
        ),
        (serde_json::Value::Array(a), serde_json::Value::Array(b)) => {
            assert_eq!(a.len(), b.len(), "{context}");
            for (a, b) in a.iter().zip(b) {
                close_json(a, b, context);
            }
        }
        (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
            assert_eq!(a.len(), b.len(), "{context}");
            for (k, b) in b {
                close_json(&a[k], b, &format!("{context}/{k}"));
            }
        }
        (a, b) => assert_eq!(a, b, "{context}"),
    }
}
/// Same indexed geometry, chart recipes and sampled PBR assertions run natively and in browser Wasm.
pub fn check_reference_pipes() {
    let cases: Cases =
        serde_json::from_str(include_str!("../../../tests/fixtures/rust/pipes.json")).unwrap();
    let materials = cases
        .raster_materials
        .into_iter()
        .map(|m| (m.id.clone(), m))
        .collect();
    let images = std::collections::BTreeMap::new();
    let inputs = superspec_engine::materialspec::surface::Inputs {
        materials: &materials,
        images: &images,
    };
    for case in cases.cases {
        let label = format!(
            "{}/{}/{}",
            case.frame,
            case.operation,
            case.options.weathering.is_some()
        );
        let pen = Pen::new(matrix(&case.frame), "fixture").unwrap();
        let mut mesh = Mesh::new(Mode::Render, 20000, &|| false).unwrap();
        draw(
            &mut Fixture {
                pen: pen.clone(),
                mesh: &mut mesh,
                repeats: &repeat,
            },
            &case,
        )
        .unwrap();
        assert_eq!(mesh.indices, case.indices, "{label} indices");
        for (name, actual, expected) in [
            (
                "positions",
                mesh.vertices
                    .iter()
                    .flat_map(|v| v.position)
                    .collect::<Vec<_>>(),
                &case.positions,
            ),
            (
                "normals",
                mesh.vertices.iter().flat_map(|v| v.normal).collect(),
                &case.normals,
            ),
            (
                "uv",
                mesh.vertices.iter().flat_map(|v| v.uv).collect(),
                &case.uv,
            ),
            (
                "colors",
                mesh.vertices.iter().flat_map(|v| v.color).collect(),
                &case.colors,
            ),
        ] {
            assert_eq!(actual.len(), expected.len(), "{label}/{name} count");
            for (i, (a, b)) in actual.iter().zip(expected).enumerate() {
                assert!((a - b).abs() < 2e-10, "{label}/{name}/{i}: {a} != {b}");
            }
        }
        assert_eq!(
            mesh.triangle_surfaces
                .iter()
                .map(|s| mesh.surfaces[*s].slot.clone())
                .collect::<Vec<_>>(),
            case.slots,
            "{label} slots"
        );
        let plan = weathered::finish(&mut mesh, &|| false).unwrap();
        assert_eq!(
            plan.receivers().len(),
            case.receivers.len(),
            "{label} receiver count"
        );
        for (range, expected) in plan.receivers().iter().zip(&case.receivers) {
            assert_eq!(
                (range.first_triangle, range.triangle_count),
                (expected.first, expected.count),
                "{label}"
            );
            if let Some(settings) = &expected.settings {
                let job = &plan.jobs()[range.job];
                close_json(
                    &serde_json::to_value(job.metal_settings().unwrap()).unwrap(),
                    &serde_json::to_value(settings).unwrap(),
                    &label,
                );
                let field = job.runoff_recipe().map(|r| r.build(&|| false).unwrap());
                let runoff = |u, v| field.as_ref().unwrap().sample(u, v);
                let metal = Metal::new(
                    job.metal_settings().unwrap(),
                    field
                        .as_ref()
                        .map(|_| &runoff as &dyn Fn(f64, f64) -> Result<f64, String>),
                )
                .unwrap();
                assert_eq!(expected.points.len(), expected.samples.len());
                for (uv, sample) in expected.points.iter().zip(&expected.samples) {
                    close_json(
                        &serde_json::to_value(metal.sample(uv[0], uv[1]).unwrap()).unwrap(),
                        &serde_json::to_value(sample).unwrap(),
                        &label,
                    );
                }
            }
        }
        if let Some(raster) = &case.raster {
            let bakes = plan.bake(&inputs, "golden-pipe", &|| false).unwrap();
            assert_eq!(bakes.len(), 1);
            assert_eq!(
                (bakes[0].layout().width, bakes[0].layout().height),
                (raster.width, raster.height)
            );
            let material = bakes[0].material();
            let bindings = bakes
                .iter()
                .map(|b| (b.material().id.clone(), b.material().id.clone()))
                .collect();
            let mut scene=superspec_engine::compile(r#"<architecture version="1" id="pipe-raster"><box id="host" width="1" depth="1" height="1"/></architecture>"#,0).unwrap();
            scene
                .append_baked_visual("host", &mesh, &bindings, &bakes)
                .unwrap();
            for (id, expected) in [
                (material.texture.as_ref().unwrap(), &raster.color),
                (material.normal_texture.as_ref().unwrap(), &raster.normal),
                (
                    material.metallic_roughness_texture.as_ref().unwrap(),
                    &raster.properties,
                ),
            ] {
                let asset = scene.manifest.assets.iter().find(|a| a.id == *id).unwrap();
                let image = superspec_geometry::image::decode(
                    &scene.asset_bytes[asset.byte_start..asset.byte_start + asset.byte_length],
                )
                .unwrap();
                assert_eq!(&image.rgba, expected, "{label} {id} baked pixels");
            }
        } else {
            drop(plan);
        }
        let mut survey = Mesh::new(Mode::Survey, 20000, &|| false).unwrap();
        draw(
            &mut Fixture {
                pen,
                mesh: &mut survey,
                repeats: &|_| panic!("survey resolved appearance"),
            },
            &case,
        )
        .unwrap();
        assert_eq!(survey.survey.len(), mesh.indices.len(), "{label} survey");
        for (p, i) in survey.survey.iter().zip(&mesh.indices) {
            assert_eq!(
                *p, mesh.vertices[*i as usize].position,
                "{label} survey position"
            );
        }
    }
}

#[derive(Debug, PartialEq)]
struct Snapshot {
    vertices: Vec<Vec<u64>>,
    indices: Vec<u32>,
    surfaces: Vec<(String, String)>,
    triangle_surfaces: Vec<usize>,
    survey: Vec<Vec<u64>>,
}
fn snapshot(mesh: &Mesh<'_>) -> Snapshot {
    Snapshot {
        vertices: mesh
            .vertices
            .iter()
            .map(|v| {
                v.position
                    .into_iter()
                    .chain(v.normal)
                    .chain(v.uv)
                    .chain(v.color)
                    .map(f64::to_bits)
                    .collect()
            })
            .collect(),
        indices: mesh.indices.clone(),
        surfaces: mesh
            .surfaces
            .iter()
            .map(|s| (s.part.clone(), s.slot.clone()))
            .collect(),
        triangle_surfaces: mesh.triangle_surfaces.clone(),
        survey: mesh
            .survey
            .iter()
            .map(|p| p.iter().map(|v| v.to_bits()).collect())
            .collect(),
    }
}
#[test]
fn pipes_match_frozen_reference() {
    check_reference_pipes();
}

#[test]
fn pipe_preview_has_baked_maps_and_keeps_collision_separate() {
    let scene = scene_fixture::scene();
    assert!(scene.manifest.assets.len() > 9);
    assert!(scene.manifest.meshes.iter().any(|m| {
        m.visible
            && m.vertex_count > 0
            && m.material
                .as_deref()
                .is_some_and(|s| s.starts_with("$surface/raster/pipes/"))
    }));
    assert!(
        scene
            .manifest
            .colliders
            .iter()
            .all(|id| !id.contains("dressing"))
    );
    assert_eq!(scene.manifest.colliders.len(), 1);
}

#[test]
fn pipe_sharing_tracks_orientation_seams_and_variant_but_not_translation() {
    let mut mesh = Mesh::new(Mode::Render, 20000, &|| false).unwrap();
    let base = Pen::new(Matrix::default(), "pipes").unwrap();
    let mut expected = Vec::new();
    for i in 0..5 {
        let mut options = scene_fixture::options();
        if i == 4 {
            options.variant = "other".into();
        }
        let pen = if i == 2 {
            Pen::new(matrix("reflection"), "pipes").unwrap()
        } else {
            base.moved([i as f64, 0., 0.]).unwrap()
        };
        expected.push(mesh.triangle_count());
        PipeSystem::new(options)
            .unwrap()
            .straight(
                &mut Fixture {
                    pen,
                    mesh: &mut mesh,
                    repeats: &|_| 1.,
                },
                [0., 0., 0.],
                [0., 0., 1.],
                if i == 3 { &[0.6] } else { &[0.4] },
            )
            .unwrap();
    }
    let plan = weathered::finish(&mut mesh, &|| false).unwrap();
    assert_eq!(plan.receivers().len(), 5);
    let jobs: Vec<_> = plan.receivers().iter().map(|r| r.job).collect();
    assert_eq!(jobs, vec![0, 0, 1, 2, 3]);
    assert_eq!(
        plan.receivers()
            .iter()
            .map(|r| r.first_triangle)
            .collect::<Vec<_>>(),
        expected
    );
}

#[test]
fn explicit_receiver_releases_unaccounted_spare_capacity() {
    use superspec_engine::worldspec::themes::kowloon::pipe_runoff::{Recipe, Ring};
    let cases: Cases =
        serde_json::from_str(include_str!("../../../tests/fixtures/rust/pipes.json")).unwrap();
    let mut settings = cases
        .cases
        .into_iter()
        .flat_map(|c| c.receivers)
        .find_map(|r| r.settings)
        .unwrap();
    settings.contacts = Some(Vec::with_capacity(1_000_000));
    settings.folds.reserve(10000);
    settings.substrate.material.reserve(1_000_000);
    let mut key = String::with_capacity(1_000_000);
    key.push_str("pipe");
    let mut rings = Vec::with_capacity(4096);
    for distance in [0., 2.] {
        rings.push(Ring {
            center: [0., 0., distance],
            radial: [0., 1., 0.],
            across: [-1., 0., 0.],
            radius: 0.07,
            distance,
        });
    }
    let mut seams = Vec::with_capacity(4096);
    seams.push(0.4);
    let recipe = Recipe {
        rings,
        seams,
        circumference: std::f64::consts::PI * 0.14,
        seed: 1.,
    };
    let mut options = scene_fixture::options();
    options.weathering = None;
    let mut mesh = Mesh::new(Mode::Render, 1000, &|| false).unwrap();
    PipeSystem::new(options)
        .unwrap()
        .straight(
            &mut Fixture {
                pen: Pen::new(Matrix::default(), "pipe").unwrap(),
                mesh: &mut mesh,
                repeats: &|_| 1.,
            },
            [0.; 3],
            [0., 0., 2.],
            &[],
        )
        .unwrap();
    let count = mesh.triangle_count();
    weathered::receive(&mut mesh, 0, count, settings, 128., key, Some(recipe)).unwrap();
    let plan = weathered::finish(&mut mesh, &|| false).unwrap();
    let job = &plan.jobs()[0];
    assert_eq!(
        job.metal_settings()
            .unwrap()
            .contacts
            .as_ref()
            .unwrap()
            .capacity(),
        0
    );
    assert_eq!(
        job.metal_settings().unwrap().folds.capacity(),
        job.metal_settings().unwrap().folds.len()
    );
    assert_eq!(
        job.metal_settings().unwrap().substrate.material.capacity(),
        job.metal_settings().unwrap().substrate.material.len()
    );
    let recipe = job.runoff_recipe().unwrap();
    assert_eq!(recipe.rings.capacity(), recipe.rings.len());
    assert_eq!(recipe.seams.capacity(), recipe.seams.len());
}

#[test]
fn invalid_routes_emit_nothing_and_cancelled_detached_drawings_never_bind() {
    use std::cell::Cell;
    let pipe = PipeSystem::new(scene_fixture::options()).unwrap();
    for path in [
        vec![[0.; 3]],
        vec![[0.; 3], [0.; 3]],
        vec![[0.; 3], [0., 0., 1.], [0.; 3]],
        vec![[0.; 3], [0., 0., 0.01], [0.01, 0., 0.01]],
        vec![[0.; 3], [0., 0., f64::NAN]],
        vec![[0.; 3]; 129],
        vec![[0.; 3], [0., 0., 1001.]],
    ] {
        let mut mesh = Mesh::new(Mode::Render, 20000, &|| false).unwrap();
        assert!(
            pipe.run(
                &mut Fixture {
                    pen: Pen::new(Matrix::default(), "pipe").unwrap(),
                    mesh: &mut mesh,
                    repeats: &|_| 1.
                },
                &path,
                None
            )
            .is_err()
        );
        assert_eq!(mesh.triangle_count(), 0);
    }
    for invalid in [f64::NAN, 0., -1., 11.] {
        let mut options = scene_fixture::options();
        options.radius = invalid;
        assert!(PipeSystem::new(options).is_err());
    }
    let calls = Cell::new(0);
    let cancelled = || {
        calls.set(calls.get() + 1);
        calls.get() > 20
    };
    let mut mesh = Mesh::new(Mode::Render, 20000, &cancelled).unwrap();
    assert!(
        pipe.run(
            &mut Fixture {
                pen: Pen::new(Matrix::default(), "pipe").unwrap(),
                mesh: &mut mesh,
                repeats: &|_| 1.
            },
            &[[0.; 3], [0., 0., 3.]],
            None
        )
        .unwrap_err()
        .contains("cancel")
    );
    assert!(
        mesh.surfaces
            .iter()
            .all(|s| !s.slot.starts_with("$surface/"))
    );
}

#[test]
fn runoff_baking_is_atomic_and_parallel_output_matches_serial() {
    use std::{cell::Cell, collections::BTreeMap};
    let materials = scene_fixture::materials();
    let inputs = superspec_engine::materialspec::surface::Inputs {
        materials: &materials,
        images: &BTreeMap::new(),
    };
    let mut packets = Vec::new();
    for mode in [0, 1, 2] {
        let mut mesh = Mesh::new(Mode::Render, 20000, &|| false).unwrap();
        let pipe = PipeSystem::new(scene_fixture::options()).unwrap();
        // Inherited appearance must not collect a second overlapping pipe chart.
        let base = Pen::new(Matrix::default(), "pipes")
            .unwrap()
            .with_weathering(
                &mut mesh,
                scene_fixture::options().weathering.unwrap(),
                metal::Kind::Frame,
                None,
                vec!["metal".into()],
            )
            .unwrap();
        for (i, seams) in [&[0.2, 0.4][..], &[0.25][..]].into_iter().enumerate() {
            pipe.straight(
                &mut Fixture {
                    pen: base.moved([i as f64, 0., 0.]).unwrap(),
                    mesh: &mut mesh,
                    repeats: &|_| 1.,
                },
                [0.; 3],
                [0., 0., 0.6],
                seams,
            )
            .unwrap();
        }
        let positions: Vec<_> = mesh
            .vertices
            .iter()
            .map(|v| (v.position, v.normal))
            .collect();
        let original = snapshot(&mesh);
        let plan = weathered::finish(&mut mesh, &|| false).unwrap();
        assert_eq!(plan.receivers().len(), 2);
        if mode == 0 {
            let calls = Cell::new(0);
            let cancel = || {
                calls.set(calls.get() + 1);
                calls.get() > 30
            };
            assert!(plan.bake(&inputs, "pipe-test", &cancel).is_err());
            assert_eq!(snapshot(&mesh), original);
            continue;
        }
        let bakes = if mode == 1 {
            plan.bake(&inputs, "pipe-test", &|| false).unwrap()
        } else {
            plan.bake_parallel(
                &inputs,
                "pipe-test",
                &weathered::ParallelBaking {
                    workers: 4,
                    working_bytes: 128 * 1024 * 1024,
                },
                &|| false,
            )
            .unwrap()
        };
        assert_eq!(
            positions,
            mesh.vertices
                .iter()
                .map(|v| (v.position, v.normal))
                .collect::<Vec<_>>()
        );
        let mut scene=superspec_engine::compile(r#"<architecture version="1" id="pipe-bake"><box id="host" width="1" depth="1" height="1"/></architecture>"#,0).unwrap();
        let bindings = bakes
            .iter()
            .map(|b| (b.material().id.clone(), b.material().id.clone()))
            .collect();
        scene
            .append_baked_visual("host", &mesh, &bindings, &bakes)
            .unwrap();
        packets.push(scene.packet());
    }
    assert_eq!(packets[0], packets[1]);
}
