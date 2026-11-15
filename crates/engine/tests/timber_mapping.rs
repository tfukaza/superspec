use serde::Deserialize;
use superspec_engine::worldspec::themes::kowloon::{
    mesh::{Mesh, Mode},
    pen::{Pen, Tint},
    timber::Settings,
};
use superspec_geometry::Matrix;

#[derive(Deserialize)]
struct Case {
    frame: String,
    operation: String,
    textured: bool,
    finish: Settings,
    axis: Option<usize>,
    key: String,
    indices: Vec<u32>,
    positions: Vec<f64>,
    normals: Vec<f64>,
    uv: Vec<f64>,
    colors: Vec<f64>,
    slots: Vec<String>,
}
#[derive(Deserialize)]
struct Formatting {
    bits: String,
    text: String,
}
#[derive(Deserialize)]
struct Fixture {
    cases: Vec<Case>,
    formatting: Vec<Formatting>,
}
fn drawing(case: &Case, mode: Mode) -> Mesh<'static> {
    let matrix = match case.frame.as_str() {
        "identity" => Matrix::default(),
        "reflection" => Matrix([[-1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [4., -3., 2.]]),
        "rotation" => Matrix([[0., 1., 0.], [-1., 0., 0.], [0., 0., 1.], [4., -3., 2.]]),
        _ => panic!("Unknown fixture frame"),
    };
    let mut settings = case.finish.clone();
    settings.textured = case.textured;
    settings.face_grain_axis = case.axis;
    settings.timber_repeat = 2.;
    settings.atlas_repeat = 2.;
    let mut pen = Pen::new(matrix, "木/🌿")
        .unwrap()
        .with_timber(settings)
        .unwrap();
    if case.operation == "derived" {
        pen = pen
            .moved([0.3, -0.2, 0.1])
            .unwrap()
            .into_part("moved/🌿")
            .unwrap()
            .facing([0.1, 0.2, 0.3], [0., 1., 0.], [-1., 0., 0.])
            .unwrap();
    }
    let mut mesh = Mesh::new(mode, 50000, &|| false).unwrap();
    assert_eq!(
        superspec_engine::worldspec::themes::kowloon::timber::seed_key(
            &pen,
            &[0., -0., 1e-7, 1e21]
        )
        .unwrap(),
        case.key
    );
    let field = |p: [f64; 3]| {
        assert_eq!(mode, Mode::Render);
        Ok([0.2 + p[0] * 0.1, 0.3 + p[2] * 0.02, 0.4])
    };
    let tint = if case.operation == "field-bevel" {
        Tint::Field(&field)
    } else {
        Tint::Color([0.18, 0.09, 0.04])
    };
    let slot = "timber";
    let repeat = 2.;
    match case.operation.as_str() {
        "panel" | "variant" | "untextured" | "derived" | "grain-axis" | "empty-slots" => {
            pen.panel(&mut mesh, slot, [-0.2, 1.1, -0.3, 3.5], 0.1, &tint, repeat)
        }
        "convex" => pen.face(
            &mut mesh,
            slot,
            &[
                [-0.2, 0.1, 0.],
                [0.8, 0.1, -0.1],
                [1.1, 0.1, 1.6],
                [0.2, 0.1, 2.4],
                [-0.3, 0.1, 1.],
            ],
            [0., 1., 0.],
            &tint,
            repeat,
            None,
            None,
        ),
        "box-x" => pen.cuboid(
            &mut mesh,
            slot,
            [-0.2, 0.3, -0.4],
            [3.3, 0.6, -0.35],
            &tint,
            repeat,
            "",
        ),
        "box-y" => pen.cuboid(
            &mut mesh,
            slot,
            [-0.2, 0.3, -0.4],
            [0.1, 3.8, -0.2],
            &tint,
            repeat,
            "",
        ),
        "box-z" | "frontwear" | "omitted" => pen.cuboid(
            &mut mesh,
            slot,
            [-0.02, -0.03, -0.4],
            [0.02, 0.03, 3.2],
            &tint,
            repeat,
            if case.operation == "omitted" {
                "bd"
            } else {
                ""
            },
        ),
        "box-tie" => pen.cuboid(&mut mesh, slot, [0.; 3], [0.6; 3], &tint, repeat, ""),
        "bevel" | "slanted-bevel" | "field-bevel" | "beveled-frontwear" => pen.beveled_bar(
            &mut mesh,
            slot,
            [0.2, 0.3, -0.4],
            if case.operation == "bevel" {
                [0.2, 0.3, 3.2]
            } else {
                [1.2, 0.3, 2.6]
            },
            if case.operation == "bevel" {
                [1., 0., 0.]
            } else {
                [0., 1., 0.]
            },
            0.12,
            0.09,
            0.002,
            &tint,
            repeat,
        ),
        "explicit-uv" | "untextured-uv" => pen.face(
            &mut mesh,
            slot,
            &[[0., 0., 0.], [1., 0., 0.], [1., 0., 1.], [0., 0., 1.]],
            [0., 1., 0.],
            &tint,
            repeat,
            Some(&[[0., 0.], [1., 0.], [1., 1.], [0., 1.]]),
            None,
        ),
        "explicit-normals" | "untextured-normals" => pen.face(
            &mut mesh,
            slot,
            &[[0., 0., 0.], [1., 0., 0.], [1., 0., 1.], [0., 0., 1.]],
            [0., 1., 0.],
            &tint,
            repeat,
            None,
            Some(&[[0., 1., 0.]; 4]),
        ),
        "nonplanar" => pen.face(
            &mut mesh,
            slot,
            &[[0., 0., 0.], [1., 0.2, 0.], [0.2, 0., 1.]],
            [0., 1., 0.],
            &tint,
            repeat,
            None,
            None,
        ),
        "field-tint" | "untextured-field" => pen.panel(
            &mut mesh,
            slot,
            [0., 1., 0., 1.],
            0.,
            &Tint::Field(&|p| {
                assert_eq!(mode, Mode::Render);
                Ok([0.2 + p[0] * 0.1, 0.3, 0.4])
            }),
            repeat,
        ),
        "metal" | "metal-slots" => pen.cuboid(
            &mut mesh,
            "metal",
            [0.; 3],
            [0.2, 0.3, 0.4],
            &tint,
            repeat,
            "",
        ),
        _ => panic!("Unknown timber fixture operation"),
    }
    .unwrap();
    mesh
}
/// Native and browser execute identical frozen mapping and seed-format assertions.
pub fn check_reference_timber() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../tests/fixtures/rust/timber.json")).unwrap();
    for formatting in fixture.formatting {
        let bytes: Vec<u8> = formatting
            .bits
            .as_bytes()
            .chunks(2)
            .map(|hex| u8::from_str_radix(std::str::from_utf8(hex).unwrap(), 16).unwrap())
            .collect();
        let value = f64::from_le_bytes(bytes.try_into().unwrap());
        let pen = Pen::new(Matrix::default(), "format").unwrap();
        let key =
            superspec_engine::worldspec::themes::kowloon::timber::seed_key(&pen, &[value]).unwrap();
        assert_eq!(key.rsplit('/').next().unwrap(), formatting.text);
    }
    assert_eq!(fixture.cases.len(), 78);
    for case in fixture.cases {
        let mesh = drawing(&case, Mode::Render);
        assert_eq!(
            mesh.indices, case.indices,
            "{}/{} indices",
            case.frame, case.operation
        );
        let actual = [
            mesh.vertices
                .iter()
                .flat_map(|v| v.position)
                .collect::<Vec<_>>(),
            mesh.vertices.iter().flat_map(|v| v.normal).collect(),
            mesh.vertices.iter().flat_map(|v| v.uv).collect(),
            mesh.vertices.iter().flat_map(|v| v.color).collect(),
        ];
        for (channel, (actual, expected)) in actual
            .iter()
            .zip([&case.positions, &case.normals, &case.uv, &case.colors])
            .enumerate()
        {
            assert_eq!(actual.len(), expected.len());
            for (i, (a, b)) in actual.iter().zip(expected).enumerate() {
                assert!(
                    (a - b).abs() < 2e-12,
                    "{}/{} channel{channel}[{i}]:{a} != {b}",
                    case.frame,
                    case.operation
                );
            }
        }
        let slots: Vec<_> = mesh
            .triangle_surfaces
            .iter()
            .map(|i| mesh.surfaces[*i].slot.clone())
            .collect();
        assert_eq!(slots, case.slots);
        let survey = drawing(&case, Mode::Survey);
        assert!(survey.vertices.is_empty());
        assert_eq!(
            survey.survey,
            mesh.indices
                .iter()
                .map(|i| mesh.vertices[*i as usize].position)
                .collect::<Vec<_>>()
        );
    }
}
#[test]
fn frozen_timber_preserves_geometry_uv_grain_wear_and_survey() {
    check_reference_timber();
}

