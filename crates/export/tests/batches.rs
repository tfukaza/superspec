//! Instanced batches in packets and GLB: the mesh once, the authored placements as they are.
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};
use superspec_export::{Instancing, Options, Purpose, glb, instance_glb};
use superspec_model::{
    Scene,
    batches::{Collision, InstanceBatch, PLACEMENT_FLOATS, Placement},
    instances::{IDENTITY, Instance, LocalMesh, Model},
};
use superspec_types::{diagnostic::SourceLocation, material::Material};

fn source() -> SourceLocation {
    SourceLocation {
        line: 3,
        column: 1,
        offset: 20,
        end: None,
    }
}

fn placements() -> Vec<Placement> {
    let half = std::f64::consts::FRAC_1_SQRT_2;
    vec![
        Placement {
            translation: [1., 2., 3.],
            ..Default::default()
        },
        Placement {
            translation: [-4., 0.5, 0.],
            rotation: [0., 0., half, half],
            scale: [2., 2., 0.5],
        },
        Placement {
            translation: [0., 8., 1.25],
            rotation: [half, 0., 0., half],
            scale: [1., 1., 1.],
        },
    ]
}

/// One ordinary placed triangle and one batch of three instances of the same local mesh.
fn model(collision: Collision) -> Model {
    let mesh = LocalMesh {
        positions: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
        normals: vec![[0., 0., 1.]; 3],
        uvs: vec![[0., 0.], [1., 0.], [0., 1.]],
        colors: vec![[1.; 3]; 3],
        indices: vec![0, 1, 2],
        tangents: vec![],
    };
    Model {
        id: "placed".into(),
        name: "Placed".into(),
        meshes: BTreeMap::from([("triangle".into(), Arc::new(mesh))]),
        instances: vec![Instance {
            id: "single".into(),
            object_id: "single".into(),
            mesh: "triangle".into(),
            material: Some("steel".into()),
            transform: IDENTITY,
            source: source(),
            visible: true,
            collidable: true,
        }],
        batches: vec![InstanceBatch {
            id: "bolts".into(),
            object_id: "bolts".into(),
            source: source(),
            mesh: "triangle".into(),
            material: Some("steel".into()),
            template: None,
            visible: true,
            collision,
            placements: placements(),
            values: vec![[0.25, 0., 0., 1.], [0.5, 1., 0., 1.], [0.75, 2., 0., 1.]],
            tints: vec![[1., 1., 1.], [0.5, 0.25, 0.125], [2., 1., 1.]],
            vectors: BTreeMap::from([(
                "shadeNormal".into(),
                vec![[0., 0., 1.], [0., 1., 0.], [1., 0., 0.]],
            )]),
            scalars: BTreeMap::from([("variation".into(), vec![0.1, 0.2, 0.3])]),
            vertex_scalars: BTreeMap::from([("heightRatio".into(), vec![0., 0.5, 1.])]),
            vertex_vectors: BTreeMap::new(),
        }],
        materials: BTreeMap::from([(
            "steel".into(),
            Material {
                id: "steel".into(),
                roughness: 0.5,
                metalness: 1.,
                ..Material::default()
            },
        )]),
        ..Default::default()
    }
}

fn document(bytes: &[u8]) -> (Value, &[u8]) {
    assert_eq!(&bytes[..4], b"glTF");
    let json = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    (
        serde_json::from_slice(&bytes[20..20 + json]).unwrap(),
        &bytes[28 + json..],
    )
}

/// The floats of an accessor, read from the binary chunk.
fn floats(document: &Value, binary: &[u8], accessor: &Value) -> Vec<f32> {
    let accessor = &document["accessors"][accessor.as_u64().unwrap() as usize];
    assert_eq!(accessor["componentType"], 5126);
    let view = &document["bufferViews"][accessor["bufferView"].as_u64().unwrap() as usize];
    let components = match accessor["type"].as_str().unwrap() {
        "SCALAR" => 1,
        "VEC3" => 3,
        "VEC4" => 4,
        other => panic!("{other}"),
    };
    let start = view["byteOffset"].as_u64().unwrap() as usize;
    let count = accessor["count"].as_u64().unwrap() as usize * components;
    assert_eq!(view["byteLength"].as_u64().unwrap() as usize, count * 4);
    binary[start..start + count * 4]
        .chunks_exact(4)
        .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap()))
        .collect()
}

fn fixture(name: &str, bytes: &[u8]) {
    if let Some(directory) = std::env::var_os("BATCH_FIXTURE_OUTPUT") {
        std::fs::write(std::path::Path::new(&directory).join(name), bytes).unwrap();
    }
}

