use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use superspec_export::{Options, glb, prepare_lightmap_sized};
use superspec_model::Scene;

fn source() -> Scene {
    let mut scene = superspec_engine::compile(r#"<architecture version="1" id="immutable-export"><box id="box" width="2" depth="1" height="1"/><box id="hidden" position="3 0 0" width="1" depth="1" height="1" visible="false"/></architecture>"#, 9).unwrap();
    for mesh in &scene.manifest.meshes {
        let tangents = (0..mesh.vertex_count)
            .map(|index| {
                let start = (mesh.vertex_start + index) * 3;
                let n = &scene.normals[start..start + 3];
                let axis = if n[0].abs() < 0.5 {
                    [1., 0., 0.]
                } else {
                    [0., 1., 0.]
                };
                let dot = axis[0] * n[0] + axis[1] * n[1] + axis[2] * n[2];
                let direction: [f32; 3] = std::array::from_fn(|i| axis[i] - n[i] * dot);
                let length = direction.iter().map(|v| v * v).sum::<f32>().sqrt();
                [
                    direction[0] / length,
                    direction[1] / length,
                    direction[2] / length,
                    if index % 2 == 0 { 1. } else { -1. },
                ]
            })
            .collect();
        scene
            .manifest
            .mesh_tangents
            .insert(mesh.id.clone(), tangents);
        for index in 0..mesh.vertex_count {
            scene.colors[(mesh.vertex_start + index) * 3] = 0.25 + index as f32 / 128.;
        }
    }
    superspec_engine::validation::audit(&mut scene, &|| false).unwrap();
    scene
}

fn corners(scene: &Scene) -> BTreeMap<String, Vec<[f32; 3]>> {
    scene
        .manifest
        .meshes
        .iter()
        .map(|mesh| {
            let points = scene.indices[mesh.index_start..mesh.index_start + mesh.index_count]
                .iter()
                .map(|index| {
                    let start = (mesh.vertex_start + *index as usize) * 3;
                    scene.positions[start..start + 3].try_into().unwrap()
                })
                .collect();
            (mesh.id.clone(), points)
        })
        .collect()
}

#[test]
fn independent_lightmap_views_preserve_source_collision_and_authored_tangents() {
    let source = Arc::new(source());
    let original = source.packet();
    let before = glb(&source, &Options::default()).unwrap();
    let (small, large) = std::thread::scope(|scope| {
        let small = scope.spawn(|| prepare_lightmap_sized(&source, 128, &|| false).unwrap());
        let large = scope.spawn(|| prepare_lightmap_sized(&source, 256, &|| false).unwrap());
        (small.join().unwrap(), large.join().unwrap())
    });
    assert_eq!(source.packet(), original);
    assert!(source.lightmap_uvs.is_none() && source.manifest.lightmap.is_none());
    assert_eq!(source.manifest.colliders, small.manifest.colliders);
    assert_eq!(
        source.manifest.geometry_validation,
        small.manifest.geometry_validation
    );
    assert_eq!(
        source.manifest.navigation_validation,
        small.manifest.navigation_validation
    );
    assert_eq!(corners(&source), corners(&small));
    assert_eq!(corners(&source), corners(&large));
    assert_eq!(small.manifest.lightmap.as_ref().unwrap().width, 128);
    assert_eq!(large.manifest.lightmap.as_ref().unwrap().width, 256);
    assert_eq!(
        small.lightmap_uvs.as_ref().unwrap().len(),
        small.manifest.vertex_count * 2
    );
    for mesh in &small.manifest.meshes {
        let original_mesh = source
            .manifest
            .meshes
            .iter()
            .find(|old| old.id == mesh.id)
            .unwrap();
        let basis = &small.manifest.mesh_tangents[&mesh.id];
        assert_eq!(basis.len(), mesh.vertex_count);
        for (index, tangent) in basis.iter().enumerate() {
            let p = (mesh.vertex_start + index) * 3;
            let source_index = (0..original_mesh.vertex_count)
                .find(|i| {
                    let q = (original_mesh.vertex_start + i) * 3;
                    source.positions[q..q + 3] == small.positions[p..p + 3]
                        && source.normals[q..q + 3] == small.normals[p..p + 3]
                        && source.colors[q..q + 3] == small.colors[p..p + 3]
                })
                .expect("Every seam vertex retains an authored corner");
            assert_eq!(
                *tangent,
                source.manifest.mesh_tangents[&mesh.id][source_index]
            );
            let q = (original_mesh.vertex_start + source_index) * 2;
            let p = (mesh.vertex_start + index) * 2;
            assert_eq!(small.uvs[p..p + 2], source.uvs[q..q + 2]);
        }
    }
    assert!(glb(&small, &Options::default()).is_ok());
    assert!(glb(&large, &Options::default()).is_ok());
    assert_eq!(glb(&source, &Options::default()).unwrap(), before);
}

#[test]
fn failed_and_cancelled_preparation_leaves_inputs_and_other_exports_usable() {
    let source = source();
    let original = source.packet();
    let prepared = prepare_lightmap_sized(&source, 128, &|| false).unwrap();
    let ready = glb(&prepared, &Options::default()).unwrap();
    assert!(
        prepare_lightmap_sized(&source, 127, &|| false)
            .unwrap_err()
            .contains("power of two")
    );
    assert!(
        prepare_lightmap_sized(&source, 128, &|| true)
            .unwrap_err()
            .contains("cancelled")
    );
    let calls = AtomicUsize::new(0);
    assert!(
        prepare_lightmap_sized(&source, 128, &|| calls.fetch_add(1, Ordering::Relaxed) > 3)
            .is_err()
    );
    assert!(calls.load(Ordering::Relaxed) > 3);
    let mut invalid = source.clone();
    invalid.manifest.meshes[0].vertex_count = usize::MAX;
    assert!(
        prepare_lightmap_sized(&invalid, 128, &|| false)
            .unwrap_err()
            .contains("range")
    );
    assert_eq!(source.packet(), original);
    assert_eq!(glb(&prepared, &Options::default()).unwrap(), ready);
    assert_eq!(
        glb(
            &prepare_lightmap_sized(&source, 128, &|| false).unwrap(),
            &Options::default()
        )
        .unwrap(),
        ready
    );
}
