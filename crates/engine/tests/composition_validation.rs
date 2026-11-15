use std::collections::BTreeSet;
use superspec_engine::{
    Scene,
    authoring::{Request, Session},
    validation,
};
use superspec_model::{Mesh, composition};

const SOURCE: &str = "<architecture version=\"1\" id=\"validation\"><box id=\"slab\" width=\"4\" depth=\"4\" height=\"0.2\"/></architecture>";
fn audited() -> Scene {
    let mut scene = superspec_engine::compile(SOURCE, 1).unwrap();
    validation::audit(&mut scene, &|| false).unwrap();
    assert_eq!(scene.manifest.geometry_validation, "checked");
    assert!(scene.manifest.validation.is_some());
    scene
}
fn pending(scene: &Scene) {
    assert_eq!(scene.manifest.geometry_validation, "not-run");
    assert_eq!(scene.manifest.navigation_validation, "not-run");
    assert!(scene.manifest.validation.is_none());
}
fn expanded(scene: &Scene, id: &str) -> Scene {
    let source = scene
        .manifest
        .meshes
        .iter()
        .find(|mesh| mesh.id == id)
        .unwrap();
    let mut result = Scene::empty(2, "composition-test");
    for index in &scene.indices[source.index_start..source.index_start + source.index_count] {
        let i = source.vertex_start + *index as usize;
        result
            .positions
            .extend_from_slice(&scene.positions[i * 3..i * 3 + 3]);
        result
            .normals
            .extend_from_slice(&scene.normals[i * 3..i * 3 + 3]);
        result
            .colors
            .extend_from_slice(&scene.colors[i * 3..i * 3 + 3]);
        result.uvs.extend_from_slice(&scene.uvs[i * 2..i * 2 + 2]);
        result.indices.push(result.indices.len() as u32);
    }
    result.manifest.vertex_count = result.positions.len() / 3;
    result.manifest.index_count = result.indices.len();
    result.manifest.meshes.push(Mesh {
        id: id.into(),
        object_id: source.object_id.clone(),
        source: source.source.clone(),
        material: None,
        visible: true,
        vertex_start: 0,
        vertex_count: result.manifest.vertex_count,
        index_start: 0,
        index_count: result.indices.len(),
        bounds: source.bounds,
    });
    result
}

#[test]
fn append_requires_a_new_audit_of_the_assembled_scene() {
    let base = audited();
    let output=Session::default().build(&Request{kind:"item".into(),entry:"pipe.item.json".into(),source:r#"{"format":"itemspec","version":1,"id":"pipe","generator":{"kind":"pipe","length":1,"radius":0.1},"collision":"none"}"#.into(),..Default::default()},2,&|name|Err(format!("Unexpected {name}")),&||false).unwrap();
    let mut composed = composition::append(base, output.scene(2).unwrap()).unwrap();
    pending(&composed);
    validation::audit(&mut composed, &|| false).unwrap();
    assert_eq!(composed.manifest.geometry_validation, "checked");
    assert!(composed.manifest.validation.is_some());
}

#[test]
fn visual_bindings_retain_audit_but_changed_physical_triangles_clear_it() {
    let base = audited();
    let report = serde_json::to_value(&base.manifest.validation).unwrap();
    let id = base
        .manifest
        .meshes
        .iter()
        .find(|mesh| mesh.visible)
        .unwrap()
        .id
        .clone();
    assert!(!base.manifest.colliders.contains(&id));
    let replacement = expanded(&base, &id);
    let visual = composition::replace_receivers(base, replacement, &BTreeSet::from([id])).unwrap();
    assert_eq!(visual.manifest.geometry_validation, "checked");
    assert_eq!(
        serde_json::to_value(&visual.manifest.validation).unwrap(),
        report
    );
    for change in [false, true] {
        let mut base = audited();
        let id = base.manifest.colliders[0].clone();
        base.manifest
            .meshes
            .iter_mut()
            .find(|mesh| mesh.id == id)
            .unwrap()
            .visible = true;
        let mut replacement = expanded(&base, &id);
        if change {
            replacement.positions[0] += 0.125;
        }
        let composed =
            composition::replace_receivers(base, replacement, &BTreeSet::from([id])).unwrap();
        if change {
            pending(&composed);
        } else {
            assert_eq!(composed.manifest.geometry_validation, "checked");
            assert!(composed.manifest.validation.is_some());
        }
    }
}
