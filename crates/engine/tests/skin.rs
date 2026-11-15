use serde::Deserialize;
use std::collections::BTreeMap;
use superspec_aml::material::Material;
use superspec_engine::{
    materialspec::surface::{Inputs, Sample},
    worldspec::themes::kowloon::{
        facade_material::Facade,
        fixture::Fixture,
        mesh::{Mesh, Mode},
        pen::Pen,
        skin::{self, Deposited},
        weathered::{self, Recipe},
    },
};
use superspec_geometry::Matrix;
#[path = "support/skin_scene.rs"]
mod scene_fixture;
#[derive(Deserialize)]
struct Receiver {
    first: usize,
    count: usize,
    point: [f64; 2],
    sample: Sample,
    shared: Option<bool>,
    #[serde(default)]
    samples: Vec<PointSample>,
}
#[derive(Deserialize)]
struct PointSample {
    point: [f64; 2],
    sample: Sample,
}
#[derive(Deserialize)]
struct Stain {
    x0: f64,
    x1: f64,
    top: f64,
    bottom: f64,
    wash: [f64; 3],
    depth: f64,
    offset: f64,
    before: bool,
}
#[derive(Deserialize)]
struct Pixels {
    receiver: usize,
    width: usize,
    height: usize,
    color: Vec<u8>,
    normal: Vec<u8>,
    packed: Vec<u8>,
}
#[derive(Deserialize)]
struct Case {
    #[serde(default)]
    stains: Vec<Stain>,
    settings: skin::Settings,
    reflected: bool,
    indices: Vec<u32>,
    positions: Vec<f64>,
    normals: Vec<f64>,
    uv: Vec<f64>,
    colors: Vec<f64>,
    receivers: Vec<Receiver>,
    rasters: Vec<Pixels>,
    repairs: Vec<superspec_engine::worldspec::themes::kowloon::render_repair::Patch>,
}
#[derive(Deserialize)]
struct Golden {
    materials: Vec<Material>,
    cases: Vec<Case>,
}
fn fixture() -> Golden {
    serde_json::from_str(include_str!("../../../tests/fixtures/rust/skin.json")).unwrap()
}

