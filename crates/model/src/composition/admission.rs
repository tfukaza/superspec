//! Admission summaries contain no expanded vertex or encoded-image copies.
use crate::{
    MAX_SCENE_IMAGE_BYTES, MAX_SCENE_IMAGE_PIXELS, Scene,
    batches::BatchUsage,
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
    /// Delivered ordinary vertices plus the template vertices stored once per batch.
    pub vertices: usize,
    /// Delivered ordinary indices plus the template indices stored once per batch.
    pub indices: usize,
    pub image_bytes: usize,
    pub image_pixels: usize,
    pub retained_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            vertices: superspec_types::limits::MAX_WORLD_VERTICES,
            indices: superspec_types::limits::MAX_WORLD_INDICES,
            image_bytes: MAX_SCENE_IMAGE_BYTES,
            image_pixels: MAX_SCENE_IMAGE_PIXELS,
            retained_bytes: 1024 * 1024 * 1024,
        }
    }
}
impl Limits {
    pub(super) fn geometry(
        self,
        vertices: usize,
        indices: usize,
        batches: BatchUsage,
    ) -> Result<(), String> {
        let vertices = vertices.saturating_add(batches.vertices);
        let indices = indices.saturating_add(batches.indices);
        if vertices > self.vertices || indices > self.indices {
            return Err(format!(
                "Composed scene exceeds its geometry allowance: {vertices} vertices and {indices} indices, of {} and {}.",
                self.vertices, self.indices
            ));
        }
        Ok(())
    }

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
    batch_materials: BTreeMap<String, Option<String>>,
    batch_usage: BatchUsage,
    owners: BTreeSet<String>,
    geometry_lod_ids: BTreeSet<String>,
    materials: BTreeMap<String, Material>,
    images: BTreeMap<String, Image>,
    vertices: usize,
    indices: usize,
    encoded_bytes: usize,
    pixels: usize,
}
impl Admission {
    pub(super) fn has_owner(&self, id: &str) -> bool {
        self.owners.contains(id)
    }

    pub(super) fn from_validated_scene(scene: Arc<Scene>) -> Result<Self, String> {
        let mut result = Self::default();
        result
            .geometry_lod_ids
            .extend(scene.manifest.geometry_lods.iter().map(|g| g.id.clone()));
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
        result.batch_usage = BatchUsage::scene(&scene);
        for batch in &scene.manifest.batches {
            result
                .batch_materials
                .insert(batch.id.clone(), batch.material.clone());
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
        model.check_resource_counts()?;
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
            vertices: superspec_types::limits::MAX_MODEL_VERTICES,
            indices: superspec_types::limits::MAX_MODEL_INDICES,
            ..Limits::default()
        })?;
        for batch in &model.batches {
            let mesh = model.meshes.get(&batch.mesh).ok_or_else(|| {
                format!("Batch {} references missing mesh {}.", batch.id, batch.mesh)
            })?;
            if result.meshes.contains_key(&batch.id)
                || result.batch_materials.contains_key(&batch.id)
            {
                return Err(format!("Duplicate batch or mesh identity {}.", batch.id));
            }
            if let Some(id) = &batch.material
                && !result.materials.contains_key(id)
            {
                return Err(format!(
                    "Batch {} references missing material {id}.",
                    batch.id
                ));
            }
            result.batch_usage = result.batch_usage.plus(BatchUsage::source(batch, mesh));
            if !result.batch_usage.fits() {
                return Err(format!(
                    "Batch {} exceeds the scene's batch allowance.",
                    batch.id
                ));
            }
            batch.validate_for_mesh(mesh)?;
            result
                .batch_materials
                .insert(batch.id.clone(), batch.material.clone());
            result.owners.insert(batch.object_id.clone());
        }
        let groups = crate::lod::with_foliage(&model.geometry_lods, model.batches.iter())?;
        crate::lod::validate(
            &groups,
            model.instances.iter().map(|m| (m.id.as_str(), m.visible)),
            model.batches.iter().map(|b| (b.id.as_str(), b.visible)),
        )?;
        result.geometry_lod_ids = groups.into_iter().map(|g| g.id).collect();
        Ok(result)
    }
    pub(super) fn append(&self, addition: Self, limits: Limits) -> Result<Self, String> {
        if addition
            .geometry_lod_ids
            .iter()
            .any(|id| self.geometry_lod_ids.contains(id))
        {
            return Err("Composed scenes contain duplicate geometry LOD groups.".into());
        }
        if addition.owners.iter().any(|id| self.owners.contains(id)) {
            return Err("Composed scenes contain duplicate source owner identities.".into());
        }
        if addition
            .meshes
            .keys()
            .chain(addition.batch_materials.keys())
            .any(|id| self.meshes.contains_key(id) || self.batch_materials.contains_key(id))
        {
            return Err("Composed scenes contain duplicate mesh or batch identities.".into());
        }
        let mut result = self.clone();
        result.materials(addition.materials)?;
        result.images(addition.images)?;
        result.vertices = result.vertices.saturating_add(addition.vertices);
        result.indices = result.indices.saturating_add(addition.indices);
        result.batch_usage = result.batch_usage.plus(addition.batch_usage);
        result.batch_materials.extend(addition.batch_materials);
        result.meshes.extend(addition.meshes);
        result.owners.extend(addition.owners);
        result.geometry_lod_ids.extend(addition.geometry_lod_ids);
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
        let used = super::receiver_materials(
            result.meshes.values().map(|m| m.material.as_deref()),
            result.batch_materials.values().map(Option::as_deref),
        );
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
        if !self.batch_usage.fits() {
            return Err("Composed scene exceeds its batch allowance.".into());
        }
        limits.geometry(self.vertices, self.indices, self.batch_usage)?;
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
                .batch_materials
                .iter()
                .map(|(id, material)| {
                    256 + id.capacity() + material.as_ref().map_or(0, String::capacity)
                })
                .sum::<usize>()
            + self
                .geometry_lod_ids
                .iter()
                .map(|id| 128 + id.capacity())
                .sum::<usize>()
            + self
                .owners
                .iter()
                .map(|id| 128 + id.capacity())
                .sum::<usize>()
            + self
                .materials
                .iter()
                .map(|(id, material)| {
                    256 + id.capacity() + super::retention::material_bytes(material)
                })
                .sum::<usize>()
            + self
                .images
                .keys()
                .map(|id| 256 + id.capacity())
                .sum::<usize>()
    }
}
