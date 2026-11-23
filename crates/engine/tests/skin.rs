use serde_json::json;
use std::collections::BTreeMap;
use superspec_aml::material::Material;
use superspec_engine::{
    materialspec::surface::Inputs,
    worldspec::themes::kowloon::{
        fixture::Fixture,
        mesh::{Mesh, Mode},
        pen::Pen,
        skin,
        weathered::{self, Recipe},
    },
};
use superspec_geometry::Matrix;
#[path = "support/skin_scene.rs"]
mod scene_fixture;
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
struct Case {
    stains: Vec<Stain>,
    settings: skin::Settings,
    reflected: bool,
}
/// A two-storey wall with one window, two ledges and a ground-floor wash.
fn settings(grime: f64, washed: bool, tiled: bool) -> skin::Settings {
    let mut value = json!({
        "rects":[{"u0":-0.4,"u1":5.1,"v0":0,"v1":2.1}],
        "openings":[{"u0":1,"u1":2,"v0":0.65,"v1":1.65}],
        "u_marks":[1,2,2.8,4.7995],"z_marks":[0.65,1.65],
        "finish":{"concrete":false,"coat":{"material":"wall","repeat":1.3,"tint":[0.7,0.9,0.8]},
            "backing":{"material":"concrete","repeat":2.7},"wear":0.65,"density":32,"variant":1},
        "condition":{"base":0,"ledges":[{"u0":-0.4,"u1":5.1,"z":2.1,"flow":0.65},
            {"u0":1,"u1":2,"z":0.65,"flow":1}],
            "openings":[{"u0":1,"u1":2,"z0":0.65,"z1":1.65}],
            "grime":grime,"wear":0.65,"seed":123},
        "washes":[]
    });
    if washed {
        value["washes"] = json!([{"bounds":[2.8,0,5.1,2.1],"tint":[0.9,0.6,0.5]}]);
    }
    if tiled {
        value["tiles"] = json!({"seed":123,
            "field":{"seed":22,"wear":0.7,"grime":0.6,"size":0.18,"aspect":1,"share":1},
            "materials":{"ceramic":"$kowloon/wall-ceramic","bed":"$kowloon/wall-tile-bed",
                "grime":"$kowloon/wall-tile-grime","repeat":1,"bedRepeat":2.4,"bedRoughness":0.9,"roughness":0.65},
            "repair":{"material":"render-repair","repeat":1}});
    }
    serde_json::from_value(value).unwrap()
}
fn case(settings: skin::Settings) -> Case {
    Case {
        stains: vec![],
        settings,
        reflected: false,
    }
}
fn stains() -> Vec<Stain> {
    vec![
        Stain {
            x0: 0.2,
            x1: 0.8,
            top: 1.8,
            bottom: 0.2,
            wash: [0.7, 0.8, 0.9],
            depth: 0.6,
            offset: 0.,
            before: true,
        },
        Stain {
            x0: 0.5,
            x1: 1.3,
            top: 1.7,
            bottom: 0.1,
            wash: [0.9, 0.2, 0.1],
            depth: 0.8,
            offset: 0.,
            before: false,
        },
        Stain {
            x0: 2.4,
            x1: 3.4,
            top: 2.,
            bottom: 0.1,
            wash: [0.1, 0.2, 0.9],
            depth: 1.,
            offset: 0.02,
            before: false,
        },
    ]
}
fn materials() -> BTreeMap<String, Material> {
    serde_json::from_value::<Vec<Material>>(json!([
        {"id":"wall","color":"#caaf92","repeat":1.3,"roughness":0.8,"metalness":0},
        {"id":"concrete","color":"#807a70","repeat":2.7,"roughness":0.8,"metalness":0},
        {"id":"render-repair","color":"#aaaaaa","repeat":1,"roughness":0.8,"metalness":0},
        {"id":"$kowloon/wall-ceramic","color":"#b4c4a2","roughness":0.65,"metalness":0,"repeat":1},
        {"id":"$kowloon/wall-tile-bed","color":"#737373","roughness":0.9,"metalness":0,"repeat":2.4},
        {"id":"$kowloon/wall-tile-grime","color":"#725139","roughness":1,"metalness":0,"repeat":1}
    ]))
    .unwrap()
    .into_iter()
    .map(|m| (m.id.clone(), m))
    .collect()
}

