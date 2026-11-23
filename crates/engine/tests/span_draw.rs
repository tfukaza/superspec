use superspec_engine::worldspec::themes::kowloon::{
    kit::Kit,
    mesh::{Mesh, Mode},
    metal::Binding,
    signs::panel,
    spans::{
        anchors::Anchor,
        draw,
        plan::{Plan, Rule},
    },
    timber, weathered,
};
use superspec_geometry::obstructions::Budget;
#[path = "support/balcony_props_scene.rs"]
#[allow(dead_code)]
mod props;
#[path = "support/span_street.rs"]
mod street;
use street::{RULE, Street, street};
/// A drawn span case: street gap, plan seed and rule, kit detail and cloth.
struct Case {
    gap: f64,
    seed: &'static str,
    rule: Rule,
    detail: u8,
    cloth: bool,
}
const CASES: [Case; 3] = [
    Case {
        gap: 6.,
        seed: "span-plan/1",
        rule: Rule {
            clotheslines: 0.,
            ..RULE
        },
        detail: 0,
        cloth: true,
    },
    Case {
        gap: 4.,
        seed: "span-plan/4",
        rule: RULE,
        detail: 1,
        cloth: true,
    },
    Case {
        gap: 4.,
        seed: "span-plan/8",
        rule: Rule { signs: 0., ..RULE },
        detail: 2,
        cloth: false,
    },
];
fn kit(seed: &str, detail: u8, repeat: &dyn Fn(&str) -> f64) -> Kit {
    let binding = |id: &str| Binding {
        material: id.into(),
        repeat: repeat(id),
    };
    Kit::new(
        panel::Settings {
            weathering: weathered::Settings {
                seed: format!("{seed}/spans"),
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
        detail,
    )
    .unwrap()
}
struct Drawn {
    street: Street,
    plan: Plan,
    anchors: Vec<Anchor>,
    kit: Kit,
}
fn drawn(c: &Case, budget: &mut Budget<'_>) -> Drawn {
    let street = street(c.gap, budget);
    let plan = street.plan(c.rule, c.seed, budget).unwrap();
    let anchors = street.anchors();
    Drawn {
        kit: kit(c.seed, c.detail, &|_| 1.),
        street,
        plan,
        anchors,
    }
}
fn corners(mesh: &Mesh<'_>) -> Vec<[f64; 3]> {
    mesh.indices
        .iter()
        .map(|&i| mesh.vertices[i as usize].position)
        .collect()
}
fn parts(mesh: &Mesh<'_>) -> Vec<String> {
    mesh.triangle_surfaces
        .iter()
        .map(|&s| mesh.surfaces[s].part.clone())
        .collect()
}
#[test]
fn span_drawing_cancels_after_emission_and_rejects_detached_anchors() {
    let c = &CASES[0];
    let mut budget = Budget::new(32_000_000, &|| false).unwrap();
    let d = drawn(c, &mut budget);
    let settings = draw::Settings {
        kit: &d.kit,
        cloth: c.cloth,
    };
    let draw = |mesh: &mut Mesh<'_>, anchors: &[Anchor], budget: &mut Budget<'_>| {
        draw::draw(
            mesh,
            &d.plan,
            c.seed,
            &settings,
            &d.street.planes,
            anchors,
            &|_| 1.,
            budget,
        )
    };
    let calls = std::cell::Cell::new(0);
    let cancelled = || {
        calls.set(calls.get() + 1);
        calls.get() > 20
    };
    let mut partial = Mesh::new(Mode::Render, 500_000, &cancelled).unwrap();
    let error = draw(&mut partial, &d.anchors, &mut budget).unwrap_err();
    assert!(error.contains("cancel"));
    assert!(calls.get() > 20);
    assert!(partial.triangle_count() > 0);
    let mut tiny = Mesh::new(Mode::Render, 1, &|| false).unwrap();
    assert!(draw(&mut tiny, &d.anchors, &mut budget).is_err());
    let mut invalid = d.anchors.clone();
    invalid[0].plane = usize::MAX;
    let mut detached = Mesh::new(Mode::Render, 500_000, &|| false).unwrap();
    assert!(draw(&mut detached, &invalid, &mut budget).is_err());
    assert_eq!(detached.triangle_count(), 0);
    let mut recovered = Mesh::new(Mode::Render, 500_000, &|| false).unwrap();
    draw(&mut recovered, &d.anchors, &mut budget).unwrap();
    assert!(recovered.triangle_count() > 0);

    let hosts = ["wall-0", "wall-1"];
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
            kit: &d.kit,
            cloth: c.cloth,
            repeats,
        })
        .collect();
    let error = draw::draw_hosts(
        outputs,
        &d.plan,
        c.seed,
        &d.street.planes,
        &d.anchors,
        &mut budget,
    )
    .unwrap_err();
    assert!(error.contains("cancel"), "{error}");
    assert!(
        partial.iter().all(|m| m.triangle_count() > 0),
        "Cancellation must follow emission to both hosts"
    );
}
#[test]
fn surveys_and_per_host_output_agree_with_the_single_drawing() {
    for c in &CASES {
        let mut budget = Budget::new(32_000_000, &|| false).unwrap();
        let d = drawn(c, &mut budget);
        let settings = draw::Settings {
            kit: &d.kit,
            cloth: c.cloth,
        };
        let mut drawings = [Mode::Render, Mode::Survey].map(|mode| {
            let mut mesh = Mesh::new(mode, 500_000, &|| false).unwrap();
            let contributors = draw::draw(
                &mut mesh,
                &d.plan,
                c.seed,
                &settings,
                &d.street.planes,
                &d.anchors,
                &|_| 1.,
                &mut budget,
            )
            .unwrap();
            (mesh, serde_json::to_value(contributors).unwrap())
        });
        let [(render, contributors), (survey, surveyed)] = &mut drawings;
        assert!(render.triangle_count() > 0, "{}", c.seed);
        assert_eq!(contributors, surveyed, "{}", c.seed);
        assert_eq!(
            render.triangle_count(),
            survey.triangle_count(),
            "{}",
            c.seed
        );
        assert!(survey.surveying());
        let expected = corners(render);
        assert_eq!(survey.survey.len(), expected.len());
        for (a, b) in survey.survey.iter().zip(&expected) {
            assert!(
                a.iter().zip(b).all(|(a, b)| (a - b).abs() <= 2e-10),
                "{}",
                c.seed
            );
        }
        weathered::finish(render, &|| false).unwrap();

        let hosts = ["wall-0", "wall-1"];
        let mut meshes = hosts
            .iter()
            .map(|_| Mesh::new(Mode::Render, 500_000, &|| false).unwrap())
            .collect::<Vec<_>>();
        let unit = |_: &str| 1.;
        let outputs = hosts
            .iter()
            .zip(&mut meshes)
            .map(|(host, mesh)| draw::Output {
                host,
                mesh,
                kit: &d.kit,
                cloth: c.cloth,
                repeats: &unit,
            })
            .collect();
        let split = draw::draw_hosts(
            outputs,
            &d.plan,
            c.seed,
            &d.street.planes,
            &d.anchors,
            &mut budget,
        )
        .unwrap();
        assert_eq!(&serde_json::to_value(split).unwrap(), contributors);
        let single = parts(render);
        let positions = corners(render);
        assert_eq!(
            meshes.iter().map(|m| m.triangle_count()).sum::<usize>(),
            render.triangle_count(),
            "{} host split keeps every triangle",
            c.seed
        );
        for (host, mesh) in hosts.iter().zip(&mut meshes) {
            let prefix = format!("{host}/$dressing/");
            let selected = single
                .iter()
                .enumerate()
                .filter_map(|(t, p)| p.starts_with(&prefix).then_some(t))
                .collect::<Vec<_>>();
            assert_eq!(selected.len(), mesh.triangle_count(), "{} {host}", c.seed);
            let local = parts(mesh);
            let corners = corners(mesh);
            for (t, &source) in selected.iter().enumerate() {
                assert_eq!(format!("{prefix}{}", local[t]), single[source]);
                for k in 0..3 {
                    let (a, b) = (corners[t * 3 + k], positions[source * 3 + k]);
                    assert!(a.iter().zip(b).all(|(a, b)| (a - b).abs() <= 2e-10));
                }
            }
            weathered::finish(mesh, &|| false).unwrap();
        }
    }
}
#[test]
fn per_host_material_repeats_keep_span_geometry() {
    let c = &CASES[0];
    let mut budget = Budget::new(32_000_000, &|| false).unwrap();
    let d = drawn(c, &mut budget);
    let draw = |repeat: [f64; 2], budget: &mut Budget<'_>| {
        let hosts = ["wall-0", "wall-1"];
        let kits = repeat.map(|r| {
            let mut settings = d.kit.settings().clone();
            settings.weathering.metal.repeat = r;
            settings.weathering.sheet.repeat = r;
            settings.weathering.paint.repeat = r;
            settings.timber.timber_repeat = r;
            settings.timber.atlas_repeat = r;
            Kit::new(settings, c.detail).unwrap()
        });
        let repeats = repeat.map(|r| move |_: &str| r);
        let mut meshes = hosts.map(|_| Mesh::new(Mode::Render, 500_000, &|| false).unwrap());
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
        draw::draw_hosts(
            outputs,
            &d.plan,
            c.seed,
            &d.street.planes,
            &d.anchors,
            budget,
        )
        .unwrap();
        meshes
    };
    let unit = draw([1., 1.], &mut budget);
    let mut routed = draw([1.7, 3.1], &mut budget);
    for (a, b) in unit.iter().zip(&mut routed) {
        assert_eq!(corners(a), corners(b));
        assert_eq!(parts(a), parts(b));
        weathered::finish(b, &|| false).unwrap();
    }
    assert!(
        unit.iter().zip(&routed).any(|(a, b)| a
            .vertices
            .iter()
            .zip(&b.vertices)
            .any(|(a, b)| a.uv != b.uv)),
        "routed repeats reach the emitted texture coordinates"
    );
}

/// Browser fixture with real garment/glyph images and a separately compiled physical support scene.
pub fn scene() -> superspec_engine::Scene {
    use std::collections::BTreeMap;
    use superspec_aml::material::Material;
    use superspec_engine::materialspec::surface::Inputs;
    use superspec_engine::{Asset, compile};
    let c = &CASES[1];
    let mut budget = Budget::new(32_000_000, &|| false).unwrap();
    let street = street(c.gap, &mut budget);
    let plan = street.plan(c.rule, c.seed, &mut budget).unwrap();
    let anchors = street.anchors();
    let planes = street.planes;
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
    let kit = kit(c.seed, c.detail, &|id| materials[id].repeat);
    let mut mesh = Mesh::new(Mode::Render, 500_000, &|| false).unwrap();
    draw::draw(
        &mut mesh,
        &plan,
        c.seed,
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
