use std::collections::BTreeMap;
use wasm_bindgen::prelude::*;
mod authoring;
mod recipe;
mod staged;
mod study_shaders;
pub use authoring::Authoring;

#[cfg(all(target_arch = "wasm32", feature = "threads"))]
pub use wasm_bindgen_rayon::init_thread_pool;

/// One coarse call; returned bytes are copied into a JS-owned transferable buffer.
#[wasm_bindgen]
pub fn compile_packet(source: &str, revision: u32) -> Vec<u8> {
    superspec_engine::compile_packet(source, revision)
}

/// Browser-owned asset bytes remain in a worker-local compiler session.
#[wasm_bindgen]
#[derive(Default)]
pub struct Compiler {
    assets: BTreeMap<String, Vec<u8>>,
    session: superspec_engine::Session,
    preview: superspec_engine::worldspec::preview::Preview,
    staged: staged::StagedExport,
}

#[wasm_bindgen]
impl Compiler {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_asset(&mut self, name: String, bytes: &[u8]) -> Result<(), JsValue> {
        let previous = self.assets.get(&name).map_or(0, Vec::len);
        let total = self.assets.values().map(Vec::len).sum::<usize>() - previous + bytes.len();
        if let Err(message) = admit_asset(
            bytes.len(),
            total,
            self.assets.len(),
            self.assets.contains_key(&name),
        ) {
            return Err(JsValue::from_str(&message));
        }
        self.assets.insert(name, bytes.to_vec());
        Ok(())
    }

    pub fn compile(&self, source: &str, revision: u32) -> Vec<u8> {
        let result = self.session.compile(source, revision, &|path| {
            self.assets
                .get(path)
                .cloned()
                .ok_or_else(|| format!("Asset {path} was not supplied."))
        });
        superspec_engine::result_packet(result, revision)
    }

    /// Sends complete metadata with only changed geometry and encoded assets.
    pub fn compile_update(&mut self, source: &str, revision: u32) -> Result<Vec<u8>, JsValue> {
        self.compile_update_against(source, revision, self.preview.revision(), None, None)
    }

    /// The browser may have discarded an update produced by this worker.
    pub fn compile_update_against(
        &mut self,
        source: &str,
        revision: u32,
        base_revision: Option<u32>,
        batch_spans: Option<bool>,
        foliage_mass_refs: Option<bool>,
    ) -> Result<Vec<u8>, JsValue> {
        self.preview_update(
            source,
            revision,
            base_revision,
            batch_spans,
            foliage_mass_refs,
        )
        .map_err(|e| {
            JsValue::from_str(&format!(
                "{}:{} {} — {}",
                e.source.line, e.source.column, e.code, e.message
            ))
        })
    }

    /// Returns an accepted scene update or a diagnostic scene packet. A rejection retains the baseline.
    pub fn preview_packet(
        &mut self,
        source: &str,
        revision: u32,
        base_revision: Option<u32>,
        batch_spans: Option<bool>,
        foliage_mass_refs: Option<bool>,
    ) -> Vec<u8> {
        self.preview_update(
            source,
            revision,
            base_revision,
            batch_spans,
            foliage_mass_refs,
        )
        .unwrap_or_else(|error| superspec_engine::result_packet(Err(error), revision))
    }

    /// Runs physical and sampled navigation checks without procedural material baking.
    pub fn validate(&self, source: &str, revision: u32) -> String {
        let result = self.session.compile_collision(source, revision, &|path| {
            self.assets
                .get(path)
                .cloned()
                .ok_or_else(|| format!("Asset {path} was not supplied."))
        });
        match result {
            Ok(mut scene) => match superspec_engine::validation::audit(&mut scene, &|| false) {
                Ok(()) => serde_json::to_string(&scene.manifest.validation)
                    .expect("finite validation report"),
                Err(error) => {
                    serde_json::json!({"success":false,"error":error,"diagnostics":[]}).to_string()
                }
            },
            Err(diagnostic) => {
                serde_json::json!({"success":false,"diagnostics":[diagnostic]}).to_string()
            }
        }
    }

    /// Drops the prior asset snapshot before installing the next; admission cache owns its own bound.
    pub fn clear_assets(&mut self) {
        self.assets.clear();
    }
    pub fn cache_stats(&self) -> String {
        serde_json::to_string(&self.session.stats()).expect("finite cache counters")
    }
    pub fn bake_stats(&self) -> String {
        serde_json::to_string(&self.session.bake_stats()).expect("finite bake counters")
    }
    /// Coarse dependency batch: procedural scenes preload the shipped library once per worker.
    /// User companion files remain explicit inputs; malformed AML is diagnosed by compile().
    pub fn library_assets(&self, source: &str) -> String {
        superspec_engine::library_assets(source)
    }
    /// Exports using the same portable implementation as the native CLI, with no renderer readback.
    /// `ktx2` stores textures as KTX2 (await `prepareTextures` first); otherwise PNG.
    pub fn export_glb(
        &self,
        source: &str,
        collision: bool,
        ktx2: bool,
    ) -> Result<Vec<u8>, JsValue> {
        let scene = export_scene(self, source, collision)?;
        let scene = if !collision {
            superspec_export::prepare_lightmap(&scene, &|| false)
                .map_err(|e| JsValue::from_str(&e))?
        } else {
            scene
        };
        superspec_export::glb(
            &scene,
            &superspec_export::Options {
                purpose: if collision {
                    superspec_export::Purpose::Collision
                } else {
                    superspec_export::Purpose::Visual
                },
                textures: textures(ktx2),
                ..Default::default()
            },
        )
        .map_err(|e| JsValue::from_str(&e))
    }

