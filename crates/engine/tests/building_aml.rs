use std::path::Path;
use superspec_aml::material_catalog;
use superspec_engine::{
    Scene, Session, compile, compile_collision_with_assets, compile_with_assets,
};
#[path = "support/building_color_reference.rs"]
mod color_reference;
#[path = "support/building_shop_contract.rs"]
mod shop_contract;

const SOURCE: &str = include_str!("../../../tests/fixtures/aml/rust-building-dressing.aml");
const GOLDEN: &str = include_str!("../../../tests/fixtures/rust/building-aml.json");
#[test]
fn parallel_host_emission_matches_serial_packets() {
    let source = include_str!("../../../tests/fixtures/aml/rust-multiple-buildings.aml")
        .replace("width=\"6\"", "width=\"3\"")
        .replace("depth=\"6\"", "depth=\"3\"")
        .replace("height=\"6.35\"", "height=\"3.2\"")
        .replace(
            "texture-density=\"16\"",
            "texture-density=\"16\" shops=\"0\"",
        );
    let mut serial = Session::default();
    serial.set_workers(1).unwrap();
    let mut parallel = Session::default();
    parallel.set_workers(4).unwrap();
    let expected = serial.compile(&source, 7, &assets).unwrap().packet();
    assert_eq!(
        parallel.compile(&source, 7, &assets).unwrap().packet(),
        expected
    );
    assert_eq!(
        parallel.compile(&source, 7, &assets).unwrap().packet(),
        expected
    );
    assert!(parallel.bake_stats().binding_hits > 0);
    let edited = source.replacen("height=\"3.2\"", "height=\"3.4\"", 1);
    let reused = parallel.compile(&edited, 8, &assets).unwrap().packet();
    let fresh = Session::default()
        .compile(&edited, 8, &assets)
        .unwrap()
        .packet();
    assert_eq!(reused, fresh);
}
fn assets(id: &str) -> Result<Vec<u8>, String> {
    let name = material_catalog::image_name(id).ok_or_else(|| format!("Unexpected asset {id}"))?;
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/materials")
            .join(name),
    )
    .map_err(|e| e.to_string())
}
fn physical(scene: &Scene) -> serde_json::Value {
    serde_json::json!(scene.manifest.meshes.iter().filter(|m|scene.manifest.colliders.contains(&m.id)).map(|m| {
        serde_json::json!({"mesh":m,"positions":scene.positions[m.vertex_start*3..(m.vertex_start+m.vertex_count)*3],
            "normals":scene.normals[m.vertex_start*3..(m.vertex_start+m.vertex_count)*3],
            "indices":scene.indices[m.index_start..m.index_start+m.index_count]})
    }).collect::<Vec<_>>())
}
fn retained_render(scene: &Scene, hosts: &[&str]) -> serde_json::Value {
    serde_json::json!(
        scene
            .manifest
            .meshes
            .iter()
            .filter(|m| m.visible
                && !hosts.iter().any(|host| m.object_id == *host
                    || m.object_id.starts_with(&format!("{host}/$dressing/"))))
            .map(|m| serde_json::json!({
                "object": m.object_id, "material": m.material,
                "positions": scene.positions[m.vertex_start*3..(m.vertex_start+m.vertex_count)*3],
                "normals": scene.normals[m.vertex_start*3..(m.vertex_start+m.vertex_count)*3],
                "colors": scene.colors[m.vertex_start*3..(m.vertex_start+m.vertex_count)*3],
                "uvs": scene.uvs[m.vertex_start*2..(m.vertex_start+m.vertex_count)*2],
                "indices": scene.indices[m.index_start..m.index_start+m.index_count]
            }))
            .collect::<Vec<_>>()
    )
}
fn closed_references(scene: &Scene) {
    use std::collections::BTreeSet;
    assert_eq!(
        scene.manifest.materials.len(),
        scene
            .manifest
            .materials
            .iter()
            .map(|m| &m.id)
            .collect::<BTreeSet<_>>()
            .len()
    );
    assert_eq!(
        scene.manifest.assets.len(),
        scene
            .manifest
            .assets
            .iter()
            .map(|a| &a.id)
            .collect::<BTreeSet<_>>()
            .len()
    );
    for asset in &scene.manifest.assets {
        assert!(
            asset.byte_length > 0
                && asset.byte_start + asset.byte_length <= scene.asset_bytes.len()
        );
    }
    for mesh in &scene.manifest.meshes {
        if mesh.object_id.contains("/$dressing/") {
            let context =
                serde_json::to_value(scene.manifest.architecture.as_ref().unwrap()).unwrap();
            let object = context["objects"]
                .as_array()
                .unwrap()
                .iter()
                .find(|o| o["id"] == mesh.object_id)
                .unwrap();
            let host = object["attributes"]["host"].as_str().unwrap();
            assert!(mesh.object_id.starts_with(&format!("{host}/$dressing/")));
            assert_eq!(object["attributes"]["collidable"], false);
            assert_eq!(object["sourceId"], host);
        }
        assert!(mesh.vertex_start + mesh.vertex_count <= scene.positions.len() / 3);
        assert!(mesh.index_start + mesh.index_count <= scene.indices.len());
        assert!(
            scene.indices[mesh.index_start..mesh.index_start + mesh.index_count]
                .iter()
                .all(|i| (*i as usize) < mesh.vertex_count)
        );
        if let Some(id) = mesh.material.as_deref().filter(|_| mesh.visible) {
            assert!(
                scene.manifest.materials.iter().any(|m| m.id == id),
                "Missing material {id}"
            );
        }
    }
    for material in &scene.manifest.materials {
        for id in material.companions() {
            assert!(
                scene.manifest.assets.iter().any(|a| a.id == id),
                "Missing asset {id}"
            );
        }
    }
}
#[test]
fn aml_building_bakes_real_materials_without_changing_authored_collision() {
    let bare = SOURCE[..SOURCE.find("  <dressing").unwrap()].to_owned() + "</architecture>";
    let original = compile(&bare, 0).unwrap();
    let source=SOURCE.replace("  <dressing", "  <material id=\"custom-restaurant-food\" texture=\"unused.png\"/>\n  <dressing finishes=\"custom\"")
        .replace("texture-density=\"16\"","texture-density=\"16\" shops=\"1\"");
    let scene = compile_with_assets(&source, 1, &|id| {
        assert_ne!(id, "unused.png", "Unused restaurant atlas was requested");
        assets(id)
    })
    .unwrap();
    let warning = scene
        .manifest
        .diagnostics
        .iter()
        .find(|d| d.code == "DRESSING_NO_SHOP_FIT")
        .unwrap();
    assert_eq!(warning.severity, "warning");
    assert_eq!(warning.objects, ["host"]);
    assert_eq!(warning.spatial.as_ref().unwrap().inspection_target, "host");
    assert_eq!(
        warning.spatial.as_ref().unwrap().bounds,
        [[0., 0., 0.], [3., 3., 3.2]]
    );
    assert!(warning.source.line >= 8);
    let physical_only = compile_collision_with_assets(&source, 1, &|_| {
        panic!("Collision validation requested an image")
    })
    .unwrap();
    assert_eq!(
        physical_only.manifest.diagnostics,
        scene.manifest.diagnostics
    );
    assert_eq!(physical(&scene), physical(&original));
    assert_eq!(scene.manifest.colliders, original.manifest.colliders);
    assert!(
        !scene
            .manifest
            .meshes
            .iter()
            .any(|m| m.visible && m.object_id == "host")
    );
    assert!(
        scene
            .manifest
            .meshes
            .iter()
            .any(|m| m.object_id.starts_with("host/$dressing/"))
    );
    assert!(
        scene
            .manifest
            .materials
            .iter()
            .any(|m| m.id.starts_with("$surface/raster/"))
    );
    let context = serde_json::to_value(scene.manifest.architecture.as_ref().unwrap()).unwrap();
    for object in context["objects"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|o| o["id"].as_str().unwrap().starts_with("host/$dressing/"))
    {
        assert_eq!(object["attributes"]["host"], "host");
        assert_eq!(object["attributes"]["collidable"], false);
        assert_eq!(object["sourceId"], "host");
    }
    closed_references(&scene);
    reference_geometry(&scene, GOLDEN);
    assert!(!scene.packet().is_empty());
}
#[test]
fn hidden_and_collision_only_buildings_do_not_request_visual_images() {
    let no_images =
        |id: &str| -> Result<Vec<u8>, String> { panic!("Unexpected image request {id}") };
    let hidden = SOURCE.replace(
        "collidable=\"false\"",
        "collidable=\"false\" visible=\"false\"",
    );
    let scene = compile_with_assets(&hidden, 0, &no_images).unwrap();
    assert!(
        !scene
            .manifest
            .meshes
            .iter()
            .any(|m| m.object_id.starts_with("host/$dressing/"))
    );
    compile_collision_with_assets(SOURCE, 0, &no_images).unwrap();
}

