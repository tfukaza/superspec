//! Delivery of a completed preserved-geometry raster through ordinary scene materials and assets.
use super::surface::{Layout, Raster};
use superspec_geometry::image;
use superspec_model::visual::BakedVisualInput;
use superspec_types::material::Material;

#[derive(Clone)]
pub struct BakedMaterial {
    pub(crate) material: Material,
    pub(crate) images: Vec<(String, std::sync::Arc<[u8]>)>,
    pub(crate) pixels: usize,
    layout: Layout,
}
impl BakedVisualInput for BakedMaterial {
    fn material(&self) -> &Material {
        &self.material
    }
    fn images(&self) -> &[(String, std::sync::Arc<[u8]>)] {
        &self.images
    }
    fn pixels(&self) -> usize {
        self.pixels
    }
}
impl BakedMaterial {
    /// A cached intrinsic raster can occupy a different deterministic job slot after an edit.
    pub(crate) fn renamed(&self, id: &str) -> Self {
        let mut result = self.clone();
        let material_id = format!("$surface/raster/{id}/0");
        if result.material.id == material_id {
            return result;
        }
        result.material.id = material_id;
        for (name, _) in &mut result.images {
            let channel = name.rsplit('/').next().unwrap();
            let replacement = format!("$generated/surfaces/{id}/{channel}");
            for path in [
                &mut result.material.texture,
                &mut result.material.normal_texture,
                &mut result.material.metallic_roughness_texture,
            ] {
                if path.as_deref() == Some(name.as_str()) {
                    *path = Some(replacement.clone());
                }
            }
            *name = replacement;
        }
        result
    }
    /// Encodes an owned, single-layer raster. The caller's stable recipe ID is unique within a scene.
    /// Shared raster jobs should share this binding; IDs are not content hashes or cache keys.
    pub fn encode(
        id: &str,
        raster: Raster,
        source: &Material,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, String> {
        source.validate()?;
        raster.layout.validate()?;
        if source.opacity.is_some_and(|value| value < 1.) || source.alpha_cutoff.is_some() {
            return Err("Baked surfaces require opaque, non-cutout source materials.".into());
        }
        if source.id.len()
            + source.color.len()
            + source.emissive.as_ref().map_or(0, String::len)
            + source.companions().iter().map(|s| s.len()).sum::<usize>()
            > 8192
        {
            return Err("Baked source material exceeds its identity budget.".into());
        }
        if id.is_empty() || id.len() > 2048 || raster.layers.len() != 1 {
            return Err("A preserved raster needs a bounded identity and one layer.".into());
        }
        let layer = raster.layers.into_iter().next().unwrap();
        if layer
            .color
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[3] != 255)
        {
            return Err("Baked surfaces do not support transparent color texels yet.".into());
        }
        if layer.source != source.id {
            return Err("Raster source material does not match the binding.".into());
        }
        let mut images = Vec::new();
        let mut pixels = 0;
        let mut encoded = 0;
        let mut material = source.clone();
        material.id = format!("$surface/raster/{id}/0");
        material.color = "#ffffff".into();
        material.roughness = 1.;
        material.metalness = 1.;
        material.texture = None;
        material.normal_texture = None;
        material.metallic_roughness_texture = None;
        material.normal_scale = None;
        material.height_texture = None;
        material.height_scale = None;
        material.height_offset = None;
        for (channel, image) in [
            ("color", Some(layer.color)),
            ("properties", Some(layer.metallic_roughness)),
            ("normal", layer.normal),
        ] {
            if cancelled() {
                return Err("Raster delivery cancelled.".into());
            }
            let Some(image) = image else {
                continue;
            };
            if image.width != raster.layout.width || image.height != raster.layout.height {
                return Err("Raster channel dimensions do not match its layout.".into());
            }
            let bytes = image::encode_cancellable(&image, cancelled)?;
            encoded += bytes.len();
            pixels += image.width * image.height;
            if bytes.len() > 16 * 1024 * 1024 || encoded > 128 * 1024 * 1024 || pixels > 33_554_432
            {
                return Err("Baked material exceeds the image budget.".into());
            }
            let reference = format!("$generated/surfaces/{id}/{channel}.png");
            match channel {
                "color" => material.texture = Some(reference.clone()),
                "properties" => material.metallic_roughness_texture = Some(reference.clone()),
                _ => {
                    material.normal_texture = Some(reference.clone());
                    material.normal_scale = Some(1.);
                }
            }
            images.push((reference, bytes.into()));
        }
        if cancelled() {
            return Err("Raster delivery cancelled.".into());
        }
        Ok(Self {
            material,
            images,
            pixels,
            layout: raster.layout,
        })
    }
    pub fn material(&self) -> &Material {
        &self.material
    }
    pub fn layout(&self) -> &Layout {
        &self.layout
    }
}
