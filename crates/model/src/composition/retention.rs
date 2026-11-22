//! Capacity-based ownership charges shared across persistent bases and superseded layers.
use crate::{Scene, batches::InstanceBatch, instances::Model};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use superspec_types::material::Material;

fn json_size(value: &impl serde::Serialize) -> usize {
    // Conservative charge for nested owned records and map allocation overhead.
    serde_json::to_vec(value).map_or(usize::MAX, |bytes| bytes.len().saturating_mul(4))
}
fn vector<T>(values: &Vec<T>) -> usize {
    values.capacity().saturating_mul(std::mem::size_of::<T>())
}
pub(super) fn material_bytes(material: &Material) -> usize {
    std::mem::size_of::<Material>()
        + material.id.capacity()
        + material.color.capacity()
        + [
            &material.emissive,
            &material.texture,
            &material.normal_texture,
            &material.metallic_roughness_texture,
            &material.height_texture,
            &material.emissive_texture,
            &material.occlusion_texture,
        ]
        .into_iter()
        .flatten()
        .map(|s| s.capacity() + 32)
        .sum::<usize>()
        + material
            .live
            .iter()
            .map(|operation| {
                std::mem::size_of_val(operation)
                    + operation.id.capacity()
                    + operation.stage.capacity()
                    + operation
                        .requires
                        .iter()
                        .map(|r| r.capacity() + 24)
                        .sum::<usize>()
                    + operation
                        .arguments
                        .iter()
                        .map(|(name, argument)| {
                            name.capacity()
                                + 64
                                + match argument {
                                    superspec_types::material::LiveArgument::Texture {
                                        texture,
                                        ..
                                    } => texture.capacity(),
                                    superspec_types::material::LiveArgument::Constant {
                                        ..
                                    } => 0,
                                }
                        })
                        .sum::<usize>()
            })
            .sum::<usize>()
}

fn attribute_bytes<T>(attributes: &BTreeMap<String, Vec<T>>) -> usize {
    attributes
        .iter()
        .map(|(name, values)| 128 + name.capacity() + vector(values))
        .sum()
}

fn batch_bytes(batch: &InstanceBatch) -> usize {
    let mut bytes = batch.id.capacity()
        + batch.object_id.capacity()
        + batch.mesh.capacity()
        + batch.material.as_ref().map_or(0, String::capacity)
        + batch.template.as_ref().map_or(0, String::capacity)
        + vector(&batch.placements)
        + vector(&batch.values)
        + vector(&batch.tints)
        + attribute_bytes(&batch.scalars)
        + attribute_bytes(&batch.vectors)
        + attribute_bytes(&batch.vertex_scalars)
        + attribute_bytes(&batch.vertex_vectors);
    if let Some(foliage) = &batch.foliage {
        bytes += foliage.patch.capacity() + foliage.material.as_ref().map_or(0, String::capacity);
        if let Some(mass) = &foliage.mass {
            bytes += mass.look.capacity()
                + vector(&mass.sites)
                + vector(&mass.twigs)
                + mass.twigs.iter().map(vector).sum::<usize>();
        }
    }
    bytes
}

/// Counts distinct live Arc payloads once, including superseded layer resources.
pub(super) fn retained_bytes<'a>(
    scene: Option<&Scene>,
    models: impl Iterator<Item = &'a Model>,
) -> usize {
    let mut total = 0usize;
    let mut charge = |bytes: usize| total = total.saturating_add(bytes);
    if let Some(scene) = scene {
        charge(std::mem::size_of::<Scene>());
        charge(json_size(&scene.manifest));
        for values in [&scene.positions, &scene.normals, &scene.uvs, &scene.colors] {
            charge(vector(values));
        }
        for stream in [
            &scene.batches.positions,
            &scene.batches.normals,
            &scene.batches.uvs,
            &scene.batches.colors,
            &scene.batches.placements,
            &scene.batches.values,
            &scene.batches.tints,
            &scene.batches.attributes,
        ] {
            charge(vector(stream));
        }
        charge(vector(&scene.batches.indices));
        charge(vector(&scene.indices));
        charge(vector(&scene.asset_bytes));
        if let Some(uvs) = &scene.lightmap_uvs {
            charge(vector(uvs));
        }
        if let Some(charts) = &scene.lightmap_charts {
            charge(vector(charts));
        }
        charge(vector(&scene.export_solids));
        for id in scene.image_sizes.keys() {
            charge(id.capacity() + 128);
        }
        for solid in &scene.export_solids {
            charge(vector(&solid.geometry.points));
            charge(vector(&solid.geometry.triangles));
            charge(vector(&solid.faces));
            for face in &solid.faces {
                charge(face.material.capacity() + face.object.capacity());
            }
        }
    }
    let mut meshes = BTreeSet::new();
    let mut images = BTreeSet::new();
    for model in models {
        charge(json_size(&model.geometry_lods));
        charge(
            std::mem::size_of::<Model>()
                + model.id.capacity()
                + model.name.capacity()
                + vector(&model.instances)
                + vector(&model.batches),
        );
        for batch in &model.batches {
            charge(batch_bytes(batch));
        }
        for instance in &model.instances {
            charge(
                instance.id.capacity()
                    + instance.object_id.capacity()
                    + instance.mesh.capacity()
                    + instance.material.as_ref().map_or(0, String::capacity),
            );
        }
        for (key, mesh) in &model.meshes {
            charge(key.capacity() + 256);
            if meshes.insert(Arc::as_ptr(mesh) as usize) {
                charge(std::mem::size_of_val(mesh.as_ref()) + 128);
                charge(vector(&mesh.positions));
                charge(vector(&mesh.normals));
                charge(vector(&mesh.uvs));
                charge(vector(&mesh.colors));
                charge(vector(&mesh.tangents));
                charge(vector(&mesh.indices));
            }
        }
        for (key, identity) in &model.mesh_artifacts {
            charge(key.capacity() + identity.capacity() + 256);
        }
        for (key, material) in &model.materials {
            charge(key.capacity() + 256 + material_bytes(material));
        }
        for (key, image) in &model.images {
            charge(key.capacity() + 256);
            if images.insert(image.bytes.as_ptr() as usize) {
                charge(image.bytes.len() + 64);
            }
        }
    }
    total
}
