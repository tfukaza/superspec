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
const OPERATIONS: [&str; 6] = ["vertical", "diagonal", "elbow", "coupling", "mount", "run"];
fn draw(drawing: &mut Fixture<'_, '_>, options: &Options, operation: &str) -> Result<(), String> {
    let pipe = PipeSystem::new(options.clone())?;
    match operation {
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

#[test]
fn every_pipe_operation_surveys_its_rendered_corners_and_receivers_cover_owned_triangles() {
    for frame in ["identity", "reflection", "tilted"] {
        for weathered in [false, true] {
            for operation in OPERATIONS {
                let label = format!("{frame}/{operation}/{weathered}");
                let mut options = scene_fixture::options();
                if !weathered {
                    options.weathering = None;
                }
                let pen = Pen::new(matrix(frame), "fixture").unwrap();
                let mut mesh = Mesh::new(Mode::Render, 20000, &|| false).unwrap();
                draw(
                    &mut Fixture {
                        pen: pen.clone(),
                        mesh: &mut mesh,
                        repeats: &repeat,
                    },
                    &options,
                    operation,
                )
                .unwrap();
                assert!(mesh.triangle_count() > 0, "{label}");
                let triangles = mesh.triangle_count();
                let plan = weathered::finish(&mut mesh, &|| false).unwrap();
                if !weathered {
                    assert!(plan.receivers().is_empty(), "{label} receivers");
                }
                let mut end = 0;
                for range in plan.receivers() {
                    assert!(
                        range.first_triangle >= end && range.triangle_count > 0,
                        "{label}"
                    );
                    end = range.first_triangle + range.triangle_count;
                    assert!(end <= triangles, "{label}");
                    let job = &plan.jobs()[range.job];
                    let field = job.runoff_recipe().map(|r| r.build(&|| false).unwrap());
                    let runoff = |u, v| field.as_ref().unwrap().sample(u, v);
                    let metal = Metal::new(
                        job.metal_settings().unwrap(),
                        field
                            .as_ref()
                            .map(|_| &runoff as &dyn Fn(f64, f64) -> Result<f64, String>),
                    )
                    .unwrap();
                    for [u, v] in [[0.1, 0.2], [0.5, 0.5], [0.9, 0.7]] {
                        let _: Sample = metal.sample(u, v).unwrap();
                    }
                }
                drop(plan);
                let mut survey = Mesh::new(Mode::Survey, 20000, &|| false).unwrap();
                draw(
                    &mut Fixture {
                        pen,
                        mesh: &mut survey,
                        repeats: &|_| panic!("survey resolved appearance"),
                    },
                    &options,
                    operation,
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
    // Start from the receiver settings an ordinary weathered pipe produces.
    let mut settings = {
        let mut mesh = Mesh::new(Mode::Render, 20000, &|| false).unwrap();
        draw(
            &mut Fixture {
                pen: Pen::new(Matrix::default(), "pipe").unwrap(),
                mesh: &mut mesh,
                repeats: &repeat,
            },
            &scene_fixture::options(),
            "vertical",
        )
        .unwrap();
        let plan = weathered::finish(&mut mesh, &|| false).unwrap();
        plan.jobs()[plan.receivers()[0].job]
            .metal_settings()
            .unwrap()
            .clone()
    };
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
