use superspec_engine::{Scene, compile, compile_packet};

fn aml(body: &str) -> String {
    format!("<architecture version=\"1\" id=\"test\">{body}</architecture>")
}

fn volume(scene: &Scene, id: &str) -> f64 {
    scene
        .manifest
        .meshes
        .iter()
        .filter(|mesh| {
            mesh.id == format!("$collision/{id}")
                || scene.manifest.openings.iter().any(|o| {
                    o.text("host") == Some(id) && mesh.id == format!("$collision/{}", o.id)
                })
        })
        .map(|mesh| {
            let point = |index: u32| -> [f64; 3] {
                let start = (mesh.vertex_start + index as usize) * 3;
                std::array::from_fn(|axis| scene.positions[start + axis] as f64)
            };
            scene.indices[mesh.index_start..mesh.index_start + mesh.index_count]
                .as_chunks::<3>()
                .0
                .iter()
                .map(|triangle| {
                    let a = point(triangle[0]);
                    let b = point(triangle[1]);
                    let c = point(triangle[2]);
                    (a[0] * (b[1] * c[2] - b[2] * c[1])
                        + a[1] * (b[2] * c[0] - b[0] * c[2])
                        + a[2] * (b[0] * c[1] - b[1] * c[0]))
                        / 6.0
                })
                .sum::<f64>()
        })
        .sum()
}

