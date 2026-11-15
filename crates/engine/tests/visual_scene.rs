#[path = "support/procedural_scene.rs"]
mod fixture;
use superspec_engine::{Scene, compile_packet};

type PhysicalMesh = (String, Vec<f32>, Vec<f32>, Vec<u32>);
fn collision(scene: &Scene) -> Vec<PhysicalMesh> {
    scene
        .manifest
        .meshes
        .iter()
        .filter(|m| scene.manifest.colliders.contains(&m.id))
        .map(|m| {
            (
                m.id.clone(),
                scene.positions[m.vertex_start * 3..(m.vertex_start + m.vertex_count) * 3].to_vec(),
                scene.normals[m.vertex_start * 3..(m.vertex_start + m.vertex_count) * 3].to_vec(),
                scene.indices[m.index_start..m.index_start + m.index_count].to_vec(),
            )
        })
        .collect()
}
#[test]
fn procedural_groups_preserve_colors_assets_collision_and_instanced_source_ancestry() {
    let original = fixture::architecture();
    let scene = fixture::scene();
    assert_eq!(collision(&original), collision(&scene));
    assert_eq!(original.asset_bytes, scene.asset_bytes);
    assert_eq!(original.manifest.colliders, scene.manifest.colliders);
    assert!(
        scene.colors[..original.colors.len()]
            .iter()
            .all(|v| *v == 1.)
    );
    let mut offset = original.manifest.vertex_count;
    let drawing = fixture::drawing();
    for (part, tint) in [
        ("assembly:paint", [0.25, 0.5, 1.5]),
        ("rail:metal", [0.75, 0., 0.25]),
    ] {
        let mesh = scene
            .manifest
            .meshes
            .iter()
            .find(|m| m.id == format!("placement/host/$dressing/{part}"))
            .unwrap();
        assert_eq!(mesh.vertex_start, offset);
        assert_eq!(mesh.material.as_deref(), Some("finish"));
        assert!(mesh.visible);
        assert!(
            scene.colors[mesh.vertex_start * 3..(mesh.vertex_start + mesh.vertex_count) * 3]
                .as_chunks::<3>()
                .0
                .iter()
                .all(|c| *c == tint)
        );
        assert!(
            scene.indices[mesh.index_start..mesh.index_start + mesh.index_count]
                .iter()
                .all(|i| *i < (mesh.vertex_count as u32))
        );
        offset += mesh.vertex_count;
        let surface = drawing
            .surfaces
            .iter()
            .position(|s| format!("{}:{}", s.part, s.slot) == part)
            .unwrap();
        let expected: Vec<_> = drawing
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .zip(&drawing.triangle_surfaces)
            .filter(|(_, s)| **s == surface)
            .flat_map(|(t, _)| t.iter())
            .map(|i| drawing.vertices[*i as usize])
            .collect();
        let actual = &scene.indices[mesh.index_start..mesh.index_start + mesh.index_count];
        assert_eq!(actual.len(), expected.len());
        for (index, vertex) in actual.iter().zip(expected) {
            let index = mesh.vertex_start + *index as usize;
            assert_eq!(
                scene.positions[index * 3..index * 3 + 3],
                vertex.position.map(|v| v as f32)
            );
            assert_eq!(
                scene.normals[index * 3..index * 3 + 3],
                vertex.normal.map(|v| v as f32)
            );
            assert_eq!(
                scene.uvs[index * 2..index * 2 + 2],
                vertex.uv.map(|v| v as f32)
            );
            assert_eq!(
                scene.colors[index * 3..index * 3 + 3],
                vertex.color.map(|v| v as f32)
            );
        }
    }
    let context = serde_json::to_value(scene.manifest.architecture.as_ref().unwrap()).unwrap();
    let objects = context["objects"].as_array().unwrap();
    assert_eq!(
        objects
            .iter()
            .filter(|o| o["id"].as_str().unwrap().contains("/$dressing/"))
            .count(),
        2
    );
    for id in [
        "placement/host/$dressing/assembly",
        "placement/host/$dressing/rail",
    ] {
        let object = objects
            .iter()
            .find(|o| o["id"] == id)
            .expect("generated source object must be published");
        assert_eq!(object["sourceId"], "host");
        assert_eq!(object["instances"], serde_json::json!(["placement"]));
        assert_eq!(object["attributes"]["host"], "placement/host");
        assert_eq!(object["attributes"]["collidable"], false);
        assert_eq!(object["attributes"]["role"], "visual-mesh");
    }
    let packet = scene.packet();
    assert_eq!(&packet[..4], b"LSP3");
    let json = u32::from_le_bytes(packet[4..8].try_into().unwrap()) as usize;
    let colors = 8 + json + scene.manifest.vertex_count * 32;
    for (bytes, expected) in packet[colors..colors + scene.colors.len() * 4]
        .as_chunks::<4>()
        .0
        .iter()
        .zip(&scene.colors)
    {
        assert_eq!(f32::from_le_bytes(*bytes), *expected);
    }
    assert_eq!(
        &packet[packet.len() - scene.asset_bytes.len()..],
        scene.asset_bytes
    );
    assert_eq!(
        packet.len(),
        8 + json
            + scene.manifest.vertex_count * 44
            + scene.manifest.index_count * 4
            + scene.asset_bytes.len()
    );
    let failed = compile_packet("<invalid/>", 72);
    assert_eq!(&failed[..4], b"LSP3");
    let n = u32::from_le_bytes(failed[4..8].try_into().unwrap()) as usize;
    assert_eq!(failed.len(), n + 8);
    let manifest: serde_json::Value = serde_json::from_slice(&failed[8..]).unwrap();
    assert_eq!(manifest["success"], false);
    assert_eq!(manifest["vertexCount"], 0);
}

