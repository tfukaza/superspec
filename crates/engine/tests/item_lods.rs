//! Authored ItemSpec alternatives use the shared native/Wasm authoring boundary.
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
use superspec_engine::{
    authoring::{ModelSource, Project, Request, Session},
    itemspec::definition,
};

fn source() -> Value {
    json!({"format":"itemspec","version":1,"id":"pipe",
        "parameters": {
            "segments":{"type":"integer","role":"geometry","default":24,"min":3,"max":64},
            "length":{"type":"number","role":"geometry","default":2,"min":0.1,"max":20},
            "roughness":{"type":"number","role":"finish","default":0.5,"min":0,"max":1}
        },
        "generator":{"kind":"pipe","length":{"parameter":"length"},"radius":0.2,"segments":{"parameter":"segments"},"capped":true},
        "collision":"mesh",
        "lods":[{"distance":20,"parameters":{"segments":8}},{"distance":50,"parameters":{"segments":4}}]})
}
fn absent(name: &str) -> Result<Arc<[u8]>, String> {
    Err(format!("Unexpected asset {name}"))
}
fn request(value: &Value) -> Request {
    Request {
        kind: "item-geometry".into(),
        entry: "pipe.item.json".into(),
        source: value.to_string(),
        ..Default::default()
    }
}

#[test]
fn authored_levels_keep_near_geometry_and_collision_and_have_independent_render_meshes() {
    let value = source();
    let mut near = value.clone();
    near.as_object_mut().unwrap().remove("lods");
    let baseline = Session::default()
        .build(&request(&near), 1, &absent, &|| false)
        .unwrap()
        .scene(1)
        .unwrap();
    let mut session = Session::default();
    let result = session
        .build(&request(&value), 1, &absent, &|| false)
        .unwrap();
    let scene = result.scene(1).unwrap();
    let groups = &scene.manifest.geometry_lods;
    assert_eq!(groups.len(), 1);
    assert_eq!(
        groups[0]
            .levels
            .iter()
            .map(|l| l.distance)
            .collect::<Vec<_>>(),
        vec![0., 20., 50.]
    );
    assert_eq!(
        groups[0]
            .levels
            .iter()
            .map(|l| l.meshes.len())
            .collect::<Vec<_>>(),
        vec![1, 1, 1]
    );
    let counts: Vec<_> = groups[0]
        .levels
        .iter()
        .map(|level| {
            let id = &level.meshes[0];
            scene
                .manifest
                .meshes
                .iter()
                .find(|mesh| &mesh.id == id)
                .unwrap()
                .index_count
        })
        .collect();
    assert!(counts[0] > counts[1] && counts[1] > counts[2]);
    assert_eq!(scene.manifest.colliders, baseline.manifest.colliders);
    for id in &groups[0].levels[0].meshes {
        let full = result
            .model
            .instances
            .iter()
            .find(|mesh| &mesh.id == id)
            .unwrap();
        assert!(full.collidable && full.visible);
    }
    for level in &groups[0].levels[1..] {
        for id in &level.meshes {
            let alternative = result
                .model
                .instances
                .iter()
                .find(|mesh| &mesh.id == id)
                .unwrap();
            assert!(!alternative.collidable && alternative.visible);
        }
    }
    let warm = session
        .build(&request(&value), 1, &absent, &|| false)
        .unwrap()
        .scene(1)
        .unwrap();
    assert_eq!(scene.packet(), warm.packet());
    assert_eq!(
        result.metadata["geometryKey"],
        Session::default()
            .build(&request(&near), 1, &absent, &|| false)
            .unwrap()
            .metadata["geometryKey"]
    );
}

