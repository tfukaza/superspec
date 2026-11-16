use super::{GeneratedItem, ImportedMaterial, ImportedTexture, ItemGeometry, ItemMesh, Values};
use crate::materialspec::program::MaterialRequest;
use serde_json::Value;
use std::mem::size_of;

fn sum(values: impl IntoIterator<Item = usize>) -> usize {
    values.into_iter().fold(0, usize::saturating_add)
}
fn text(value: &String) -> usize {
    value.capacity().saturating_add(32)
}
fn vector<T>(value: &Vec<T>) -> usize {
    value
        .capacity()
        .saturating_mul(size_of::<T>())
        .saturating_add(32)
}
// A complete BTree allocation per entry covers partly occupied nodes and child pointers.
fn map_storage<K, V>(count: usize) -> usize {
    count.saturating_mul(
        11usize
            .saturating_mul(size_of::<(K, V)>())
            .saturating_add(128),
    )
}
fn json(value: &Value) -> usize {
    match value {
        Value::String(value) => text(value),
        Value::Array(values) => sum([vector(values), sum(values.iter().map(json))]),
        Value::Object(values) => sum([
            map_storage::<String, Value>(values.len()),
            sum(values
                .iter()
                .map(|(name, value)| sum([text(name), json(value)]))),
        ]),
        _ => 0,
    }
}
fn values(values: &Values) -> usize {
    sum([
        map_storage::<String, Value>(values.len()),
        sum(values
            .iter()
            .map(|(name, value)| sum([text(name), json(value)]))),
    ])
}
fn mesh(mesh: &ItemMesh) -> usize {
    sum([
        text(&mesh.id),
        text(&mesh.material_slot),
        vector(&mesh.positions),
        vector(&mesh.normals),
        vector(&mesh.uvs),
        vector(&mesh.colors),
        vector(&mesh.tangents),
        vector(&mesh.indices),
        map_storage::<u32, Vec<[f64; 2]>>(mesh.additional_uvs.len()),
        sum(mesh.additional_uvs.values().map(vector)),
    ])
}
fn material(material: &ImportedMaterial) -> usize {
    sum([
        text(&material.id),
        material.name.as_ref().map_or(0, text),
        text(&material.alpha_mode),
        map_storage::<String, ImportedTexture>(material.textures.len()),
        sum(material.textures.iter().map(|(name, texture)| {
            sum([
                text(name),
                text(&texture.image),
                json(&texture.sampler),
                texture.transform.as_ref().map_or(0, json),
            ])
        })),
        values(&material.extensions),
    ])
}

impl ItemGeometry {
    /// Conservative retained-allocation charge, including every attribute capacity,
    /// raw material/hierarchy record and encoded image. Shared Arcs are charged in
    /// full to each owner; this is a cache admission bound, not allocator telemetry.
    pub fn retained_bytes(&self) -> usize {
        sum([
            size_of::<Self>(),
            32,
            vector(&self.meshes),
            sum(self.meshes.iter().map(mesh)),
            vector(&self.nodes),
            sum(self.nodes.iter().map(|node| {
                sum([
                    text(&node.id),
                    node.name.as_ref().map_or(0, text),
                    vector(&node.meshes),
                    node.extras.as_ref().map_or(0, json),
                ])
            })),
            map_storage::<String, ImportedMaterial>(self.imported_materials.len()),
            sum(self
                .imported_materials
                .iter()
                .map(|(name, value)| sum([text(name), material(value)]))),
            vector(&self.images),
            sum(self
                .images
                .iter()
                .map(|image| sum([text(&image.id), text(&image.mime), image.bytes.len(), 32]))),
            vector(&self.parts),
            sum(self.parts.iter().map(|part| {
                sum([
                    text(&part.id),
                    text(&part.geometry_key),
                    vector(&part.meshes),
                ])
            })),
            vector(&self.features),
            sum(self.features.iter().map(|feature| {
                sum([
                    text(&feature.name),
                    text(&feature.part),
                    feature.source.as_ref().map_or(0, text),
                    text(&feature.face),
                    vector(&feature.points),
                ])
            })),
            vector(&self.anchors),
            sum(self.anchors.iter().map(|anchor| text(&anchor.part))),
        ])
    }
}
impl GeneratedItem {
    /// Includes geometry plus current binding, attachment and provenance allocations.
    pub fn retained_bytes(&self) -> usize {
        let provenance = &self.provenance;
        sum([
            size_of::<Self>(),
            32,
            text(&self.id),
            text(&self.geometry_key),
            self.geometry.retained_bytes(),
            map_storage::<String, MaterialRequest>(self.materials.len()),
            sum(self.materials.iter().map(|(slot, request)| {
                sum([
                    text(slot),
                    text(&request.entry),
                    values(&request.parameters),
                ])
            })),
            vector(&self.attachments),
            sum(self
                .attachments
                .iter()
                .map(|attachment| text(&attachment.name))),
            vector(&self.decals),
            sum(self.decals.iter().map(|decal| {
                sum([
                    text(&decal.id),
                    text(&decal.part),
                    text(&decal.face),
                    text(&decal.image),
                    vector(&decal.positions),
                    vector(&decal.normals),
                    vector(&decal.uvs),
                    vector(&decal.indices),
                ])
            })),
            vector(&self.handles),
            sum(self
                .handles
                .iter()
                .map(|handle| sum([text(&handle.id), text(&handle.parameter)]))),
            text(&provenance.definition_id),
            text(&provenance.source),
            values(&provenance.parameters),
            map_storage::<String, String>(provenance.assets.len()),
            sum(provenance
                .assets
                .iter()
                .map(|(name, identity)| sum([text(name), text(identity)]))),
        ])
    }
}
