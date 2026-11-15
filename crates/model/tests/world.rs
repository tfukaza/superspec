use std::{collections::BTreeMap, sync::Arc};
use superspec_model::{
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
