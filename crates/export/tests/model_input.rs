use std::collections::BTreeMap;
use superspec_export::{Options, Purpose, glb, runtime_json};
use superspec_model::{
    Asset, Mesh, Scene,
    architecture::{Context, Object},
};
use superspec_types::{diagnostic::SourceLocation, material::Material};

#[test]
fn a_caller_created_model_exports_without_compiler_or_aml_construction() {
    let mut scene = Scene::empty(7, "standalone-model-test");
    scene.manifest.id = "triangle".into();
    scene.manifest.name = "Standalone triangle".into();
    scene.manifest.success = true;
    let source = SourceLocation {
        line: 1,
        column: 1,
        offset: 0,
        end: Some(1),
    };
    scene.manifest.architecture = Some(Context {
        metadata: vec![],
        declarations: vec![],
        objects: vec![Object {
            id: "triangle".into(),
            source_id: "triangle-source".into(),
            kind: "item".into(),
            source: source.clone(),
            matrix: superspec_geometry::Matrix::default().0,
            instances: vec![],
            assemblies: vec![],
            contributors: vec![],
            attributes: BTreeMap::new(),
        }],
        floors: vec![],
        water: vec![],
        water_obstacles: BTreeMap::new(),
        connections: vec![],
        connection_obstructions: BTreeMap::new(),
        volumes: vec![],
    });
    scene.manifest.materials.push(Material {
        texture: Some("color.png".into()),
        roughness: 0.4,
        metalness: 0.25,
        ..Material::default()
    });
    scene.asset_bytes = superspec_geometry::image::encode(&superspec_geometry::image::Image {
        mime: "image/png",
        width: 1,
        height: 1,
        rgba: vec![128, 64, 32, 255],
    })
    .unwrap();
    scene.manifest.assets.push(Asset {
        id: "color.png".into(),
        mime: "image/png",
        byte_start: 0,
        byte_length: scene.asset_bytes.len(),
    });
    scene.manifest.asset_byte_count = scene.asset_bytes.len();
    scene.reserve_image_pixels(1).unwrap();
    scene.manifest.meshes.push(Mesh {
        id: "triangle/render".into(),
        object_id: "triangle".into(),
        source,
        material: Some("default".into()),
        visible: true,
        vertex_start: 0,
        vertex_count: 3,
        index_start: 0,
        index_count: 3,
        bounds: [[0., 0., 0.], [1., 1., 0.]],
    });
    scene.manifest.vertex_count = 3;
    scene.manifest.index_count = 3;
    scene.manifest.colliders.push("triangle/render".into());
    scene.positions = vec![0., 0., 0., 1., 0., 0., 0., 1., 0.];
    scene.normals = vec![0., 0., 1., 0., 0., 1., 0., 0., 1.];
    scene.uvs = vec![0., 0., 1., 0., 0., 1.];
    scene.colors = vec![1.; 9];
    scene.indices = vec![0, 1, 2];
    let packet = scene.packet();
    let bytes = glb(&scene, &Options::default()).unwrap();
    assert_eq!(&bytes[..4], b"glTF");
    let json_bytes = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let document: serde_json::Value = serde_json::from_slice(&bytes[20..20 + json_bytes]).unwrap();
    assert_eq!(document["meshes"].as_array().unwrap().len(), 1);
    assert_eq!(document["extras"]["superspec"]["id"], "triangle");
    assert_eq!(
        document["extras"]["superspec"]["architecture"]["objects"][0]["sourceId"],
        "triangle-source"
    );
    assert_eq!(document["images"].as_array().unwrap().len(), 1);
    assert_eq!(
        document["materials"][0]["pbrMetallicRoughness"]["roughnessFactor"],
        0.4
    );
    assert!(
        glb(
            &scene,
            &Options {
                purpose: Purpose::Collision,
                ..Options::default()
            }
        )
        .is_ok()
    );
    let runtime: serde_json::Value =
        serde_json::from_slice(&runtime_json(&scene).unwrap()).unwrap();
    assert_eq!(runtime["manifest"]["compiler"], "standalone-model-test");
    assert_eq!(
        runtime["positions"],
        serde_json::json!([0., 0., 0., 1., 0., 0., 0., 1., 0.])
    );
    assert_eq!(scene.packet(), packet);
    let update = superspec_model::updates::encode(&scene, Some(&scene));
    assert_eq!(&update[..4], b"LSU1");
    let header_length = u32::from_le_bytes(update[4..8].try_into().unwrap()) as usize;
    let header: serde_json::Value = serde_json::from_slice(&update[8..8 + header_length]).unwrap();
    assert_eq!(header["baseRevision"], 7);
    assert_eq!(header["meshes"], serde_json::json!([0]));
    assert_eq!(header["assets"], serde_json::json!([0]));
    assert_eq!(update.len(), 8 + header_length);
    scene.manifest.meshes[0].vertex_count = 4;
    assert!(
        glb(&scene, &Options::default())
            .unwrap_err()
            .contains("Invalid export mesh range")
    );
    scene.manifest.meshes[0].vertex_count = 3;
    assert_eq!(glb(&scene, &Options::default()).unwrap(), bytes);
}