#[test]
fn atlas_charts_preserve_box_area_planes_metric_density_and_end_grain_bank() {
    use superspec_geometry::math::{cross, dot, sub};
    for (along, size) in [[3.6, 0.3, 0.04], [0.3, 3.6, 0.2], [0.04, 0.06, 3.6]]
        .into_iter()
        .enumerate()
    {
        let pen = Pen::new(Matrix::default(), "metric")
            .unwrap()
            .with_timber(Settings::default())
            .unwrap();
        let mut mesh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
        pen.cuboid(
            &mut mesh,
            "timber",
            [0.; 3],
            size,
            &Tint::Color([0.18, 0.09, 0.04]),
            2.,
            "",
        )
        .unwrap();
        let mut area = 0.;
        let mut side_family = None;
        for triangle in mesh.indices.as_chunks::<3>().0 {
            let a = mesh.vertices[triangle[0] as usize];
            let b = mesh.vertices[triangle[1] as usize];
            let c = mesh.vertices[triangle[2] as usize];
            let normal = cross(sub(b.position, a.position), sub(c.position, a.position));
            area += dot(normal, normal).sqrt() / 2.;
            let axis = (0..3)
                .max_by(|i, j| a.normal[*i].abs().total_cmp(&a.normal[*j].abs()))
                .unwrap();
            for vertex in [a, b, c] {
                assert!(
                    vertex.position[axis].abs() < 1e-8
                        || (vertex.position[axis] - size[axis]).abs() < 1e-8
                );
            }
            let centroid_u = (a.uv[0] + b.uv[0] + c.uv[0]) / 3. * 1024.;
            let family = (centroid_u / 256.).floor() as usize;
            assert!(family < 4);
            let cap = axis == along;
            let region_y = if cap { 1536. } else { 0. };
            let region_height = if cap { 256. } else { 1536. };
            if !cap {
                if let Some(previous) = side_family {
                    assert_eq!(family, previous);
                } else {
                    side_family = Some(family);
                }
            }
            for vertex in [a, b, c] {
                let u = vertex.uv[0] * 1024.;
                let v = (1. - vertex.uv[1]) * 1792.;
                assert!(
                    u >= family as f64 * 256. + 4.5 - 1e-8
                        && u <= family as f64 * 256. + 251.5 + 1e-8
                );
                assert!(v >= region_y + 4.5 - 1e-8 && v <= region_y + region_height - 4.5 + 1e-8);
            }
            for (a, b) in [(a, b), (b, c), (c, a)] {
                let delta = sub(a.position, b.position);
                let metric = dot(delta, delta).sqrt() * 494.;
                let du = (a.uv[0] - b.uv[0]) * 1024.;
                let dv = (a.uv[1] - b.uv[1]) * 1792.;
                assert!((metric - (du * du + dv * dv).sqrt()).abs() < 0.015);
            }
        }
        let expected = 2. * (size[0] * size[1] + size[0] * size[2] + size[1] * size[2]);
        assert!((area - expected).abs() < 2e-6, "{area} != {expected}");
    }
}

