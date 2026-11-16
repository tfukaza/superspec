//! KTX2 exports with concurrent texture encoding. Browser Wasm has no threads, so the host stages
//! a visual scene, encodes the listed textures with `encodeTexture` in parallel workers and then
//! finishes the GLB here. The result is byte-identical to a one-call KTX2 export.
use std::collections::BTreeMap;
use superspec_export::{Options, TextureKind, Textures};
use wasm_bindgen::prelude::*;

fn error(message: impl Into<String>) -> JsValue {
    JsValue::from_str(&message.into())
}

/// One visual scene held between `stage` and `finish`; staging again replaces it.
#[derive(Default)]
pub(crate) struct StagedExport {
    scene: Option<superspec_model::Scene>,
}

impl StagedExport {
    /// Holds the scene and lists the textures to encode as JSON `[{id, kind: "color"|"data"}]`.
    pub fn stage(&mut self, scene: superspec_model::Scene) -> Result<String, JsValue> {
        self.scene = None;
        let jobs = superspec_export::texture_jobs(&scene, &ktx2(None)).map_err(error)?;
        let listed: Vec<_> = jobs
            .iter()
            .map(|job| {
                serde_json::json!({"id": job.id, "kind": match job.kind {
                    TextureKind::Color => "color",
                    TextureKind::Data => "data",
                }})
            })
            .collect();
        drop(jobs);
        self.scene = Some(scene);
        Ok(serde_json::Value::from(listed).to_string())
    }

    /// The PNG or JPEG source of the staged texture at `index`, in `stage` order.
    pub fn source(&self, index: usize) -> Result<Vec<u8>, JsValue> {
        let scene = self
            .scene
            .as_ref()
            .ok_or_else(|| error("No export is staged."))?;
        let jobs = superspec_export::texture_jobs(scene, &ktx2(None)).map_err(error)?;
        jobs.get(index)
            .map(|job| job.source.to_vec())
            .ok_or_else(|| error("Staged texture index is out of range."))
    }

    /// Writes the GLB from encoded textures concatenated in `stage` order with their `lengths`,
    /// and releases the staged scene.
    pub fn finish(&mut self, encoded: &[u8], lengths: &[u32]) -> Result<Vec<u8>, JsValue> {
        let scene = self
            .scene
            .take()
            .ok_or_else(|| error("No export is staged."))?;
        let jobs = superspec_export::texture_jobs(&scene, &ktx2(None)).map_err(error)?;
        if jobs.len() != lengths.len()
            || lengths.iter().map(|&length| length as usize).sum::<usize>() != encoded.len()
        {
            return Err(error("Encoded textures do not match the staged export."));
        }
        let mut payloads = BTreeMap::new();
        let mut start = 0;
        for (job, &length) in jobs.iter().zip(lengths) {
            let end = start + length as usize;
            payloads.insert(job.id.to_owned(), encoded[start..end].to_vec());
            start = end;
        }
        drop(jobs);
        superspec_export::glb(&scene, &ktx2(Some(&payloads))).map_err(error)
    }
}

/// Visual KTX2 export options, optionally with textures already encoded.
fn ktx2(encoded: Option<&BTreeMap<String, Vec<u8>>>) -> Options<'_> {
    Options {
        textures: Textures::Ktx2,
        encoded,
        ..Default::default()
    }
}

/// Encodes one staged texture source as KTX2 for its role (`data` for normal and property maps).
/// Await `prepareTextures` in this worker first.
#[wasm_bindgen(js_name = encodeTexture)]
pub fn encode_texture(source: &[u8], data: bool) -> Result<Vec<u8>, JsValue> {
    let kind = if data {
        TextureKind::Data
    } else {
        TextureKind::Color
    };
    superspec_export::encode_texture(source, kind).map_err(error)
}