#[test]
fn alternatives_inherit_placement_and_non_overridden_controls_in_composed_worlds() {
    let project = Project {
        kind: "material".into(),
        modules: BTreeMap::from([
            ("pipe.item.json".into(), source().to_string()),
            (
                "main.ms".into(),
                "material 1; output base_color = vec3(0.5); output roughness = 0.6;".into(),
            ),
        ]),
        models: vec![ModelSource {
            id: "fixture".into(),
            kind: "item".into(),
            entry: Some("pipe.item.json".into()),
            parameters: BTreeMap::from([("length".into(), json!(5))]),
            transform: Some([[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [3., 4., 6.]]),
            ..Default::default()
        }],
        ..Default::default()
    };
    let output = Session::default()
        .build_project(&project, 1, &absent, &|| false)
        .unwrap();
    let scene = output.scene(1).unwrap();
    let group = &scene.manifest.geometry_lods[0];
    assert_eq!(group.levels.len(), 3);
    for level in &group.levels {
        for id in &level.meshes {
            assert!(id.starts_with("fixture/"));
            let mesh = scene
                .manifest
                .meshes
                .iter()
                .find(|mesh| &mesh.id == id)
                .unwrap();
            let z: Vec<_> = scene.positions
                [mesh.vertex_start * 3..(mesh.vertex_start + mesh.vertex_count) * 3]
                .chunks_exact(3)
                .map(|p| p[2])
                .collect();
            assert_eq!(z.iter().copied().reduce(f32::min), Some(6.));
            assert_eq!(z.iter().copied().reduce(f32::max), Some(11.));
        }
    }
    assert_eq!(scene.manifest.colliders.len(), 1);
}

#[test]
fn invalid_lod_controls_report_the_authored_source_and_pointer() {
    for (level, pointer) in [
        (
            json!({"distance":0,"parameters":{"segments":8}}),
            "/lods/0/distance",
        ),
        (
            json!({"distance":20,"parameters":{"roughness":0.3}}),
            "/lods/0/parameters/roughness",
        ),
        (
            json!({"distance":20,"parameters":{"segments":2}}),
            "/lods/0/parameters/segments",
        ),
        (
            json!({"distance":20,"parameters":{"missing":8}}),
            "/lods/0/parameters/missing",
        ),
    ] {
        let mut value = source();
        value["lods"] = json!([level]);
        let error = definition::parse("pipe.item.json", &value.to_string()).unwrap_err();
        assert_eq!(error.code, "ITEM_LOD");
        assert_eq!(error.source, "pipe.item.json");
        assert_eq!(error.pointer, pointer);
    }
    let mut value = source();
    value["lods"][1]["distance"] = json!(10);
    assert_eq!(
        definition::parse("pipe.item.json", &value.to_string())
            .unwrap_err()
            .pointer,
        "/lods/1/distance"
    );
}

#[test]
fn an_alternative_edit_reuses_the_canonical_item_and_changes_only_its_render_chain() {
    let mut session = Session::default();
    let first = session
        .build(&request(&source()), 1, &absent, &|| false)
        .unwrap();
    let mut edited = source();
    edited["lods"][1]["parameters"]["segments"] = json!(3);
    let second = session
        .build(&request(&edited), 2, &absent, &|| false)
        .unwrap();
    assert_eq!(
        first.metadata["geometryKey"],
        second.metadata["geometryKey"]
    );
    let full = &first.model.instances[0];
    assert_eq!(
        first.model.meshes[&full.mesh].as_ref(),
        second.model.meshes[&full.mesh].as_ref()
    );
    let first_far = &first.model.geometry_lods[0].levels[2].meshes[0];
    let second_far = &second.model.geometry_lods[0].levels[2].meshes[0];
    let mesh = |model: &superspec_model::instances::Model, id: &String| {
        model.meshes[&model
            .instances
            .iter()
            .find(|instance| &instance.id == id)
            .unwrap()
            .mesh]
            .indices
            .len()
    };
    assert!(mesh(&second.model, second_far) < mesh(&first.model, first_far));
    assert!(
        session
            .build(&request(&edited), 3, &absent, &|| true)
            .is_err()
    );
    assert!(
        session
            .build(&request(&edited), 4, &absent, &|| false)
            .is_ok()
    );
}

#[test]
fn kowloon_static_tree_keeps_its_established_tier_and_exposes_the_authored_far_tier() {
    let source = include_str!("../src/worldspec/themes/kowloon/items/kowloon-tree.item.json");
    let value: Value = serde_json::from_str(source).unwrap();
    let output = Session::default()
        .build(&request(&value), 1, &absent, &|| false)
        .unwrap();
    assert_eq!(output.model.geometry_lods[0].levels[1].distance, 45.);
    let generated = definition::parse("tree.item.json", source).unwrap();
    assert_eq!(generated.parameters["level"].default, json!(1));
    let counts: Vec<_> = output.model.geometry_lods[0]
        .levels
        .iter()
        .map(|level| {
            level
                .meshes
                .iter()
                .map(|id| {
                    let instance = output
                        .model
                        .instances
                        .iter()
                        .find(|instance| &instance.id == id)
                        .unwrap();
                    output.model.meshes[&instance.mesh].indices.len()
                })
                .sum::<usize>()
        })
        .collect();
    assert!(counts[1] < counts[0], "{counts:?}");
}