#[test]
fn cancellation_and_subdivision_limits_leave_no_member_state_in_the_caller_pen() {
    use std::cell::Cell;
    let pen = Pen::new(Matrix::default(), "scope")
        .unwrap()
        .with_timber(Settings::default())
        .unwrap();
    let tint = Tint::Color([0.18, 0.09, 0.04]);
    let calls = Cell::new(0);
    let enabled = Cell::new(true);
    let cancelled = || {
        calls.set(calls.get() + 1);
        enabled.get() && calls.get() > 8
    };
    let mut cancelled_mesh = Mesh::new(Mode::Render, 10000, &cancelled).unwrap();
    assert!(
        pen.cuboid(
            &mut cancelled_mesh,
            "timber",
            [0.; 3],
            [3.6, 0.3, 0.04],
            &tint,
            1.,
            ""
        )
        .unwrap_err()
        .contains("cancelled")
    );
    enabled.set(false);
    let mut tiny = Mesh::new(Mode::Render, 1, &|| false).unwrap();
    assert!(
        pen.cuboid(
            &mut tiny,
            "timber",
            [0.; 3],
            [3.6, 0.3, 0.04],
            &tint,
            1.,
            ""
        )
        .is_err()
    );
    let mut large = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
    assert!(
        pen.panel(&mut large, "timber", [0., 1e7, 0., 1.], 0., &tint, 1.)
            .is_err()
    );
    assert!(large.indices.is_empty());
    let mut reused = Mesh::new(Mode::Render, 10000, &cancelled).unwrap();
    let mut fresh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
    pen.panel(&mut reused, "timber", [-0.2, 0.9, 0., 1.], 0., &tint, 1.)
        .unwrap();
    Pen::new(Matrix::default(), "scope")
        .unwrap()
        .with_timber(Settings::default())
        .unwrap()
        .panel(&mut fresh, "timber", [-0.2, 0.9, 0., 1.], 0., &tint, 1.)
        .unwrap();
    assert_eq!(reused.indices, fresh.indices);
    for (a, b) in reused.vertices.iter().zip(&fresh.vertices) {
        assert_eq!(a.position, b.position);
        assert_eq!(a.uv, b.uv);
        assert_eq!(a.color, b.color);
    }
    assert!(
        pen.panel(&mut fresh, "timber", [0., 1., 0., 1.], 0., &tint, f64::NAN)
            .is_err()
    );
}

