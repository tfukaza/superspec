//! Transactional assembly of procedural visual groups into the portable scene contract.
use crate::{Mesh, Scene};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use superspec_types::limits;
use superspec_types::{architecture::V3, material::Material};

/// Minimum viewer preset at which a visual entity is delivered. Collision
/// and navigation use their own canonical admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum GraphicsTier {
    VeryLow,
    Low,
    Medium,
    High,
    VeryHigh,
}

#[derive(Clone, Copy, Debug)]
pub struct Vertex {
    pub position: V3,
    pub normal: V3,
    pub uv: [f64; 2],
    pub color: V3,
}
#[derive(Clone, Debug)]
pub struct Surface {
    pub part: String,
    pub slot: String,
}

/// Borrowed completed render triangles. Producer-only planning, survey and cache state stays outside the model.
pub trait VisualInput {
    fn vertices(&self) -> &[Vertex];
    fn indices(&self) -> &[u32];
    fn surfaces(&self) -> &[Surface];
    fn triangle_surfaces(&self) -> &[usize];
    fn surveying(&self) -> bool;
}

/// A completed artifact whose producer has validated image encoding, channels and pixel counts.
/// This publication interface does not decode or authenticate arbitrary supplied image bytes.
pub trait BakedVisualInput {
    fn material(&self) -> &Material;
    fn images(&self) -> &[(String, Arc<[u8]>)];
    fn pixels(&self) -> usize;
}

