use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
use superspec_export::{Instancing, Options, Purpose, glb, instance_glb, world_glb};
use superspec_model::{
    batches::{Collision, InstanceBatch, Placement},
    composition::{WorldModel, append, prefixed, replace_receivers},
    instances::{IDENTITY, Instance, LocalMesh, Model},
};
use superspec_types::{
    diagnostic::SourceLocation,
    lod::{GeometryLodGroup, GeometryLodLevel},
};

fn fixture() -> Model {
    let mesh = Arc::new(LocalMesh {
        positions: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
        normals: vec![[0., 0., 1.]; 3],
        uvs: vec![[0., 0.], [1., 0.], [0., 1.]],
        colors: vec![[1.; 3]; 3],
        indices: vec![0, 1, 2],
        tangents: vec![],
    });
    let source = SourceLocation {
        line: 1,
        column: 1,
        offset: 0,
        end: Some(1),
    };
    Model {
        id: "lod-fixture".into(),
        meshes: BTreeMap::from([("triangle".into(), mesh)]),
        instances: ["near", "far", "physical"]
            .into_iter()
            .map(|id| Instance {
                id: id.into(),
                object_id: id.into(),
                mesh: "triangle".into(),
                material: None,
                transform: IDENTITY,
                source: source.clone(),
                visible: id != "physical",
                collidable: true,
            })
            .collect(),
        batches: ["leaves-near", "leaves-far"]
            .into_iter()
            .map(|id| InstanceBatch {
                id: id.into(),
                object_id: id.into(),
                mesh: "triangle".into(),
                placements: vec![
                    Placement::default(),
                    Placement {
                        translation: [2., 0., 0.],
                        ..Default::default()
                    },
                ],
                collision: Collision::Instances,
                ..Default::default()
            })
            .collect(),
        geometry_lods: vec![GeometryLodGroup {
            id: "plant".into(),
            levels: vec![
                GeometryLodLevel {
                    distance: 0.,
                    meshes: vec!["near".into()],
                    batches: vec!["leaves-near".into()],
                    screen_coverage: Some(0.2),
                },
                GeometryLodLevel {
                    distance: 30.,
                    meshes: vec!["far".into()],
                    batches: vec!["leaves-far".into()],
                    screen_coverage: Some(0.),
                },
            ],
        }],
        ..Default::default()
    }
}
fn document(bytes: &[u8]) -> Value {
    let n = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    serde_json::from_slice(&bytes[20..20 + n]).unwrap()
}
fn reachable(document: &Value) -> Vec<usize> {
    fn visit(d: &Value, n: usize, out: &mut Vec<usize>) {
        assert!(!out.contains(&n), "core scene has multiply parented nodes");
        out.push(n);
        if let Some(children) = d["nodes"][n]["children"].as_array() {
            for child in children {
                visit(d, child.as_u64().unwrap() as usize, out)
            }
        }
    }
    let mut output = Vec::new();
    for root in document["scenes"][0]["nodes"].as_array().unwrap() {
        visit(document, root.as_u64().unwrap() as usize, &mut output)
    }
    output
}

