//! Completed procedural image admission and baking for Kowloon visual hosts.
use super::{cache::Cache, weathered};
use crate::materialspec::{bake::BakedMaterial, surface::Inputs};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    sync::Mutex,
};
use superspec_types::{architecture::Node, diagnostic::Diagnostic, material::Material};

pub(crate) struct VisualBaking<'a> {
    pub(crate) cache: Option<&'a Mutex<Cache>>,
    pub(crate) asset: &'a dyn Fn(&str) -> Result<Arc<[u8]>, String>,
    pub(crate) workers: usize,
    pub(crate) cancelled: &'a (dyn Fn() -> bool + Sync),
    pub(crate) observe: &'a dyn Fn(&str),
}

struct SourceImage {
    encoded: Arc<[u8]>,
    identity: u64,
    pixels: Option<usize>,
}

impl VisualBaking<'_> {
    pub(crate) fn bake(
        &self,
        baking: weathered::Plan<'_, '_>,
        host: &str,
        rule: &Node,
        materials: &BTreeMap<String, Material>,
    ) -> Result<Vec<BakedMaterial>, Diagnostic> {
        let fail = |e: String| rule.fail("AML_DRESSING", e);
        baking.validate_materials(materials).map_err(fail)?;
        let dependencies = baking.material_dependencies().map_err(fail)?;
        (self.observe)("surface-asset-admission");
        let mut encoded_images = BTreeMap::new();
        let mut identities = BTreeMap::new();
        let mut bytes = 0;
        for id in dependencies.iter().flatten().collect::<BTreeSet<_>>() {
            let material = materials
                .get(id)
                .ok_or_else(|| fail(format!("Missing procedural material slot {id}.")))?;
            for path in material.companions() {
                if (self.cancelled)() {
                    return Err(fail("Source image admission cancelled.".into()));
                }
                if encoded_images.contains_key(path) {
                    continue;
                }
                let encoded = (self.asset)(path).map_err(|e| rule.fail("AML_ASSET", e))?;
                bytes += encoded.len();
                if encoded.len() > 16 * 1024 * 1024 || bytes > 128 * 1024 * 1024 {
                    return Err(rule.fail(
                        "AML_COMPLEXITY",
                        "Procedural source image byte allowance exceeded.",
                    ));
                }
                let (identity, pixels) = if let Some(cache) = self.cache {
                    let mut cache = cache.lock().expect("bake cache");
                    let identity = cache.image(&encoded);
                    (identity, cache.source_pixels(identity))
                } else {
                    (0, None)
                };
                identities.insert(path.to_owned(), identity);
                encoded_images.insert(
                    path.to_owned(),
                    SourceImage {
                        encoded,
                        identity,
                        pixels,
                    },
                );
            }
        }
        // Geometry binding keeps the conservative host-wide material identity, including height inputs.
        let binding_prefix = if let Some(cache) = self.cache {
            cache
                .lock()
                .expect("bake cache")
                .image(
                    &serde_json::to_vec(&(materials, &identities))
                        .map_err(|e| fail(e.to_string()))?,
                )
                .to_le_bytes()
        } else {
            [0; 8]
        };
        (self.observe)("surface-cache-lookup");
        let batch = if let Some(cache) = self.cache {
            let input_keys = dependencies
                .iter()
                .map(|required| {
                    let inputs: Vec<_> = required
                        .iter()
                        .map(|id| {
                            let material = &materials[id];
                            let companions: Vec<_> = material
                                .companions()
                                .into_iter()
                                .map(|path| (path, identities[path]))
                                .collect();
                            (id, material, companions)
                        })
                        .collect();
                    serde_json::to_vec(&inputs).map_err(|e| fail(e.to_string()))
                })
                .collect::<Result<Vec<_>, _>>()?;
            Some(
                baking
                    .prepare_cached(host, |i| input_keys[i].as_slice(), cache, self.cancelled)
                    .map_err(fail)?,
            )
        } else {
            None
        };
        let needed: BTreeSet<_> = if let Some(batch) = &batch {
            batch
                .missing()
                .iter()
                .flat_map(|&i| &dependencies[i])
                .flat_map(|id| materials[id].companions())
                .map(str::to_owned)
                .collect()
        } else {
            encoded_images.keys().cloned().collect()
        };

        (self.observe)("surface-source-decode");
        let mut images = BTreeMap::new();
        let mut pixels = 0;
        for (path, source) in encoded_images {
            if (self.cancelled)() {
                return Err(fail("Source image decoding cancelled.".into()));
            }
            let count = if needed.contains(&path) || source.pixels.is_none() {
                let image = superspec_geometry::image::decode(&source.encoded)
                    .map_err(|e| rule.fail("AML_ASSET", e))?;
                let count = image.width * image.height;
                if let Some(cache) = self.cache {
                    cache
                        .lock()
                        .expect("bake cache")
                        .record_source_decode(source.identity, count);
                }
                if needed.contains(&path) {
                    images.insert(path, image);
                }
                count
            } else {
                source.pixels.unwrap()
            };
            pixels += count;
            if pixels > 96 * 1024 * 1024 {
                return Err(rule.fail(
                    "AML_COMPLEXITY",
                    "Procedural source image pixel allowance exceeded.",
                ));
            }
        }
        let inputs = Inputs {
            materials,
            images: &images,
        };
        if let Some(batch) = batch {
            (self.observe)(if batch.missing().is_empty() {
                "surface-binding"
            } else {
                "surface-raster"
            });
            baking.bake_prepared(
                &inputs,
                host,
                &binding_prefix,
                batch,
                self.cache.unwrap(),
                self.workers,
                self.cancelled,
            )
        } else {
            (self.observe)("surface-raster");
            #[cfg(not(target_arch = "wasm32"))]
            {
                baking.bake_parallel(
                    &inputs,
                    host,
                    &weathered::ParallelBaking {
                        workers: self.workers,
                        ..Default::default()
                    },
                    self.cancelled,
                )
            }
            #[cfg(target_arch = "wasm32")]
            {
                baking.bake(&inputs, host, self.cancelled)
            }
        }
        .map_err(fail)
    }
}

#[cfg(test)]
mod tests;
