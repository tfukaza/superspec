use std::{collections::BTreeMap, sync::Arc};
use superspec_model::{
    batches::{InstanceBatch, Placement},
    composition::{Layer, Limits, WorldModel},
    instances::{IDENTITY, Instance, LocalMesh, Model},
};
use superspec_types::diagnostic::SourceLocation;

fn item(id: &str, x: f64) -> Model {
    let mesh = Arc::new(LocalMesh {
        positions: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
        normals: vec![[0., 0., 1.]; 3],
        uvs: vec![[0., 0.], [1., 0.], [0., 1.]],
        colors: vec![[1.; 3]; 3],
        indices: vec![0, 1, 2],
        tangents: vec![[1., 0., 0., 1.]; 3],
    });
    let mut transform = IDENTITY;
    transform[12] = x;
    Model {
        id: id.into(),
        name: id.into(),
        meshes: BTreeMap::from([("local".into(), mesh)]),
        mesh_artifacts: BTreeMap::from([("local".into(), "producer-v1/triangle".into())]),
        instances: vec![Instance {
            id: id.into(),
            object_id: id.into(),
            mesh: "local".into(),
            material: None,
            transform,
            source: SourceLocation {
                line: 1,
                column: 1,
                offset: 0,
                end: Some(1),
            },
            visible: true,
            collidable: true,
        }],
        ..Default::default()
    }
}

#[test]
fn shared_resources_count_once_for_retention_but_every_placement_counts_for_delivery() {
    let mut world = WorldModel::from_instances(item("first", 0.))
        .unwrap()
        .with_limits(Limits {
            vertices: 6,
            indices: 6,
            ..Default::default()
        })
        .unwrap();
    world.append(item("second", 3.)).unwrap();
    let accepted = world.scene(1).unwrap().packet();
    assert!(
        world
            .append(item("third", 6.))
            .unwrap_err()
            .contains("geometry allowance")
    );
    assert_eq!(world.layers().len(), 1);
    let mut expanded = item("first", 0.);
    expanded.mesh_artifacts.clear();
    Arc::make_mut(expanded.meshes.get_mut("local").unwrap())
        .indices
        .extend([0, 1, 2]);
    assert!(
        world
            .replace_receivers(expanded, ["first".into()].into())
            .unwrap_err()
            .contains("geometry allowance")
    );
    assert_eq!(world.scene(1).unwrap().packet(), accepted);
    world
        .replace_receivers(item("first", 0.), ["first".into()].into())
        .unwrap();
    assert_eq!(world.scene(1).unwrap().manifest.vertex_count, 6);
    assert_eq!(world.scene(1).unwrap().manifest.index_count, 6);
}

fn textured(id: &str, image_id: &str, bytes: Arc<[u8]>) -> Model {
    let mut model = item(id, 0.);
    model.images.insert(
        image_id.into(),
        superspec_model::instances::ImageResource {
            mime: "image/png",
            bytes,
            pixels: 1,
        },
    );
    let material = superspec_types::material::Material {
        id: format!("finish/{image_id}"),
        texture: Some(image_id.into()),
        ..Default::default()
    };
    model.instances[0].material = Some(material.id.clone());
    model.materials.insert(material.id.clone(), material);
    model
}
fn png(red: u8) -> Arc<[u8]> {
    superspec_geometry::image::encode(&superspec_geometry::image::Image {
        width: 1,
        height: 1,
        rgba: vec![red, 20, 30, 255],
        mime: "image/png",
    })
    .unwrap()
    .into()
}

