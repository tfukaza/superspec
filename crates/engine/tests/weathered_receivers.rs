use serde_json::{Value, json};
use std::collections::BTreeMap;
use superspec_aml::material::Material;
use superspec_engine::{
    materialspec::surface::{Inputs, Layout, RasterSettings},
    worldspec::themes::kowloon::{
        mesh::{Mesh, Mode, RasterBinding},
        metal::{Binding, Kind, Metal},
        pen::{Pen, Tint},
        weathered::{self, Mapping, Settings},
    },
};
use superspec_geometry::Matrix;

fn settings() -> Settings {
    Settings {
        seed: "金属🌿".into(),
        wear: 0.25,
        grime: 0.3,
        rust: 0.2,
        texture_density: 128.,
        metal: Binding {
            material: "steel".into(),
            repeat: 2.,
        },
        sheet: Binding {
            material: "steel".into(),
            repeat: 2.,
        },
        paint: Binding {
            material: "paint".into(),
            repeat: 3.,
        },
    }
}
fn mapping() -> Mapping {
    Mapping {
        bounds: [0., 0., 1., 1.],
        axes: [[1., 0., 0.], [0., 0., 1.]],
        offset: [0., 0.],
        gravity: [0., -1.],
        paint: [0.3, 0.5, 0.7],
        key: "fold".into(),
    }
}
fn draw(pen: &Pen, mesh: &mut Mesh<'_>, shape: &str) {
    let color = Tint::Color([0.3, 0.5, 0.7]);
    match shape {
        "shutter" => superspec_engine::worldspec::themes::kowloon::shutters::roller_shutter(
            pen,
            mesh,
            [0., 1.2, 0.3, 1.65],
            [0.3, 0.5, 0.7],
            &settings(),
        )
        .unwrap(),
        "bar" | "compact-bar" => pen
            .bar(
                mesh,
                "metal",
                [0.; 3],
                [0., 0., 1.],
                if shape == "bar" { 0.08 } else { 0.025 },
                &color,
                2.,
            )
            .unwrap(),
        "box" | "compact-box" => pen
            .cuboid(
                mesh,
                "metal",
                [0.; 3],
                if shape == "box" {
                    [0.02, 0.2, 1.]
                } else {
                    [0.03, 0.04, 1.]
                },
                &color,
                2.,
                "b",
            )
            .unwrap(),
        "bevel" | "compact-bevel" => pen
            .beveled_bar(
                mesh,
                "metal",
                [0.; 3],
                [0., 0., 1.],
                [1., 0., 0.],
                if shape == "bevel" { 0.12 } else { 0.03 },
                0.04,
                0.0015,
                &color,
                2.,
            )
            .unwrap(),
        "pipe" | "wide-tube" | "compact-tube" => pen
            .tube(
                mesh,
                "metal",
                &[[0.; 3], [0., 0., 1.]],
                if shape == "wide-tube" { 0.1 } else { 0.02 },
                12,
                &color,
                2.,
            )
            .unwrap(),
        "joints" => {
            pen.bar(mesh, "metal", [0.; 3], [0., 0., 1.], 0.08, &color, 2.)
                .unwrap();
            pen.moved([-0.3, 0., 0.5])
                .unwrap()
                .into_part("cross")
                .unwrap()
                .bar(mesh, "metal", [0.; 3], [0.6, 0., 0.], 0.08, &color, 2.)
                .unwrap();
        }
        "folds" => {
            pen.face(
                mesh,
                "metal",
                &[[0., 0., 0.], [1., 0., 0.], [1., 0.05, 0.5], [0., 0.05, 0.5]],
                [0., 1., -0.1],
                &color,
                2.,
                None,
                None,
            )
            .unwrap();
            pen.moved([0., 0.05, 0.5])
                .unwrap()
                .into_part("upper")
                .unwrap()
                .face(
                    mesh,
                    "metal",
                    &[
                        [0., 0., 0.],
                        [1., 0., 0.],
                        [1., -0.05, 0.5],
                        [0., -0.05, 0.5],
                    ],
                    [0., 1., 0.1],
                    &color,
                    2.,
                    None,
                    None,
                )
                .unwrap();
        }
        "bypass" => {
            pen.bar(
                mesh,
                "metal",
                [0.; 3],
                [0., 0., 1.],
                0.02,
                &Tint::Field(&|_| Ok([0.3, 0.5, 0.7])),
                2.,
            )
            .unwrap();
            pen.bar(mesh, "sheet", [1., 0., 0.], [1., 0., 1.], 0.02, &color, 2.)
                .unwrap();
        }
        _ => panic!("Unknown reference shape {shape}"),
    }
}
const SHAPES: [&str; 13] = [
    "shutter",
    "bar",
    "compact-bar",
    "box",
    "compact-box",
    "bevel",
    "compact-bevel",
    "pipe",
    "wide-tube",
    "compact-tube",
    "joints",
    "folds",
    "bypass",
];
fn weathered_mesh<'a>(mesh: &mut Mesh<'a>, shape: &str, reflected: bool) {
    let matrix = Matrix([
        [if reflected { -1. } else { 1. }, 0., 0.],
        [0., 1., 0.],
        [0., 0., 1.],
        [0.; 3],
    ]);
    let base = Pen::new(matrix, "assembly").unwrap();
    let pen = base
        .with_weathering(
            mesh,
            settings(),
            if shape == "pipe" {
                Kind::Pipe
            } else if shape == "folds" {
                Kind::Sheet
            } else {
                Kind::Frame
            },
            if shape == "folds" {
                Some(mapping())
            } else {
                None
            },
            vec!["metal".into(), "paint".into()],
        )
        .unwrap();
    draw(&pen, mesh, shape);
}

