use superspec_aml::rng::SeededRng;
use superspec_engine::{
    materialspec::surface::Sample,
    worldspec::themes::kowloon::{
        doors::{self, Unit},
        fixture::Fixture,
        frosted::Frosted,
        mesh::{Mesh, Mode},
        metal::{Binding, Metal},
        painted::Paint,
        palette::srgb,
        pen::Pen,
        posters, timber,
        weathered::{self, Recipe},
    },
};
use superspec_geometry::Matrix;
#[path = "support/door_scene.rs"]
mod scene_fixture;
fn repeat(slot: &str) -> f64 {
    match slot {
        "metal" => 1.,
        "paint" => 1.25,
        "wall" => 1.5,
        "pane" => 1.75,
        "timber" => 2.,
        "shop-timber" => 2.25,
        "posters" => 2.5,
        _ => panic!("unknown slot"),
    }
}
pub fn settings(density: f64, textured: bool) -> doors::Settings {
    doors::Settings {
        weathering: weathered::Settings {
            seed: "doors/finish".into(),
            wear: 0.25,
            grime: 0.6,
            rust: 0.4,
            texture_density: density,
            metal: Binding {
                material: "metal".into(),
                repeat: 1.,
            },
            paint: Binding {
                material: "paint".into(),
                repeat: 1.25,
            },
            sheet: Binding {
                material: "metal".into(),
                repeat: 1.,
            },
        },
        timber: timber::Settings {
            textured,
            timber_repeat: 2.,
            atlas_repeat: 2.25,
            ..Default::default()
        },
        trim: srgb("#e6e2d6"),
        detail: 1,
        posters: textured,
    }
}
const SEEDS: [&str; 10] = [
    "door/0", "door/1", "door/2", "door/3", "door/4", "door/5", "door/6", "door/7", "door/8",
    "door/9",
];
fn frame(name: &str) -> Matrix {
    if name == "identity" {
        Matrix::default()
    } else {
        Matrix([[-1., 0., 0.], [0., -1., 0.], [0., 0., -1.], [4., 3., 5.]])
    }
}
fn unit(seed: &str) -> Unit {
    Unit {
        u0: 0.,
        u1: if seed == "door/9" { 1.7 } else { 3.6 },
        height: if seed == "door/8" { 2.5 } else { 3.4 },
        key: "unit".into(),
    }
}