#[derive(Default)]
struct Group {
    vertices: Vec<usize>,
    indices: Vec<u32>,
}
impl Scene {
    /// Commits generated image bindings together with their visual geometry, or leaves this scene intact.
    pub fn append_baked_visual(
        &mut self,
        host: &str,
        drawing: &impl VisualInput,
        bindings: &BTreeMap<String, String>,
        bakes: &[impl BakedVisualInput],
    ) -> Result<(), String> {
        let mut material_ids: BTreeSet<_> = self
            .manifest
            .materials
            .iter()
            .map(|m| m.id.as_str())
            .collect();
        let mut asset_ids: BTreeSet<_> =
            self.manifest.assets.iter().map(|a| a.id.as_str()).collect();
        let mut pixels = self.image_pixels;
        let mut byte_count = self.asset_bytes.len();
        let mut identity_bytes = self
            .manifest
            .assets
            .iter()
            .map(|a| a.id.len() + 512)
            .sum::<usize>()
            + self
                .manifest
                .materials
                .iter()
                .map(|m| m.id.len() + 512 + m.companions().iter().map(|s| s.len()).sum::<usize>())
                .sum::<usize>();
        for bake in bakes {
            if !material_ids.insert(&bake.material().id) {
                return Err("Baked material identity is already occupied.".into());
            }
            pixels = pixels
                .checked_add(bake.pixels())
                .ok_or("Baked pixel budget overflow.")?;
            for (id, bytes) in bake.images() {
                if !asset_ids.insert(id) {
                    return Err("Baked asset identity is already occupied.".into());
                }
                byte_count = byte_count
                    .checked_add(bytes.len())
                    .ok_or("Baked byte budget overflow.")?;
                identity_bytes = identity_bytes
                    .checked_add(id.len() + 512)
                    .ok_or("Baked identity budget overflow.")?;
            }
            identity_bytes = identity_bytes
                .checked_add(
                    bake.material().id.len()
                        + 512
                        + bake
                            .material()
                            .companions()
                            .iter()
                            .map(|s| s.len())
                            .sum::<usize>(),
                )
                .ok_or("Baked identity budget overflow.")?;
        }
        if pixels > crate::MAX_SCENE_IMAGE_PIXELS
            || byte_count > crate::MAX_SCENE_IMAGE_BYTES
            || material_ids.len() > limits::MAX_SCENE_MATERIALS
            || asset_ids.len() > limits::MAX_SCENE_IMAGES
            || identity_bytes > 16 * 1024 * 1024
        {
            return Err(format!(
                "Baked scene assets exceed the delivery budget: {pixels} pixels, {byte_count} encoded bytes, {} materials, {} images, {identity_bytes} identity bytes.",
                material_ids.len(),
                asset_ids.len()
            ));
        }
        let materials = self.manifest.materials.len();
        let assets = self.manifest.assets.len();
        let bytes = self.asset_bytes.len();
        for bake in bakes {
            self.manifest.materials.push(bake.material().clone());
            for (id, bytes) in bake.images() {
                self.manifest.assets.push(crate::Asset {
                    id: id.clone(),
                    mime: "image/png",
                    byte_start: self.asset_bytes.len(),
                    byte_length: bytes.len(),
                });
                self.asset_bytes.extend_from_slice(bytes);
            }
        }
        if let Err(error) = self.append_visual(host, drawing, bindings) {
            self.manifest.materials.truncate(materials);
            self.manifest.assets.truncate(assets);
            self.asset_bytes.truncate(bytes);
            return Err(error);
        }
        self.image_pixels = pixels;
        self.manifest.asset_byte_count = self.asset_bytes.len();
        Ok(())
    }
    /// Appends completed world-space dressing, preserving canonical collision and source ancestry.
    /// Slot bindings name existing scene materials. Any admission error leaves the scene unchanged.
    pub fn append_visual(
        &mut self,
        host: &str,
        drawing: &impl VisualInput,
        bindings: &BTreeMap<String, String>,
    ) -> Result<(), String> {
        if !self.manifest.success
            || drawing.surveying()
            || drawing.surfaces().len() > 16384
            || !drawing.indices().len().is_multiple_of(3)
            || drawing.triangle_surfaces().len() != drawing.indices().len() / 3
        {
            return Err(
                "Visual assembly requires a successful scene and complete render triangles.".into(),
            );
        }
        if self
            .manifest
            .vertex_count
            .checked_add(drawing.vertices().len())
            .is_none_or(|n| n > limits::MAX_WORLD_VERTICES)
            || self
                .manifest
                .index_count
                .checked_add(drawing.indices().len())
                .is_none_or(|n| n > limits::MAX_WORLD_INDICES)
        {
            return Err("Scene visual geometry budget exceeded.".into());
        }
        let context = self
            .manifest
            .architecture
            .as_ref()
            .ok_or("Scene has no architectural source context.")?;
        let source = context
            .source(host)
            .ok_or("Unknown procedural host.")?
            .clone();
        for vertex in drawing.vertices() {
            if vertex
                .position
                .iter()
                .chain(&vertex.normal)
                .chain(&vertex.uv)
                .chain(&vertex.color)
                .any(|v| !v.is_finite() || !(*v as f32).is_finite())
                || vertex.color.iter().any(|v| *v < 0.)
            {
                return Err("Visual vertex attributes require finite f32 values and nonnegative linear color.".into());
            }
        }
        let mut groups: Vec<Group> = (0..drawing.surfaces().len())
            .map(|_| Group::default())
            .collect();
        let mut remap = vec![(u32::MAX, u32::MAX); drawing.vertices().len()];
        for (triangle, surface) in drawing
            .indices()
            .as_chunks::<3>()
            .0
            .iter()
            .zip(drawing.triangle_surfaces())
        {
            let group = groups
                .get_mut(*surface)
                .ok_or("Visual triangle references an unknown surface.")?;
            for vertex in triangle {
                let entry = remap
                    .get_mut(*vertex as usize)
                    .ok_or("Visual index exceeds the vertex range.")?;
                if entry.0 == u32::MAX {
                    *entry = (*surface as u32, group.vertices.len() as u32);
                    group.vertices.push(*vertex as usize);
                } else if entry.0 != *surface as u32 {
                    return Err("Visual vertices cannot cross part/material ownership.".into());
                }
                group.indices.push(entry.1);
            }
        }
        if remap.iter().any(|v| v.0 == u32::MAX) {
            return Err("Visual output contains unreferenced vertices.".into());
        }
        let mut ids: BTreeSet<_> = self.manifest.meshes.iter().map(|m| m.id.clone()).collect();
        let mut parts = BTreeSet::new();
        let mut prepared = vec![];
        let mut identity_bytes = self.visual_identity_bytes;
        let mut vertices = self.manifest.vertex_count;
        let mut indices = self.manifest.index_count;
        for (surface, group) in drawing.surfaces().iter().zip(&groups) {
            identity_bytes = identity_bytes
                .checked_add(2 * (host.len() + surface.part.len()) + surface.slot.len() + 64)
                .ok_or("Visual identity byte budget overflow.")?;
            if identity_bytes > 8 * 1024 * 1024 {
                return Err("Visual identity byte budget exceeded.".into());
            }
            if group.indices.is_empty() {
                return Err("Visual output contains an empty surface.".into());
            }
            if surface.part.len() > 4096 || surface.slot.len() > 4096 {
                return Err("Visual identity exceeds its byte budget.".into());
            }
            let id = format!("{host}/$dressing/{}:{}", surface.part, surface.slot);
            if !ids.insert(id.clone()) {
                return Err(format!("Generated mesh identity is occupied: {id}"));
            }
            let material = bindings
                .get(&surface.slot)
                .filter(|id| self.manifest.materials.iter().any(|m| m.id == **id))
                .ok_or_else(|| format!("Unknown visual material binding for {}.", surface.slot))?;
            identity_bytes = identity_bytes
                .checked_add(material.len() + 512)
                .ok_or("Visual identity byte budget overflow.")?;
            if identity_bytes > 8 * 1024 * 1024 {
                return Err("Visual identity byte budget exceeded.".into());
            }
            parts.insert(surface.part.clone());
            let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
            for vertex in &group.vertices {
                for axis in 0..3 {
                    let value = f64::from(drawing.vertices()[*vertex].position[axis] as f32);
                    bounds[0][axis] = bounds[0][axis].min(value);
                    bounds[1][axis] = bounds[1][axis].max(value);
                }
            }
            prepared.push(Mesh {
                id,
                object_id: format!("{host}/$dressing/{}", surface.part),
                source: source.clone(),
                material: Some(material.clone()),
                visible: true,
                vertex_start: vertices,
                vertex_count: group.vertices.len(),
                index_start: indices,
                index_count: group.indices.len(),
                bounds,
            });
            vertices += group.vertices.len();
            indices += group.indices.len();
        }
        let (objects, ancestry_bytes) =
            context.visual_objects(host, &parts, 16 * 1024 * 1024 - self.visual_ancestry_bytes)?;
        for group in groups {
            for index in group.vertices {
                let vertex = drawing.vertices()[index];
                self.positions.extend(vertex.position.map(|v| v as f32));
                self.normals.extend(vertex.normal.map(|v| v as f32));
                self.uvs.extend(vertex.uv.map(|v| v as f32));
                self.colors.extend(vertex.color.map(|v| v as f32));
            }
            self.indices.extend(group.indices);
        }
        self.manifest.meshes.extend(prepared);
        self.manifest
            .architecture
            .as_mut()
            .unwrap()
            .objects
            .extend(objects);
        self.manifest.vertex_count = vertices;
        self.manifest.index_count = indices;
        self.visual_identity_bytes = identity_bytes;
        self.visual_ancestry_bytes += ancestry_bytes;
        Ok(())
    }
}
