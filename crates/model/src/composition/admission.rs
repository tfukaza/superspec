//! Admission summaries contain no expanded vertex or encoded-image copies.
use crate::{
    MAX_SCENE_IMAGE_BYTES, MAX_SCENE_IMAGE_PIXELS, Scene,
    instances::{Model, determinant},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use superspec_types::material::Material;

/// Hosts may lower these limits. Retained bytes also bound superseded visual layers.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub vertices: usize,
    pub indices: usize,
    pub image_bytes: usize,
    pub image_pixels: usize,
    pub retained_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            vertices: 6_000_000,
            indices: 18_000_000,
            image_bytes: MAX_SCENE_IMAGE_BYTES,
            image_pixels: MAX_SCENE_IMAGE_PIXELS,
            retained_bytes: 1024 * 1024 * 1024,
        }
    }
}
impl Limits {
    pub(super) fn validate(self) -> Result<(), String> {
        let maximum = Self::default();
        if self.vertices > maximum.vertices
            || self.indices > maximum.indices
            || self.image_bytes > maximum.image_bytes
            || self.image_pixels > maximum.image_pixels
            || self.retained_bytes > maximum.retained_bytes
        {
            return Err("World limits may only lower the supported allowances.".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
struct Mesh {
    vertices: usize,
    indices: usize,
    visible: bool,
    material: Option<String>,
}
#[derive(Clone, Debug)]
enum Bytes {
    Scene(Arc<Scene>, usize, usize),
    Image(Arc<[u8]>),
}
impl Bytes {
    fn bytes(&self) -> &[u8] {
        match self {
            Self::Scene(scene, start, len) => &scene.asset_bytes[*start..*start + *len],
            Self::Image(bytes) => bytes,
        }
    }
}
#[derive(Clone, Debug)]
struct Image {
    mime: &'static str,
    bytes: Bytes,
    pixels: usize,
}
#[derive(Clone, Debug, Default)]
pub(super) struct Admission {
    meshes: BTreeMap<String, Mesh>,
    owners: BTreeSet<String>,
    materials: BTreeMap<String, Material>,
    images: BTreeMap<String, Image>,
    vertices: usize,
    indices: usize,
    encoded_bytes: usize,
    pixels: usize,
}
impl Admission {
    pub(super) fn scene(scene: Arc<Scene>) -> Result<Self, String> {
        super::ranges(&scene)?;
        let mut result = Self::default();
        for mesh in &scene.manifest.meshes {
            if result
                .meshes
                .insert(
                    mesh.id.clone(),
                    Mesh {
                        vertices: mesh.vertex_count,
                        indices: mesh.index_count,
                        visible: mesh.visible,
                        material: mesh.material.clone(),
                    },
                )
                .is_some()
            {
                return Err("Composed scenes contain duplicate mesh identities.".into());
            }
        }
        if let Some(context) = &scene.manifest.architecture {
            result
                .owners
                .extend(context.objects.iter().map(|object| object.id.clone()));
        }
        for material in &scene.manifest.materials {
            result
                .materials
                .insert(material.id.clone(), material.clone());
        }
        for image in &scene.manifest.assets {
            result.images.insert(
                image.id.clone(),
                Image {
                    mime: image.mime,
                    bytes: Bytes::Scene(scene.clone(), image.byte_start, image.byte_length),
                    pixels: super::image_pixels(&scene, image)?,
                },
            );
        }
        result.vertices = scene.positions.len() / 3;
        result.indices = scene.indices.len();
        result.encoded_bytes = scene.asset_bytes.len();
        result.pixels = scene.image_pixels().max(
            result
                .images
                .values()
                .fold(0usize, |sum, image| sum.saturating_add(image.pixels)),
        );
        Ok(result)
    }
    pub(super) fn model(model: &Model) -> Result<Self, String> {
        if model.meshes.len() > 16384
            || model.instances.len() > 16384
            || model.materials.len() > 16384
            || model.images.len() > 16384
        {
            return Err("Instance model exceeds its resource/placement allowance.".into());
        }
        let mut result = Self::default();
        for (id, image) in &model.images {
            if id.is_empty()
                || id.len() > 4096
                || image.bytes.is_empty()
                || !matches!(image.mime, "image/png" | "image/jpeg" | "image/ktx2")
            {
                return Err("Instance model image has invalid identity, encoding or bytes.".into());
            }
            result.images.insert(
                id.clone(),
                Image {
                    mime: image.mime,
                    bytes: Bytes::Image(image.bytes.clone()),
                    pixels: image.pixels,
                },
            );
        }
        for (id, material) in &model.materials {
            if id != &material.id || id.is_empty() || id.len() > 4096 {
                return Err("Material identity disagrees with its resource key.".into());
            }
            material.validate()?;
            for image in material.companions() {
                if !model.images.contains_key(image) {
                    return Err(format!("Material {id} references missing image {image}."));
                }
            }
            result.materials.insert(id.clone(), material.clone());
        }
        for mesh in model.meshes.values() {
            mesh.validate()?;
        }
        for instance in &model.instances {
            if instance.id.is_empty()
                || instance.id.len() > 4096
                || instance.object_id.is_empty()
                || instance.object_id.len() > 4096
            {
                return Err("Instance identity is invalid or duplicated.".into());
            }
            let mesh = model
                .meshes
                .get(&instance.mesh)
                .ok_or("Instance references missing mesh.")?;
            determinant(&instance.transform)?;
            if instance
                .material
                .as_ref()
                .is_some_and(|id| !model.materials.contains_key(id))
            {
                return Err("Instance references a missing material.".into());
            }
            if result
                .meshes
                .insert(
                    instance.id.clone(),
                    Mesh {
                        vertices: mesh.positions.len(),
                        indices: mesh.indices.len(),
                        visible: instance.visible,
                        material: instance.material.clone(),
                    },
                )
                .is_some()
            {
                return Err("Instance identity is invalid or duplicated.".into());
            }
            result.owners.insert(instance.object_id.clone());
        }
        result.recount();
        result.check(Limits {
            vertices: 1_500_000,
            indices: 9_000_000,
            ..Limits::default()
        })?;
        Ok(result)
    }
    pub(super) fn append(&self, addition: Self, limits: Limits) -> Result<Self, String> {
        if addition.owners.iter().any(|id| self.owners.contains(id)) {
            return Err("Composed scenes contain duplicate source owner identities.".into());
        }
        if addition
            .meshes
            .keys()
            .any(|id| self.meshes.contains_key(id))
        {
            return Err("Composed scenes contain duplicate mesh identities.".into());
        }
        let mut result = self.clone();
        result.materials(addition.materials)?;
        result.images(addition.images)?;
        result.vertices = result.vertices.saturating_add(addition.vertices);
        result.indices = result.indices.saturating_add(addition.indices);
        result.meshes.extend(addition.meshes);
        result.owners.extend(addition.owners);
        result.check(limits)?;
        Ok(result)
    }
    pub(super) fn replace(
        &self,
        replacement: Self,
        ids: &BTreeSet<String>,
        limits: Limits,
    ) -> Result<Self, String> {
        let mut result = self.clone();
        for id in ids {
            if !self.meshes.get(id).is_some_and(|mesh| mesh.visible)
                || !replacement.meshes.get(id).is_some_and(|mesh| mesh.visible)
            {
                return Err(format!("Unknown replacement receiver {id}."));
            }
            result
                .meshes
                .insert(id.clone(), replacement.meshes[id].clone());
        }
        result.materials(replacement.materials)?;
        let used: BTreeSet<_> = result
            .meshes
            .values()
            .filter_map(|mesh| mesh.material.as_deref())
            .collect();
        result.materials.retain(|id, _| used.contains(id.as_str()));
        let required: BTreeSet<String> = result
            .materials
            .values()
            .flat_map(|material| material.companions().into_iter().map(str::to_owned))
            .collect();
        result.images.retain(|id, _| required.contains(id));
        result.recount();
        result.images(
            replacement
                .images
                .into_iter()
                .filter(|(id, _)| required.contains(id))
                .collect(),
        )?;
        result.check(limits)?;
        Ok(result)
    }
    fn materials(&mut self, materials: BTreeMap<String, Material>) -> Result<(), String> {
        for (id, material) in materials {
            if self.materials.get(&id).is_some_and(|old| old != &material) {
                return Err(format!("Conflicting composed material {id}."));
            }
            self.materials.entry(id).or_insert(material);
        }
        Ok(())
    }
    fn images(&mut self, images: BTreeMap<String, Image>) -> Result<(), String> {
        for (id, image) in images {
            if self.images.get(&id).is_some_and(|old| {
                old.mime != image.mime || old.bytes.bytes() != image.bytes.bytes()
            }) {
                return Err(format!("Conflicting composed image {id}."));
            }
            if !self.images.contains_key(&id) {
                self.encoded_bytes = self.encoded_bytes.saturating_add(image.bytes.bytes().len());
                self.pixels = self.pixels.saturating_add(image.pixels);
                self.images.insert(id, image);
            }
        }
        Ok(())
    }
    pub(super) fn check(&self, limits: Limits) -> Result<(), String> {
        if self.vertices > limits.vertices || self.indices > limits.indices {
            return Err("Composed scene exceeds its geometry allowance.".into());
        }
        if self.encoded_bytes > limits.image_bytes {
            return Err("Composed image byte allowance exceeded.".into());
        }
        if self.pixels > limits.image_pixels {
            return Err("Scene image pixel budget exceeded.".into());
        }
        Ok(())
    }
    fn recount(&mut self) {
        self.vertices = self
            .meshes
            .values()
            .fold(0usize, |sum, m| sum.saturating_add(m.vertices));
        self.indices = self
            .meshes
            .values()
            .fold(0usize, |sum, m| sum.saturating_add(m.indices));
        self.encoded_bytes = self.images.values().fold(0usize, |sum, image| {
            sum.saturating_add(image.bytes.bytes().len())
        });
        self.pixels = self
            .images
            .values()
            .fold(0usize, |sum, image| sum.saturating_add(image.pixels));
    }
    pub(super) fn retained_bytes(&self) -> usize {
        self.meshes
            .iter()
            .map(|(id, mesh)| {
                256 + id.capacity() + mesh.material.as_ref().map_or(0, String::capacity)
            })
            .sum::<usize>()
            + self
                .owners
                .iter()
                .map(|id| 128 + id.capacity())
                .sum::<usize>()
            + self
                .materials
                .iter()
                .map(|(id, material)| 256 + id.capacity() + material_bytes(material))
                .sum::<usize>()
            + self
                .images
                .keys()
                .map(|id| 256 + id.capacity())
                .sum::<usize>()
    }
}
fn json_size(value: &impl serde::Serialize) -> usize {
    // Conservative charge for nested owned records and map allocation overhead.
    serde_json::to_vec(value).map_or(usize::MAX, |bytes| bytes.len().saturating_mul(4))
}
fn vector<T>(values: &Vec<T>) -> usize {
    values.capacity().saturating_mul(std::mem::size_of::<T>())
}
fn material_bytes(material: &Material) -> usize {
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
        charge(
            std::mem::size_of::<Model>()
                + model.id.capacity()
                + model.name.capacity()
                + vector(&model.instances),
        );
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