#[test]
fn repeated_visual_appends_bound_material_and_inherited_identity_storage() {
    use std::collections::BTreeMap;
    use superspec_engine::{
        compile,
        worldspec::themes::kowloon::{
            mesh::{Mesh, Mode},
            pen::{Pen, Tint},
        },
    };
    use superspec_geometry::Matrix;
    let drawing = |prefix: &str, count: usize| {
        let mut mesh = Mesh::new(Mode::Render, 1000, &|| false).unwrap();
        for i in 0..count {
            Pen::new(Matrix::default(), &format!("{prefix}-{i}"))
                .unwrap()
                .panel(
                    &mut mesh,
                    "finish",
                    [0., 1., 0., 1.],
                    0.,
                    &Tint::Color([1.; 3]),
                    1.,
                )
                .unwrap();
        }
        mesh
    };
    let long = "a".repeat(65536);
    for (host, material, expected_error) in [
        ("host", long.as_str(), "Visual identity"),
        ("child", "finish", "source ancestry"),
    ] {
        let child =
            format!(r##"<box id="{host}" width="1" depth="1" height="1" material="{material}"/>"##);
        let child = if host == "child" {
            format!("<assembly id=\"{}\">{child}</assembly>", long.repeat(3))
        } else {
            child
        };
        let source = format!(
            r##"<architecture version="1" id="budget"><material id="{material}" color="#ffffff"/>{child}</architecture>"##
        );
        let mut scene = compile(&source, 0).unwrap();
        let bindings = BTreeMap::from([("finish".into(), material.into())]);
        let initial = scene.packet();
        assert!(
            scene
                .append_visual(host, &drawing("oversized", 300), &bindings)
                .is_err()
        );
        assert_eq!(scene.packet(), initial);
        scene
            .append_visual(host, &drawing("first", 60), &bindings)
            .unwrap();
        if host == "host" {
            scene
                .append_visual(host, &drawing("second", 60), &bindings)
                .unwrap();
        }
        let accepted = scene.packet();
        let error = scene
            .append_visual(host, &drawing("over-limit", 60), &bindings)
            .unwrap_err();
        assert!(error.contains(expected_error), "{error}");
        assert_eq!(scene.packet(), accepted);
        // Rejected allocations do not consume remaining capacity.
        scene
            .append_visual(host, &drawing("recovery", 1), &bindings)
            .unwrap();
    }
}
#[test]
fn append_rejection_is_atomic_across_all_groups_and_all_scene_data() {
    let mut scene = fixture::architecture();
    let original = scene.packet();
    let mut bindings = fixture::bindings();
    bindings.insert("metal".into(), "missing".into());
    assert!(
        scene
            .append_visual("placement/host", &fixture::drawing(), &bindings)
            .is_err()
    );
    assert_eq!(scene.packet(), original);
    let mut drawing = fixture::drawing();
    let first = drawing.vertices[0].color;
    for bad in [f64::INFINITY, f64::NAN, -0.1, f64::MAX] {
        drawing.vertices[0].color = [bad, 0., 0.];
        assert!(
            scene
                .append_visual("placement/host", &drawing, &fixture::bindings())
                .is_err()
        );
        assert_eq!(scene.packet(), original);
    }
    drawing.vertices[0].color = first;
    scene
        .append_visual("placement/host", &drawing, &fixture::bindings())
        .unwrap();
    let accepted = scene.packet();
    let mut collision = fixture::drawing();
    collision.surfaces[0].part = "new-first".into();
    assert!(
        scene
            .append_visual("placement/host", &collision, &fixture::bindings())
            .is_err()
    );
    assert_eq!(
        scene.packet(),
        accepted,
        "later group conflict must not publish earlier valid group"
    );
    collision.surfaces[1].part = "new-second".into();
    scene
        .append_visual("placement/host", &collision, &fixture::bindings())
        .unwrap();
    assert!(
        scene
            .manifest
            .meshes
            .iter()
            .any(|m| m.id.contains("new-second"))
    );
}