#[test]
fn connected_transforms_polygon_elevation_and_final_support_match_source() {
    let cases = [
        (
            "transformed",
            include_str!("../../../tests/fixtures/aml/rust-building-transformed.aml"),
            include_str!("../../../tests/fixtures/rust/building-aml-transformed.json"),
        ),
        (
            "polygon",
            include_str!("../../../tests/fixtures/aml/rust-building-polygon.aml"),
            include_str!("../../../tests/fixtures/rust/building-aml-polygon.json"),
        ),
        (
            "shops-open",
            include_str!("../../../tests/fixtures/aml/rust-building-shops-open.aml"),
            include_str!("../../../tests/fixtures/rust/building-aml-shops-open.json"),
        ),
        (
            "shops-hole",
            include_str!("../../../tests/fixtures/aml/rust-building-shops-hole.aml"),
            include_str!("../../../tests/fixtures/rust/building-aml-shops-hole.json"),
        ),
        (
            "shops-hidden",
            include_str!("../../../tests/fixtures/aml/rust-building-shops-hidden.aml"),
            include_str!("../../../tests/fixtures/rust/building-aml-shops-hidden.json"),
        ),
    ];
    for (name, source, golden) in cases {
        let bare = source[..source.find("  <dressing").unwrap()].to_owned() + "</architecture>";
        let original = compile(&bare, 0).unwrap();
        let scene =
            compile_with_assets(source, 0, &assets).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(physical(&scene), physical(&original), "{name}");
        assert_eq!(
            retained_render(&scene, &["host"]),
            retained_render(&original, &["host"]),
            "{name}"
        );
        closed_references(&scene);
        reference_geometry(&scene, golden);
        if name.starts_with("shops-") {
            assert_eq!(
                scene
                    .manifest
                    .meshes
                    .iter()
                    .any(|m| m.object_id == "host/$dressing/shop-frontage"),
                name == "shops-open",
                "{name}"
            );
        }
    }
}