#[test]
fn image_admission_is_aggregate_exact_and_transactional() {
    let pixels = png(10);
    let mut world = WorldModel::from_instances(textured("a", "a.png", pixels.clone()))
        .unwrap()
        .with_limits(Limits {
            image_bytes: pixels.len() * 2,
            image_pixels: 2,
            ..Default::default()
        })
        .unwrap();
    world
        .append(textured("b", "b.png", pixels.clone()))
        .unwrap();
    let accepted = world.scene(1).unwrap().packet();
    assert!(
        world
            .append(textured("c", "c.png", pixels.clone()))
            .unwrap_err()
            .contains("byte allowance")
    );
    assert!(
        world
            .append(textured("c", "a.png", png(200)))
            .unwrap_err()
            .contains("Conflicting composed image")
    );
    assert_eq!(world.layers().len(), 1);
    assert_eq!(world.scene(1).unwrap().packet(), accepted);
    world
        .append(textured("c", "a.png", pixels.clone()))
        .unwrap();
    assert_eq!(world.scene(1).unwrap().manifest.assets.len(), 2);
    let mut pixel_limited = WorldModel::from_instances(textured("a", "a.png", pixels.clone()))
        .unwrap()
        .with_limits(Limits {
            image_pixels: 1,
            ..Default::default()
        })
        .unwrap();
    assert!(
        pixel_limited
            .append(textured("b", "b.png", pixels))
            .unwrap_err()
            .contains("pixel budget")
    );
}

#[test]
fn retained_limit_counts_shared_arcs_once_and_keeps_superseded_layers_bounded() {
    let pixels = png(10);
    let base = textured("receiver", "base.png", pixels.clone());
    let mut shared = WorldModel::from_instances(base.clone()).unwrap();
    shared
        .append(textured("other", "base.png", pixels.clone()))
        .unwrap();
    let mut copied = WorldModel::from_instances(base.clone()).unwrap();
    copied
        .append(textured("other", "base.png", Arc::from(pixels.to_vec())))
        .unwrap();
    assert!(copied.retained_bytes() - shared.retained_bytes() >= pixels.len());

    let replacement = textured("receiver", "new.png", png(70));
    let selected = std::collections::BTreeSet::from(["receiver".into()]);
    let mut probe = WorldModel::from_instances(base.clone()).unwrap();
    probe
        .replace_receivers(replacement.clone(), selected.clone())
        .unwrap();
    let cap = probe.retained_bytes();
    let mut world = WorldModel::from_instances(base.clone())
        .unwrap()
        .with_limits(Limits {
            retained_bytes: cap,
            ..Default::default()
        })
        .unwrap();
    let before = world.scene(1).unwrap().packet();
    let mut oversized = replacement.clone();
    oversized.mesh_artifacts.clear();
    Arc::make_mut(oversized.meshes.get_mut("local").unwrap())
        .positions
        .reserve(200);
    assert!(
        world
            .replace_receivers(oversized, selected.clone())
            .unwrap_err()
            .contains("Retained world resource")
    );
    assert!(world.layers().is_empty());
    assert_eq!(world.scene(1).unwrap().packet(), before);
    world
        .replace_receivers(replacement.clone(), selected.clone())
        .unwrap();
    assert_eq!(world.retained_bytes(), cap);
    let accepted = world.scene(1).unwrap().packet();
    assert_eq!(world.scene(1).unwrap().manifest.assets.len(), 1);
    assert!(
        world
            .replace_receivers(textured("receiver", "last.png", png(120)), selected)
            .unwrap_err()
            .contains("Retained world resource")
    );
    assert_eq!(world.layers().len(), 1);
    assert_eq!(world.scene(1).unwrap().packet(), accepted);
}