#[test]
fn a_batch_is_written_once_with_the_instancing_extension() {
    let model = model(Collision::None);
    let bytes = instance_glb(&model, &Options::default()).unwrap();
    assert_eq!(instance_glb(&model, &Options::default()).unwrap(), bytes);
    fixture("batches.glb", &bytes);
    let (document, binary) = document(&bytes);
    assert!(
        document["extensionsUsed"]
            .as_array()
            .unwrap()
            .contains(&"EXT_mesh_gpu_instancing".into())
    );
    assert_eq!(
        document["extensionsRequired"],
        serde_json::json!(["EXT_mesh_gpu_instancing"])
    );
    // The single placement and the batch's mesh: two meshes, whatever the instance count.
    assert_eq!(document["meshes"].as_array().unwrap().len(), 2);
    let nodes = document["nodes"].as_array().unwrap();
    let node = nodes.iter().find(|node| node["name"] == "bolts").unwrap();
    assert_eq!(node["extras"]["batchId"], "bolts");
    assert_eq!(node["extras"]["instances"], 3);
    assert_eq!(node["extras"]["collidable"], false);
    assert_eq!(node["extras"]["source"]["line"], 3);
    // The batch is under the root, whose rotation takes the Z-up placements to glTF's Y-up.
    let root = nodes.last().unwrap();
    let index = nodes.iter().position(|n| n["name"] == "bolts").unwrap();
    assert!(root["children"].as_array().unwrap().contains(&index.into()));
    let mesh = &document["meshes"][node["mesh"].as_u64().unwrap() as usize];
    assert_eq!(mesh["primitives"][0]["material"], 0);
    let attributes = &node["extensions"]["EXT_mesh_gpu_instancing"]["attributes"];
    // Independently of the exporter: the authored numbers, in single precision.
    let authored = placements();
    let expect = |values: Vec<f64>| values.into_iter().map(|v| v as f32).collect::<Vec<_>>();
    assert_eq!(
        floats(&document, binary, &attributes["TRANSLATION"]),
        expect(authored.iter().flat_map(|p| p.translation).collect())
    );
    assert_eq!(
        floats(&document, binary, &attributes["ROTATION"]),
        expect(authored.iter().flat_map(|p| p.rotation).collect())
    );
    assert_eq!(
        floats(&document, binary, &attributes["SCALE"]),
        expect(authored.iter().flat_map(|p| p.scale).collect())
    );
    assert_eq!(
        floats(&document, binary, &attributes["_INSTANCE"]),
        [0.25, 0., 0., 1., 0.5, 1., 0., 1., 0.75, 2., 0., 1.]
    );
    assert_eq!(
        floats(&document, binary, &attributes["_TINT"]),
        [1., 1., 1., 0.5, 0.25, 0.125, 2., 1., 1.]
    );
    // Named attributes: per-instance ones beside the placements, per-vertex ones on the primitive.
    assert_eq!(
        floats(&document, binary, &attributes["_SHADE_NORMAL"]),
        [0., 0., 1., 0., 1., 0., 1., 0., 0.]
    );
    assert_eq!(
        floats(&document, binary, &attributes["_VARIATION"]),
        [0.1, 0.2, 0.3]
    );
    assert_eq!(
        floats(
            &document,
            binary,
            &mesh["primitives"][0]["attributes"]["_HEIGHT_RATIO"]
        ),
        [0., 0.5, 1.]
    );
    // Per-instance accessors are not vertex attributes: their views name no target.
    for name in [
        "TRANSLATION",
        "ROTATION",
        "SCALE",
        "_INSTANCE",
        "_TINT",
        "_SHADE_NORMAL",
        "_VARIATION",
    ] {
        let accessor = &document["accessors"][attributes[name].as_u64().unwrap() as usize];
        assert_eq!(accessor["count"], 3, "{name}");
        let view = &document["bufferViews"][accessor["bufferView"].as_u64().unwrap() as usize];
        assert!(view.get("target").is_none(), "{name}");
    }
}

#[test]
fn the_fallback_places_one_node_per_instance_on_the_shared_mesh() {
    let model = model(Collision::None);
    let options = Options {
        instancing: Instancing::Nodes,
        ..Options::default()
    };
    let bytes = instance_glb(&model, &options).unwrap();
    fixture("batches-nodes.glb", &bytes);
    let (document, _) = document(&bytes);
    assert!(document.get("extensionsUsed").is_none());
    assert!(document.get("extensionsRequired").is_none());
    assert_eq!(document["meshes"].as_array().unwrap().len(), 2);
    let nodes: Vec<&Value> = document["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|node| node["extras"]["batchId"] == "bolts")
        .collect();
    assert_eq!(nodes.len(), 3);
    let authored = placements();
    for (index, node) in nodes.iter().enumerate() {
        assert_eq!(node["name"], format!("bolts/{index}"));
        assert_eq!(node["mesh"], nodes[0]["mesh"]);
        assert_eq!(node["extras"]["instance"], index);
        let numbers = |value: &Value| -> Vec<f32> {
            value
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap() as f32)
                .collect()
        };
        assert_eq!(
            numbers(&node["translation"]),
            authored[index].translation.map(|v| v as f32)
        );
        assert_eq!(
            numbers(&node["rotation"]),
            authored[index].rotation.map(|v| v as f32)
        );
        assert_eq!(
            numbers(&node["scale"]),
            authored[index].scale.map(|v| v as f32)
        );
    }
    assert_eq!(
        nodes[1]["extras"]["tint"],
        serde_json::json!([0.5, 0.25, 0.125])
    );
    assert_eq!(nodes[2]["extras"]["value"][0], 0.75);
}