// A y-directed ray intersects front/back faces exactly when its XZ point lies in their triangle.
fn ray_hits_wall(scene: &Scene, x: f64, z: f64) -> bool {
    let mesh = &scene.manifest.meshes[0];
    scene.indices[..mesh.index_count]
        .as_chunks::<3>()
        .0
        .iter()
        .any(|triangle| {
            let points: Vec<_> = triangle
                .iter()
                .map(|index| {
                    let at = *index as usize * 3;
                    [scene.positions[at] as f64, scene.positions[at + 2] as f64]
                })
                .collect();
            let edge = |a: [f64; 2], b: [f64; 2], p: [f64; 2]| {
                (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
            };
            let area = edge(points[0], points[1], points[2]);
            if area.abs() < 1e-12 {
                return false;
            }
            let weights = [
                edge(points[0], points[1], [x, z]),
                edge(points[1], points[2], [x, z]),
                edge(points[2], points[0], [x, z]),
            ];
            weights.iter().all(|value| *value >= -1e-10)
                || weights.iter().all(|value| *value <= 1e-10)
        })
}

#[test]
fn boxes_have_authored_bounds_outward_normals_and_physical_selection() {
    let scene=compile(&aml(r#"<box id="default"/><box id="placed" width="2" depth="4" height="3" position="5 -2 1" anchor="bottom-center" visible="false"/><box id="decor" collidable="false"/>"#), 42).unwrap();
    assert_eq!(scene.manifest.meshes[0].bounds, [[0.0; 3], [1.0; 3]]);
    assert_eq!(
        scene.manifest.meshes[1].bounds,
        [[4.0, -4.0, 1.0], [6.0, 0.0, 4.0]]
    );
    assert!(!scene.manifest.meshes[1].visible);
    assert_eq!(
        scene.manifest.colliders,
        ["$collision/default", "$collision/placed"]
    );
    assert_eq!(scene.manifest.revision, 42);
    assert_eq!(scene.manifest.navigation_validation, "not-run");
    assert!((volume(&scene, "placed") - 24.0).abs() < 1e-6);
    for mesh in &scene.manifest.meshes {
        for t in scene.indices[mesh.index_start..mesh.index_start + mesh.index_count]
            .as_chunks::<3>()
            .0
            .iter()
        {
            let at = |i: usize| &scene.positions[(mesh.vertex_start + t[i] as usize) * 3..][..3];
            let a = at(0);
            let b = at(1);
            let c = at(2);
            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let cross = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let n = &scene.normals[(mesh.vertex_start + t[0] as usize) * 3..][..3];
            assert!(cross.iter().zip(n).map(|(x, y)| x * y).sum::<f32>() > 0.0);
        }
    }
}

#[test]
fn rectangular_opening_removes_volume_only_from_its_host() {
    for center in ["3 1", "3 1.5"] {
        let source = aml(&format!(
            r#"<path id="axis" points="0 0 0; 6 0 0"/><wall id="wall" path="axis" thickness="0.5" height="3"/><opening id="hole" host="wall" center="{center}" width="2" height="2"/><box id="other" width="6" depth="0.5" height="3"/>"#
        ));
        let scene = compile(&source, 0).unwrap();
        assert!((volume(&scene, "wall") - 7.0).abs() < 1e-6);
        assert!((volume(&scene, "other") - 9.0).abs() < 1e-6);
        // Every front/back triangle lies outside the aperture's interior, not just the right total volume.
        let mesh = &scene.manifest.meshes[0];
        let low = if center == "3 1" { 0.0 } else { 0.5 };
        for (x, z) in [
            (3.0, low + 1.0),
            (2.001, low + 0.001),
            (3.999, low + 0.001),
            (2.001, low + 1.999),
            (3.999, low + 1.999),
        ] {
            assert!(
                !ray_hits_wall(&scene, x, z as f64),
                "opening ray blocked at {x},{z}"
            );
        }
        assert!(ray_hits_wall(&scene, 1.0, 1.0));
        assert!(ray_hits_wall(&scene, 3.0, 2.999));
        for t in scene.indices[..mesh.index_count].as_chunks::<3>().0.iter() {
            let points: Vec<_> = t
                .iter()
                .map(|i| &scene.positions[*i as usize * 3..][..3])
                .collect();
            let x = points.iter().map(|p| p[0]).sum::<f32>() / 3.0;
            let z = points.iter().map(|p| p[2]).sum::<f32>() / 3.0;
            assert!(!(x > 2.0 && x < 4.0 && z > low && z < low + 2.0));
        }
    }
}

#[test]
fn wall_station_follows_diagonal_path_and_elevation() {
    let scene=compile(&aml(r#"<path id="p" points="2 3 1; 5 7 1"/><wall id="w" path="p" thickness="0.5" height="3"/><opening id="o" host="w" center="2 1" width="1" height="2"/>"#),0).unwrap();
    assert!((volume(&scene, "w") - 6.5).abs() < 2e-6);
    assert_eq!(scene.manifest.meshes[0].bounds[0][2], 1.0);
}

#[test]
fn invalid_or_unsupported_input_never_returns_a_partial_scene() {
    for body in [
        r#"<box id="a" width="0"/>"#,
        r#"<box id="a" height="-1"/>"#,
        r#"<box id="a" depth="NaN"/>"#,
        r#"<box id="a" width="inf"/>"#,
        r#"<box id="a" scale="1 0 1"/>"#,
        r#"<sphere id="a" radius="-1"/>"#,
        r#"<box id="a"/><box id="a"/>"#,
        r#"<box id="a" material="missing"/>"#,
        r#"<wall id="w" path="missing"/>"#,
        r#"<opening id="o" host="missing"/>"#,
        r#"<box id="a"><box id="b"/></box>"#,
        r#"<path id="p" points="0 0 0"/>"#,
        r#"<path id="p" points="0 0 0; 4 0 0"/><wall id="w" path="p"/><opening id="o" host="w" center="8 1"/>"#,
    ] {
        let source = aml(&format!("<!-- 🏠 -->\n<box id=\"valid\"/>{body}"));
        let diagnostic = compile(&source, 7).unwrap_err();
        assert_eq!(diagnostic.source.line, 2, "{body}");
        assert!(!diagnostic.objects.is_empty());
        let packet = compile_packet(&source, 7);
        let length = u32::from_le_bytes(packet[4..8].try_into().unwrap()) as usize;
        let manifest: serde_json::Value = serde_json::from_slice(&packet[8..8 + length]).unwrap();
        assert_eq!(manifest["success"], false);
        assert_eq!(manifest["vertexCount"], 0);
        assert_eq!(packet.len(), 8 + length);
    }
    assert!(compile("<architecture", 0).is_err());
}

#[test]
fn packet_is_deterministic_and_has_exact_binary_lengths() {
    let source = include_str!("../../../tests/fixtures/aml/rust-stage1.aml");
    let scene = compile(source, 0).unwrap();
    let packet = scene.packet();
    assert_eq!(&packet[..4], b"LSP3");
    assert_eq!(packet, compile_packet(source, 0));
    let length = u32::from_le_bytes(packet[4..8].try_into().unwrap()) as usize;
    assert_eq!(length % 4, 0);
    assert_eq!(
        packet.len(),
        8 + length
            + scene.manifest.vertex_count * 44
            + scene.manifest.index_count * 4
            + scene.manifest.asset_byte_count
    );
}

#[test]
fn units_and_default_material_follow_existing_aml_semantics() {
    for material in ["", " material=\"default\""] {
        let source = aml(&format!(
            "<material id=\"default\" color=\"#123456\"/><box id=\"b\"{material}/>"
        ))
        .replace("version=\"1\"", "version=\"1\" units=\"meters\"");
        let scene = compile(&source, 0).unwrap();
        assert_eq!(
            scene
                .manifest
                .meshes
                .iter()
                .find(|m| m.id == "b")
                .unwrap()
                .material
                .as_deref(),
            Some("default")
        );
        assert_eq!(
            scene
                .manifest
                .materials
                .iter()
                .find(|m| m.id == "default")
                .unwrap()
                .color,
            "#123456"
        );
        let builtin = compile(&aml(&format!("<box id=\"b\"{material}/>")), 0).unwrap();
        assert_eq!(builtin.manifest.materials[0].color, "#b9c7d1");
    }
    for namespace in ["xmlns:p=\"urn:unused\"", "xmlns=\"\""] {
        assert!(
            compile(
                &format!("<architecture version=\"1\" id=\"a\" {namespace}/>"),
                0
            )
            .is_err()
        );
    }
}