#[test]
fn fallback_uv_scale_belongs_to_the_selected_material() {
    let points = [[0., 0., 0.], [1., 0., 0.], [1., 0., 1.], [0., 0., 1.]];
    let nonplanar = [[0., 0., 0.], [1., 0.2, 0.], [0.2, 0., 1.]];
    let authored = [[0.1, 0.2], [0.3, 0.4], [0.5, 0.6], [0.7, 0.8]];
    let normals = [[0., 1., 0.]; 4];
    let plain = Pen::new(Matrix::default(), "fallback").unwrap();
    let color = Tint::Color([0.2; 3]);
    let field = Tint::Field(&|_| Ok([0.2; 3]));
    for textured in [true, false] {
        let pen = plain
            .with_timber(Settings {
                textured,
                timber_repeat: 2.,
                atlas_repeat: 5.,
                ..Settings::default()
            })
            .unwrap();
        for kind in ["unrelated", "field", "normals", "nonplanar", "uv"] {
            let slot = if kind == "unrelated" {
                "metal"
            } else {
                "timber"
            };
            let target = if kind == "unrelated" {
                "metal"
            } else if textured {
                "timber"
            } else {
                "shop-timber"
            };
            let repeat = if kind == "unrelated" {
                3.
            } else if textured {
                2.
            } else {
                5.
            };
            let vertices = if kind == "nonplanar" {
                &nonplanar[..]
            } else {
                &points[..]
            };
            let tint = if kind == "field" { &field } else { &color };
            let uv = (kind == "uv").then_some(&authored[..]);
            let normals = (kind == "normals").then_some(&normals[..]);
            let mut actual = Mesh::new(Mode::Render, 100, &|| false).unwrap();
            let mut expected = Mesh::new(Mode::Render, 100, &|| false).unwrap();
            pen.face(
                &mut actual,
                slot,
                vertices,
                [0., 1., 0.],
                tint,
                3.,
                uv,
                normals,
            )
            .unwrap();
            plain
                .face(
                    &mut expected,
                    target,
                    vertices,
                    [0., 1., 0.],
                    tint,
                    repeat,
                    uv,
                    normals,
                )
                .unwrap();
            assert_eq!(actual.indices, expected.indices);
            assert!(actual.surfaces.iter().all(|surface| surface.slot == target));
            for (a, b) in actual.vertices.iter().zip(&expected.vertices) {
                assert_eq!(a.uv, b.uv, "{textured}/{kind}");
            }
        }
    }
}

