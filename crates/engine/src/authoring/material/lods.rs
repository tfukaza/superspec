use super::{Artifact, Built, Diagnostic, Output, Request, Session, hash, json};
use crate::runtime::{dependencies::Runtime, store::Retention};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::Arc};
use superspec_geometry::image::{self, Image};
use superspec_model::instances::ImageResource;
use superspec_types::material::{LiveArgument, Material};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Role {
    Pigment,
    Normal,
    Data,
}

const MATERIAL_MIPS: Retention = Retention {
    name: "material-lod-images",
    bytes: 16 * 1024 * 1024,
    entries: 256,
};

// Keep the near UV chart, including its gutters. Repacking a smaller atlas
// would move triangles' UVs, which a whole-material switch cannot change.
fn lower_finish(
    source: &Material,
    images: &BTreeMap<String, ImageResource>,
    output: &mut BTreeMap<String, ImageResource>,
    edge: Option<usize>,
    runtime: &Runtime<Artifact>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Material, String> {
    let mut material = source.clone();
    let mut map = |id: &mut String, role| -> Result<(), String> {
        let resource = images
            .get(id)
            .ok_or_else(|| format!("Missing lower material image {id}."))?;
        let Some(edge) = edge else {
            output.entry(id.clone()).or_insert_with(|| resource.clone());
            return Ok(());
        };
        // Baked source image IDs are content hashes. Placement and project names
        // cannot affect a mip and would prevent reuse during incremental edits.
        let key = Sha256::digest(json(&("material-mip-v1", &*id, role as u8, edge))).into();
        if let Some(cached) = runtime.lookup(MATERIAL_MIPS, &key) {
            if let Artifact::MaterialMip(name, image) = cached.as_ref() {
                id.clone_from(name);
                output.entry(id.clone()).or_insert_with(|| image.clone());
                return Ok(());
            }
        }
        let pixels = image::decode(&resource.bytes)?;
        if pixels.width.max(pixels.height) > edge {
            let filtered = downsample(&pixels, edge, role, cancelled)?;
            let bytes = image::encode_cancellable(&filtered, cancelled)?;
            *id = format!("studio/image/{}", hash(&bytes));
            output.entry(id.clone()).or_insert_with(|| ImageResource {
                mime: "image/png",
                pixels: filtered.width * filtered.height,
                bytes: bytes.into(),
            });
        } else {
            output.entry(id.clone()).or_insert_with(|| resource.clone());
        }
        let image = &output[id.as_str()];
        runtime.retain(
            MATERIAL_MIPS,
            key,
            Artifact::MaterialMip(id.clone(), image.clone()),
            image.bytes.len() + id.len() + 128,
            false,
        );
        Ok(())
    };
    for (slot, role) in [
        (&mut material.texture, Role::Pigment),
        (&mut material.emissive_texture, Role::Pigment),
        (&mut material.normal_texture, Role::Normal),
        (&mut material.metallic_roughness_texture, Role::Data),
        (&mut material.height_texture, Role::Data),
        (&mut material.occlusion_texture, Role::Data),
    ] {
        if let Some(id) = slot {
            map(id, role)?;
        }
    }
    for operation in &mut material.live {
        for arg in operation.arguments.values_mut() {
            if let LiveArgument::Texture { texture, .. } = arg {
                map(texture, Role::Data)?;
            }
        }
    }
    material.id.clear();
    material.id = format!("studio/material/{}", hash(&json(&material)));
    Ok(material)
}

/// Area averages on the original normalized UV grid. Pigment is alpha-weighted
/// in linear light, normals are unit vectors, and packed data channels are independent.
fn downsample(
    src: &Image,
    edge: usize,
    role: Role,
    cancelled: &dyn Fn() -> bool,
) -> Result<Image, String> {
    use superspec_types::color::LINEAR;
    let longest = src.width.max(src.height);
    let [width, height] =
        [src.width, src.height].map(|n| ((n * edge + longest / 2) / longest).max(1));
    let [sx, sy] = [
        src.width as f64 / width as f64,
        src.height as f64 / height as f64,
    ];
    let mut rgba = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        if cancelled() {
            return Err("Material LOD filtering cancelled.".into());
        }
        for x in 0..width {
            let [x0, x1, y0, y1] = [
                x as f64 * sx,
                (x + 1) as f64 * sx,
                y as f64 * sy,
                (y + 1) as f64 * sy,
            ];
            let mut sum = [0.; 4];
            for j in y0 as usize..(y1.ceil() as usize).min(src.height) {
                let dy = y1.min((j + 1) as f64) - y0.max(j as f64);
                for i in x0 as usize..(x1.ceil() as usize).min(src.width) {
                    let w = dy * (x1.min((i + 1) as f64) - x0.max(i as f64));
                    let start = 4 * (j * src.width + i);
                    let p = &src.rgba[start..start + 4];
                    for c in 0..3 {
                        sum[c] += w * match role {
                            Role::Pigment => LINEAR[p[c] as usize] * f64::from(p[3]),
                            Role::Normal => f64::from(p[c]) - 127.5,
                            Role::Data => f64::from(p[c]),
                        };
                    }
                    sum[3] += w * f64::from(p[3]);
                }
            }
            let area = (x1 - x0) * (y1 - y0);
            let norm = (sum[0] * sum[0] + sum[1] * sum[1] + sum[2] * sum[2]).sqrt();
            for c in 0..3 {
                let value = match role {
                    Role::Pigment => {
                        let linear = if sum[3] > 0. { sum[c] / sum[3] } else { 0. };
                        let upper = LINEAR.partition_point(|v| *v < linear).min(255);
                        let lower = upper.saturating_sub(1);
                        if linear - LINEAR[lower] < LINEAR[upper] - linear {
                            lower as f64
                        } else {
                            upper as f64
                        }
                    }
                    Role::Normal if norm > 1e-10 => 127.5 * (1. + sum[c] / norm),
                    Role::Normal => {
                        if c == 2 {
                            255.
                        } else {
                            128.
                        }
                    }
                    Role::Data => sum[c] / area,
                };
                rgba.push(value.round().clamp(0., 255.) as u8);
            }
            rgba.push((sum[3] / area).round().clamp(0., 255.) as u8);
        }
    }
    Ok(Image {
        width,
        height,
        rgba,
        mime: "image/png",
    })
}