    /// Stages a visual KTX2 export so the host can encode its textures in parallel workers;
    /// returns the texture list (see `StagedExport::stage`). Then `staged_texture` for each
    /// source, `encodeTexture` elsewhere, and `finish_glb`.
    pub fn stage_glb(&mut self, source: &str) -> Result<String, JsValue> {
        let scene = export_scene(self, source, false)?;
        let scene = superspec_export::prepare_lightmap(&scene, &|| false)
            .map_err(|e| JsValue::from_str(&e))?;
        self.staged.stage(scene)
    }
    pub fn staged_texture(&self, index: usize) -> Result<Vec<u8>, JsValue> {
        self.staged.source(index)
    }
    pub fn finish_glb(&mut self, encoded: &[u8], lengths: &[u32]) -> Result<Vec<u8>, JsValue> {
        self.staged.finish(encoded, lengths)
    }

    /// JSON file pairs carry the OBJ, MTL and original diffuse image bytes together.
    pub fn export_obj_files(&self, source: &str) -> Result<String, JsValue> {
        let scene = export_scene(self, source, false)?;
        let object = superspec_export::obj(&scene, &Default::default())
            .map_err(|e| JsValue::from_str(&e))?;
        let mut files = vec![
            ("scene.obj".to_owned(), object.geometry),
            ("scene.mtl".to_owned(), object.materials),
        ];
        files.extend(object.textures);
        serde_json::to_string(&files).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    pub fn export_plan(&self, source: &str) -> Result<Vec<u8>, JsValue> {
        let scene = export_scene(self, source, true)?;
        superspec_export::svg_plan(&scene, &Default::default()).map_err(|e| JsValue::from_str(&e))
    }
}

impl Compiler {
    fn preview_update(
        &mut self,
        source: &str,
        revision: u32,
        base_revision: Option<u32>,
        batch_spans: Option<bool>,
        foliage_mass_refs: Option<bool>,
    ) -> Result<Vec<u8>, superspec_types::diagnostic::Diagnostic> {
        self.preview.batch_spans = batch_spans.unwrap_or(false);
        self.preview.foliage_mass_refs = foliage_mass_refs.unwrap_or(false);
        self.preview.update(
            &self.session,
            source,
            revision,
            base_revision,
            &|name| {
                self.assets
                    .get(name)
                    .cloned()
                    .ok_or_else(|| format!("Asset {name} was not supplied."))
            },
            &|| false,
        )
    }
}

fn export_scene(
    compiler: &Compiler,
    source: &str,
    collision: bool,
) -> Result<superspec_engine::Scene, JsValue> {
    let assets = |path: &str| {
        compiler
            .assets
            .get(path)
            .cloned()
            .ok_or_else(|| format!("Asset {path} was not supplied."))
    };
    let mut scene = if collision {
        compiler.session.compile_collision(source, 0, &assets)
    } else {
        compiler.session.compile(source, 0, &assets)
    }
    .map_err(|e| JsValue::from_str(&e.to_string()))?;
    superspec_engine::validation::audit(&mut scene, &|| false)
        .map_err(|e| JsValue::from_str(&e))?;
    if !scene.manifest.success {
        return Err(JsValue::from_str(
            "Declared validation requirements failed.",
        ));
    }
    Ok(scene)
}

/// Loads the KTX2 texture codec, a separate embedded module of about 2 MB. Await it once per
/// worker before an export that stores KTX2 textures; later calls resolve immediately.
#[wasm_bindgen(js_name = prepareTextures)]
pub async fn prepare_textures() {
    superspec_geometry::ktx2::prepare().await;
}

/// The shared companion allowances, with messages that name the exceeded limit.
pub(crate) fn admit_asset(
    bytes: usize,
    total: usize,
    count: usize,
    replacing: bool,
) -> Result<(), String> {
    use superspec_types::limits::{MAX_COMPANION_BYTES, MAX_COMPANIONS, MAX_FILE_BYTES};
    if bytes > MAX_FILE_BYTES {
        Err(format!(
            "A file exceeds the {} MiB limit.",
            MAX_FILE_BYTES >> 20
        ))
    } else if total > MAX_COMPANION_BYTES {
        Err(format!(
            "Files exceed the {} MiB total limit.",
            MAX_COMPANION_BYTES >> 20
        ))
    } else if count >= MAX_COMPANIONS && !replacing {
        Err(format!("More than {MAX_COMPANIONS} files were supplied."))
    } else {
        Ok(())
    }
}

pub(crate) fn textures(ktx2: bool) -> superspec_export::Textures {
    if ktx2 {
        superspec_export::Textures::Ktx2
    } else {
        superspec_export::Textures::Png
    }
}