#[test]
fn authored_tile_mapping_survives_local_slots_and_dependency_only_baking() {
    use superspec_geometry::image::Image;
    let render = |aliased: bool, dependencies_only: bool| {
        let g = fixture();
        let mut case = g
            .cases
            .into_iter()
            .find(|c| c.settings.tiles.is_some())
            .unwrap();
        let mut materials = g
            .materials
            .into_iter()
            .map(|m| (m.id.clone(), m))
            .collect::<BTreeMap<_, _>>();
        let mut bindings = BTreeMap::new();
        for (local, source) in [
            ("local-coat", &mut case.settings.finish.coat.material),
            ("local-backing", &mut case.settings.finish.backing.material),
        ] {
            let original = source.clone();
            let definition = materials.get_mut(&original).unwrap();
            definition.texture = Some("asymmetric.png".into());
            if aliased {
                let mut definition = definition.clone();
                definition.id = local.into();
                materials.insert(local.into(), definition);
                bindings.insert(local.into(), original);
                *source = local.into();
            }
        }
        let images = BTreeMap::from([(
            "asymmetric.png".into(),
            Image {
                width: 2,
                height: 2,
                mime: "image/png",
                rgba: vec![
                    241, 19, 47, 255, 38, 219, 73, 255, 21, 64, 233, 255, 205, 171, 32, 255,
                ],
            },
        )]);
        let mut mesh = Mesh::new(Mode::Render, 200_000, &|| false).unwrap();
        if aliased {
            mesh.bind_source_materials(&bindings).unwrap();
        }
        draw(&mut mesh, &case).unwrap();
        let plan = weathered::finish(&mut mesh, &|| false).unwrap();
        if dependencies_only {
            let required = plan.required_materials().unwrap();
            materials.retain(|id, _| required.contains(id));
        }
        let bakes = plan
            .bake(
                &Inputs {
                    materials: &materials,
                    images: &images,
                },
                "alias-check",
                &|| false,
            )
            .unwrap();
        let mut scene=superspec_engine::compile(r#"<architecture version="1" id="alias"><box id="host" width="1" depth="1" height="1" visible="false"/></architecture>"#,0).unwrap();
        scene.manifest.materials = materials.values().cloned().collect();
        let mut slots = materials
            .keys()
            .map(|id| (id.clone(), id.clone()))
            .collect::<BTreeMap<_, _>>();
        slots.extend(
            bakes
                .iter()
                .map(|b| (b.material().id.clone(), b.material().id.clone())),
        );
        scene
            .append_baked_visual("host", &mesh, &slots, &bakes)
            .unwrap();
        scene.asset_bytes
    };
    let direct = render(false, false);
    assert!(!direct.is_empty());
    assert_eq!(
        direct,
        render(true, false),
        "Authored identities must control seeded tile UVs"
    );
    assert_eq!(
        direct,
        render(true, true),
        "All optional coatings must survive dependency filtering"
    );
}
pub fn check_household_wash_boundaries() {
    use superspec_engine::worldspec::themes::kowloon::tiled_skin::{Field, Recipe};
    let settings = fixture()
        .cases
        .into_iter()
        .find(|c| c.settings.tiles.is_some())
        .unwrap()
        .settings;
    let left = [0.8, 0.4, 0.3];
    let right = [0.3, 0.5, 0.9];
    let wash = |bounds, tint| skin::Wash {
        bounds,
        tint,
        lower_inclusive: true,
    };
    let recipe = Recipe {
        bounds: [0., 0., 2., 2.],
        finish: settings.finish,
        condition: settings.condition,
        tiles: settings.tiles.unwrap(),
        washes: vec![wash([0., 0., 1., 2.], left), wash([1., 0., 2., 2.], right)],
        openings: vec![],
        mapping_seed: 42,
    };
    let field = Field::new(&recipe).unwrap();
    for (point, tint) in [
        ([0., 0.5], Some(left)),
        ([1., 0.5], Some(right)),
        ([1., 0.], Some(right)),
        ([2., 0.5], None),
        ([1., 2.], None),
    ] {
        let mut homogeneous = recipe.clone();
        homogeneous.washes = tint
            .map(|t| wash([-10., -10., 10., 10.], t))
            .into_iter()
            .collect();
        assert_eq!(
            serde_json::json!(field.sample(point[0], point[1]).unwrap()),
            serde_json::json!(
                Field::new(&homogeneous)
                    .unwrap()
                    .sample(point[0], point[1])
                    .unwrap()
            ),
            "wash boundary {point:?}"
        );
    }
    let mut open = recipe.clone();
    for wash in &mut open.washes {
        wash.lower_inclusive = false;
    }
    let mut bare = recipe.clone();
    bare.washes.clear();
    let p = [1., 0.5];
    assert_eq!(
        serde_json::json!(Field::new(&open).unwrap().sample(p[0], p[1]).unwrap()),
        serde_json::json!(Field::new(&bare).unwrap().sample(p[0], p[1]).unwrap())
    );
    assert_ne!(
        serde_json::json!(field.sample(p[0], p[1]).unwrap()),
        serde_json::json!(Field::new(&bare).unwrap().sample(p[0], p[1]).unwrap())
    );
}
#[test]
fn adjacent_household_washes_own_lower_edges_and_leave_upper_edges_open() {
    check_household_wash_boundaries();
}
fn pen(reflected: bool) -> Pen {
    Pen::new(
        if reflected {
            Matrix([[-1., 0., 0.], [0., -1., 0.], [0., 0., -1.], [4., 3., 5.]])
        } else {
            Matrix::default()
        },
        "skin",
    )
    .unwrap()
}
fn draw(mesh: &mut Mesh<'_>, case: &Case) -> Result<(), String> {
    let deposit = |mesh: &mut Mesh<'_>, before| -> Result<(), String> {
        for s in case.stains.iter().filter(|s| s.before == before) {
            superspec_engine::worldspec::themes::kowloon::stains::stain(
                mesh,
                &pen(case.reflected).moved([0., s.offset, 0.])?,
                [s.x0, s.bottom, s.x1, s.top],
                s.wash,
                s.depth,
            )?;
        }
        Ok(())
    };
    deposit(mesh, true)?;
    skin::draw(
        &mut Fixture {
            pen: pen(case.reflected),
            mesh,
            repeats: &|_| 1.,
        },
        "wall",
        &case.settings,
        "skin",
    )?;
    deposit(mesh, false)
}
fn close(actual: &serde_json::Value, expected: &serde_json::Value) {
    use serde_json::Value;
    match (actual, expected) {
        (Value::Number(a), Value::Number(b)) => assert!(
            (a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 2e-10,
            "{a} != {b}"
        ),
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len());
            for (a, b) in a.iter().zip(b) {
                close(a, b);
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            for (k, b) in b {
                close(&a[k], b);
            }
        }
        _ => assert_eq!(actual, expected),
    }
}
pub fn check_reference_skin() {
    check(fixture());
}
pub fn check_reference_stains() {
    check(serde_json::from_str(include_str!("../../../tests/fixtures/rust/stains.json")).unwrap());
}
fn sample(job: &weathered::Job, p: [f64; 2]) -> Sample {
    match &job.recipe {
        Recipe::Facade(r) => Facade::new(r).unwrap().sample(p[0], p[1]),
        Recipe::FacadeDeposits(r) => Deposited::new(r).unwrap().sample(p[0], p[1]),
        Recipe::TiledSkin(r) => {
            superspec_engine::worldspec::themes::kowloon::tiled_skin::Field::new(r)
                .unwrap()
                .sample(p[0], p[1])
        }
        Recipe::Stained(r) => superspec_engine::worldspec::themes::kowloon::stains::Field::new(r)
            .unwrap()
            .sample(p[0], p[1]),
        _ => panic!("Unexpected skin recipe"),
    }
    .unwrap()
}
fn check(golden: Golden) {
    let materials = golden
        .materials
        .into_iter()
        .map(|m| (m.id.clone(), m))
        .collect();
    let inputs = Inputs {
        materials: &materials,
        images: &BTreeMap::new(),
    };
    for (ci, case) in golden.cases.iter().enumerate() {
        if let Some(tiles) = &case.settings.tiles
            && !case.settings.finish.concrete
        {
            let patches = superspec_engine::worldspec::themes::kowloon::render_repair::plan(
                [-0.4, 0., 5.1, 2.1],
                tiles.seed,
                &case
                    .settings
                    .openings
                    .iter()
                    .map(|r| [r.u0, r.v0, r.u1, r.v1])
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            close(
                &serde_json::to_value(patches).unwrap(),
                &serde_json::to_value(&case.repairs).unwrap(),
            );
        }
        let mut mesh = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
        draw(&mut mesh, case).unwrap();
        assert_eq!(mesh.indices.len(), case.indices.len(), "skin {ci} indices");
        assert_eq!(
            mesh.indices
                .iter()
                .zip(&case.indices)
                .position(|(a, b)| a != b),
            None,
            "skin {ci} indexed order"
        );
        for (actual, expected, label) in [
            (
                mesh.vertices
                    .iter()
                    .flat_map(|v| v.position)
                    .collect::<Vec<_>>(),
                &case.positions,
                "position",
            ),
            (
                mesh.vertices.iter().flat_map(|v| v.normal).collect(),
                &case.normals,
                "normal",
            ),
            (
                mesh.vertices.iter().flat_map(|v| v.color).collect(),
                &case.colors,
                "color",
            ),
            (
                mesh.vertices.iter().flat_map(|v| v.uv).collect(),
                &case.uv,
                "uv",
            ),
        ] {
            assert_eq!(actual.len(), expected.len(), "skin {ci} {label}");
            assert_eq!(
                actual
                    .iter()
                    .zip(expected)
                    .position(|(a, b)| (a - b).abs() > 2e-10),
                None,
                "skin {ci} {label}"
            );
        }
        let plan = weathered::finish(&mut mesh, &|| false).unwrap();
        assert_eq!(plan.receivers().len(), case.receivers.len());
        let job_ids: Vec<_> = plan.receivers().iter().map(|r| r.job).collect();
        for (got, want) in plan.receivers().iter().zip(&case.receivers) {
            assert_eq!(
                (got.first_triangle, got.triangle_count),
                (want.first, want.count)
            );
            let job = &plan.jobs()[got.job];
            if let Some(shared) = want.shared {
                assert_eq!(
                    matches!(job.recipe, Recipe::Facade(_)),
                    shared,
                    "case {ci} receiver {} sharing",
                    want.first
                );
                if !shared {
                    for p in &want.samples {
                        close(
                            &serde_json::to_value(sample(job, p.point)).unwrap(),
                            &serde_json::to_value(&p.sample).unwrap(),
                        );
                    }
                }
            }
            close(
                &serde_json::to_value(sample(job, want.point)).unwrap(),
                &serde_json::to_value(&want.sample).unwrap(),
            );
            if case.settings.condition.grime > 0. && case.settings.tiles.is_none() {
                assert_eq!(job.density, case.settings.finish.density.min(128.));
            }
        }
        if case.settings.condition.grime == 0. && case.stains.is_empty() {
            assert_eq!(plan.jobs().len(), 1);
        }
        if !case.rasters.is_empty() {
            let bakes = plan.bake(&inputs, "skin", &|| false).unwrap();
            let mut scene=superspec_engine::compile(r#"<architecture version="1" id="skin"><box id="host" width="6" depth=".1" height="3" visible="false"/></architecture>"#,0).unwrap();
            let bindings = bakes
                .iter()
                .map(|b| (b.material().id.clone(), b.material().id.clone()))
                .collect();
            scene
                .append_baked_visual("host", &mesh, &bindings, &bakes)
                .unwrap();
            for r in &case.rasters {
                let bake = &bakes[job_ids[r.receiver]];
                assert_eq!(
                    (bake.layout().width, bake.layout().height),
                    (r.width, r.height)
                );
                let m = bake.material();
                for (id, expected) in [
                    (m.texture.as_ref().unwrap(), &r.color),
                    (m.normal_texture.as_ref().unwrap(), &r.normal),
                    (m.metallic_roughness_texture.as_ref().unwrap(), &r.packed),
                ] {
                    let a = scene.manifest.assets.iter().find(|a| &a.id == id).unwrap();
                    let decoded = superspec_geometry::image::decode(
                        &scene.asset_bytes[a.byte_start..a.byte_start + a.byte_length],
                    )
                    .unwrap();
                    assert_eq!(decoded.rgba.len(), expected.len());
                    assert_eq!(
                        decoded.rgba.iter().zip(expected).position(|(a, b)| a != b),
                        None,
                        "skin {ci} raster {id}"
                    );
                }
            }
            for (receiver, job) in case.receivers.iter().zip(&job_ids) {
                for t in receiver.first..receiver.first + receiver.count {
                    for &index in &mesh.indices[t * 3..t * 3 + 3] {
                        let vertex = mesh.vertices[index as usize];
                        let local = pen(case.reflected)
                            .matrix
                            .inverse()
                            .unwrap()
                            .point(vertex.position);
                        let expected = bakes[*job].layout().uv([local[0], local[2]]);
                        assert!(
                            vertex
                                .uv
                                .iter()
                                .zip(expected)
                                .all(|(a, b)| (a - b).abs() < 1e-12)
                        );
                    }
                }
            }
        }
        let mut survey = Mesh::new(Mode::Survey, 100000, &|| false).unwrap();
        draw(&mut survey, case).unwrap();
        assert_eq!(survey.triangle_count() * 3, case.indices.len());
    }
}
#[test]
fn full_wall_plans_and_deposit_pixels_match_reference() {
    check_reference_skin();
}
#[test]
fn fixture_stains_preserve_frames_order_repairs_and_unaffected_sharing() {
    check_reference_stains();
}
#[test]
fn stains_reject_invalid_input_limit_work_and_leave_other_planes_reusable() {
    use superspec_engine::worldspec::themes::kowloon::stains::stain;
    let case = fixture().cases.remove(0);
    let pen = pen(false);
    let mut mesh = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
    assert!(stain(&mut mesh, &pen, [0., 0., 1., f64::NAN], [1.; 3], 0.5).is_err());
    assert!(stain(&mut mesh, &pen, [0., 0., 1., 1.], [f64::INFINITY; 3], 0.5).is_err());
    assert!(stain(&mut mesh, &pen, [0., 0., 1., 1.], [1.; 3], -0.1).is_err());
    for plane in [
        pen.moved([0., 0.02, 0.]).unwrap(),
        Pen::new(
            Matrix([[1., 0., 0.], [0., -1., 0.], [0., 0., 1.], [0.; 3]]),
            "skin",
        )
        .unwrap(),
    ] {
        stain(&mut mesh, &plane, [0., 0., 1., 1.], [1.; 3], 0.5).unwrap();
    }
    draw(&mut mesh, &case).unwrap();
    let plan = weathered::finish(&mut mesh, &|| false).unwrap();
    assert_eq!(plan.jobs().len(), 1);
    assert!(matches!(plan.jobs()[0].recipe, Recipe::Facade(_)));
    drop(plan);
    assert!(stain(&mut mesh, &pen, [0., 0., 1., 1.], [1.; 3], 0.5).is_err());
    let mut mesh = Mesh::new(Mode::Render, 100000, &|| true).unwrap();
    assert!(stain(&mut mesh, &pen, [0., 0., 1., 1.], [1.; 3], 0.5).is_err());
    let mut mesh = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
    for _ in 0..257 {
        stain(&mut mesh, &pen, [0., 0., 1., 1.], [1.; 3], 0.5).unwrap();
    }
    draw(&mut mesh, &case).unwrap();
    let before = mesh.vertices.iter().map(|v| v.uv).collect::<Vec<_>>();
    assert!(weathered::finish(&mut mesh, &|| false).is_err());
    assert_eq!(
        before,
        mesh.vertices.iter().map(|v| v.uv).collect::<Vec<_>>()
    );
    for tiled in [false, true] {
        assert_eq!(
            scene_fixture::stained_scene(tiled).manifest.colliders.len(),
            1
        );
    }
}
#[test]
fn complete_baked_skin_keeps_collision_separate() {
    let scene = scene_fixture::scene(false);
    assert_eq!(scene.manifest.assets.len(), 12);
    assert_eq!(scene.manifest.colliders.len(), 1);
    assert_eq!(
        scene.manifest.meshes.iter().filter(|m| m.visible).count(),
        4
    );
    assert!(!scene.packet().is_empty());
}
#[test]
fn tiled_wall_bakes_one_continuous_field_across_panels_and_washes() {
    let scene = scene_fixture::scene(true);
    assert_eq!(scene.manifest.assets.len(), 3);
    assert_eq!(scene.manifest.colliders.len(), 1);
    assert_eq!(
        scene.manifest.meshes.iter().filter(|m| m.visible).count(),
        1
    );
}
#[test]
fn invalid_work_and_exposure_are_rejected_before_emission_and_cancellation_recovers() {
    let mut case = fixture().cases.remove(1);
    for error in 0..5 {
        let original = case.settings.clone();
        match error {
            0 => case.settings.rects[0].u1 = 10000.,
            1 => case.settings.finish.density = 0.,
            2 => case.settings.condition.ledges[0].u0 = f64::INFINITY,
            3 => case.settings.u_marks.clear(),
            _ => case.settings.washes[0].tint[0] = f64::NAN,
        }
        let mut mesh = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
        assert!(draw(&mut mesh, &case).is_err());
        assert!(mesh.indices.is_empty());
        case.settings = original;
    }
    let mut mesh = Mesh::new(Mode::Render, 100000, &|| true).unwrap();
    assert!(draw(&mut mesh, &case).is_err());
    assert!(mesh.indices.is_empty());
    let mut mesh = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
    draw(&mut mesh, &case).unwrap();
    let before: Vec<_> = mesh.vertices.iter().map(|v| v.uv).collect();
    assert!(weathered::finish(&mut mesh, &|| true).is_err());
    assert_eq!(
        before,
        mesh.vertices.iter().map(|v| v.uv).collect::<Vec<_>>()
    );
    // Finalization consumes a detached drawing even on cancellation; the next revision owns a new one.
    let mut mesh = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
    draw(&mut mesh, &case).unwrap();
    assert!(
        !weathered::finish(&mut mesh, &|| false)
            .unwrap()
            .jobs()
            .is_empty()
    );
}