#[test]
fn shared_admission_enforces_identity_and_displacement_limits() {
    let sources = [
        SOURCE.replace("connected-building", &"s".repeat(1025)),
        SOURCE.replace("\"host\"", &format!("\"{}\"", "h".repeat(513))),
        SOURCE.replace(
            "</dressing>",
            "<building host=\"host\" material=\"metal\"/></dressing>",
        ),
        SOURCE.replace(
            "repeat=\"1.7\"",
            "repeat=\"1.7\" height-texture=\"height.png\" height-scale=\".1\"",
        ),
    ];
    for source in sources {
        let error =
            compile_collision_with_assets(&source, 0, &|_| panic!("Admission read an image"))
                .unwrap_err();
        assert!(
            matches!(
                error.code,
                "AML_DRESSING" | "AML_UNSUPPORTED" | "AML_COMPLEXITY"
            ),
            "{error}"
        );
    }
}
#[test]
fn malformed_building_recipes_fail_at_the_authoring_boundary() {
    for (old, new) in [
        ("host=\"host\"", "host=\"missing\""),
        ("collidable=\"false\"", "collidable=\"true\""),
        (
            "collidable=\"false\"",
            "collidable=\"false\" dynamic=\"true\"",
        ),
        (
            "collidable=\"false\"",
            "collidable=\"false\" scale=\"2 1 1\"",
        ),
        (
            "collidable=\"false\"",
            "collidable=\"false\" rotation=\"10 0 0\"",
        ),
        ("height=\"3.2\"", "height=\"3.1\""),
        ("width=\"3\"", "width=\"2.9\""),
        ("texture-density=\"16\"", "stories=\"1.5\""),
        ("texture-density=\"16\"", "texture-density=\"15\""),
        ("texture-density=\"16\"", "windows=\"false\""),
        ("texture-density=\"16\"", "face=\"back\""),
        (
            "detail=\"low\"",
            "detail=\"low\" wall-materials=\"metal metal\"",
        ),
        ("detail=\"low\"", "detail=\"low\" finishes=\"missing\""),
        ("texture-density=\"16\"", "base-wall=\"support\""),
    ] {
        let source = SOURCE.replace(old, new);
        let error =
            compile_collision_with_assets(&source, 0, &|_| panic!("Invalid input loaded an image"))
                .unwrap_err();
        assert!(error.source.line > 0, "{new}");
        assert!(
            matches!(error.code, "AML_DRESSING" | "AML_UNSUPPORTED"),
            "{new}: {error}"
        );
    }
}
#[test]
fn asset_failure_does_not_poison_a_session() {
    let session = Session::default();
    let reads = std::cell::Cell::new(0);
    let error = session
        .compile(SOURCE, 1, &|id| {
            reads.set(reads.get() + 1);
            if reads.get() == 2 {
                Err("missing source image".into())
            } else {
                assets(id)
            }
        })
        .unwrap_err();
    assert_eq!(reads.get(), 2);
    assert_eq!(error.code, "AML_ASSET");
    assert!(error.source.line > 0);
    let recovered = session.compile(SOURCE, 2, &assets).unwrap();
    assert!(recovered.manifest.success);
    closed_references(&recovered);
    reference_geometry(&recovered, GOLDEN);
}