#[test]
fn field_tints_and_unselected_slots_keep_their_authored_slots() {
    let mut mesh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
    weathered_mesh(&mut mesh, "bypass", false);
    // Each bar is eight triangles: the field-tinted metal bar, then the unselected sheet bar.
    assert_eq!(mesh.triangle_count(), 16);
    for (i, surface) in mesh.triangle_surfaces.iter().enumerate() {
        assert_eq!(
            mesh.surfaces[*surface].slot,
            if i < 8 { "metal" } else { "sheet" }
        );
    }
}

#[test]
fn weathered_shapes_plan_disjoint_receivers_with_sampleable_metal() {
    for shape in SHAPES {
        for reflected in [false, true] {
            let mut mesh = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
            weathered_mesh(&mut mesh, shape, reflected);
            let triangles = mesh.triangle_count();
            let indices = mesh.indices.clone();
            let positions: Vec<_> = mesh.vertices.iter().map(|v| v.position).collect();
            let plan = weathered::finish(&mut mesh, &|| false).unwrap();
            let mut covered = vec![false; triangles];
            for r in plan.receivers() {
                for t in r.first_triangle..r.first_triangle + r.triangle_count {
                    assert!(!covered[t], "{shape}: triangle {t} received twice");
                    covered[t] = true;
                }
                let job = &plan.jobs()[r.job];
                let settings = job.metal_settings().unwrap();
                assert!(job.density > 0.);
                let metal = Metal::new(settings, None).unwrap();
                let [u0, v0, u1, v1] = settings.bounds;
                assert!(u0 <= u1 && v0 <= v1, "{shape}");
                for (fu, fv) in [(0., 0.), (0.5, 0.5), (1., 1.)] {
                    let sample = metal
                        .sample(u0 + (u1 - u0) * fu, v0 + (v1 - v0) * fv)
                        .unwrap();
                    assert!(!sample.layers.is_empty(), "{shape}");
                }
            }
            if shape != "bypass" && !shape.starts_with("compact") {
                assert!(!plan.receivers().is_empty(), "{shape}");
            }
            drop(plan);
            assert_eq!(mesh.indices, indices, "{shape}: planning keeps topology");
            for (v, p) in mesh.vertices.iter().zip(&positions) {
                assert_eq!(v.position, *p);
            }
        }
    }
}