#[test]
fn authored_mesh_and_batch_levels_have_one_portable_fallback_in_both_instancing_modes() {
    for instancing in [Instancing::Extension, Instancing::Nodes] {
        let model = fixture();
        let scene = model.scene(1).unwrap();
        let original = scene.packet();
        let d = document(
            &glb(
                &scene,
                &Options {
                    instancing,
                    ..Default::default()
                },
            )
            .unwrap(),
        );
        let reachable = reachable(&d);
        let reachable_meshes: Vec<_> = reachable
            .iter()
            .filter_map(|i| d["nodes"][*i]["extras"]["meshId"].as_str())
            .collect();
        assert_eq!(reachable_meshes, vec!["near"]);
        let batch_nodes: Vec<_> = reachable
            .iter()
            .filter_map(|i| d["nodes"][*i]["extras"]["batchId"].as_str())
            .collect();
        assert!(batch_nodes.iter().all(|id| *id == "leaves-near"));
        assert_eq!(
            batch_nodes.len(),
            if instancing == Instancing::Extension {
                1
            } else {
                2
            }
        );
        let (near_id, near) = d["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .find(|(_, n)| n["extensions"]["MSFT_lod"].is_object())
            .unwrap();
        assert!(reachable.contains(&near_id));
        let far_id = near["extensions"]["MSFT_lod"]["ids"][0].as_u64().unwrap() as usize;
        assert!(!reachable.contains(&far_id));
        assert_eq!(
            near["extras"]["worldSpecLod"],
            json!({"version":1,"distances":[0.,30.]})
        );
        assert_eq!(near["extras"]["MSFT_screencoverage"], json!([0.2, 0.]));
        for child in d["nodes"][far_id]["children"].as_array().unwrap() {
            assert_eq!(
                d["nodes"][child.as_u64().unwrap() as usize]["extras"]["collidable"],
                false
            );
        }
        assert!(
            d["extensionsUsed"]
                .as_array()
                .unwrap()
                .contains(&json!("MSFT_lod"))
        );
        assert!(
            !d["extensionsRequired"]
                .as_array()
                .is_some_and(|a| a.contains(&json!("MSFT_lod")))
        );
        let root = d["scenes"][0]["nodes"][0].as_u64().unwrap() as usize;
        assert_eq!(
            d["nodes"][root]["rotation"],
            json!([
                -std::f64::consts::FRAC_1_SQRT_2,
                0.,
                0.,
                std::f64::consts::FRAC_1_SQRT_2
            ])
        );
        assert_eq!(scene.packet(), original);
    }
}

#[test]
fn additional_render_levels_never_expand_collision_even_when_misflagged_by_producer() {
    let scene = fixture().scene(1).unwrap();
    let d = document(
        &glb(
            &scene,
            &Options {
                purpose: Purpose::Collision,
                ..Default::default()
            },
        )
        .unwrap(),
    );
    let names: Vec<_> = d["meshes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["near", "physical", "leaves-near"]);
    assert!(
        d["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|n| n["extensions"]["MSFT_lod"].is_null())
    );
    assert!(d["extensionsUsed"].is_null());
}

#[test]
fn invalid_or_ambiguous_authored_references_fail_at_model_and_export_boundaries() {
    let mut cases = Vec::new();
    let mut m = fixture();
    m.geometry_lods[0].levels[1].meshes[0] = "unknown".into();
    cases.push(m);
    let mut m = fixture();
    m.geometry_lods[0].levels[1].meshes[0] = "near".into();
    cases.push(m);
    let mut m = fixture();
    m.geometry_lods.push(m.geometry_lods[0].clone());
    cases.push(m);
    let mut m = fixture();
    m.geometry_lods[0].levels[0].distance = 1.;
    cases.push(m);
    let mut m = fixture();
    m.geometry_lods[0].levels[1].distance = f64::NAN;
    cases.push(m);
    let mut m = fixture();
    m.geometry_lods[0].levels[1].screen_coverage = None;
    cases.push(m);
    let mut m = fixture();
    m.instances[1].visible = false;
    cases.push(m);
    for model in cases {
        assert!(model.scene(0).is_err());
        assert!(WorldModel::from_instances(model).is_err());
    }
    let mut scene = fixture().scene(0).unwrap();
    scene.manifest.geometry_lods[0].levels[1]
        .batches
        .push("missing".into());
    assert!(glb(&scene, &Options::default()).is_err());
}

#[test]
fn grouping_survives_composition_rebinding_and_shared_model_export_paths() {
    let model = fixture();
    let scene = model.scene(9).unwrap();
    let d = document(&instance_glb(&model, &Options::default()).unwrap());
    let world = WorldModel::from_instances(model.clone()).unwrap();
    assert_eq!(
        d,
        document(&world_glb(&world, 0, &Options::default()).unwrap())
    );
    let replacement = replace_receivers(
        scene.clone(),
        scene.clone(),
        &["near".into(), "far".into()].into(),
    )
    .unwrap();
    assert_eq!(
        replacement.manifest.geometry_lods,
        scene.manifest.geometry_lods
    );
    let prefixed = prefixed(scene.clone(), "copy/");
    assert_eq!(prefixed.manifest.geometry_lods[0].id, "copy/plant");
    assert_eq!(
        prefixed.manifest.geometry_lods[0].levels[1].batches,
        vec!["copy/leaves-far"]
    );
    let joined = append(scene.clone(), prefixed).unwrap();
    assert_eq!(joined.manifest.geometry_lods.len(), 2);
    assert!(glb(&joined, &Options::default()).is_ok());
    let packet = scene.packet();
    let length = u32::from_le_bytes(packet[4..8].try_into().unwrap()) as usize;
    let manifest: Value = serde_json::from_slice(&packet[8..8 + length]).unwrap();
    assert_eq!(manifest["geometryLods"][0]["levels"][1]["distance"], 30.);
}