#[test]
fn collision_exports_hold_only_colliding_batches() {
    let collision = Options {
        purpose: Purpose::Collision,
        ..Options::default()
    };
    let (render_only, _) = {
        let bytes = instance_glb(&model(Collision::None), &collision).unwrap();
        let (document, _) = document(&bytes);
        (document, ())
    };
    // The collidable single triangle alone: a render-only batch is no physical geometry.
    assert_eq!(render_only["meshes"].as_array().unwrap().len(), 1);
    assert!(
        render_only["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|node| node["extras"].get("batchId").is_none())
    );
    let bytes = instance_glb(&model(Collision::Instances), &collision).unwrap();
    let (solid, _) = document(&bytes);
    assert!(solid.get("extensionsUsed").is_none());
    let instances = solid["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|node| node["extras"]["batchId"] == "bolts")
        .count();
    assert_eq!(instances, 3);
    // And a visual export of a hidden batch omits it, whatever its collision.
    let mut hidden = model(Collision::Instances);
    hidden.batches[0].visible = false;
    let bytes = instance_glb(&hidden, &Options::default()).unwrap();
    let (visual, _) = document(&bytes);
    assert_eq!(visual["meshes"].as_array().unwrap().len(), 1);
    assert!(visual.get("extensionsUsed").is_none());
}

#[test]
fn packets_carry_batches_between_indices_and_assets_and_are_unchanged_without_them() {
    let with = model(Collision::None).scene(2).unwrap();
    let mut plain = model(Collision::None);
    plain.batches.clear();
    let without = plain.scene(2).unwrap();
    assert!(
        !String::from_utf8_lossy(&without.packet()).contains("batch"),
        "a scene without batches has the manifest it always had"
    );
    let packet = with.packet();
    fixture("batches.lsp3", &packet);
    fixture(
        "batches.lsu1",
        &superspec_model::updates::encode(&with, None),
    );
    let json = u32::from_le_bytes(packet[4..8].try_into().unwrap()) as usize;
    let manifest: Value = serde_json::from_slice(&packet[8..8 + json]).unwrap();
    assert_eq!(
        manifest["batchCounts"],
        serde_json::json!({"vertices": 3, "indices": 3, "instances": 3, "values": 3, "tints": 3, "attributes": 15})
    );
    let batch = &manifest["batches"][0];
    assert_eq!(batch["id"], "bolts");
    assert_eq!(batch["collision"], "none");
    assert_eq!(batch["instanceCount"], 3);
    assert_eq!(batch["material"], "steel");
    // Mesh data (44 bytes a vertex, 4 an index), then the batch section, then no assets.
    let section = 8 + json + 3 * 44 + 3 * 4;
    let expected = 4 * (3 * 11 + 3 + 3 * PLACEMENT_FLOATS + 3 * 4 + 3 * 3 + 15);
    assert_eq!(packet.len(), section + expected);
    let float =
        |index: usize| f32::from_le_bytes(packet[section + index * 4..][..4].try_into().unwrap());
    // The second batch vertex is (1, 0, 0); the first placement follows 33 floats and 3 indices.
    assert_eq!([float(3), float(4), float(5)], [1., 0., 0.]);
    assert_eq!([float(36), float(37), float(38)], [1., 2., 3.]);
    // The same scene exports through `glb` directly.
    assert_eq!(
        glb(&with, &Options::default()).unwrap(),
        instance_glb(&model(Collision::None), &Options::default()).unwrap()
    );
}

#[test]
fn composition_keeps_batches_and_refuses_shared_identities() {
    use superspec_model::composition::{append, prefixed};
    let base = model(Collision::None).scene(1).unwrap();
    let other = prefixed(model(Collision::None).scene(1).unwrap(), "b/");
    let composed = append(base.clone(), other).unwrap();
    let ids: Vec<_> = composed
        .manifest
        .batches
        .iter()
        .map(|batch| batch.id.as_str())
        .collect();
    assert_eq!(ids, ["b/bolts", "bolts"]);
    assert_eq!(composed.batches.counts().instances, 6);
    superspec_model::batches::check(&composed).unwrap();
    let duplicate: Scene = model(Collision::None).scene(1).unwrap();
    let mut renamed = duplicate.clone();
    for mesh in &mut renamed.manifest.meshes {
        mesh.id = format!("x/{}", mesh.id);
        mesh.object_id = format!("x/{}", mesh.object_id);
    }
    for object in &mut renamed.manifest.architecture.as_mut().unwrap().objects {
        object.id = format!("x/{}", object.id);
    }
    assert!(
        append(base, renamed)
            .unwrap_err()
            .contains("duplicate batch identities")
    );
}