#[test]
fn continuous_fold_bakes_share_one_job_and_keep_geometry() {
    let mut mesh = Mesh::new(Mode::Render, 100, &|| false).unwrap();
    let base = Pen::new(Matrix::default(), "assembly").unwrap();
    let pen = base
        .with_weathering(
            &mut mesh,
            settings(),
            Kind::Sheet,
            Some(mapping()),
            vec!["metal".into()],
        )
        .unwrap();
    draw(&pen, &mut mesh, "folds");
    let before = mesh.vertices.clone();
    let indices = mesh.indices.clone();
    let plan = weathered::finish(&mut mesh, &|| false).unwrap();
    assert_eq!(plan.jobs().len(), 1);
    assert_eq!(plan.receivers().len(), 2);
    assert_eq!(plan.jobs()[0].metal_settings().unwrap().folds.len(), 1);
    let materials = BTreeMap::from([
        (
            "steel".into(),
            Material {
                id: "steel".into(),
                color: "#b0bac0".into(),
                roughness: 0.5,
                metalness: 0.8,
                ..Material::default()
            },
        ),
        (
            "paint".into(),
            Material {
                id: "paint".into(),
                ..Material::default()
            },
        ),
    ]);
    let bakes = plan
        .bake(
            &Inputs {
                materials: &materials,
                images: &BTreeMap::new(),
            },
            "fold",
            &|| false,
        )
        .unwrap();
    assert_eq!(bakes.len(), 1);
    assert_eq!(mesh.indices, indices);
    for (a, b) in mesh.vertices.iter().zip(&before) {
        assert_eq!(a.position, b.position);
        assert_eq!(a.normal, b.normal);
        assert_eq!(a.color, b.color);
        assert_eq!(a.uv, bakes[0].layout().uv(b.uv));
    }
    assert!(
        mesh.surfaces
            .iter()
            .all(|s| s.slot == bakes[0].material().id)
    );
    let seam: Vec<_> = mesh
        .vertices
        .iter()
        .filter(|v| v.position[2] == 0.5)
        .collect();
    assert_eq!(seam.len(), 4);
    for a in &seam {
        for b in &seam {
            if a.position == b.position {
                assert_eq!(a.uv, b.uv);
            }
        }
    }
    assert!(
        pen.bar(
            &mut mesh,
            "metal",
            [0.; 3],
            [0., 0., 1.],
            0.1,
            &Tint::Color([1.; 3]),
            1.
        )
        .is_err()
    );
}

#[test]
fn batch_binding_is_atomic_and_preserves_unselected_slots() {
    let mut mesh = Mesh::new(Mode::Render, 100, &|| false).unwrap();
    let pen = Pen::new(Matrix::default(), "panel").unwrap();
    for i in 0..3 {
        pen.moved([i as f64, 0., 0.])
            .unwrap()
            .into_part(if i == 2 { "other" } else { "panel" })
            .unwrap()
            .panel(
                &mut mesh,
                "metal",
                [0., 1., 0., 1.],
                0.,
                &Tint::Color([1.; 3]),
                1.,
            )
            .unwrap();
    }
    let layout = Layout::new(RasterSettings {
        bounds: [0., 0., 1., 1.],
        texels_per_meter: 8.,
        preserve: true,
        repeat: false,
        uv_coordinates: true,
        bump: true,
    })
    .unwrap();
    let before = mesh.vertices.iter().map(|v| v.uv).collect::<Vec<_>>();
    let bad = [
        RasterBinding {
            first_triangle: 0,
            triangle_count: 2,
            slot: "bake",
            layout: &layout,
        },
        RasterBinding {
            first_triangle: 1,
            triangle_count: 2,
            slot: "bake",
            layout: &layout,
        },
    ];
    assert!(mesh.bind_rasters(&bad).is_err());
    assert_eq!(
        mesh.vertices.iter().map(|v| v.uv).collect::<Vec<_>>(),
        before
    );
    let bindings = [
        RasterBinding {
            first_triangle: 0,
            triangle_count: 2,
            slot: "bake",
            layout: &layout,
        },
        RasterBinding {
            first_triangle: 4,
            triangle_count: 2,
            slot: "bake",
            layout: &layout,
        },
    ];
    let last = mesh.vertices.len() - 1;
    mesh.vertices[last].uv[0] = f64::NAN;
    let invalid = state(&mesh);
    assert!(mesh.bind_rasters(&bindings).is_err());
    assert_eq!(
        state(&mesh),
        invalid,
        "Late invalid attributes cannot commit earlier ranges"
    );
    mesh.vertices[last].uv = before[last];
    let previous = mesh.indices[6];
    mesh.indices[6] = mesh.indices[0];
    let shared = state(&mesh);
    assert!(mesh.bind_rasters(&bindings).is_err());
    assert_eq!(
        state(&mesh),
        shared,
        "Selected and unselected ranges cannot share vertices"
    );
    mesh.indices[6] = previous;
    mesh.bind_rasters(&bindings).unwrap();
    assert_eq!(mesh.surfaces.len(), 3);
    for (t, tri) in mesh.indices.as_chunks::<3>().0.iter().enumerate() {
        let surface = &mesh.surfaces[mesh.triangle_surfaces[t]];
        assert_eq!(
            surface.slot,
            if (2..4).contains(&t) { "metal" } else { "bake" }
        );
        assert_eq!(surface.part, if t >= 4 { "other" } else { "panel" });
        for i in tri {
            assert_eq!(
                mesh.vertices[*i as usize].uv,
                if (2..4).contains(&t) {
                    before[*i as usize]
                } else {
                    layout.uv(before[*i as usize])
                }
            );
        }
    }
    pen.panel(
        &mut mesh,
        "bake",
        [0., 1., 0., 1.],
        2.,
        &Tint::Color([1.; 3]),
        1.,
    )
    .unwrap();
    assert_eq!(
        mesh.surfaces.len(),
        3,
        "Future geometry uses rebuilt ownership lookup"
    );
}