impl Session {
    pub(super) fn attach_material_lods(
        &mut self,
        output: &mut Output,
        request: &Request,
        revision: u64,
        assets: &dyn Fn(&str) -> Result<Arc<[u8]>, String>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), Vec<Diagnostic>> {
        let Some(levels) = request
            .material_lods
            .get(&request.entry)
            .filter(|levels| !levels.is_empty())
        else {
            return Ok(());
        };
        // Inspection passes display an intermediate channel; only final materials own variants.
        if request.inspection.is_some() {
            return Ok(());
        }
        let fail = |message: String| vec![Diagnostic::message(&request.entry, message)];
        let mut last = 1.;
        if levels.len() > 16 {
            return Err(fail(
                "Material LOD declaration exceeds 16 alternatives.".into(),
            ));
        }
        let mut variants = BTreeMap::<String, Vec<superspec_types::material::MaterialLod>>::new();
        for level in levels {
            if !(level.screen_coverage > 0. && level.screen_coverage < last) {
                return Err(fail(
                    "Material LOD screenCoverage must decrease strictly between 1 and 0.".into(),
                ));
            }
            last = level.screen_coverage;
            let mut alternative = request.clone();
            alternative.material_lods.clear();
            if let Some(entry) = &level.entry {
                if entry != &request.entry {
                    alternative.entry = entry.clone();
                    alternative.source = request
                        .modules
                        .get(entry)
                        .ok_or_else(|| fail(format!("Missing material LOD module {entry}.")))?
                        .clone();
                    alternative.parameters.clear();
                    alternative.layers.clear();
                }
            }
            alternative.parameters.extend(level.parameters.clone());
            if let Some(resolution) = level.resolution {
                if !(16..=request.resolution).contains(&resolution) {
                    return Err(fail(format!(
                        "Material LOD resolution {resolution} must be between 16 and the near resolution."
                    )));
                }
            }
            let Built::Output(lower) =
                self.material_with(&alternative, revision, assets, cancelled, None)?
            else {
                unreachable!("no share was requested")
            };
            let receivers: BTreeMap<_, _> = lower
                .model
                .instances
                .iter()
                .map(|item| (item.id.as_str(), item))
                .collect();
            for near in &output.model.instances {
                if near.material.is_none() {
                    continue;
                }
                let far = receivers
                    .get(near.id.as_str())
                    .ok_or_else(|| fail(format!("Missing material LOD receiver {}.", near.id)))?;
                if output.model.meshes.get(&near.mesh) != lower.model.meshes.get(&far.mesh)
                    || near.transform != far.transform
                {
                    return Err(fail(format!(
                        "Material LOD {} changes receiver {} geometry or UV layout.",
                        alternative.entry, near.id
                    )));
                }
                let mat = far
                    .material
                    .as_ref()
                    .and_then(|id| lower.model.materials.get(id))
                    .ok_or_else(|| fail(format!("Missing lower material for {}.", near.id)))?;
                let material = lower_finish(
                    mat,
                    &lower.model.images,
                    &mut output.model.images,
                    level.resolution,
                    &self.runtime,
                    cancelled,
                )
                .map_err(&fail)?;
                variants.entry(near.id.clone()).or_default().push(
                    superspec_types::material::MaterialLod {
                        screen_coverage: level.screen_coverage,
                        material: Box::new(material),
                    },
                );
            }
        }
        for receiver in &mut output.model.instances {
            let Some(alternatives) = variants.remove(&receiver.id) else {
                continue;
            };
            let mut material = output.model.materials[receiver.material.as_ref().unwrap()].clone();
            material.id.clear();
            material.lods = alternatives;
            material.id = format!("studio/material/{}", hash(&json(&material)));
            material.validate().map_err(fail)?;
            receiver.material = Some(material.id.clone());
            output
                .model
                .materials
                .entry(material.id.clone())
                .or_insert(material);
        }
        let used: std::collections::BTreeSet<_> = output
            .model
            .instances
            .iter()
            .filter_map(|item| item.material.as_ref())
            .chain(
                output
                    .model
                    .batches
                    .iter()
                    .filter_map(|batch| batch.material.as_ref()),
            )
            .collect();
        output.model.materials.retain(|id, _| used.contains(id));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coarse_texels_filter_in_their_authored_space() {
        let image = Image {
            width: 2,
            height: 1,
            mime: "image/png",
            rgba: vec![255, 0, 0, 255, 0, 255, 0, 0],
        };
        assert_eq!(
            downsample(&image, 1, Role::Pigment, &|| false)
                .unwrap()
                .rgba,
            [255, 0, 0, 128]
        );
        assert_eq!(
            downsample(&image, 1, Role::Data, &|| false).unwrap().rgba,
            [128, 128, 0, 128]
        );
        let normals = Image {
            rgba: vec![255, 128, 128, 255, 128, 255, 128, 255],
            ..image
        };
        assert_eq!(
            downsample(&normals, 1, Role::Normal, &|| false)
                .unwrap()
                .rgba,
            [218, 218, 128, 255]
        );
        assert!(downsample(&normals, 1, Role::Normal, &|| true).is_err());
    }
}