#[test]
fn frosted_panels_deliver_the_standalone_raster_of_their_recipe() {
    use std::collections::BTreeMap;
    use superspec_engine::{
        materialspec::surface::{Inputs, RasterSettings, rasterize},
        worldspec::themes::kowloon::frosted,
    };
    let material: superspec_aml::material::Material = serde_json::from_value(serde_json::json!({
        "id": "pane",
        "source": {"line": 1, "column": 1, "offset": 0, "end": 1},
        "color": "#ffffff",
        "repeat": 1.75,
        "roughness": 0.8,
        "metalness": 0
    }))
    .unwrap();
    let materials = BTreeMap::from([(material.id.clone(), material.clone())]);
    let inputs = Inputs {
        materials: &materials,
        images: &BTreeMap::new(),
    };
    for bounds in [
        [0.55, 2.095, 1.95, 2.41],
        [1.9924566599103857, 2.095, 2.9358477800298717, 2.41],
    ] {
        let mut scene=superspec_engine::compile(r#"<architecture version="1" id="frosted"><box id="host" width="1" depth="1" height="1" visible="false"/></architecture>"#,0).unwrap();
        scene.manifest.materials.push(material.clone());
        let mut mesh = Mesh::new(Mode::Render, 100, &|| false).unwrap();
        frosted::panel(
            &mut Fixture {
                pen: Pen::new(Matrix::default(), "glass").unwrap(),
                mesh: &mut mesh,
                repeats: &repeat,
            },
            bounds,
            0.05,
            4049726258.,
            "glass",
            true,
        )
        .unwrap();
        let plan = weathered::finish(&mut mesh, &|| false).unwrap();
        assert_eq!(plan.receivers().len(), 1);
        let Recipe::Frosted(recipe) = &plan.jobs()[plan.receivers()[0].job].recipe else {
            panic!("frosted panel emitted another recipe");
        };
        assert_eq!(recipe.bounds, bounds);
        let field = Frosted::new(recipe).unwrap();
        let raster = rasterize(
            RasterSettings {
                bounds,
                texels_per_meter: 256.,
                preserve: true,
                repeat: false,
                uv_coordinates: true,
                bump: true,
            },
            &|u, v| field.sample(u, v),
            &inputs,
            16 * 1024 * 1024,
            &|| false,
        )
        .unwrap();
        let bakes = plan.bake(&inputs, "glass", &|| false).unwrap();
        assert_eq!(bakes.len(), 1);
        let baked = bakes[0].material().clone();
        scene
            .append_baked_visual(
                "host",
                &mesh,
                &BTreeMap::from([(baked.id.clone(), baked.id.clone())]),
                &bakes,
            )
            .unwrap();
        for (id, expected) in [
            (
                baked.texture.as_ref().unwrap(),
                &raster.layers[0].color.rgba,
            ),
            (
                baked.normal_texture.as_ref().unwrap(),
                &raster.layers[0].normal.as_ref().unwrap().rgba,
            ),
            (
                baked.metallic_roughness_texture.as_ref().unwrap(),
                &raster.layers[0].metallic_roughness.rgba,
            ),
        ] {
            let asset = scene.manifest.assets.iter().find(|a| a.id == *id).unwrap();
            let image = superspec_geometry::image::decode(
                &scene.asset_bytes[asset.byte_start..asset.byte_start + asset.byte_length],
            )
            .unwrap();
            assert_eq!(
                (image.width, image.height),
                (raster.layout.width, raster.layout.height)
            );
            assert_eq!(&image.rgba, expected, "baked frosted {id}");
        }
    }
}

#[test]
fn entrances_survey_their_rendered_corners_and_emit_only_door_recipes() {
    let cases = SEEDS
        .iter()
        .map(|seed| (*seed, "identity", 64., true))
        .chain([
            ("door/0", "reflection", 64., true),
            ("door/1", "identity", 0., false),
        ]);
    for (seed, frame_name, density, textured) in cases {
        let label = format!("{seed}/{frame_name}/{density}");
        let pen = Pen::new(frame(frame_name), "fixture").unwrap();
        let settings = settings(density, textured);
        let unit = unit(seed);
        let mut mesh = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
        let mut rng = SeededRng::from_text(seed);
        let bounds = doors::entrance(
            &mut Fixture {
                pen: pen.clone(),
                mesh: &mut mesh,
                repeats: &repeat,
            },
            &unit,
            &settings,
            &mut rng,
        )
        .unwrap();
        assert!(mesh.triangle_count() > 0, "{label}");
        assert_eq!(mesh.triangle_surfaces.len(), mesh.triangle_count());
        let triangles = mesh.triangle_count();
        let plan = weathered::finish(&mut mesh, &|| false).unwrap();
        for r in plan.receivers() {
            assert!(r.triangle_count > 0 && r.first_triangle + r.triangle_count <= triangles);
            let job = &plan.jobs()[r.job];
            let runoff = job.runoff_recipe().map(|r| r.build(&|| false).unwrap());
            let callback = |u, v| runoff.as_ref().unwrap().sample(u, v);
            let sample: Sample = match &job.recipe {
                Recipe::Metal { settings, .. } => Metal::new(
                    settings,
                    runoff
                        .as_ref()
                        .map(|_| &callback as &dyn Fn(f64, f64) -> Result<f64, String>),
                )
                .unwrap()
                .sample(0.4, 0.6)
                .unwrap(),
                Recipe::Paint(recipe) => Paint::new(recipe).unwrap().sample(0.4, 0.6).unwrap(),
                Recipe::Frosted(recipe) => Frosted::new(recipe).unwrap().sample(0.4, 0.6).unwrap(),
                Recipe::Facade(_)
                | Recipe::FacadeDeposits(_)
                | Recipe::Tile(_)
                | Recipe::Sign(_)
                | Recipe::Stained(_)
                | Recipe::Pavement(_)
                | Recipe::TiledSkin(_) => {
                    panic!("Door emitted an unexpected surface recipe")
                }
            };
            let _ = sample;
        }
        drop(plan);
        let mut survey = Mesh::new(Mode::Survey, 100000, &|| false).unwrap();
        let mut survey_rng = SeededRng::from_text(seed);
        let surveyed = doors::entrance(
            &mut Fixture {
                pen,
                mesh: &mut survey,
                repeats: &|_| panic!("survey lookup"),
            },
            &unit,
            &settings,
            &mut survey_rng,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(surveyed).unwrap(),
            serde_json::to_value(bounds).unwrap(),
            "{label} bounds"
        );
        assert_eq!(survey_rng.next_u32(), rng.next_u32(), "{label} RNG");
        assert_eq!(
            survey.survey.len(),
            mesh.indices.len(),
            "{label} survey count"
        );
        for (p, i) in survey.survey.iter().zip(&mesh.indices) {
            assert_eq!(*p, mesh.vertices[*i as usize].position, "{label} survey");
        }
    }
}

#[test]
fn posters_are_white_posters_slot_quads_whose_bounds_contain_the_rotated_sheet() {
    const DESIGNS: [&str; 16] = [
        "calendar",
        "roast-menu",
        "tea-ad",
        "soda-ad",
        "no-smoking",
        "repairs",
        "room-rental",
        "hiring",
        "herbal-ad",
        "opera",
        "film",
        "laundry",
        "rice-ad",
        "clinic",
        "closure",
        "electrical-ad",
    ];
    for (i, design) in DESIGNS.into_iter().enumerate() {
        let options = posters::Options {
            design: design.into(),
            width: 0.2 + i as f64 * 0.07,
            rotation: i as f64 - 8.,
            layer: i % 9,
            wear: i as f64 / 15.,
            seed: Some(format!("poster/{i}")),
        };
        let [w, h] = options.bounds().unwrap();
        assert!(w >= options.width - 1e-12 && h > 0., "{design}");
        if options.rotation == 0. {
            assert!((w - options.width).abs() < 1e-12, "{design}");
        }
        let mirrored = posters::Options {
            rotation: -options.rotation,
            ..options.clone()
        };
        let [mw, mh] = mirrored.bounds().unwrap();
        assert!((mw - w).abs() < 1e-12 && (mh - h).abs() < 1e-12, "{design}");
        for frame in [
            Matrix::default(),
            Matrix([[-1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [0.; 3]]),
        ] {
            let pen = Pen::new(frame, "poster").unwrap();
            let mut mesh = Mesh::new(Mode::Render, 1000, &|| false).unwrap();
            posters::draw(
                &mut Fixture {
                    pen: pen.clone(),
                    mesh: &mut mesh,
                    repeats: &repeat,
                },
                &options,
                true,
            )
            .unwrap();
            assert!(mesh.triangle_count() > 0);
            assert!(mesh.vertices.iter().all(|v| v.color == [1.; 3]));
            assert!(
                mesh.surfaces
                    .iter()
                    .all(|s| s.slot == "posters" && s.part == "poster")
            );
            let mut survey = Mesh::new(Mode::Survey, 1000, &|| false).unwrap();
            posters::draw(
                &mut Fixture {
                    pen,
                    mesh: &mut survey,
                    repeats: &|_| panic!("survey lookup"),
                },
                &options,
                true,
            )
            .unwrap();
            assert_eq!(survey.survey.len(), mesh.indices.len());
            for (p, i) in survey.survey.iter().zip(&mesh.indices) {
                assert_eq!(*p, mesh.vertices[*i as usize].position);
            }
        }
    }
}
#[test]
fn invalid_units_do_not_advance_rng_or_change_geometry() {
    let s = settings(64., true);
    let mut mesh = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
    let pen = Pen::new(Matrix::default(), "fixture").unwrap();
    for (u1, height) in [(1., 3.), (101., 3.), (3., 2.), (3., f64::NAN)] {
        let mut rng = SeededRng::from_text("invalid");
        let mut before = rng.clone();
        assert!(
            doors::entrance(
                &mut Fixture {
                    pen: pen.clone(),
                    mesh: &mut mesh,
                    repeats: &repeat
                },
                &Unit {
                    u0: 0.,
                    u1,
                    height,
                    key: "unit".into()
                },
                &s,
                &mut rng
            )
            .is_err()
        );
        assert_eq!(rng.next_u32(), before.next_u32());
        assert_eq!(mesh.triangle_count(), 0);
    }
}
#[test]
fn baked_entrances_deliver_shipped_atlases_and_separate_collision() {
    let scene = scene_fixture::scene();
    assert_eq!(scene.manifest.colliders.len(), 1);
    assert!(
        scene
            .manifest
            .assets
            .iter()
            .any(|a| a.id == "superspec-material:posters-atlas.jpg" && a.mime == "image/jpeg")
    );
    assert!(
        scene
            .manifest
            .meshes
            .iter()
            .any(|m| m.material.as_deref() == Some("shop-timber"))
    );
    assert!(
        scene
            .manifest
            .meshes
            .iter()
            .any(|m| m.material.as_deref() == Some("posters"))
    );
    assert!(scene.manifest.meshes.iter().any(|m| {
        m.material
            .as_deref()
            .is_some_and(|s| s.starts_with("$surface/raster/doors/"))
    }));
}
#[test]
fn ground_bays_deliver_complete_materials_without_changing_collision() {
    let scene = scene_fixture::ground_scene();
    assert_eq!(scene.manifest.colliders.len(), 1);
    for mesh in scene.manifest.meshes.iter().filter(|m| m.visible) {
        assert!(
            scene
                .manifest
                .materials
                .iter()
                .any(|m| Some(&m.id) == mesh.material.as_ref())
        );
    }
    for m in &scene.manifest.materials {
        for id in m.companions() {
            assert!(scene.manifest.assets.iter().any(|a| a.id == id));
        }
    }
    assert!(!scene.packet().is_empty());
}
#[test]
fn invalid_appearance_and_cancelled_jobs_recover_without_changing_the_callers_scope() {
    use std::cell::Cell;
    let pen = Pen::new(Matrix::default(), "fixture").unwrap();
    let mut board = Mesh::new(Mode::Render, 10000, &|| false).unwrap();
    for wear in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
        let mut s = settings(64., true);
        s.weathering.wear = wear;
        assert!(
            doors::timber_board(
                &mut Fixture {
                    pen: pen.clone(),
                    mesh: &mut board,
                    repeats: &repeat
                },
                [0.; 3],
                [0.1, 0.02, 1.],
                [0.4; 3],
                &s
            )
            .is_err()
        );
        assert_eq!(board.triangle_count(), 0);
    }
    doors::timber_board(
        &mut Fixture {
            pen: pen.clone(),
            mesh: &mut board,
            repeats: &repeat,
        },
        [0.; 3],
        [0.1, 0.02, 1.],
        [0.4; 3],
        &settings(64., true),
    )
    .unwrap();
    assert!(board.triangle_count() > 0);
    let unit = Unit {
        u0: 0.,
        u1: 3.6,
        height: 3.4,
        key: "unit".into(),
    };
    for i in 0..4 {
        let mut s = settings(64., true);
        match i {
            0 => s.weathering.paint.repeat = 0.,
            1 => s.timber.atlas_repeat = f64::NAN,
            2 => s.weathering.seed = "x".repeat(2049),
            _ => s.trim[0] = f64::NAN,
        };
        let mut mesh = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
        let mut rng = SeededRng::from_text("door/4");
        let mut expected = rng.clone();
        assert!(
            doors::entrance(
                &mut Fixture {
                    pen: pen.clone(),
                    mesh: &mut mesh,
                    repeats: &repeat
                },
                &unit,
                &s,
                &mut rng
            )
            .is_err()
        );
        assert_eq!(mesh.triangle_count(), 0);
        assert_eq!(rng.next_u32(), expected.next_u32());
    }
    let calls = Cell::new(0);
    let cancel = || {
        calls.set(calls.get() + 1);
        calls.get() > 40
    };
    let mut mesh = Mesh::new(Mode::Render, 100000, &cancel).unwrap();
    let s = settings(64., true);
    let mut drawing = Fixture {
        pen: pen.clone(),
        mesh: &mut mesh,
        repeats: &repeat,
    };
    assert!(
        doors::entrance(&mut drawing, &unit, &s, &mut SeededRng::from_text("door/4"))
            .unwrap_err()
            .contains("cancel")
    );
    assert_eq!(drawing.pen.part, pen.part);
    assert_eq!(drawing.pen.matrix.0, pen.matrix.0);
    assert!(calls.get() > 40);
    let mut fresh = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
    doors::entrance(
        &mut Fixture {
            pen,
            mesh: &mut fresh,
            repeats: &repeat,
        },
        &unit,
        &s,
        &mut SeededRng::from_text("door/4"),
    )
    .unwrap();
    assert!(fresh.triangle_count() > 0);
}
#[test]
fn poster_admission_precedes_emission_and_missing_atlases_skip_notice_rng() {
    let mut mesh = Mesh::new(Mode::Render, 1000, &|| false).unwrap();
    let pen = Pen::new(Matrix::default(), "poster").unwrap();
    let valid = posters::Options {
        design: "calendar".into(),
        width: 0.3,
        rotation: 0.,
        wear: 0.3,
        layer: 0,
        seed: None,
    };
    for i in 0..5 {
        let mut options = valid.clone();
        match i {
            0 => options.design = "unknown".into(),
            1 => options.width = 0.01,
            2 => options.rotation = f64::NAN,
            3 => options.layer = 9,
            _ => options.wear = 1.1,
        };
        assert!(
            posters::draw(
                &mut Fixture {
                    pen: pen.clone(),
                    mesh: &mut mesh,
                    repeats: &repeat
                },
                &options,
                true
            )
            .is_err()
        );
        assert_eq!(mesh.triangle_count(), 0);
    }
    assert!(
        posters::draw(
            &mut Fixture {
                pen: pen.clone(),
                mesh: &mut mesh,
                repeats: &repeat
            },
            &valid,
            false
        )
        .is_err()
    );
    assert_eq!(mesh.triangle_count(), 0);
    let mut rng = SeededRng::from_text("notice");
    let mut expected = rng.clone();
    posters::notices(
        &mut Fixture {
            pen: pen.clone(),
            mesh: &mut mesh,
            repeats: &repeat,
        },
        [0., 1., 0., 1.],
        &mut rng,
        false,
    )
    .unwrap();
    assert_eq!(rng.next_u32(), expected.next_u32());
    assert_eq!(mesh.triangle_count(), 0);
    posters::draw(
        &mut Fixture {
            pen,
            mesh: &mut mesh,
            repeats: &repeat,
        },
        &valid,
        true,
    )
    .unwrap();
    assert!(mesh.triangle_count() > 0);
}
