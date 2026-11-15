use serde::Deserialize;
use serde_json::Value;
use superspec_engine::worldspec::themes::kowloon::{
    kit::Kit,
    mesh::{Mesh, Mode},
    metal::Binding,
    signs::panel,
    spans::{
        anchors::{Anchor, Plane},
        draw,
        plan::{self, Rule},
        walking::{Surface, Walking},
    },
    timber, weathered,
};
use superspec_geometry::V3;
use superspec_geometry::obstructions::{Budget, Obstructions, Triangle};
#[path = "support/assembly_reference.rs"]
#[allow(dead_code)]
mod assembly;
#[path = "support/balcony_props_scene.rs"]
#[allow(dead_code)]
mod props;
#[path = "support/sign_reference.rs"]
#[allow(dead_code)]
mod reference;
#[derive(Deserialize)]
struct PlaneInput {
    host: String,
    normal: V3,
    offset: f64,
    faces: Vec<Triangle>,
}
#[derive(Deserialize)]
struct Input {
    seed: String,
    rule: Rule,
    planes: Vec<PlaneInput>,
    physical: Vec<Triangle>,
    triangles: Vec<Triangle>,
    utility: Vec<Anchor>,
    windows: Vec<Anchor>,
}
#[derive(Deserialize)]
struct Case {
    input: Input,
    detail: u8,
    cloth: bool,
    geometry: assembly::Geometry,
    charts: Vec<assembly::Chart>,
    parts: Vec<String>,
    contributors: Value,
    #[serde(default)]
    repeats: std::collections::BTreeMap<String, f64>,
}
#[derive(Deserialize)]
struct Golden {
    cases: Vec<Case>,
}
pub fn check_reference_span_drawings() {
    let mut golden: Golden =
        serde_json::from_str(include_str!("../../../tests/fixtures/rust/span-draw.json")).unwrap();
    let routed: Golden = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/span-routed-materials.json"
    ))
    .unwrap();
    golden.cases.extend(routed.cases);
    for (case_index, c) in golden.cases.into_iter().enumerate() {
        let input = c.input;
        let mut budget = Budget::new(32_000_000, &|| false).unwrap();
        let planes = input
            .planes
            .iter()
            .map(|p| Plane::new(&p.host, p.normal, p.offset, &p.faces, &mut budget).unwrap())
            .collect::<Vec<_>>();
        let floors = Walking::new(
            &input
                .physical
                .iter()
                .map(|&points| Surface {
                    points,
                    floor_or_stairs: true,
                    collidable: true,
                })
                .collect::<Vec<_>>(),
            &mut budget,
        )
        .unwrap();
        let obstacles = Obstructions::new(&input.triangles, &mut budget).unwrap();
        let plan = plan::build(
            input.rule,
            &input.seed,
            &planes,
            &input.utility,
            &input.windows,
            &floors,
            obstacles,
            &mut budget,
        )
        .unwrap();
        let anchors = input
            .utility
            .into_iter()
            .chain(input.windows)
            .collect::<Vec<_>>();
        let binding = |id: &str| Binding {
            material: id.into(),
            repeat: 1.,
        };
        let kit = Kit::new(
            panel::Settings {
                weathering: weathered::Settings {
                    seed: format!("{}/spans", input.seed),
                    wear: 0.25,
                    grime: 0.6,
                    rust: 0.4,
                    texture_density: 64.,
                    metal: binding("metal"),
                    sheet: binding("sheet"),
                    paint: binding("paint"),
                },
                lettering: true,
                timber: timber::Settings::default(),
            },
            c.detail,
        )
        .unwrap();
        if case_index == 0 {
            let settings = draw::Settings {
                kit: &kit,
                cloth: c.cloth,
            };
            let calls = std::cell::Cell::new(0);
            let cancelled = || {
                calls.set(calls.get() + 1);
                calls.get() > 20
            };
            let mut partial = Mesh::new(Mode::Render, 500_000, &cancelled).unwrap();
            let error = draw::draw(
                &mut partial,
                &plan,
                &input.seed,
                &settings,
                &planes,
                &anchors,
                &|_| 1.,
                &mut budget,
            )
            .unwrap_err();
            assert!(error.contains("cancel"));
            assert!(calls.get() > 20);
            assert!(partial.triangle_count() > 0);
            let mut tiny = Mesh::new(Mode::Render, 1, &|| false).unwrap();
            assert!(
                draw::draw(
                    &mut tiny,
                    &plan,
                    &input.seed,
                    &settings,
                    &planes,
                    &anchors,
                    &|_| 1.,
                    &mut budget
                )
                .is_err()
            );
            let mut invalid = anchors.clone();
            invalid[0].plane = usize::MAX;
            let mut detached = Mesh::new(Mode::Render, 500_000, &|| false).unwrap();
            assert!(
                draw::draw(
                    &mut detached,
                    &plan,
                    &input.seed,
                    &settings,
                    &planes,
                    &invalid,
                    &|_| 1.,
                    &mut budget
                )
                .is_err()
            );
            assert_eq!(detached.triangle_count(), 0);
        }
        for mode in [Mode::Render, Mode::Survey] {
            if !c.repeats.is_empty() {
                continue;
            }
            let mut mesh = Mesh::new(mode, 500_000, &|| false).unwrap();
            let contributors = draw::draw(
                &mut mesh,
                &plan,
                &input.seed,
                &draw::Settings {
                    kit: &kit,
                    cloth: c.cloth,
                },
                &planes,
                &anchors,
                &|_| 1.,
                &mut budget,
            )
            .unwrap();
            assert_eq!(serde_json::to_value(contributors).unwrap(), c.contributors);
            if !mesh.surveying() {
                assembly::check_triangles(&mesh, &c.geometry, &input.seed);
                for (t, part) in c.parts.iter().enumerate() {
                    assert_eq!(mesh.surfaces[mesh.triangle_surfaces[t]].part, *part);
                }
                assembly::check_finishes(&mut mesh, &c.geometry, &c.charts, &input.seed);
            } else {
                assembly::check_survey(&mesh, &c.geometry, &input.seed);
            }
            assert_eq!(mesh.triangle_count() * 3, c.geometry.indices.len());
        }
        let hosts = planes
            .iter()
            .map(|p| p.host().to_owned())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let mut meshes = hosts
            .iter()
            .map(|_| Mesh::new(Mode::Render, 500_000, &|| false).unwrap())
            .collect::<Vec<_>>();
        if case_index == 0 {
            let seen = std::cell::Cell::new(0usize);
            let after_both = std::cell::Cell::new(0usize);
            let cancelled = || {
                if seen.get() == (1usize << hosts.len()) - 1 {
                    after_both.set(after_both.get() + 1);
                }
                after_both.get() > 30
            };
            let repeats = (0..hosts.len())
                .map(|i| {
                    let seen = &seen;
                    move |_: &str| {
                        seen.set(seen.get() | (1 << i));
                        1.
                    }
                })
                .collect::<Vec<_>>();
            let mut partial = hosts
                .iter()
                .map(|_| Mesh::new(Mode::Render, 500_000, &cancelled).unwrap())
                .collect::<Vec<_>>();
            let outputs = hosts
                .iter()
                .zip(&mut partial)
                .zip(&repeats)
                .map(|((host, mesh), repeats)| draw::Output {
                    host,
                    mesh,
                    kit: &kit,
                    cloth: c.cloth,
                    repeats,
                })
                .collect();
            let error =
                draw::draw_hosts(outputs, &plan, &input.seed, &planes, &anchors, &mut budget)
                    .unwrap_err();
            assert!(error.contains("cancel"), "{error}");
            assert!(
                partial.iter().all(|m| m.triangle_count() > 0),
                "Cancellation must follow emission to both hosts"
            );
        }
        let kits = hosts
            .iter()
            .map(|host| {
                let repeat = c.repeats.get(host).copied().unwrap_or(1.);
                let mut settings = kit.settings().clone();
                settings.weathering.metal.repeat = repeat;
                settings.weathering.sheet.repeat = repeat;
                settings.weathering.paint.repeat = repeat;
                settings.timber.timber_repeat = repeat;
                settings.timber.atlas_repeat = repeat;
                Kit::new(settings, c.detail).unwrap()
            })
            .collect::<Vec<_>>();
        let repeats = hosts
            .iter()
            .map(|host| {
                let repeat = c.repeats.get(host).copied().unwrap_or(1.);
                move |_: &str| repeat
            })
            .collect::<Vec<_>>();
        let outputs = hosts
            .iter()
            .zip(&mut meshes)
            .zip(&kits)
            .zip(&repeats)
            .map(|(((host, mesh), kit), repeats)| draw::Output {
                host,
                mesh,
                kit,
                cloth: c.cloth,
                repeats,
            })
            .collect();
        let contributors =
            draw::draw_hosts(outputs, &plan, &input.seed, &planes, &anchors, &mut budget).unwrap();
        assert_eq!(serde_json::to_value(contributors).unwrap(), c.contributors);
        for (host, mesh) in hosts.iter().zip(&mut meshes) {
            let prefix = format!("{host}/$dressing/");
            let selected = c
                .parts
                .iter()
                .enumerate()
                .filter_map(|(t, p)| p.starts_with(&prefix).then_some(t))
                .collect::<Vec<_>>();
            let (geometry, charts) = assembly::partition(&c.geometry, &c.charts, &selected);
            assembly::check_triangles(mesh, &geometry, host);
            for (t, &source) in selected.iter().enumerate() {
                assert_eq!(
                    format!("{prefix}{}", mesh.surfaces[mesh.triangle_surfaces[t]].part),
                    c.parts[source]
                );
            }
            assembly::check_finishes(mesh, &geometry, &charts, host);
        }
    }
}
#[test]
fn complete_spans_keep_cables_laundry_hardware_sign_finishes_and_host_ownership() {
    check_reference_span_drawings();
}

