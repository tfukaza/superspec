use superspec_aml::rng::SeededRng;
use superspec_engine::{
    materialspec::surface::Sample,
    worldspec::themes::kowloon::{
        fixture::Fixture,
        mesh::{Mesh, Mode},
        metal::Metal,
        pen::{Pen, Tint},
        steel::{self, Finish, Plate, Steel},
        water_tanks::{self, Settings},
        weathered,
    },
};
use superspec_geometry::Matrix;
#[path = "support/tank_scene.rs"]
mod scene_fixture;
const OPERATIONS: [&str; 17] = [
    "member",
    "angle",
    "gusset",
    "rod",
    "plate2",
    "plate4",
    "turnbuckle",
    "chain",
    "lattice",
    "corrugated",
    "round-tank",
    "box-tank",
    "brushed-box",
    "brushed-slope",
    "brushed-uv",
    "brushed-derived",
    "brushed-curves",
];
struct Case {
    density: f64,
    operation: &'static str,
    settings: Settings,
}
fn repeat(slot: &str) -> f64 {
    match slot {
        "metal" => 1.,
        "paint" => 1.25,
        "brushed-metal" => 1.5,
        "concrete" => 1.75,
        _ => panic!("unknown slot"),
    }
}
fn matrix(frame: &str) -> Matrix {
    if frame == "identity" {
        Matrix::default()
    } else {
        Matrix([[-1., 0., 0.], [0., -1., 0.], [0., 0., -1.], [4., 3., 5.]])
    }
}
fn draw(drawing: &mut Fixture<'_, '_>, case: &Case, rng: &mut SeededRng) -> Result<(), String> {
    let s = Steel {
        slot: "metal",
        tint: [0.3, 0.5, 0.7],
        finish: Finish::Metal((case.density > 0.).then_some(&case.settings.weathering)),
    };
    match case.operation {
        "corrugated" => superspec_engine::worldspec::themes::kowloon::corrugated::draw(
            drawing,
            "metal",
            [[0.; 3], [0.8, 0., 0.], [0.8, 0.4, 0.2], [0., 0.4, 0.2]],
            0.09,
            s.tint,
            (case.density > 0.).then_some(&case.settings.weathering),
        ),
        "member" => steel::member(
            drawing,
            s,
            [[0.; 3], [0., 0., 1.]],
            [1., 0., 0.],
            [0.08, 0.06],
        ),
        "angle" => steel::angle(
            drawing,
            s,
            [[0.; 3], [0., 0., 1.]],
            [[1., 0., 0.], [0., 1., 0.]],
            [0.08, 0.008],
        ),
        "gusset" => steel::gusset(
            drawing,
            s,
            [[0.; 3], [1., 0., 0.], [0., 0., 0.6]],
            [0., -1., 0.],
            0.006,
        ),
        "rod" => steel::rod(
            drawing,
            s,
            &[[0.; 3], [0., 0., 0.6], [0.2, 0.1, 0.8]],
            0.025,
            8,
        ),
        "plate2" | "plate4" => steel::fixing_plate(
            drawing,
            s,
            &Plate {
                center: [0.2, 0., 0.3],
                across: [1., 0., 0.],
                up: [0., 0., 1.],
                out: [0., 1., 0.],
                size: [0.16, 0.25],
                bolts: if case.operation == "plate2" { 2 } else { 4 },
            },
        ),
        "turnbuckle" => steel::turnbuckle(drawing, s, [[0.; 3], [1., 0.2, 1.]], 0.02),
        "chain" => steel::chain(drawing, s, [[0.; 3], [0.2, 0.1, 1.]], 0.1),
        "lattice" => steel::lattice(
            drawing,
            s,
            [[[0.; 3], [1., 0., 0.]], [[0., 0., 0.3], [1., 0., 0.3]]],
            3,
            [0.06, 0.025],
        ),
        "round-tank" => water_tanks::round_tank(drawing, [0.1, 0.2, 0.3], rng, &case.settings),
        "box-tank" => water_tanks::box_tank(drawing, [0.1, 0.2, 0.3], rng, &case.settings),
        op => {
            let repeat = if drawing.mesh.surveying() {
                1.
            } else {
                repeat("brushed-metal")
            };
            let mut pen = drawing.pen.with_brushed_metal(repeat)?;
            if op == "brushed-derived" {
                pen = pen.moved([0.3, 0.2, 0.1])?.into_part("part-地")?.facing(
                    [0., 0., 0.2],
                    [0., 1., 0.],
                    [-1., 0., 0.],
                )?;
            }
            let mut d = Fixture {
                pen,
                mesh: drawing.mesh,
                repeats: drawing.repeats,
            };
            match op {
                "brushed-box" | "brushed-derived" => {
                    d.cuboid("metal", [0.; 3], [0.8, 0.3, 0.6], s.tint, "b")
                }
                "brushed-slope" => d.face(
                    "metal",
                    &[[0.; 3], [0.8, 0., 0.2], [0.8, 0.3, 0.4], [0., 0.3, 0.2]],
                    [0., -0.5, 1.],
                    s.tint,
                ),
                "brushed-uv" => {
                    d.pen.face(
                        d.mesh,
                        "metal",
                        &[[0.; 3], [1., 0., 0.], [1., 0., 1.], [0., 0., 1.]],
                        [0., 1., 0.],
                        &Tint::Color(s.tint),
                        1.,
                        Some(&[[0.13, 0.23], [0.77, 0.23], [0.77, 0.83], [0.13, 0.83]]),
                        None,
                    )?;
                    d.cuboid("paint", [0., 0.1, 0.], [0.1, 0.2, 0.1], s.tint, "b")
                }
                "brushed-curves" => {
                    d.drum("metal", [0.; 3], 0.15, 0.4, s.tint, 16)?;
                    d.tube(
                        "metal",
                        &[[0.3, 0., 0.], [0.5, 0., 0.4], [0.7, 0.1, 0.6]],
                        0.02,
                        s.tint,
                        8,
                    )
                }
                _ => panic!("unknown operation"),
            }
        }
    }
}
#[test]
fn hardware_surveys_its_rendered_corners_with_identical_rng_use_and_bounded_receivers() {
    for frame in ["identity", "reflection"] {
        for density in [0., 128.] {
            for operation in OPERATIONS {
                let case = Case {
                    density,
                    operation,
                    settings: scene_fixture::settings(),
                };
                let label = format!("{frame}/{density}/{operation}");
                let pen = Pen::new(matrix(frame), "fixture").unwrap();
                let mut mesh = Mesh::new(Mode::Render, 50000, &|| false).unwrap();
                let mut rng = SeededRng::from_text("hardware/7");
                draw(
                    &mut Fixture {
                        pen: pen.clone(),
                        mesh: &mut mesh,
                        repeats: &repeat,
                    },
                    &case,
                    &mut rng,
                )
                .unwrap();
                assert!(mesh.triangle_count() > 0, "{label}");
                assert_eq!(mesh.triangle_surfaces.len(), mesh.triangle_count());
                let triangles = mesh.triangle_count();
                let plan = weathered::finish(&mut mesh, &|| false).unwrap();
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
                    let sample: Sample = metal.sample(0.4, 0.6).unwrap();
                    let _ = sample;
                }
                drop(plan);
                let mut survey = Mesh::new(Mode::Survey, 50000, &|| false).unwrap();
                let mut survey_rng = SeededRng::from_text("hardware/7");
                draw(
                    &mut Fixture {
                        pen,
                        mesh: &mut survey,
                        repeats: &|_| panic!("survey resolved appearance"),
                    },
                    &case,
                    &mut survey_rng,
                )
                .unwrap();
                assert_eq!(survey_rng.next_u32(), rng.next_u32(), "{label} RNG");
                assert_eq!(
                    survey.survey.len(),
                    mesh.indices.len(),
                    "{label} survey count"
                );
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
#[test]
fn complete_tanks_bake_with_stainless_maps_and_separate_collision() {
    for round in [true, false] {
        let scene = scene_fixture::scene(round);
        assert!(scene.manifest.assets.len() > 12);
        assert_eq!(scene.manifest.colliders.len(), 1);
        assert!(
            scene
                .manifest
                .colliders
                .iter()
                .all(|id| !id.contains("dressing"))
        );
        assert!(scene.manifest.meshes.iter().any(|m| {
            m.visible
                && m.vertex_count > 0
                && m.material
                    .as_deref()
                    .is_some_and(|m| m.starts_with("$surface/raster/tank/"))
        }));
        if round {
            assert!(
                scene
                    .manifest
                    .meshes
                    .iter()
                    .any(|m| m.visible && m.material.as_deref() == Some("brushed-metal"))
            );
        }
    }
}

#[test]
fn invalid_hardware_is_bounded_and_scoped_styles_survive_failure() {
    let mut mesh = Mesh::new(Mode::Render, 50000, &|| false).unwrap();
    let base = Pen::new(Matrix::default(), "owner")
        .unwrap()
        .with_brushed_metal(1.5)
        .unwrap();
    let settings = scene_fixture::settings();
    let s = Steel {
        slot: "metal",
        tint: [0.3, 0.5, 0.7],
        finish: Finish::Metal(Some(&settings.weathering)),
    };
    let mut drawing = Fixture {
        pen: base.clone(),
        mesh: &mut mesh,
        repeats: &repeat,
    };
    assert!(steel::chain(&mut drawing, s, [[0.; 3], [0., 0., 1.]], 1e-10).is_err());
    for legs in [
        [[1., 0., 0.], [1., 0., 0.]],
        [[1., 0., 0.], [0., 0., 1.]],
        [[1., 0., 0.], [0., 2., 0.]],
    ] {
        assert!(steel::angle(&mut drawing, s, [[0.; 3], [0., 0., 1.]], legs, [0.1, 0.01]).is_err());
    }
    for steel in [
        s.toned(f64::NAN),
        Steel {
            tint: [f64::NAN, 0.5, 0.7],
            ..s
        }
        .toned(0.8),
    ] {
        assert!(
            steel::member(
                &mut drawing,
                steel,
                [[0.; 3], [0., 0., 1.]],
                [1., 0., 0.],
                [0.05, 0.02]
            )
            .is_err()
        );
    }
    assert!(
        steel::lattice(
            &mut drawing,
            s,
            [[[0.; 3], [1., 0., 0.]], [[0., 0., 1.], [1., 0., 1.]]],
            0,
            [0.05, 0.02]
        )
        .is_err()
    );
    assert!(
        steel::fixing_plate(
            &mut drawing,
            s,
            &Plate {
                center: [0.; 3],
                across: [1., 0., 0.],
                up: [0., 0., 1.],
                out: [1., 0., 0.],
                size: [0.1, 0.2],
                bolts: 4
            }
        )
        .is_err()
    );
    assert!(
        steel::member(
            &mut drawing,
            s,
            [[0.; 3], [0., 0., 1.]],
            [0., 0., 1.],
            [0.05, 0.02]
        )
        .is_err()
    );
    assert_eq!(drawing.mesh.triangle_count(), 0);
    drawing
        .cuboid("metal", [0.; 3], [0.8, 0.3, 0.6], [1.; 3], "b")
        .unwrap();
    let mut control = Mesh::new(Mode::Render, 100, &|| false).unwrap();
    Fixture {
        pen: base,
        mesh: &mut control,
        repeats: &repeat,
    }
    .cuboid("metal", [0.; 3], [0.8, 0.3, 0.6], [1.; 3], "b")
    .unwrap();
    assert_eq!(mesh.indices, control.indices);
    assert_eq!(mesh.vertices.len(), control.vertices.len());
    for (a, b) in mesh.vertices.iter().zip(&control.vertices) {
        assert_eq!(a.position, b.position);
        assert_eq!(a.normal, b.normal);
        assert_eq!(a.uv, b.uv);
        assert_eq!(a.color, b.color);
    }
    assert!(
        mesh.surfaces
            .iter()
            .all(|s| s.slot == "brushed-metal" && s.part == "owner")
    );
    assert!(
        weathered::finish(&mut mesh, &|| false)
            .unwrap()
            .receivers()
            .is_empty()
    );
}

#[test]
fn finish_replacement_keeps_one_style_and_does_not_mutate_the_caller() {
    use superspec_engine::worldspec::themes::kowloon::{metal::Kind, timber};
    let settings = scene_fixture::settings();
    let style = |pen: &Pen, mesh: &mut Mesh<'_>, kind: &str| match kind {
        "brushed" => pen.with_brushed_metal(1.5),
        "timber" => pen.with_timber(timber::Settings {
            slots: vec!["metal".into()],
            textured: false,
            ..Default::default()
        }),
        "weathered" => pen.with_weathering(
            mesh,
            settings.weathering.clone(),
            Kind::Frame,
            None,
            vec!["metal".into()],
        ),
        _ => unreachable!(),
    };
    let emit = |pen: &Pen, mesh: &mut Mesh<'_>| {
        pen.panel(
            mesh,
            "metal",
            [0., 0.8, 0., 0.6],
            0.,
            &Tint::Color([0.3, 0.5, 0.7]),
            1.,
        )
    };
    let vertices = |mesh: &Mesh<'_>| {
        mesh.vertices
            .iter()
            .map(|v| {
                v.position
                    .into_iter()
                    .chain(v.normal)
                    .chain(v.uv)
                    .chain(v.color)
                    .map(f64::to_bits)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    };
    for (from, to) in [
        ("brushed", "timber"),
        ("brushed", "weathered"),
        ("timber", "brushed"),
        ("weathered", "brushed"),
    ] {
        let plain = Pen::new(Matrix::default(), "scope").unwrap();
        let mut mesh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
        let parent = style(&plain, &mut mesh, from).unwrap();
        let child = style(&parent, &mut mesh, to).unwrap();
        emit(&child, &mut mesh).unwrap();
        let mut expected = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
        let target = style(&plain, &mut expected, to).unwrap();
        emit(&target, &mut expected).unwrap();
        assert_eq!(vertices(&mesh), vertices(&expected), "{from} -> {to}");
        assert_eq!(mesh.indices, expected.indices);
        assert_eq!(
            mesh.surfaces
                .iter()
                .map(|s| s.slot.as_str())
                .collect::<Vec<_>>(),
            expected
                .surfaces
                .iter()
                .map(|s| s.slot.as_str())
                .collect::<Vec<_>>()
        );
        let first = mesh.vertices.len();
        let first_triangle = mesh.triangle_count();
        emit(&parent, &mut mesh).unwrap();
        let mut original = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
        let source = style(&plain, &mut original, from).unwrap();
        emit(&source, &mut original).unwrap();
        assert_eq!(
            vertices(&mesh)[first..],
            vertices(&original),
            "{from} caller restored"
        );
        let plan = weathered::finish(&mut mesh, &|| false).unwrap();
        assert_eq!(
            plan.receivers().len(),
            usize::from(from == "weathered" || to == "weathered")
        );
        for r in plan.receivers() {
            assert_eq!(
                r.first_triangle,
                if to == "weathered" { 0 } else { first_triangle }
            );
        }
    }
}

#[test]
fn brushed_faces_preserve_authored_uv_normals_and_per_corner_tint() {
    use std::cell::Cell;
    let pen = Pen::new(matrix("reflection"), "brushed").unwrap();
    let brushed = pen.with_brushed_metal(1.5).unwrap();
    let points = [[0., 0., 0.], [1., 0., 0.], [1., 0., 1.], [0., 0., 1.]];
    let uv = [[0.13, 0.23], [0.77, 0.23], [0.77, 0.83], [0.13, 0.83]];
    let normals = [
        [0., 1., 0.],
        [0.6, 0.8, 0.],
        [0., 0.8, 0.6],
        [-0.6, 0.8, 0.],
    ];
    let calls = Cell::new(0);
    let color = |p: [f64; 3]| {
        calls.set(calls.get() + 1);
        Ok([0.2 + p[0] * 0.3, 0.5, 0.3 + p[2] * 0.4])
    };
    let mut mesh = Mesh::new(Mode::Render, 100, &|| false).unwrap();
    let mut control = Mesh::new(Mode::Render, 100, &|| false).unwrap();
    brushed
        .face(
            &mut mesh,
            "metal",
            &points,
            [0., 1., 0.],
            &Tint::Field(&color),
            1.,
            Some(&uv),
            Some(&normals),
        )
        .unwrap();
    let expected_calls = calls.get();
    calls.set(0);
    pen.face(
        &mut control,
        "brushed-metal",
        &points,
        [0., 1., 0.],
        &Tint::Field(&color),
        1.,
        Some(&uv),
        Some(&normals),
    )
    .unwrap();
    assert_eq!(calls.get(), expected_calls);
    assert!(expected_calls > 0);
    assert_eq!(mesh.indices, control.indices);
    assert_eq!(mesh.vertices.len(), control.vertices.len());
    for (a, b) in mesh.vertices.iter().zip(&control.vertices) {
        assert_eq!(a.position, b.position);
        assert_eq!(a.normal, b.normal);
        assert_eq!(a.uv, b.uv);
        assert_eq!(a.color, b.color);
    }
    let mut survey = Mesh::new(Mode::Survey, 100, &|| false).unwrap();
    calls.set(0);
    brushed
        .face(
            &mut survey,
            "metal",
            &points,
            [0., 1., 0.],
            &Tint::Field(&color),
            1.,
            Some(&uv),
            Some(&normals),
        )
        .unwrap();
    assert_eq!(calls.get(), 0);
    assert_eq!(
        survey.survey,
        mesh.indices
            .iter()
            .map(|i| mesh.vertices[*i as usize].position)
            .collect::<Vec<_>>()
    );
}

#[test]
fn cancelled_tanks_discard_partial_jobs_and_fresh_drawings_recover() {
    use std::cell::Cell;
    let settings = scene_fixture::settings();
    let pen = Pen::new(matrix("reflection"), "tank").unwrap();
    for round in [true, false] {
        let cancelled = Cell::new(false);
        let checks = Cell::new(0);
        let cancel = || {
            checks.set(checks.get() + 1);
            cancelled.get() && checks.get() > 40
        };
        let mut failed = Mesh::new(Mode::Render, 50000, &cancel).unwrap();
        cancelled.set(true);
        let mut drawing = Fixture {
            pen: pen.clone(),
            mesh: &mut failed,
            repeats: &repeat,
        };
        let mut rng = SeededRng::from_text("hardware/7");
        let result = if round {
            water_tanks::round_tank(&mut drawing, [0.; 3], &mut rng, &settings)
        } else {
            water_tanks::box_tank(&mut drawing, [0.; 3], &mut rng, &settings)
        };
        assert!(result.unwrap_err().to_lowercase().contains("cancel"));
        assert!(failed.triangle_count() > 0);
        assert!(
            failed
                .surfaces
                .iter()
                .all(|s| !s.slot.starts_with("$surface/"))
        );
        cancelled.set(false);
        let mut fresh = Mesh::new(Mode::Render, 50000, &cancel).unwrap();
        let mut rng = SeededRng::from_text("hardware/7");
        let mut drawing = Fixture {
            pen: pen.clone(),
            mesh: &mut fresh,
            repeats: &repeat,
        };
        if round {
            water_tanks::round_tank(&mut drawing, [0.; 3], &mut rng, &settings)
        } else {
            water_tanks::box_tank(&mut drawing, [0.; 3], &mut rng, &settings)
        }
        .unwrap();
        assert!(fresh.triangle_count() > 5000);
        assert!(
            !weathered::finish(&mut fresh, &|| false)
                .unwrap()
                .jobs()
                .is_empty()
        );
    }
}

#[test]
fn corrugated_admission_bounds_work_and_rejects_invalid_pigment() {
    use superspec_engine::worldspec::themes::kowloon::corrugated;
    let corners = [[0.; 3], [0.8, 0., 0.], [0.8, 0.4, 0.2], [0., 0.4, 0.2]];
    let pen = Pen::new(Matrix::default(), "sheet").unwrap();
    for (points, pitch, tint) in [
        (corners, 0., [1.; 3]),
        (corners, -0.1, [1.; 3]),
        (corners, f64::NAN, [1.; 3]),
        (corners, 1e-8, [1.; 3]),
        ([[0.; 3]; 4], 0.09, [1.; 3]),
        (corners, 0.09, [f64::NAN, 0.5, 0.7]),
        (corners, 0.09, [-0.1, 0.5, 0.7]),
    ] {
        let mut mesh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
        assert!(
            corrugated::draw(
                &mut Fixture {
                    pen: pen.clone(),
                    mesh: &mut mesh,
                    repeats: &repeat
                },
                "metal",
                points,
                pitch,
                tint,
                None
            )
            .is_err()
        );
        assert_eq!(mesh.triangle_count(), 0);
        assert!(mesh.vertices.is_empty());
        corrugated::draw(
            &mut Fixture {
                pen: pen.clone(),
                mesh: &mut mesh,
                repeats: &repeat,
            },
            "metal",
            corners,
            0.09,
            [0.3, 0.5, 0.7],
            None,
        )
        .unwrap();
        assert!(mesh.triangle_count() > 0);
    }
    let mut survey = Mesh::new(Mode::Survey, 10000, &|| false).unwrap();
    corrugated::draw(
        &mut Fixture {
            pen,
            mesh: &mut survey,
            repeats: &|_| panic!("survey resolved appearance"),
        },
        "metal",
        corners,
        0.09,
        [f64::NAN; 3],
        None,
    )
    .unwrap();
    assert!(!survey.survey.is_empty());
}