#[test]
fn authored_tile_mapping_survives_local_slots_and_dependency_only_baking() {
    use superspec_geometry::image::Image;
    let render = |aliased: bool, dependencies_only: bool| {
        let mut case = case(settings(0.6, true, true));
        let mut materials = materials();
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
#[test]
fn adjacent_household_washes_own_lower_edges_and_leave_upper_edges_open() {
    use superspec_engine::worldspec::themes::kowloon::tiled_skin::{Field, Recipe};
    let settings = settings(0.6, true, true);
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
            json!(field.sample(point[0], point[1]).unwrap()),
            json!(
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
        json!(Field::new(&open).unwrap().sample(p[0], p[1]).unwrap()),
        json!(Field::new(&bare).unwrap().sample(p[0], p[1]).unwrap())
    );
    assert_ne!(
        json!(field.sample(p[0], p[1]).unwrap()),
        json!(Field::new(&bare).unwrap().sample(p[0], p[1]).unwrap())
    );
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
fn variants() -> Vec<Case> {
    let mut cases = vec![];
    for stained in [false, true] {
        for (grime, washed, tiled, concrete, reflected, density) in [
            (0., false, false, false, false, 32.),
            (0.6, true, false, false, false, 32.),
            (0.6, false, false, true, false, 32.),
            (0.6, false, false, false, true, 32.),
            (0.6, false, false, false, false, 180.),
            (0.6, true, true, false, false, 32.),
            (0.6, false, true, true, false, 32.),
            (0.6, true, true, false, true, 32.),
        ] {
            let mut settings = settings(grime, washed, tiled);
            settings.finish.concrete = concrete;
            settings.finish.density = density;
            cases.push(Case {
                stains: if stained { stains() } else { vec![] },
                settings,
                reflected,
            });
        }
    }
    cases
}
#[test]
fn wall_plans_follow_finish_rules_and_surveys_match_render() {
    let materials = materials();
    let inputs = Inputs {
        materials: &materials,
        images: &BTreeMap::new(),
    };
    for (ci, case) in variants().iter().enumerate() {
        let mut mesh = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
        draw(&mut mesh, case).unwrap();
        let mut survey = Mesh::new(Mode::Survey, 100000, &|| false).unwrap();
        draw(&mut survey, case).unwrap();
        assert_eq!(survey.triangle_count(), mesh.triangle_count(), "case {ci}");
        let plan = weathered::finish(&mut mesh, &|| false).unwrap();
        assert!(!plan.receivers().is_empty());
        for r in plan.receivers() {
            let job = &plan.jobs()[r.job];
            if case.settings.condition.grime > 0. && case.settings.tiles.is_none() {
                assert_eq!(job.density, case.settings.finish.density.min(128.));
            }
        }
        if case.settings.condition.grime == 0. {
            let facade = plan
                .receivers()
                .iter()
                .filter(|r| matches!(plan.jobs()[r.job].recipe, Recipe::Facade(_)))
                .count();
            if case.stains.is_empty() {
                assert_eq!(plan.jobs().len(), 1, "case {ci}");
                assert_eq!(facade, plan.receivers().len());
            } else {
                // Stains detach only the receivers they touch from the shared facade.
                assert!(facade > 0 && facade < plan.receivers().len(), "case {ci}");
            }
        }
        let receivers: Vec<_> = plan
            .receivers()
            .iter()
            .map(|r| (r.first_triangle, r.triangle_count, r.job))
            .collect();
        let bakes = plan.bake(&inputs, "skin", &|| false).unwrap();
        let inverse = pen(case.reflected).matrix.inverse().unwrap();
        for (first, count, job) in receivers {
            for t in first..first + count {
                for &index in &mesh.indices[t * 3..t * 3 + 3] {
                    let vertex = mesh.vertices[index as usize];
                    let local = inverse.point(vertex.position);
                    let expected = bakes[job].layout().uv([local[0], local[2]]);
                    assert!(
                        vertex
                            .uv
                            .iter()
                            .zip(expected)
                            .all(|(a, b)| (a - b).abs() < 1e-12),
                        "case {ci} triangle {t}"
                    );
                }
            }
        }
    }
}
#[test]
fn stains_reject_invalid_input_limit_work_and_leave_other_planes_reusable() {
    use superspec_engine::worldspec::themes::kowloon::stains::stain;
    let case = case(settings(0., false, false));
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
    let mut case = case(settings(0.6, true, false));
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