#[test]
fn base_admission_compacts_metadata_and_counts_actual_image_pixels() {
    let mut base = superspec_model::Scene::empty(1, "caller");
    base.manifest.id = String::with_capacity(4096);
    base.manifest.id.push_str("base");
    base.manifest.meshes.reserve(64);
    base.manifest.diagnostics.reserve(64);
    base.asset_bytes = superspec_geometry::image::encode(&superspec_geometry::image::Image {
        width: 2,
        height: 1,
        rgba: vec![10, 20, 30, 255, 40, 50, 60, 255],
        mime: "image/png",
    })
    .unwrap();
    base.manifest.assets.push(superspec_model::Asset {
        id: "base.png".into(),
        mime: "image/png",
        byte_start: 0,
        byte_length: base.asset_bytes.len(),
    });
    base.manifest.asset_byte_count = base.asset_bytes.len();
    assert_eq!(base.image_pixels(), 0);
    let packet = base.packet();
    let world = WorldModel::from_scene(base).unwrap();
    let canonical = world.base_scene().unwrap();
    assert_eq!(canonical.manifest.meshes.capacity(), 0);
    assert_eq!(canonical.manifest.diagnostics.capacity(), 0);
    assert_eq!(
        canonical.manifest.id.capacity(),
        canonical.manifest.id.len()
    );
    assert_eq!(world.scene(1).unwrap().packet(), packet);
    assert_eq!(world.scene(1).unwrap().image_pixels(), 2);
    assert!(
        world
            .clone()
            .with_limits(Limits {
                image_pixels: 1,
                ..Default::default()
            })
            .unwrap_err()
            .contains("pixel budget")
    );
    let charge = world.retained_bytes();
    assert!(
        world
            .clone()
            .with_limits(Limits {
                retained_bytes: charge - 1,
                ..Default::default()
            })
            .is_err()
    );
    assert!(
        world
            .with_limits(Limits {
                retained_bytes: charge,
                ..Default::default()
            })
            .is_ok()
    );
}

#[test]
fn explicit_resource_identity_shares_ownership_and_rejects_conflicting_attributes() {
    let mut world = WorldModel::from_instances(item("first", 0.)).unwrap();
    world.append(item("second", 3.)).unwrap();
    let before = world.scene(1).unwrap().packet();
    let Layer::Append(second) = &world.layers()[0] else {
        panic!("item addition")
    };
    assert!(Arc::ptr_eq(
        &world.base_instances().unwrap().meshes["local"],
        &second.meshes["local"]
    ));
    for attribute in 0..4 {
        let mut conflicting = item("bad", 5.);
        let local = Arc::make_mut(conflicting.meshes.get_mut("local").unwrap());
        match attribute {
            0 => local.normals[0] = [0., 1., 0.],
            1 => local.uvs[0] = [0.5, 0.5],
            2 => local.colors[0] = [0.5; 3],
            _ => local.tangents[0][3] = -1.,
        }
        assert!(
            world
                .append(conflicting)
                .unwrap_err()
                .contains("Conflicting local mesh artifact")
        );
        assert_eq!(world.layers().len(), 1);
        assert_eq!(world.scene(1).unwrap().packet(), before);
    }
    let mut independent = item("independent", 6.);
    independent.mesh_artifacts.clear();
    let owned = independent.meshes["local"].clone();
    world.append(independent).unwrap();
    let Layer::Append(third) = &world.layers()[1] else {
        panic!("item addition")
    };
    assert!(Arc::ptr_eq(&third.meshes["local"], &owned));
    assert!(!Arc::ptr_eq(
        &third.meshes["local"],
        &world.base_instances().unwrap().meshes["local"]
    ));
}

fn batch_item(id: &str, owner: &str, placements: usize) -> Model {
    let mut source = item(id, 0.);
    source.instances.clear();
    source.batches.push(InstanceBatch {
        id: id.into(),
        object_id: owner.into(),
        mesh: "local".into(),
        placements: vec![Placement::default(); placements],
        ..Default::default()
    });
    source
}

fn batch_bases(source: Model) -> [WorldModel; 2] {
    [
        WorldModel::from_scene(source.scene(1).unwrap()).unwrap(),
        WorldModel::from_instances(source).unwrap(),
    ]
}