/// Browser fixture with real garment/glyph images and a separately compiled physical support scene.
pub fn scene() -> superspec_engine::Scene {
    use std::collections::BTreeMap;
    use superspec_aml::material::Material;
    use superspec_engine::materialspec::surface::Inputs;
    use superspec_engine::{Asset, compile};
    let g: Golden =
        serde_json::from_str(include_str!("../../../tests/fixtures/rust/span-draw.json")).unwrap();
    let c = g.cases.into_iter().nth(1).unwrap();
    let input = c.input;
    let mut budget = Budget::new(32_000_000, &|| false).unwrap();
    let planes = input
        .planes
        .iter()
        .map(|p| Plane::new(&p.host, p.normal, p.offset, &p.faces, &mut budget).unwrap())
        .collect::<Vec<_>>();
    let floors = Walking::new(
        &input
            .physical
            .iter()
            .map(|&points| Surface {
                points,
                floor_or_stairs: true,
                collidable: true,
            })
            .collect::<Vec<_>>(),
        &mut budget,
    )
    .unwrap();
    let obstacles = Obstructions::new(&input.triangles, &mut budget).unwrap();
    let plan = plan::build(
        input.rule,
        &input.seed,
        &planes,
        &input.utility,
        &input.windows,
        &floors,
        obstacles,
        &mut budget,
    )
    .unwrap();
    let anchors = input
        .utility
        .into_iter()
        .chain(input.windows)
        .collect::<Vec<_>>();
    let (mut assets, mut materials) = props::atlas_scene();
    let mut scene=compile(r##"<architecture version="1" id="rust-spans"><material id="walls" color="#aba29a"/><box id="host" width="12" depth=".2" height="9" position="0 -.2 0" material="walls"/><box id="far" width="12" depth=".2" height="9" position="0 4 0" material="walls"/><box id="paving" width="14" depth="8" height=".15" position="-1 -1 -.15" material="walls"/></architecture>"##,0).unwrap();
    let physical = |scene: &superspec_engine::Scene| {
        scene
            .manifest
            .meshes
            .iter()
            .filter(|m| scene.manifest.colliders.contains(&m.id))
            .map(|m| {
                (
                    serde_json::to_value(m).unwrap(),
                    scene.positions[m.vertex_start * 3..(m.vertex_start + m.vertex_count) * 3]
                        .to_vec(),
                    scene.indices[m.index_start..m.index_start + m.index_count].to_vec(),
                )
            })
            .collect::<Vec<_>>()
    };
    let collision = physical(&scene);
    let colliders = scene.manifest.colliders.clone();
    scene.asset_bytes = std::mem::take(&mut assets.asset_bytes);
    scene.manifest.assets = std::mem::take(&mut assets.manifest.assets);
    materials.extend(
        scene
            .manifest
            .materials
            .iter()
            .cloned()
            .map(|m| (m.id.clone(), m)),
    );
    for slot in ["metal", "sheet", "timber", "cloth"] {
        materials.insert(
            slot.into(),
            Material {
                id: slot.into(),
                color: "#ffffff".into(),
                roughness: 0.8,
                ..Material::default()
            },
        );
    }
    for (slot, role) in [
        ("board", "signboard"),
        ("rust", "rust"),
        ("lettering", "lettering"),
    ] {
        let mut m = superspec_aml::material_catalog::material(&format!("builtin:{role}")).unwrap();
        m.id = slot.into();
        materials.insert(slot.into(), m);
    }
    let mut images = BTreeMap::new();
    for (name, bytes) in [
        (
            "lettering.png",
            include_bytes!("../../../assets/materials/lettering.png").as_slice(),
        ),
        (
            "signboard.png",
            include_bytes!("../../../assets/materials/signboard.png").as_slice(),
        ),
        (
            "rust.png",
            include_bytes!("../../../assets/materials/rust.png").as_slice(),
        ),
    ] {
        let id = format!("superspec-material:{name}");
        images.insert(
            id.clone(),
            superspec_geometry::image::decode(bytes).unwrap(),
        );
        scene.manifest.assets.push(Asset {
            id,
            mime: "image/png",
            byte_start: scene.asset_bytes.len(),
            byte_length: bytes.len(),
        });
        scene.asset_bytes.extend_from_slice(bytes);
    }
    scene.manifest.materials = materials.values().cloned().collect();
    scene.manifest.asset_byte_count = scene.asset_bytes.len();
    let binding = |id: &str| Binding {
        material: id.into(),
        repeat: materials[id].repeat,
    };
    let kit = Kit::new(
        panel::Settings {
            weathering: weathered::Settings {
                seed: format!("{}/spans", input.seed),
                wear: 0.25,
                grime: 0.6,
                rust: 0.4,
                texture_density: 64.,
                metal: binding("metal"),
                sheet: binding("sheet"),
                paint: binding("paint"),
            },
            lettering: true,
            timber: timber::Settings::default(),
        },
        c.detail,
    )
    .unwrap();
    let mut mesh = Mesh::new(Mode::Render, 500_000, &|| false).unwrap();
    draw::draw(
        &mut mesh,
        &plan,
        &input.seed,
        &draw::Settings {
            kit: &kit,
            cloth: true,
        },
        &planes,
        &anchors,
        &|id| materials[id].repeat,
        &mut budget,
    )
    .unwrap();
    let plan = weathered::finish(&mut mesh, &|| false).unwrap();
    let bakes = plan
        .bake(
            &Inputs {
                materials: &materials,
                images: &images,
            },
            "spans",
            &|| false,
        )
        .unwrap();
    let mut bindings: BTreeMap<_, _> = materials
        .keys()
        .map(|id| (id.clone(), id.clone()))
        .collect();
    bindings.extend(
        bakes
            .iter()
            .map(|b| (b.material().id.clone(), b.material().id.clone())),
    );
    scene
        .append_baked_visual("host", &mesh, &bindings, &bakes)
        .unwrap();
    assert_eq!(scene.manifest.colliders, colliders);
    assert_eq!(physical(&scene), collision);
    scene
}

#[test]
fn complete_span_bakes_keep_source_atlases_and_physical_supports() {
    let scene = scene();
    assert!(
        scene
            .manifest
            .assets
            .iter()
            .any(|a| a.id == "superspec-material:balcony-cloth.png")
    );
    assert!(
        scene
            .manifest
            .materials
            .iter()
            .any(|m| m.id.starts_with("$surface/raster/"))
    );
    assert!(!scene.packet().is_empty());
}