fn state(mesh: &Mesh<'_>) -> Value {
    json!({"vertices":mesh.vertices.iter().map(|v|v.position.into_iter().chain(v.normal).chain(v.uv).chain(v.color).map(f64::to_bits).collect::<Vec<_>>()).collect::<Vec<_>>(),"indices":mesh.indices,"surfaces":mesh.surfaces.iter().map(|s|(&s.part,&s.slot)).collect::<Vec<_>>(),"owners":mesh.triangle_surfaces})
}

#[test]
fn sharing_respects_material_identity_and_world_gravity() {
    let mut mesh = Mesh::new(Mode::Render, 100, &|| false).unwrap();
    let frame = Matrix([[1., 0., 0.], [0., 0., 1.], [0., -1., 0.], [0.; 3]]);
    let base = Pen::new(frame, "tilted").unwrap();
    let mut settings = settings();
    settings.sheet.material = "different-sheet".into();
    let pen = base
        .with_weathering(
            &mut mesh,
            settings,
            Kind::Sheet,
            None,
            vec!["metal".into(), "sheet".into()],
        )
        .unwrap();
    for slot in ["metal", "metal", "sheet"] {
        pen.face(
            &mut mesh,
            slot,
            &[[0., 0., 0.], [1., 0., 0.], [1., 2., 0.], [0., 2., 0.]],
            [0., 0., 1.],
            &Tint::Color([1.; 3]),
            1.,
            None,
            None,
        )
        .unwrap();
    }
    let plan = weathered::finish(&mut mesh, &|| false).unwrap();
    assert_eq!(plan.jobs().len(), 2);
    assert_eq!(plan.receivers()[0].job, plan.receivers()[1].job);
    assert_ne!(plan.receivers()[0].job, plan.receivers()[2].job);
    assert_eq!(
        plan.jobs()[0]
            .metal_settings()
            .unwrap()
            .metric
            .as_ref()
            .unwrap()
            .gravity,
        [0., -1.]
    );
    assert_eq!(
        plan.jobs()[1].metal_settings().unwrap().substrate.material,
        "different-sheet"
    );
}

#[test]
fn nested_finish_changes_restrict_slots_and_keep_parent_contacts() {
    let mut mesh = Mesh::new(Mode::Render, 100, &|| false).unwrap();
    let frame = Pen::new(Matrix::default(), "assembly")
        .unwrap()
        .with_weathering(
            &mut mesh,
            settings(),
            Kind::Frame,
            None,
            vec!["paint".into(), "metal".into()],
        )
        .unwrap();
    frame
        .bar(
            &mut mesh,
            "metal",
            [0.; 3],
            [0., 0., 1.],
            0.08,
            &Tint::Color([0.3, 0.5, 0.7]),
            2.,
        )
        .unwrap();
    let pipe = frame
        .with_weathering(
            &mut mesh,
            settings(),
            Kind::Pipe,
            None,
            vec!["metal".into()],
        )
        .unwrap();
    pipe.bar(
        &mut mesh,
        "metal",
        [-0.3, 0., 0.5],
        [0.3, 0., 0.5],
        0.03,
        &Tint::Color([0.3, 0.5, 0.7]),
        2.,
    )
    .unwrap();
    let first = mesh.triangle_count();
    pipe.bar(
        &mut mesh,
        "paint",
        [1., 0., 0.],
        [1., 0., 1.],
        0.08,
        &Tint::Color([0.3, 0.5, 0.7]),
        3.,
    )
    .unwrap();
    for tri in &mesh.indices[first * 3..] {
        assert_eq!(mesh.vertices[*tri as usize].color, [0.3, 0.5, 0.7]);
    }
    let plan = weathered::finish(&mut mesh, &|| false).unwrap();
    assert_eq!(plan.receivers().len(), 8);
    assert!(plan.jobs().iter().any(|j| {
        j.metal_settings().unwrap().kind == Kind::Frame
            && !j
                .metal_settings()
                .unwrap()
                .contacts
                .as_ref()
                .unwrap()
                .is_empty()
    }));
    assert!(plan.jobs().iter().any(|j| {
        j.metal_settings().unwrap().kind == Kind::Pipe
            && !j
                .metal_settings()
                .unwrap()
                .contacts
                .as_ref()
                .unwrap()
                .is_empty()
    }));
}