#[test]
fn batches_share_the_mesh_identity_namespace_and_owner_checks_before_layer_commit() {
    let mut source = batch_item("grass", "garden", 2);
    source.instances.push(item("tile", 0.).instances.remove(0));
    for mut world in batch_bases(source) {
        let before = world.scene(1).unwrap().packet();
        let bytes = world.retained_bytes();
        for bad in [
            batch_item("grass", "distinct", 1),
            batch_item("tile", "distinct", 1),
            item("grass", 1.),
            batch_item("pipe", "garden", 1),
        ] {
            assert!(world.append(bad).unwrap_err().contains("duplicate"));
            assert!(world.layers().is_empty());
            assert_eq!(world.retained_bytes(), bytes);
        }
        assert_eq!(world.scene(1).unwrap().packet(), before);
        world.append(batch_item("flowers", "bed", 2)).unwrap();
        assert_eq!(world.scene(2).unwrap().manifest.batches.len(), 2);
    }
}

#[test]
fn batch_geometry_counts_once_per_template_in_the_host_budget() {
    for world in batch_bases(batch_item("grass", "garden", 12)) {
        for (vertices, indices) in [(1, 3), (3, 1)] {
            assert!(
                world
                    .clone()
                    .with_limits(Limits {
                        vertices,
                        indices,
                        ..Default::default()
                    })
                    .unwrap_err()
                    .contains("geometry allowance")
            );
        }
        let mut world = world
            .with_limits(Limits {
                vertices: 6,
                indices: 6,
                ..Default::default()
            })
            .unwrap();
        world.append(item("tile", 0.)).unwrap();
        let scene = world.scene(1).unwrap();
        assert_eq!(scene.manifest.vertex_count, 3);
        let batch = scene.manifest.batch_counts.unwrap();
        assert_eq!((batch.vertices, batch.indices, batch.instances), (3, 3, 12));
        let previous = scene.packet();
        assert!(
            world
                .append(batch_item("flowers", "bed", 2))
                .unwrap_err()
                .contains("geometry allowance")
        );
        let mut larger = item("tile", 0.);
        larger.mesh_artifacts.clear();
        Arc::make_mut(larger.meshes.get_mut("local").unwrap())
            .indices
            .extend([0, 1, 2]);
        assert!(
            world
                .replace_receivers(larger, ["tile".into()].into())
                .unwrap_err()
                .contains("geometry allowance")
        );
        assert_eq!(world.scene(1).unwrap().packet(), previous);
    }
}

#[test]
fn replacement_preserves_batch_only_materials_and_checks_image_conflicts_before_commit() {
    let pixels = png(5);
    let mut source = textured("tile", "grass.png", pixels.clone());
    let material = source.instances[0].material.take();
    let mut grass = batch_item("grass", "garden", 1).batches.pop().unwrap();
    grass.material = material;
    source.batches.push(grass);
    for mut world in batch_bases(source) {
        world
            .replace_receivers(item("tile", 0.), ["tile".into()].into())
            .unwrap();
        let scene = world.scene(1).unwrap();
        assert_eq!(
            scene.manifest.batches[0].material.as_deref(),
            Some("finish/grass.png")
        );
        assert_eq!(scene.manifest.materials[0].id, "finish/grass.png");
        assert_eq!(scene.asset_bytes.as_slice(), pixels.as_ref());
        assert!(
            world
                .append(textured("duplicate-image", "grass.png", png(12)))
                .unwrap_err()
                .contains("Conflicting composed image")
        );
        assert_eq!(world.layers().len(), 1);
        assert_eq!(world.scene(1).unwrap().packet(), scene.packet());
    }
}

