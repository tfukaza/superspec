use std::{collections::BTreeMap, sync::Arc};
use superspec_model::{
    Scene,
    batches::{InstanceBatch, Placement, ProjectBatch, ProjectMesh, add_project_batches},
    instances::{LocalMesh, Model},
    lod::project_foliage,
};
use superspec_types::{
    batch::Foliage,
    lod::{GeometryLodGroup, GeometryLodLevel},
};

fn mesh() -> LocalMesh {
    LocalMesh {
        positions: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
        normals: vec![[0., 0., 1.]; 3],
        uvs: vec![[0., 0.], [1., 0.], [0., 1.]],
        colors: vec![[1.; 3]; 3],
        indices: vec![0, 1, 2],
        tangents: vec![],
    }
}
fn batch(id: &str, patch: &str, family: &str, level: u32, distance: f64) -> InstanceBatch {
    let foliage: Foliage = serde_json::from_value(serde_json::json!({"family":family,"patch":patch,"level":level,"distance":distance,"far":1000})).unwrap();
    InstanceBatch {
        id: id.into(),
        object_id: patch.into(),
        mesh: "local".into(),
        foliage: Some(foliage),
        placements: vec![Placement::default()],
        ..Default::default()
    }
}
fn levels() -> Vec<InstanceBatch> {
    vec![
        batch("bark0", "plant", "bark", 0, 0.),
        batch("leaves0", "plant", "leaves", 0, 0.),
        batch("bark1", "plant", "bark", 1, 24.),
        batch("hull1", "plant", "hull", 1, 24.),
        batch("shadow", "plant", "shadow", 0, 0.),
        batch("ground", "ground", "shadow", 0, 0.),
    ]
}
#[test]
fn direct_scene_projects_complete_authored_levels_and_keeps_always_on_parts() {
    let mut scene = Scene::empty(1, "test");
    for batch in levels() {
        scene.add_batch(&batch, &mesh()).unwrap();
    }
    let original = scene.manifest.batches.clone();
    project_foliage(&mut scene).unwrap();
    let group = &scene.manifest.geometry_lods[0];
    assert_eq!(group.id, "plant/foliage-lod");
    assert_eq!(
        group.levels.iter().map(|l| l.distance).collect::<Vec<_>>(),
        [0., 24.]
    );
    assert_eq!(group.levels[0].batches, ["bark0", "leaves0"]);
    assert_eq!(group.levels[1].batches, ["bark1", "hull1"]);
    assert_eq!(
        scene.manifest.batches, original,
        "visibility, collision and foliage metadata remain exact"
    );
    let packet = scene.packet();
    project_foliage(&mut scene).unwrap();
    assert_eq!(packet, scene.packet(), "projection is idempotent");
}
#[test]
fn model_and_project_batch_delivery_share_the_projection() {
    let mut source = Model {
        id: "plant".into(),
        meshes: BTreeMap::from([("local".into(), Arc::new(mesh()))]),
        batches: levels(),
        ..Default::default()
    };
    let model = source.scene(1).unwrap();
    let batches: Vec<_> = source
        .batches
        .iter()
        .map(|batch| ProjectBatch {
            batch: batch.clone(),
            mesh: ProjectMesh::Inline(mesh()),
        })
        .collect();
    let mut project = Scene::empty(1, "test");
    add_project_batches(&mut project, &batches).unwrap();
    assert_eq!(project.manifest.geometry_lods, model.manifest.geometry_lods);
    // An explicit owner can separate bark from canopy without a duplicate claim.
    source.geometry_lods.push(GeometryLodGroup {
        id: "bark-owner".into(),
        levels: [(0., "bark0"), (24., "bark1")]
            .map(|(distance, id)| GeometryLodLevel {
                distance,
                batches: vec![id.into()],
                meshes: vec![],
                screen_coverage: None,
            })
            .into(),
    });
    let explicit = source.scene(1).unwrap();
    assert_eq!(explicit.manifest.geometry_lods.len(), 2);
    assert_eq!(explicit.manifest.geometry_lods[0], source.geometry_lods[0]);
    assert_eq!(
        explicit.manifest.geometry_lods[1].levels[0].batches,
        ["leaves0"]
    );
    let prefixed = superspec_model::composition::prefixed(model, "placed/");
    assert!(
        prefixed.manifest.batches.iter().all(|b| b
            .foliage
            .as_ref()
            .unwrap()
            .patch
            .starts_with("placed/"))
    );
    assert_eq!(
        prefixed.manifest.geometry_lods[0].id,
        "placed/plant/foliage-lod"
    );
}
#[test]
fn fixed_tiers_are_not_reinterpreted_and_bad_distances_fail_before_mutation() {
    let mut scene = Scene::empty(1, "test");
    scene
        .add_batch(&batch("fixed", "fixed", "bark", 1, 24.), &mesh())
        .unwrap();
    project_foliage(&mut scene).unwrap();
    assert!(scene.manifest.geometry_lods.is_empty());
    let batches = [
        batch("a", "bad", "grass", 0, 0.),
        batch("b", "bad", "grass", 1, 24.),
        batch("c", "bad", "grass", 1, 25.),
    ]
    .map(|batch| ProjectBatch {
        batch,
        mesh: ProjectMesh::Inline(mesh()),
    });
    let before = scene.packet();
    assert!(
        add_project_batches(&mut scene, &batches)
            .unwrap_err()
            .contains("inconsistent authored distances")
    );
    assert_eq!(scene.packet(), before);
}

#[test]
fn appending_an_orphan_alternate_cannot_leave_it_always_on() {
    let mut scene = Scene::empty(1, "test");
    for batch in levels() {
        scene.add_batch(&batch, &mesh()).unwrap();
    }
    project_foliage(&mut scene).unwrap();
    let before = scene.packet();
    let addition = ProjectBatch {
        batch: batch("bark2", "plant", "bark", 2, 50.),
        mesh: ProjectMesh::Inline(mesh()),
    };
    assert!(
        add_project_batches(&mut scene, &[addition])
            .unwrap_err()
            .contains("replace its complete authored chain")
    );
    assert_eq!(before, scene.packet());
}
