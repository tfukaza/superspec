use std::cell::Cell;
use superspec_engine::worldspec::themes::kowloon::{
    mesh::{Mesh, Mode, rounded},
    molding::{Strip, profiled_strip},
    pen::{PatchVertex, Pen, Tint},
};
use superspec_geometry::{
    Matrix, V3,
    math::{cross, dot, sub},
};

const QUAD: [V3; 4] = [[1., 0., 2.], [3., 0., 2.], [3., 0., 3.], [1., 0., 3.]];
fn frame(name: &str) -> Matrix {
    match name {
        "identity" => Matrix::default(),
        "reflection" => Matrix([[-1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [10., -3., 4.]]),
        "rotation" => Matrix([[0., 1., 0.], [-1., 0., 0.], [0., 0., 1.], [10., -3., 4.]]),
        _ => panic!("unknown fixture frame"),
    }
}
const OPERATIONS: [&str; 13] = [
    "box",
    "omitted",
    "sheet",
    "bar",
    "bevel",
    "tube",
    "disc",
    "drum",
    "patch",
    "flipped",
    "molding-sharp",
    "molding-rounded",
    "molding-miter",
];
fn draw(pen: &Pen, mesh: &mut Mesh<'_>, operation: &str, tint: &Tint<'_>) -> Result<(), String> {
    match operation {
        "box" | "omitted" => pen.cuboid(
            mesh,
            "wall",
            [-0.2, 0.3, -0.4],
            [1.3, 2.1, 2.7],
            tint,
            2.,
            if operation == "omitted" { "b" } else { "" },
        ),
        "sheet" => pen.sheet(
            mesh,
            "wall",
            &[[0., 0., 0.], [2., 0., 0.], [2., 0.2, 1.], [0., 0.2, 1.]],
            tint,
            &Tint::Color([0.6, 0.2, 0.1]),
            2.,
        ),
        "bar" => pen.bar(
            mesh,
            "wall",
            [0.2, 0.3, -0.4],
            [1.2, -0.7, 2.6],
            0.12,
            tint,
            2.,
        ),
        "bevel" => pen.beveled_bar(
            mesh,
            "wall",
            [0.2, 0.3, -0.4],
            [0.2, 0.3, 2.6],
            [1., 0., 0.],
            0.12,
            0.09,
            0.002,
            tint,
            2.,
        ),
        "tube" => pen.tube(
            mesh,
            "wall",
            &[
                [0.2, 0.3, -0.4],
                [0.2, 0.3, -0.4],
                [1.2, -0.7, 2.6],
                [1.2, -0.7, 3.2],
            ],
            0.08,
            7,
            tint,
            2.,
        ),
        "disc" => pen.disc(
            mesh,
            "wall",
            [0.2, 0.3, -0.4],
            0.35,
            [0., 1., 0.],
            7,
            tint,
            2.,
        ),
        "drum" => pen.drum(mesh, "wall", [0.2, 0.3, -0.4], 0.35, 1.2, 7, tint, 2.),
        "patch" | "flipped" => pen.patch(
            mesh,
            "wall",
            &(0..3)
                .map(|r| {
                    (0..4)
                        .map(|c| {
                            let r = r as f64;
                            let c = c as f64;
                            PatchVertex {
                                at: [c * 0.4, 0.03 * c * c + 0.07 * r * r, 1. - r * 0.5],
                                uv: [c / 3., r / 2.],
                                tint: [0.2 + c * 0.1, 0.4 + r * 0.1, 0.7],
                            }
                        })
                        .collect()
                })
                .collect::<Vec<_>>(),
            operation == "flipped",
        ),
        "molding-sharp" | "molding-rounded" | "molding-miter" => profiled_strip(
            pen,
            mesh,
            "wall",
            &Strip {
                span: [-0.3, 1.7],
                section: &[[0., 0.], [0.2, 0.], [0.25, 0.05], [0.25, 0.1], [0., 0.14]],
                omitted_edges: if operation == "molding-miter" {
                    &[1]
                } else {
                    &[]
                },
                miters: if operation == "molding-miter" {
                    [Some(1.), Some(-1.)]
                } else {
                    [None, None]
                },
                setback: if operation == "molding-sharp" {
                    0.
                } else {
                    0.006
                },
            },
            tint,
            2.,
        )
        .map(|_| ()),
        _ => panic!("unknown fixture operation"),
    }
}

#[test]
fn procedural_shapes_wind_outward_keep_one_surface_and_survey_render_corners() {
    for frame_name in ["identity", "reflection", "rotation"] {
        for operation in OPERATIONS {
            check_shape(frame_name, operation);
        }
    }
}

fn check_shape(frame_name: &str, operation: &str) {
    {
        let pen = Pen::new(frame(frame_name), "part").unwrap();
        let mut mesh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
        draw(&pen, &mut mesh, operation, &Tint::Color([0.25, 0.5, 1.])).unwrap();
        let corners: Vec<_> = mesh
            .indices
            .iter()
            .map(|i| mesh.vertices[*i as usize])
            .collect();
        // Structural counts follow from each primitive's facets, not from a capture.
        let count = match operation {
            "box" => Some(12),
            "omitted" => Some(10),
            "sheet" => Some(4),
            "bar" => Some(8),
            "disc" => Some(5),
            "patch" | "flipped" => Some(12),
            _ => None,
        };
        if let Some(count) = count {
            assert_eq!(mesh.triangle_count(), count, "{frame_name}/{operation}");
        }
        assert!(mesh.triangle_count() > 0, "{frame_name}/{operation}");
        for v in &corners {
            assert!(
                v.position
                    .iter()
                    .chain(&v.normal)
                    .chain(&v.uv)
                    .chain(&v.color)
                    .all(|x| x.is_finite())
            );
        }
        assert_eq!(mesh.surfaces.len(), 1);
        assert_eq!(mesh.surfaces[0].part, "part");
        assert_eq!(mesh.surfaces[0].slot, "wall");
        for triangle in mesh.indices.as_chunks::<3>().0 {
            let [a, b, c] = triangle.map(|i| mesh.vertices[i as usize]);
            assert!(
                dot(
                    cross(sub(b.position, a.position), sub(c.position, a.position)),
                    a.normal
                ) > 0.,
                "outward winding"
            );
        }
        let mut survey = Mesh::new(Mode::Survey, 10000, &|| false).unwrap();
        draw(
            &pen,
            &mut survey,
            operation,
            &Tint::Field(&|_| Err("survey must not shade".into())),
        )
        .unwrap();
        assert!(
            survey.vertices.is_empty()
                && survey.indices.is_empty()
                && survey.surfaces.is_empty()
                && survey.triangle_surfaces.is_empty()
        );
        assert_eq!(
            survey.survey,
            corners.iter().map(|v| v.position).collect::<Vec<_>>()
        );
    }
}

#[test]
fn rounded_moldings_add_facets_to_the_sharp_profile() {
    let pen = Pen::new(Matrix::default(), "part").unwrap();
    let count = |operation| {
        let mut mesh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
        draw(&pen, &mut mesh, operation, &Tint::Color([1.; 3])).unwrap();
        mesh.triangle_count()
    };
    assert!(count("molding-rounded") > count("molding-sharp"));
}

#[test]
fn local_frames_tint_calls_polygon_sharing_and_signed_zero_are_explicit() {
    assert_eq!(rounded(-1.5, 1.), -1.);
    assert_eq!(rounded(1.5, 1.), 2.);
    // 0.00015 * 10000 is immediately below 1.5 in IEEE arithmetic; it is not a tie.
    assert_eq!(rounded(0.00015, 1e4), 0.0001);
    for scale in [1e5, 1e4, 1e3] {
        assert_eq!(rounded(-0.5 / scale, scale).to_bits(), (-0.0f64).to_bits());
        assert_eq!(rounded(0., scale).to_bits(), 0f64.to_bits());
    }
    let pen = Pen::new(frame("rotation"), "face").unwrap();
    let local = pen
        .moved([2., 3., 4.])
        .unwrap()
        .facing([1., 2., 3.], [0., 1., 0.], [-1., 0., 0.])
        .unwrap();
    assert_eq!(local.world([2., 3., 4.]), [3., -3., 15.]);
    for host in ["identity", "reflection"] {
        let root = Pen::new(frame(host), "host").unwrap();
        let facade = root.facing([0.; 3], [1., 0., 0.], [0., -1., 0.]).unwrap();
        assert_eq!(facade.mirrored, root.mirrored);
        assert_eq!(
            facade
                .moved([1.; 3])
                .unwrap()
                .into_part("sign")
                .unwrap()
                .mirrored,
            root.mirrored
        );
    }
    for bad in [
        Matrix([[2., 0., 0.], [0., 1., 0.], [0., 0., 1.], [0.; 3]]),
        Matrix([[1., 0., 0.], [0.2, 1., 0.], [0., 0., 1.], [0.; 3]]),
    ] {
        assert!(Pen::new(bad, "face").is_err());
    }
    let mut mesh = Mesh::new(Mode::Render, 100, &|| false).unwrap();
    let count = Cell::new(0);
    let tint = |p: V3| {
        let i = count.get();
        count.set(i + 1);
        assert_eq!(p, QUAD[[0, 2, 1, 0, 3, 2][i]]);
        Ok([if i == 3 { -0. } else { 0. }, 0.5, 1.])
    };
    pen.face(
        &mut mesh,
        "paint",
        &QUAD,
        [0., 1., 0.],
        &Tint::Field(&tint),
        2.,
        None,
        None,
    )
    .unwrap();
    assert_eq!(count.get(), 6);
    assert_eq!(
        mesh.vertices.len(),
        5,
        "changed signed-zero tint must prevent anchor reuse"
    );
    assert_eq!(mesh.indices, vec![0, 1, 2, 3, 4, 1]);
    assert_eq!(mesh.vertices[0].color[0].to_bits(), 0f64.to_bits());
    assert_eq!(mesh.vertices[3].color[0].to_bits(), (-0.0f64).to_bits());
    let before = mesh.vertices.len();
    pen.face(
        &mut mesh,
        "paint",
        &QUAD,
        [0., 1., 0.],
        &Tint::Color([1.; 3]),
        2.,
        None,
        None,
    )
    .unwrap();
    assert_eq!(
        mesh.vertices.len() - before,
        4,
        "separate polygons cannot share corners"
    );
    // Two distinct input corners occupy the same point in separate nondegenerate fan pieces.
    let mut points = QUAD.to_vec();
    points.push(QUAD[1]);
    let mut duplicate = Mesh::new(Mode::Render, 10, &|| false).unwrap();
    pen.face(
        &mut duplicate,
        "paint",
        &points,
        [0., 1., 0.],
        &Tint::Color([1.; 3]),
        2.,
        Some(&[[0.; 2]; 5]),
        Some(&[[0., 1., 0.]; 5]),
    )
    .unwrap();
    assert_eq!(
        duplicate.vertices.len(),
        5,
        "source corner identity must survive equal coordinates"
    );
    assert_ne!(duplicate.indices[2], duplicate.indices[7]);
}

#[test]
fn failures_keep_complete_triangles_and_allow_later_emission() {
    let pen = Pen::new(Matrix::default(), "face").unwrap();
    let mut mesh = Mesh::new(Mode::Render, 10, &|| false).unwrap();
    let calls = Cell::new(0);
    let tint = |_| {
        calls.set(calls.get() + 1);
        if calls.get() == 5 {
            Err("pigment failed".into())
        } else {
            Ok([1.; 3])
        }
    };
    assert!(
        pen.face(
            &mut mesh,
            "wall",
            &QUAD,
            [0., 1., 0.],
            &Tint::Field(&tint),
            2.,
            None,
            None
        )
        .unwrap_err()
        .contains("pigment")
    );
    assert_eq!(
        (
            mesh.vertices.len(),
            mesh.indices.len(),
            mesh.triangle_surfaces.len(),
            mesh.surfaces.len()
        ),
        (3, 3, 1, 1)
    );
    pen.face(
        &mut mesh,
        "wall",
        &QUAD[..3],
        [0., 1., 0.],
        &Tint::Color([1.; 3]),
        2.,
        None,
        None,
    )
    .unwrap();
    assert_eq!(mesh.triangle_count(), 2);
    let previous = mesh.vertices.len();
    assert!(
        pen.face(
            &mut mesh,
            "bad",
            &QUAD,
            [0., 1., 0.],
            &Tint::Color([f64::NAN; 3]),
            2.,
            None,
            None
        )
        .is_err()
    );
    assert_eq!(mesh.vertices.len(), previous);
    assert_eq!(mesh.surfaces.len(), 1);
    let mut limited = Mesh::new(Mode::Render, 1, &|| false).unwrap();
    assert!(
        pen.face(
            &mut limited,
            "wall",
            &QUAD,
            [0., 1., 0.],
            &Tint::Color([1.; 3]),
            2.,
            None,
            None
        )
        .unwrap_err()
        .contains("budget")
    );
    assert_eq!(
        (
            limited.vertices.len(),
            limited.indices.len(),
            limited.triangle_surfaces.len()
        ),
        (3, 3, 1)
    );
    let cancelled = Cell::new(true);
    let cancel = || cancelled.get();
    let mut interrupted = Mesh::new(Mode::Render, 10, &cancel).unwrap();
    assert!(
        pen.face(
            &mut interrupted,
            "wall",
            &QUAD,
            [0., 1., 0.],
            &Tint::Color([1.; 3]),
            2.,
            None,
            None
        )
        .unwrap_err()
        .contains("cancel")
    );
    assert!(interrupted.vertices.is_empty() && interrupted.surfaces.is_empty());
    cancelled.set(false);
    pen.face(
        &mut interrupted,
        "wall",
        &QUAD,
        [0., 1., 0.],
        &Tint::Color([1.; 3]),
        2.,
        None,
        None,
    )
    .unwrap();
    assert_eq!(interrupted.triangle_count(), 2);
}

#[test]
fn shape_admission_bounds_allocations_and_preserves_distinct_degeneracy_thresholds() {
    let pen = Pen::new(Matrix::default(), "shape").unwrap();
    for (height, count) in [(0.5e-9, 0), (1e-9, 1), (2e-9, 1)] {
        let mut mesh = Mesh::new(Mode::Render, 10, &|| false).unwrap();
        pen.face(
            &mut mesh,
            "wall",
            &[[0.; 3], [1., 0., 0.], [0., height, 0.]],
            [0., 0., 1.],
            &Tint::Color([1.; 3]),
            1.,
            None,
            None,
        )
        .unwrap();
        assert_eq!(mesh.triangle_count(), count);
    }
    for (height, count) in [(0.5e-12, 0), (1e-12, 2), (2e-12, 2)] {
        let mut mesh = Mesh::new(Mode::Render, 10, &|| false).unwrap();
        let p = |x, y| PatchVertex {
            at: [x, y, 0.],
            uv: [x, y],
            tint: [1.; 3],
        };
        pen.patch(
            &mut mesh,
            "wall",
            &[
                vec![p(0., 0.), p(1., 0.)],
                vec![p(0., height), p(1., height)],
            ],
            false,
        )
        .unwrap();
        assert_eq!(mesh.triangle_count(), count);
    }
    let mut mesh = Mesh::new(Mode::Render, 10, &|| false).unwrap();
    let tint = Tint::Color([1.; 3]);
    for n in [0, 2, 4097, usize::MAX] {
        assert!(
            pen.disc(&mut mesh, "wall", [0.; 3], 1., [0., 1., 0.], n, &tint, 1.)
                .is_err()
        );
    }
    assert!(
        pen.bar(&mut mesh, "wall", [0.; 3], [0.; 3], 1., &tint, 1.)
            .is_err()
    );
    assert!(
        pen.cuboid(&mut mesh, "wall", [0.; 3], [1., -1., 1.], &tint, 1., "")
            .is_err()
    );
    assert!(
        pen.face(
            &mut mesh,
            "wall",
            &QUAD,
            [0., 1., 0.],
            &tint,
            1.,
            Some(&[[0.; 2]]),
            None
        )
        .is_err()
    );
    assert!(mesh.vertices.is_empty() && mesh.surfaces.is_empty());
}

#[test]
fn cancellation_and_work_limits_apply_even_when_no_triangles_are_emitted() {
    let pen = Pen::new(Matrix::default(), "shape").unwrap();
    let stop = Cell::new(false);
    let cancelled = || stop.get();
    let mut mesh = Mesh::new(Mode::Render, 100, &cancelled).unwrap();
    let calls = Cell::new(0);
    let tint = |_| {
        calls.set(calls.get() + 1);
        if calls.get() == 6 {
            stop.set(true);
        }
        Ok([1.; 3])
    };
    assert!(
        pen.tube(
            &mut mesh,
            "pipe",
            &[[0.; 3], [0., 0., 1.]],
            0.1,
            8,
            &Tint::Field(&tint),
            1.
        )
        .unwrap_err()
        .contains("cancel")
    );
    assert_eq!(
        mesh.triangle_count(),
        2,
        "retain exactly the first complete tube facet"
    );
    let prefix = mesh.indices.clone();
    stop.set(false);
    pen.panel(
        &mut mesh,
        "pipe",
        [0., 1., 0., 1.],
        0.,
        &Tint::Color([1.; 3]),
        1.,
    )
    .unwrap();
    assert_eq!(&mesh.indices[..prefix.len()], prefix);
    assert_eq!(mesh.triangle_count(), 4);
    let mut tiny = Mesh::new(Mode::Render, 10, &cancelled).unwrap();
    let path = vec![[0.; 3]; 65536];
    assert!(
        pen.tube(
            &mut tiny,
            "pipe",
            &path,
            1e-12,
            4096,
            &Tint::Color([1.; 3]),
            1.
        )
        .unwrap_err()
        .contains("work budget")
    );
    stop.set(true);
    assert!(
        pen.face(
            &mut tiny,
            "pipe",
            &[[0.; 3]; 3],
            [0., 1., 0.],
            &Tint::Color([1.; 3]),
            1.,
            None,
            None
        )
        .unwrap_err()
        .contains("cancel")
    );
    assert!(tiny.vertices.is_empty());
}

#[test]
fn surface_groups_are_reused_and_identity_storage_is_bounded_atomically() {
    let mut mesh = Mesh::new(Mode::Render, 1000, &|| false).unwrap();
    for (part, slot) in [
        ("a", "metal"),
        ("a", "paint"),
        ("b", "metal"),
        ("a", "metal"),
    ] {
        Pen::new(Matrix::default(), part)
            .unwrap()
            .face(
                &mut mesh,
                slot,
                &QUAD[..3],
                [0., 1., 0.],
                &Tint::Color([1.; 3]),
                1.,
                None,
                None,
            )
            .unwrap();
    }
    assert_eq!(mesh.surfaces.len(), 3);
    assert_eq!(mesh.triangle_surfaces, [0, 1, 2, 0]);
    let long = "s".repeat(4000);
    let mut rejected = false;
    for i in 0..1000 {
        let pen = Pen::new(Matrix::default(), &format!("{i}{long}")).unwrap();
        let before = (
            mesh.vertices.len(),
            mesh.indices.len(),
            mesh.triangle_surfaces.len(),
            mesh.surfaces.len(),
        );
        match pen.face(
            &mut mesh,
            &long,
            &QUAD[..3],
            [0., 1., 0.],
            &Tint::Color([1.; 3]),
            1.,
            None,
            None,
        ) {
            Ok(()) => (),
            Err(error) => {
                assert!(error.contains("identity budget"), "{error}");
                assert_eq!(
                    (
                        mesh.vertices.len(),
                        mesh.indices.len(),
                        mesh.triangle_surfaces.len(),
                        mesh.surfaces.len()
                    ),
                    before
                );
                rejected = true;
                break;
            }
        }
    }
    assert!(rejected);
    Pen::new(Matrix::default(), "a")
        .unwrap()
        .face(
            &mut mesh,
            "metal",
            &QUAD[..3],
            [0., 1., 0.],
            &Tint::Color([1.; 3]),
            1.,
            None,
            None,
        )
        .unwrap();
    assert_eq!(
        *mesh.triangle_surfaces.last().unwrap(),
        0,
        "existing surface remains usable after identity budget rejection"
    );
}

#[test]
fn molded_receivers_retain_trimmed_edge_identities_and_joined_envelopes() {
    let pen = Pen::new(Matrix::default(), "sill").unwrap();
    let tint = Tint::Color([1.; 3]);
    let section = [[0., 0.], [0.2, 0.], [0.2, 0.1], [0., 0.1]];
    let mut strip = Strip {
        span: [0., 1.],
        section: &section,
        omitted_edges: &[0],
        miters: [Some(1.), Some(-1.)],
        setback: 0.01,
    };
    let mut mesh = Mesh::new(Mode::Render, 100, &|| false).unwrap();
    let faces = profiled_strip(&pen, &mut mesh, "concrete", &strip, &tint, 2.).unwrap();
    assert_eq!(faces.len(), 4);
    assert_eq!(faces[0].a, [0.01, 0.]);
    assert_eq!(faces[0].b, [0.19, 0.]);
    assert_eq!(
        mesh.triangle_count(),
        22,
        "12 side edges minus one omitted original face, with joined caps omitted"
    );
    for vertex in &mesh.vertices {
        let [x, y, z] = vertex.position;
        assert!((0. ..=0.2).contains(&y) && (0. ..=0.1).contains(&z));
        assert!(x >= y - 1e-5 && x <= 1. - y + 1e-5);
    }
    let before = mesh.indices.len();
    strip.miters = [Some(10.), Some(-10.)];
    assert!(profiled_strip(&pen, &mut mesh, "concrete", &strip, &tint, 2.).is_err());
    assert_eq!(mesh.indices.len(), before);
    for invalid in [
        vec![[0., 0.], [1., 0.], [0.4, 0.2], [1., 1.], [0., 1.]],
        vec![[0., 0.], [1., 0.], [1., 0.], [0., 1.]],
        vec![
            [0., 0.],
            [1., 0.],
            [1., 1.],
            [0., 1.],
            [0., 0.],
            [1., 0.],
            [1., 1.],
            [0., 1.],
        ],
        vec![
            [0., 1.],
            [0.588, -0.809],
            [-0.951, 0.309],
            [0.951, 0.309],
            [-0.588, -0.809],
        ],
    ] {
        let strip = Strip {
            span: [0., 1.],
            section: &invalid,
            omitted_edges: &[],
            miters: [None, None],
            setback: 0.,
        };
        assert!(profiled_strip(&pen, &mut mesh, "concrete", &strip, &tint, 2.).is_err());
        assert_eq!(mesh.indices.len(), before);
    }
    for omitted in [vec![0, 0], vec![0; 257]] {
        let strip = Strip {
            span: [0., 1.],
            section: &section,
            omitted_edges: &omitted,
            miters: [None, None],
            setback: 0.,
        };
        assert!(profiled_strip(&pen, &mut mesh, "concrete", &strip, &tint, 2.).is_err());
        assert_eq!(mesh.indices.len(), before);
    }
    let clockwise = section.into_iter().rev().collect::<Vec<_>>();
    let strip = Strip {
        span: [0., 1.],
        section: &clockwise,
        omitted_edges: &[],
        miters: [Some(1.), None],
        setback: 0.,
    };
    let mut reversed = Mesh::new(Mode::Render, 100, &|| false).unwrap();
    profiled_strip(&pen, &mut reversed, "concrete", &strip, &tint, 2.).unwrap();
    assert_eq!(
        reversed.triangle_count(),
        10,
        "eight side triangles and one retained two-triangle cap"
    );
    let cap = reversed
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .filter(|triangle| {
            triangle
                .iter()
                .all(|i| reversed.vertices[*i as usize].normal == [1., 0., 0.])
        })
        .count();
    assert_eq!(cap, 2);
}