#[test]
fn batch_retention_charges_reserved_source_attributes_and_packed_scene_capacity() {
    let small = batch_item("grass", "garden", 1);
    let large = batch_item("grass", "garden", 2048);
    let small_source = WorldModel::from_instances(small.clone())
        .unwrap()
        .retained_bytes();
    let large_source = WorldModel::from_instances(large.clone())
        .unwrap()
        .retained_bytes();
    assert!(large_source - small_source >= 2047 * std::mem::size_of::<Placement>());
    let small_scene = small.scene(1).unwrap();
    let large_scene = large.scene(1).unwrap();
    let growth = (large_scene.batches.placements.capacity()
        - small_scene.batches.placements.capacity())
        * std::mem::size_of::<f32>();
    assert!(
        WorldModel::from_scene(large_scene)
            .unwrap()
            .retained_bytes()
            - WorldModel::from_scene(small_scene)
                .unwrap()
                .retained_bytes()
            >= growth
    );
    let mut source = batch_item("grass", "garden", 1);
    let mut weights = Vec::with_capacity(4096);
    weights.push(0.5);
    source.batches[0].scalars.insert("weight".into(), weights);
    assert!(
        WorldModel::from_instances(source).unwrap().retained_bytes() - small_source >= 4096 * 4
    );
    let mut world = WorldModel::from_instances(item("tile", 0.))
        .unwrap()
        .with_limits(Limits {
            retained_bytes: small_source + 32_000,
            ..Default::default()
        })
        .unwrap();
    let before = world.scene(1).unwrap().packet();
    assert!(
        world
            .append(large)
            .unwrap_err()
            .contains("Retained world resource allowance")
    );
    assert!(world.layers().is_empty());
    assert_eq!(world.scene(1).unwrap().packet(), before);
}

#[test]
fn rejected_source_batch_data_has_the_same_diagnostic_at_admission_and_delivery() {
    let mut named = batch_item("grass", "garden", 1);
    named.batches[0]
        .vertex_scalars
        .insert("density".into(), vec![0.2; 2]);
    let mut bounds = batch_item("grass", "garden", 1);
    bounds.batches[0].placements[0].translation = [1e9, 0., 0.];
    for source in [named, bounds] {
        let delivery_error = source.scene(1).unwrap_err();
        assert!(delivery_error.contains("grass"));
        assert_eq!(
            WorldModel::from_instances(source.clone()).unwrap_err(),
            delivery_error
        );
        let mut world = WorldModel::from_instances(item("tile", 0.)).unwrap();
        let before = world.scene(1).unwrap().packet();
        assert_eq!(
            world
                .replace_receivers(source, ["tile".into()].into())
                .unwrap_err(),
            delivery_error
        );
        assert!(world.layers().is_empty());
        assert_eq!(world.scene(1).unwrap().packet(), before);
    }
}

#[test]
fn implicit_foliage_level_identity_is_checked_before_appending_another_patch() {
    let plant = |id: &str| {
        let mut model = batch_item(id, id, 1);
        let mut far = model.batches[0].clone();
        far.id = format!("{id}-far");
        model.batches.push(far);
        for (level, batch) in model.batches.iter_mut().enumerate() {
            batch.foliage = Some(serde_json::from_value(serde_json::json!({
                "family":"grass", "patch":"shared", "level":level, "distance":level * 20, "far":100
            })).unwrap());
        }
        model
    };
    for mut world in batch_bases(plant("first")) {
        let previous = world.scene(1).unwrap().packet();
        assert!(
            world
                .append(plant("second"))
                .unwrap_err()
                .contains("duplicate geometry LOD")
        );
        assert!(world.layers().is_empty());
        assert_eq!(world.scene(1).unwrap().packet(), previous);
    }
}

#[test]
fn scene_admission_rejects_inconsistent_packed_batch_records() {
    let mut model = batch_item("grass", "garden", 1);
    model.instances.push(item("tile", 0.).instances.remove(0));
    let scene = model.scene(1).unwrap();
    for (label, mutate) in [
        (
            "identity",
            (|b| b.id = "tile".into()) as fn(&mut superspec_model::batches::Batch),
        ),
        ("missing material", |b| b.material = Some("absent".into())),
        ("range", |b| b.vertex_count = usize::MAX),
    ] {
        let mut invalid = scene.clone();
        mutate(&mut invalid.manifest.batches[0]);
        assert!(WorldModel::from_scene(invalid).unwrap_err().contains(label));
    }
}