#[test]
fn clipping_empty_cells_charges_polygon_work_before_emitting() {
    let pen = Pen::new(Matrix::default(), "thin")
        .unwrap()
        .with_timber(Settings::default())
        .unwrap();
    // A thin convex diagonal has a large chart rectangle, mostly empty cells.
    let points: Vec<_> = (0..512)
        .map(|i| {
            let angle = i as f64 * std::f64::consts::TAU / 512.;
            let along = angle.cos() * 50.;
            let across = angle.sin() * 0.001;
            [along + across, 0., along - across]
        })
        .collect();
    let mut mesh = Mesh::new(Mode::Render, 100, &|| false).unwrap();
    let error = pen
        .face(
            &mut mesh,
            "timber",
            &points,
            [0., 1., 0.],
            &Tint::Color([0.2; 3]),
            1.,
            None,
            None,
        )
        .unwrap_err();
    assert!(error.contains("work budget"), "{error}");
    assert!(mesh.indices.is_empty());
}

#[test]
fn shelf_contact_wear_is_continuous_across_the_physical_edge() {
    use superspec_engine::worldspec::themes::kowloon::palette::srgb;
    let pen = Pen::new(Matrix::default(), "shelf")
        .unwrap()
        .with_timber(Settings::default())
        .unwrap();
    let mut mesh = Mesh::new(Mode::Render, 20000, &|| false).unwrap();
    pen.cuboid(
        &mut mesh,
        "timber",
        [0.; 3],
        [1.2, 0.30, 0.022],
        &Tint::Color(srgb("#7a5a3c")),
        1.,
        "",
    )
    .unwrap();
    let sample = |at: [f64; 3], axis: usize, sign: f64| {
        let axes: Vec<_> = (0..3).filter(|i| *i != axis).collect();
        let area = |a: [f64; 3], b: [f64; 3], c: [f64; 3]| {
            (b[axes[0]] - a[axes[0]]) * (c[axes[1]] - a[axes[1]])
                - (b[axes[1]] - a[axes[1]]) * (c[axes[0]] - a[axes[0]])
        };
        let mut samples = Vec::<[f64; 3]>::new();
        for indices in mesh.indices.as_chunks::<3>().0 {
            let v = [indices[0], indices[1], indices[2]].map(|i| mesh.vertices[i as usize]);
            if !v
                .iter()
                .all(|v| v.normal[axis] * sign > 0.99 && (v.position[axis] - at[axis]).abs() < 2e-5)
            {
                continue;
            }
            let total = area(v[0].position, v[1].position, v[2].position);
            if total.abs() < 1e-12 {
                continue;
            }
            let weights: [f64; 3] = std::array::from_fn(|i| {
                area(v[(i + 1) % 3].position, v[(i + 2) % 3].position, at) / total
            });
            if weights.iter().any(|w| *w < -1e-5) {
                continue;
            }
            samples.push(std::array::from_fn(|channel| {
                (0..3).map(|i| weights[i] * v[i].color[channel]).sum()
            }));
        }
        assert!(!samples.is_empty(), "No surface at {at:?}");
        for value in &samples {
            for channel in 0..3 {
                assert!((value[channel] - samples[0][channel]).abs() < 0.003);
            }
        }
        samples[0]
    };
    let mut edges = Vec::new();
    for x in [0.11, 0.37, 0.58, 0.83, 1.09] {
        let top = sample([x, 0., 0.022], 2, 1.);
        let front = sample([x, 0., 0.022], 1, -1.);
        for i in 0..3 {
            assert!((top[i] - front[i]).abs() < 0.003);
        }
        edges.push(top[0]);
        for (point, axis, sign) in [
            ([x, 0.025, 0.022], 2, 1.),
            ([x, 0., 0.005], 1, -1.),
            ([x, 0.30, 0.011], 1, 1.),
            ([x, 0.15, 0.], 2, -1.),
        ] {
            assert!(
                sample(point, axis, sign)
                    .iter()
                    .all(|c| (*c - 0.5).abs() < 0.003)
            );
        }
    }
    let min = edges.iter().copied().fold(f64::INFINITY, f64::min);
    let max = edges.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    assert!(max > 0.51 && max - min > 0.005, "{edges:?}");
}