#[test]
fn used_sign_height_is_rejected_before_loading_it_and_session_recovers() {
    let session = Session::default();
    let source = include_str!("../../../tests/fixtures/aml/rust-building-shops-open.aml")
        .replace("  <dressing", "  <material id=\"custom-signboard\" height-texture=\"must-not-load.png\"/>\n  <dressing finishes=\"custom\"");
    let error = session
        .compile(&source, 1, &|id| {
            assert_ne!(
                id, "must-not-load.png",
                "Unsupported sign height requested an image"
            );
            assets(id)
        })
        .unwrap_err();
    assert_eq!(error.code, "AML_DRESSING");
    assert!(error.message.contains("refinement"), "{error}");
    assert!(error.source.line > 0);
    let recovered = session
        .compile(
            &source.replace(" height-texture=\"must-not-load.png\"", ""),
            2,
            &assets,
        )
        .unwrap();
    assert!(recovered.manifest.success);
    closed_references(&recovered);
    assert!(
        recovered
            .manifest
            .meshes
            .iter()
            .any(|m| m.object_id == "host/$dressing/signage")
    );
}

fn reference_geometry(scene: &Scene, golden: &str) {
    let golden: serde_json::Value = serde_json::from_str(golden).unwrap();
    let rooms: Vec<shop_contract::Room> = serde_json::from_value(
        golden
            .get("shopRooms")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([])),
    )
    .unwrap();
    shop_contract::check(scene, &rooms);
    assert_eq!(
        serde_json::to_value(&scene.manifest.sign_plans).unwrap(),
        golden["signPlans"]
    );
    let key = |part: &str, positions: &[f32], normals: &[f32]| {
        let corners = (0..3)
            .map(|i| {
                positions[i * 3..i * 3 + 3]
                    .iter()
                    .chain(&normals[i * 3..i * 3 + 3])
                    .map(|&v| if v == 0. { 0 } else { v.to_bits() })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let rotated = (0..3)
            .map(|i| {
                [
                    corners[i].clone(),
                    corners[(i + 1) % 3].clone(),
                    corners[(i + 2) % 3].clone(),
                ]
            })
            .min()
            .unwrap();
        (part.to_owned(), rotated)
    };
    let mut expected = std::collections::BTreeMap::new();
    for t in golden["triangles"].as_array().unwrap() {
        let values = |name: &str| {
            t[name]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap() as f32)
                .collect::<Vec<_>>()
        };
        *expected
            .entry(key(
                t["part"].as_str().unwrap(),
                &values("positions"),
                &values("normals"),
            ))
            .or_insert(0usize) += 1;
    }
    let mut actual = std::collections::BTreeMap::new();
    for m in scene.manifest.meshes.iter().filter(|m| {
        m.object_id.contains("/$dressing/") && !shop_contract::native_part(&rooms, &m.object_id)
    }) {
        for t in scene.indices[m.index_start..m.index_start + m.index_count]
            .as_chunks::<3>()
            .0
        {
            let values = |data: &[f32]| {
                t.iter()
                    .flat_map(|&i| {
                        data[(m.vertex_start + i as usize) * 3
                            ..(m.vertex_start + i as usize) * 3 + 3]
                            .iter()
                            .copied()
                    })
                    .collect::<Vec<_>>()
            };
            *actual
                .entry(key(
                    &m.object_id,
                    &values(&scene.positions),
                    &values(&scene.normals),
                ))
                .or_insert(0usize) += 1;
        }
    }
    let missing = expected
        .iter()
        .filter(|(k, v)| actual.get(*k) != Some(*v))
        .count();
    let unexpected = actual
        .iter()
        .filter(|(k, v)| expected.get(*k) != Some(*v))
        .count();
    let missing_parts = expected
        .iter()
        .filter(|(k, v)| actual.get(*k) != Some(*v))
        .fold(
            std::collections::BTreeMap::new(),
            |mut parts, ((part, _), count)| {
                *parts.entry(part).or_insert(0usize) += count;
                parts
            },
        );
    assert_eq!(
        (missing, unexpected),
        (0, 0),
        "Connected source geometry differs ({} source, {} Rust triangles); missing {missing_parts:?}",
        expected.values().sum::<usize>(),
        actual.values().sum::<usize>()
    );
}
#[test]
fn connected_buildings_settle_shared_signs_and_route_automatic_spans() {
    let source = include_str!("../../../tests/fixtures/aml/rust-multiple-buildings.aml");
    let bare = source[..source.find("  <dressing").unwrap()].to_owned() + "</architecture>";
    let original = compile(&bare, 0).unwrap();
    let scene = compile_with_assets(source, 0, &assets).unwrap();
    assert_eq!(physical(&scene), physical(&original));
    assert_eq!(
        retained_render(&scene, &["host", "second"]),
        retained_render(&original, &["host", "second"])
    );
    closed_references(&scene);
    reference_geometry(
        &scene,
        include_str!("../../../tests/fixtures/rust/multiple-building-aml.json"),
    );
    let context = serde_json::to_value(scene.manifest.architecture.as_ref().unwrap()).unwrap();
    for host in ["host", "second"] {
        assert!(
            !scene
                .manifest
                .meshes
                .iter()
                .any(|m| m.visible && m.object_id == host),
            "Replaced host remained visible: {host}"
        );
        let id = format!("{host}/$dressing/span-cables");
        let object = context["objects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["id"] == id)
            .unwrap();
        let other = if host == "host" { "second" } else { "host" };
        assert_eq!(object["contributors"], serde_json::json!([host, other]));
    }
    let washing = context["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["id"] == "second/$dressing/span-washing")
        .unwrap();
    assert_eq!(
        washing["contributors"],
        serde_json::json!(["second", "host"])
    );
}

#[test]
fn hidden_neighbor_does_not_obstruct_the_visible_building() {
    let single = include_str!("../../../tests/fixtures/aml/rust-building-shops-open.aml");
    let source = single.replace("  <dressing", "  <box id=\"hidden\" width=\"3\" depth=\"3\" height=\"3.2\" position=\"1 1 0\" visible=\"false\" collidable=\"false\"/>\n  <dressing")
        .replace("</dressing>","<building host=\"hidden\" material=\"metal\" shops=\"1\" texture-density=\"16\"/></dressing>");
    let scene = compile_with_assets(&source, 0, &assets).unwrap();
    reference_geometry(
        &scene,
        include_str!("../../../tests/fixtures/rust/building-aml-shops-open.json"),
    );
    assert!(
        !scene
            .manifest
            .diagnostics
            .iter()
            .any(|d| d.objects.iter().any(|id| id == "hidden"))
    );
    assert!(
        !scene
            .manifest
            .meshes
            .iter()
            .any(|m| m.visible && m.object_id.starts_with("hidden"))
    );
}

#[test]
fn second_host_material_failure_recovers_without_leaking_first_host_state() {
    let source = include_str!("../../../tests/fixtures/aml/rust-multiple-buildings-materials.aml");
    let failing = source.replace(
        "id=\"second-coat\"",
        "id=\"second-coat\" texture=\"second.png\"",
    );
    let session = Session::default();
    let reads = std::cell::Cell::new(0usize);
    let error = session
        .compile(&failing, 1, &|id| {
            if id == "second.png" {
                return Err("Missing second host coating".into());
            }
            reads.set(reads.get() + 1);
            assets(id)
        })
        .unwrap_err();
    assert_eq!(error.code, "AML_ASSET");
    assert!(error.source.line > 0);
    assert!(reads.get() > 0, "Failure must follow first host image work");
    let scene = session.compile(source, 2, &assets).unwrap();
    closed_references(&scene);
    reference_geometry(
        &scene,
        include_str!("../../../tests/fixtures/rust/multiple-building-aml-materials.json"),
    );
    let coat = scene
        .manifest
        .materials
        .iter()
        .find(|m| m.id == "second-coat")
        .unwrap();
    color_reference::check(&scene);
    assert_eq!(coat.repeat, 3.1);
    assert!(coat.texture.is_none());
    assert!(!scene.manifest.assets.iter().any(|a| a.id == "second.png"));
}

#[test]
fn native_base_walls_keep_collision_and_match_source_building_appearance() {
    for (source, golden) in [
        (
            include_str!("../../../tests/fixtures/aml/rust-building-base-wall.aml"),
            include_str!("../../../tests/fixtures/rust/building-aml-base-wall.json"),
        ),
        (
            include_str!("../../../tests/fixtures/aml/rust-building-base-wall-transformed.aml"),
            include_str!("../../../tests/fixtures/rust/building-aml-base-wall-transformed.json"),
        ),
    ] {
        let bare = source[..source.find("  <dressing").unwrap()].to_owned() + "</architecture>";
        let original = compile(&bare, 0).unwrap();
        let scene = compile_with_assets(source, 0, &assets).unwrap();
        assert_eq!(physical(&scene), physical(&original));
        assert_eq!(
            retained_render(&scene, &["host", "base"]),
            retained_render(&original, &["host", "base"])
        );
        assert!(
            !scene
                .manifest
                .meshes
                .iter()
                .any(|m| m.visible && ["host", "base"].contains(&m.object_id.as_str()))
        );
        assert!(
            scene
                .manifest
                .meshes
                .iter()
                .any(|m| m.object_id == "host/$dressing/shop-frontage")
        );
        closed_references(&scene);
        reference_geometry(&scene, golden);
        let hidden = source.replace(
            "collidable=\"false\"",
            "collidable=\"false\" visible=\"false\"",
        );
        let hidden_bare =
            hidden[..hidden.find("  <dressing").unwrap()].to_owned() + "</architecture>";
        let hidden_original = compile(&hidden_bare, 0).unwrap();
        let hidden =
            compile_with_assets(&hidden, 0, &|_| panic!("Hidden building requested images"))
                .unwrap();
        assert_eq!(physical(&hidden), physical(&hidden_original));
        assert_eq!(
            retained_render(&hidden, &["host"]),
            retained_render(&hidden_original, &["host"])
        );
        assert!(
            !hidden
                .manifest
                .meshes
                .iter()
                .any(|m| m.object_id.starts_with("host/$dressing/"))
        );
    }
}

#[test]
fn invalid_base_wall_ownership_and_geometry_fail_before_images() {
    let source = include_str!("../../../tests/fixtures/aml/rust-building-base-wall.aml");
    let mut invalid = vec![];
    for (old, new) in [
        ("base-wall=\"base\"", "base-wall=\"missing\""),
        ("base-wall=\"base\"", "base-wall=\"support\""),
        ("id=\"base\" path", "id=\"base\" collidable=\"false\" path"),
        ("id=\"base\" path", "id=\"base\" dynamic=\"true\" path"),
        ("6 .1 0", "5 .1 0"),
        ("3 .1 0", "3 .2 0"),
        ("6 .1 0", "6 .1 .1"),
        ("0 .1 0; 3 .1 0; 6 .1 0", "0 .2 0; 3 .2 0; 6 .2 0"),
        ("0 .1 0; 3 .1 0; 6 .1 0", "0 .1 .1; 3 .1 .1; 6 .1 .1"),
        ("height=\"3\"", "height=\"7\""),
        (
            "</dressing>",
            "<floor host=\"base\" material=\"metal\"/></dressing>",
        ),
        (
            "  <dressing",
            "  <opening id=\"door\" host=\"base\" shape=\"rectangle\" center=\"3 1\" width=\"1\" height=\"2\"/>\n  <dressing",
        ),
        (
            "  <dressing",
            "  <face-material id=\"paint\" host=\"base\" face=\"top\" profile=\"ground-plan\" projection=\"xy\" material=\"metal\"/>\n  <dressing",
        ),
    ] {
        assert!(source.contains(old));
        invalid.push(source.replace(old, new));
    }
    invalid.push(source.replace("id=\"host\"","id=\"host\" visible=\"false\"").replace("  <dressing","  <box id=\"second\" width=\"6\" depth=\"6\" height=\"6.35\" collidable=\"false\"/>\n  <dressing")
        .replace("</dressing>","<building host=\"second\" material=\"metal\" base-wall=\"base\"/></dressing>"));
    let polygon = include_str!("../../../tests/fixtures/aml/rust-building-polygon.aml")
        .replace("<building ", "<building base-wall=\"base\" ");
    let error = compile_collision_with_assets(&polygon, 0, &|_| {
        panic!("Invalid polygon base requested images")
    })
    .unwrap_err();
    assert!(error.message.contains("Polygon buildings"), "{error}");
    invalid.push(
        source
            .replace("id=\"host\"", "id=\"host\" visible=\"false\"")
            .replace("6 .1 0", "5 .1 0"),
    );
    for source in invalid {
        let error = compile_collision_with_assets(&source, 0, &|_| {
            panic!("Invalid base wall requested images")
        })
        .unwrap_err();
        assert_eq!(error.code, "AML_DRESSING", "{error}");
        assert!(error.source.line > 0);
        assert!(!error.objects.is_empty(), "{error}");
    }
}
