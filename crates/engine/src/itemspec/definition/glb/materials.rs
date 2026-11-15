use super::Package;
use crate::itemspec::definition::{ImportedMaterial, ImportedTexture, ItemDiagnostic};
use serde_json::{Value, json};
use std::collections::BTreeMap;

impl Package {
    pub(super) fn materials(&self) -> Result<BTreeMap<String, ImportedMaterial>, ItemDiagnostic> {
        let mut result = BTreeMap::from([(
            "material/default".into(),
            ImportedMaterial::plain("material/default"),
        )]);
        for (index, value) in self.array("materials", 1024)?.iter().enumerate() {
            let pointer = format!("/materials/{index}");
            if !value.is_object() {
                return Err(self.error(&pointer, "Material must be an object."));
            }
            let id = format!("material/{index}");
            let mut material = ImportedMaterial::plain(&id);
            material.name =
                name(value.get("name")).map_err(|message| self.error(&pointer, message))?;
            let pbr = &value["pbrMetallicRoughness"];
            if !pbr.is_null() && !pbr.is_object() {
                return Err(self.error(&pointer, "PBR material must be an object."));
            }
            material.base_color_factor = self.vector(
                pbr.get("baseColorFactor"),
                [1.; 4],
                &format!("{pointer}/pbrMetallicRoughness/baseColorFactor"),
            )?;
            material.emissive_factor = self.vector(
                value.get("emissiveFactor"),
                [0.; 3],
                &format!("{pointer}/emissiveFactor"),
            )?;
            material.metallic_factor = pbr
                .get("metallicFactor")
                .map(|v| self.number(v, &pointer))
                .transpose()?
                .unwrap_or(1.);
            material.roughness_factor = pbr
                .get("roughnessFactor")
                .map(|v| self.number(v, &pointer))
                .transpose()?
                .unwrap_or(1.);
            if material
                .base_color_factor
                .iter()
                .chain(&material.emissive_factor)
                .chain([&material.metallic_factor, &material.roughness_factor])
                .any(|v| !(0. ..=1.).contains(v))
            {
                return Err(self.error(&pointer, "PBR factors must be within zero and one."));
            }
            material.alpha_mode = value
                .get("alphaMode")
                .map(|v| {
                    v.as_str()
                        .ok_or_else(|| self.error(&pointer, "Alpha mode must be a string."))
                })
                .transpose()?
                .unwrap_or("OPAQUE")
                .into();
            if !matches!(material.alpha_mode.as_str(), "OPAQUE" | "MASK" | "BLEND") {
                return Err(self.error(&pointer, "Unknown material alpha mode."));
            }
            material.alpha_cutoff = value
                .get("alphaCutoff")
                .map(|v| self.number(v, &pointer))
                .transpose()?
                .unwrap_or(0.5);
            if material.alpha_cutoff < 0. {
                return Err(self.error(&pointer, "Alpha cutoff must be nonnegative."));
            }
            material.double_sided = value
                .get("doubleSided")
                .map(|v| {
                    v.as_bool()
                        .ok_or_else(|| self.error(&pointer, "doubleSided must be boolean."))
                })
                .transpose()?
                .unwrap_or(false);
            material.normal_scale = value["normalTexture"]
                .get("scale")
                .map(|v| self.number(v, &pointer))
                .transpose()?
                .unwrap_or(1.);
            material.occlusion_strength = value["occlusionTexture"]
                .get("strength")
                .map(|v| self.number(v, &pointer))
                .transpose()?
                .unwrap_or(1.);
            if !(0. ..=1.).contains(&material.occlusion_strength) {
                return Err(self.error(&pointer, "Occlusion strength must be within zero and one."));
            }
            for (channel, info) in [
                ("baseColor", pbr.get("baseColorTexture")),
                ("metallicRoughness", pbr.get("metallicRoughnessTexture")),
                ("normal", value.get("normalTexture")),
                ("occlusion", value.get("occlusionTexture")),
                ("emissive", value.get("emissiveTexture")),
            ] {
                if let Some(info) = info {
                    material.textures.insert(
                        channel.into(),
                        self.texture(info, &format!("{pointer}/{channel}"))?,
                    );
                }
            }
            if let Some(extensions) = value.get("extensions") {
                material.extensions = serde_json::from_value(extensions.clone())
                    .map_err(|_| self.error(&pointer, "Material extensions must be an object."))?;
                if let Some(strength) = extensions.get("KHR_materials_emissive_strength") {
                    let strength = self.number(&strength["emissiveStrength"], &pointer)?;
                    if strength < 0. {
                        return Err(self.error(&pointer, "Emissive strength must be nonnegative."));
                    }
                }
            }
            result.insert(id, material);
        }
        Ok(result)
    }
    fn texture(&self, info: &Value, pointer: &str) -> Result<ImportedTexture, ItemDiagnostic> {
        if let Some(extensions) = info.get("extensions")
            && extensions
                .as_object()
                .is_none_or(|values| values.keys().any(|key| key != "KHR_texture_transform"))
        {
            return Err(self.error(pointer, "Unsupported texture-info extension."));
        }
        let index = self.index(&info["index"], pointer)?;
        let textures = self.array("textures", 2048)?;
        let texture = textures
            .get(index)
            .ok_or_else(|| self.error(pointer, "Missing material texture."))?;
        // KTX2 (`KHR_texture_basisu`) names its image in the extension; a plain `source`, when
        // also present, is a fallback for loaders without KTX2 support.
        let source = match texture.get("extensions") {
            None => &texture["source"],
            Some(value) => match value.as_object() {
                Some(object) if object.is_empty() => &texture["source"],
                Some(object) if object.len() == 1 && object.contains_key("KHR_texture_basisu") => {
                    &object["KHR_texture_basisu"]["source"]
                }
                _ => {
                    return Err(self.error(
                        pointer,
                        "Texture extensions other than KHR_texture_basisu are not supported.",
                    ));
                }
            },
        };
        let image = self.index(source, pointer)?;
        let image = self
            .images
            .get(image)
            .ok_or_else(|| self.error(pointer, "Texture references a missing image."))?
            .id
            .clone();
        let mut tex_coord = info
            .get("texCoord")
            .map(|v| self.index(v, pointer))
            .transpose()?
            .unwrap_or(0);
        let sampler = if let Some(index) = texture.get("sampler") {
            self.array("samplers", 1024)?
                .get(self.index(index, pointer)?)
                .ok_or_else(|| self.error(pointer, "Missing texture sampler."))?
                .clone()
        } else {
            json!({})
        };
        if !sampler.is_object() {
            return Err(self.error(pointer, "Texture sampler must be an object."));
        }
        for field in ["wrapS", "wrapT"] {
            if let Some(value) = sampler.get(field)
                && !matches!(self.index(value, pointer)?, 33071 | 33648 | 10497)
            {
                return Err(self.error(pointer, "Unknown texture wrapping mode."));
            }
        }
        if let Some(value) = sampler.get("minFilter")
            && !matches!(self.index(value, pointer)?, 9728 | 9729 | 9984..=9987)
        {
            return Err(self.error(pointer, "Unknown texture minification filter."));
        }
        if let Some(value) = sampler.get("magFilter")
            && !matches!(self.index(value, pointer)?, 9728 | 9729)
        {
            return Err(self.error(pointer, "Unknown texture magnification filter."));
        }
        let transform = info["extensions"].get("KHR_texture_transform").cloned();
        if let Some(transform) = &transform {
            if !transform.is_object() {
                return Err(self.error(pointer, "Texture transform must be an object."));
            }
            self.vector(transform.get("offset"), [0.; 2], pointer)?;
            self.vector(transform.get("scale"), [1.; 2], pointer)?;
            if let Some(rotation) = transform.get("rotation") {
                self.number(rotation, pointer)?;
            }
            if let Some(coordinates) = transform.get("texCoord") {
                tex_coord = self.index(coordinates, pointer)?;
            }
        }
        if tex_coord > 7 {
            return Err(self.error(
                pointer,
                "Texture coordinates exceed the eight-set allowance.",
            ));
        }
        Ok(ImportedTexture {
            image,
            tex_coord: tex_coord as u32,
            sampler,
            transform,
        })
    }
}
pub(super) fn name(value: Option<&Value>) -> Result<Option<String>, String> {
    value
        .map(|value| {
            value
                .as_str()
                .filter(|value| value.len() <= 4096)
                .map(str::to_owned)
                .ok_or_else(|| "Name must be a bounded string.".into())
        })
        .transpose()
}
