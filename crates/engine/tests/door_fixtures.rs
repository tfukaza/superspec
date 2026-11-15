use serde::Deserialize;
use serde_json::Value;
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
#[derive(Deserialize)]
struct FrostedCase {
    recipe: superspec_engine::worldspec::themes::kowloon::frosted::Recipe,
    material: superspec_aml::material::Material,
    density: f64,
    width: usize,
    height: usize,
    color: Vec<u8>,
    normal: Vec<u8>,
    packed: Vec<u8>,
}
#[derive(Deserialize)]
struct Receiver {
    first: usize,
    count: usize,
    point: [f64; 2],
    sample: Sample,
}
#[derive(Deserialize)]
struct Case {
    seed: String,
    unit: Unit,
    frame: String,
    density: f64,
    textured: bool,
    bounds: doors::Bounds,
    next: u32,
    indices: Vec<u32>,
    positions: Vec<f64>,
    normals: Vec<f64>,
    uv: Vec<f64>,
    colors: Vec<f64>,
    slots: Vec<String>,
    parts: Vec<String>,
    receivers: Vec<Receiver>,
}
#[derive(Deserialize)]
struct Poster {
    frame: String,
    options: posters::Options,
    bounds: Value,
    indices: Vec<u32>,
    positions: Vec<f64>,
    normals: Vec<f64>,
    uv: Vec<f64>,
}
#[derive(Deserialize)]
struct Cases {
    cases: Vec<Case>,
    posters: Vec<Poster>,
    frosted: Vec<FrostedCase>,
}
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
fn check_numbers(actual: Vec<f64>, expected: &[f64], label: &str) {
    assert_eq!(actual.len(), expected.len(), "{label} count");
    for (i, (a, b)) in actual.iter().zip(expected).enumerate() {
        assert!((a - b).abs() < 2e-10, "{label}/{i}: {a} != {b}");
    }
}
fn close(a: &Value, b: &Value, label: &str) {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => assert!(
            (a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 2e-10,
            "{label}: {a} != {b}"
        ),
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len(), "{label}");
            for (a, b) in a.iter().zip(b) {
                close(a, b, label);
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.len(), b.len(), "{label}");
            for (k, a) in a {
                close(a, &b[k], &format!("{label}/{k}"));
            }
        }
        _ => assert_eq!(a, b, "{label}"),
    }
}
fn check_frosted_delivery(case: &FrostedCase) {
    use std::collections::BTreeMap;
    use superspec_engine::{materialspec::surface::Inputs, worldspec::themes::kowloon::frosted};
    let mut scene=superspec_engine::compile(r#"<architecture version="1" id="frosted"><box id="host" width="1" depth="1" height="1" visible="false"/></architecture>"#,0).unwrap();
    let materials = BTreeMap::from([(case.material.id.clone(), case.material.clone())]);
    scene.manifest.materials.push(case.material.clone());
    let mut mesh = Mesh::new(Mode::Render, 100, &|| false).unwrap();
    frosted::panel(
        &mut Fixture {
            pen: Pen::new(Matrix::default(), "glass").unwrap(),
            mesh: &mut mesh,
            repeats: &repeat,
        },
        case.recipe.bounds,
        0.05,
        case.recipe.seed,
        "glass",
        true,
    )
    .unwrap();
    let bakes = weathered::finish(&mut mesh, &|| false)
        .unwrap()
        .bake(
            &Inputs {
                materials: &materials,
                images: &BTreeMap::new(),
            },
            "glass",
            &|| false,
        )
        .unwrap();
    assert_eq!(bakes.len(), 1);
    let material = bakes[0].material().clone();
    scene
        .append_baked_visual(
            "host",
            &mesh,
            &BTreeMap::from([(material.id.clone(), material.id.clone())]),
            &bakes,
        )
        .unwrap();
    for (id, expected) in [
        (material.texture.as_ref().unwrap(), &case.color),
        (material.normal_texture.as_ref().unwrap(), &case.normal),
        (
            material.metallic_roughness_texture.as_ref().unwrap(),
            &case.packed,
        ),
    ] {
        let asset = scene.manifest.assets.iter().find(|a| a.id == *id).unwrap();
        let image = superspec_geometry::image::decode(
            &scene.asset_bytes[asset.byte_start..asset.byte_start + asset.byte_length],
        )
        .unwrap();
        assert_eq!((image.width, image.height), (case.width, case.height));
        assert_eq!(&image.rgba, expected, "baked frosted {id}");
    }
}
pub fn check_reference_doors() {
    let fixture: Cases =
        serde_json::from_str(include_str!("../../../tests/fixtures/rust/doors.json")).unwrap();
    assert_eq!(fixture.frosted.len(), 2);
    for case in fixture.frosted {
        check_frosted_delivery(&case);
        use std::collections::BTreeMap;
        use superspec_engine::materialspec::surface::{Inputs, RasterSettings, rasterize};
        let materials = BTreeMap::from([(case.material.id.clone(), case.material)]);
        let inputs = Inputs {
            materials: &materials,
            images: &BTreeMap::new(),
        };
        let field = Frosted::new(&case.recipe).unwrap();
        let raster = rasterize(
            RasterSettings {
                bounds: case.recipe.bounds,
                texels_per_meter: case.density,
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
        assert_eq!(
            (raster.layout.width, raster.layout.height),
            (case.width, case.height)
        );
        assert_eq!(raster.layers[0].color.rgba, case.color, "frosted color");
        assert_eq!(
            raster.layers[0].normal.as_ref().unwrap().rgba,
            case.normal,
            "frosted normal"
        );
        assert_eq!(
            raster.layers[0].metallic_roughness.rgba, case.packed,
            "frosted data"
        );
    }
    for case in fixture.cases {
        let label = format!("{}/{}", case.seed, case.frame);
        let matrix = if case.frame == "identity" {
            Matrix::default()
        } else {
            Matrix([[-1., 0., 0.], [0., -1., 0.], [0., 0., -1.], [4., 3., 5.]])
        };
        let pen = Pen::new(matrix, "fixture").unwrap();
        let settings = settings(case.density, case.textured);
        let mut mesh = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
        let mut rng = SeededRng::from_text(&case.seed);
        let bounds = doors::entrance(
            &mut Fixture {
                pen: pen.clone(),
                mesh: &mut mesh,
                repeats: &repeat,
            },
            &case.unit,
            &settings,
            &mut rng,
        )
        .unwrap();
        close(
            &serde_json::to_value(bounds).unwrap(),
            &serde_json::to_value(case.bounds).unwrap(),
            &label,
        );
        assert_eq!(rng.next_u32(), case.next, "{label} RNG");
        for (corner, (i, j)) in mesh.indices.iter().zip(&case.indices).enumerate() {
            let p = mesh.vertices[*i as usize].position;
            let expected = &case.positions[*j as usize * 3..*j as usize * 3 + 3];
            assert!(
                p.iter().zip(expected).all(|(a, b)| (a - b).abs() < 2e-10),
                "{label} first corner mismatch {corner} {p:?} != {expected:?}, slot {} expected {}",
                mesh.surfaces[mesh.triangle_surfaces[corner / 3]].slot,
                case.slots[corner / 3]
            );
        }
        assert_eq!(
            mesh.indices.len(),
            case.indices.len(),
            "{label} index count"
        );
        for (i, (a, b)) in mesh.indices.iter().zip(&case.indices).enumerate() {
            assert_eq!(
                a,
                b,
                "{label} index {i}, vertex counts {} {}",
                mesh.vertices.len(),
                case.positions.len() / 3
            );
        }
        for (name, actual, expected) in [
            (
                "position",
                mesh.vertices.iter().flat_map(|v| v.position).collect(),
                &case.positions,
            ),
            (
                "normal",
                mesh.vertices.iter().flat_map(|v| v.normal).collect(),
                &case.normals,
            ),
            (
                "UV",
                mesh.vertices.iter().flat_map(|v| v.uv).collect(),
                &case.uv,
            ),
            (
                "color",
                mesh.vertices.iter().flat_map(|v| v.color).collect(),
                &case.colors,
            ),
        ] {
            check_numbers(actual, expected, &format!("{label}/{name}"));
        }
        assert_eq!(mesh.triangle_surfaces.len(), case.slots.len());
        assert_eq!(case.slots.len(), case.parts.len());
        for ((s, slot), part) in mesh
            .triangle_surfaces
            .iter()
            .zip(&case.slots)
            .zip(&case.parts)
        {
            assert_eq!(&mesh.surfaces[*s].slot, slot, "{label} slot");
            assert_eq!(format!("host/$dressing/{}", mesh.surfaces[*s].part), *part);
        }
        let plan = weathered::finish(&mut mesh, &|| false).unwrap();
        assert_eq!(
            plan.receivers().len(),
            case.receivers.len(),
            "{label} receivers"
        );
        for (r, e) in plan.receivers().iter().zip(&case.receivers) {
            assert_eq!(
                (r.first_triangle, r.triangle_count),
                (e.first, e.count),
                "{label} range"
            );
            let job = &plan.jobs()[r.job];
            let runoff = job.runoff_recipe().map(|r| r.build(&|| false).unwrap());
            let callback = |u, v| runoff.as_ref().unwrap().sample(u, v);
            let sample = match &job.recipe {
                Recipe::Metal { settings, .. } => Metal::new(
                    settings,
                    runoff
                        .as_ref()
                        .map(|_| &callback as &dyn Fn(f64, f64) -> Result<f64, String>),
                )
                .unwrap()
                .sample(e.point[0], e.point[1])
                .unwrap(),
                Recipe::Paint(recipe) => Paint::new(recipe)
                    .unwrap()
                    .sample(e.point[0], e.point[1])
                    .unwrap(),
                Recipe::Frosted(recipe) => Frosted::new(recipe)
                    .unwrap()
                    .sample(e.point[0], e.point[1])
                    .unwrap(),
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
            close(
                &serde_json::to_value(sample).unwrap(),
                &serde_json::to_value(&e.sample).unwrap(),
                &label,
            );
        }
        drop(plan);
        let mut survey = Mesh::new(Mode::Survey, 100000, &|| false).unwrap();
        let mut rng = SeededRng::from_text(&case.seed);
        doors::entrance(
            &mut Fixture {
                pen,
                mesh: &mut survey,
                repeats: &|_| panic!("survey lookup"),
            },
            &case.unit,
            &settings,
            &mut rng,
        )
        .unwrap();
        assert_eq!(rng.next_u32(), case.next);
        assert_eq!(
            survey.survey.len(),
            mesh.indices.len(),
            "{label} survey count"
        );
        for (p, i) in survey.survey.iter().zip(&mesh.indices) {
            assert_eq!(*p, mesh.vertices[*i as usize].position, "{label} survey");
        }
    }
    for case in fixture.posters {
        let mut mesh = Mesh::new(Mode::Render, 1000, &|| false).unwrap();
        let matrix = if case.frame == "identity" {
            Matrix::default()
        } else {
            Matrix([[-1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [0.; 3]])
        };
        let pen = Pen::new(matrix, "poster").unwrap();
        posters::draw(
            &mut Fixture {
                pen: pen.clone(),
                mesh: &mut mesh,
                repeats: &repeat,
            },
            &case.options,
            true,
        )
        .unwrap();
        assert_eq!(mesh.indices, case.indices);
        assert!(mesh.vertices.iter().all(|v| v.color == [1.; 3]));
        assert!(
            mesh.surfaces
                .iter()
                .all(|s| s.slot == "posters" && s.part == "poster")
        );
        check_numbers(
            mesh.vertices.iter().flat_map(|v| v.position).collect(),
            &case.positions,
            "poster positions",
        );
        check_numbers(
            mesh.vertices.iter().flat_map(|v| v.normal).collect(),
            &case.normals,
            "poster normals",
        );
        check_numbers(
            mesh.vertices.iter().flat_map(|v| v.uv).collect(),
            &case.uv,
            "poster uv",
        );
        let [w, h] = case.options.bounds().unwrap();
        close(
            &serde_json::json!({"width":w,"height":h}),
            &case.bounds,
            "poster bounds",
        );
        let mut survey = Mesh::new(Mode::Survey, 1000, &|| false).unwrap();
        posters::draw(
            &mut Fixture {
                pen,
                mesh: &mut survey,
                repeats: &|_| panic!("survey lookup"),
            },
            &case.options,
            true,
        )
        .unwrap();
        assert_eq!(survey.survey.len(), mesh.indices.len());
        for (p, i) in survey.survey.iter().zip(&mesh.indices) {
            assert_eq!(*p, mesh.vertices[*i as usize].position);
        }
    }
}
#[test]
fn entrance_and_poster_geometry_receivers_and_rng_match_reference() {
    check_reference_doors();
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
    assert_eq!(mesh.triangle_count(), 6);
}