#[test]
fn cancellation_and_raster_budget_leave_geometry_unbound() {
    let mut mesh = Mesh::new(Mode::Render, 100, &|| false).unwrap();
    let base = Pen::new(Matrix::default(), "assembly").unwrap();
    let pen = base
        .with_weathering(
            &mut mesh,
            settings(),
            Kind::Frame,
            None,
            vec!["metal".into()],
        )
        .unwrap();
    draw(&pen, &mut mesh, "bar");
    let before = state(&mesh);
    let plan = weathered::finish(&mut mesh, &|| false).unwrap();
    let materials = BTreeMap::from([
        (
            "steel".into(),
            Material {
                id: "steel".into(),
                ..Material::default()
            },
        ),
        (
            "paint".into(),
            Material {
                id: "paint".into(),
                ..Material::default()
            },
        ),
    ]);
    let calls = std::cell::Cell::new(0);
    assert!(
        plan.bake(
            &Inputs {
                materials: &materials,
                images: &BTreeMap::new()
            },
            "cancel",
            &|| {
                calls.set(calls.get() + 1);
                calls.get() > 3
            }
        )
        .err()
        .unwrap()
        .contains("cancelled")
    );
    assert_eq!(state(&mesh), before);
    let mut large = Mesh::new(Mode::Render, 100, &|| false).unwrap();
    let pen = base
        .with_weathering(
            &mut large,
            settings(),
            Kind::Frame,
            None,
            vec!["metal".into()],
        )
        .unwrap();
    // Sixteen distinct 10 m receivers at 128 texels/m exceed 64 Mi channel-pixels.
    for i in 0..16 {
        pen.panel(
            &mut large,
            "metal",
            [0., 10., 0., 10.],
            0.,
            &Tint::Color([i as f64 / 16., 0.3, 0.5]),
            1.,
        )
        .unwrap();
    }
    let before = state(&large);
    assert!(
        weathered::finish(&mut large, &|| false)
            .err()
            .unwrap()
            .contains("64 Mi channel-pixels")
    );
    assert_eq!(state(&large), before);
}

#[test]
fn survey_bypasses_appearance_and_plans_cannot_cross_drawings() {
    let base = Pen::new(Matrix::default(), "assembly").unwrap();
    let mut mesh = Mesh::new(Mode::Survey, 100, &|| false).unwrap();
    let mut invalid = settings();
    invalid.wear = f64::NAN;
    let pen = base
        .with_weathering(&mut mesh, invalid, Kind::Frame, None, vec![])
        .unwrap();
    draw(&pen, &mut mesh, "compact-bar");
    assert_eq!(mesh.triangle_count(), 8);
    let mut a = Mesh::new(Mode::Render, 100, &|| false).unwrap();
    let mut b = Mesh::new(Mode::Render, 100, &|| false).unwrap();
    let pen = base
        .with_weathering(&mut a, settings(), Kind::Frame, None, vec!["metal".into()])
        .unwrap();
    assert!(
        pen.bar(
            &mut b,
            "metal",
            [0.; 3],
            [0., 0., 1.],
            0.1,
            &Tint::Color([1.; 3]),
            1.
        )
        .is_err()
    );
    assert_eq!(b.triangle_count(), 0);
    draw(&pen, &mut a, "bar");
    let before = a.vertices.iter().map(|v| v.uv).collect::<Vec<_>>();
    let plan = weathered::finish(&mut a, &|| false).unwrap();
    assert!(
        plan.bake(
            &Inputs {
                materials: &BTreeMap::new(),
                images: &BTreeMap::new()
            },
            "missing",
            &|| false
        )
        .is_err()
    );
    assert_eq!(a.vertices.iter().map(|v| v.uv).collect::<Vec<_>>(), before);
    // A fresh detached job remains usable after another job fails.
    let next = base
        .with_weathering(&mut b, settings(), Kind::Frame, None, vec!["metal".into()])
        .unwrap();
    draw(&next, &mut b, "bar");
    assert!(
        !weathered::finish(&mut b, &|| false)
            .unwrap()
            .jobs()
            .is_empty()
    );
}
