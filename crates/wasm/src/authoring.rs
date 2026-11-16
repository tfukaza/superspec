use std::{collections::BTreeMap, sync::Arc};
use superspec_engine::authoring::{Diagnostic, Output, Project, ProjectOutput, Request, Session};
use wasm_bindgen::prelude::*;

/// Worker-local authored sources and immutable generation artifacts share the native engine.
#[wasm_bindgen]
#[derive(Default)]
pub struct Authoring {
    session: Session,
    assets: BTreeMap<String, Arc<[u8]>>,
    accepted: Option<Output>,
    packet: Vec<u8>,
    field_texels: Vec<f32>,
    staged: crate::staged::StagedExport,
}

#[wasm_bindgen]
impl Authoring {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear_assets(&mut self) {
        self.assets.clear();
    }

    pub fn library_assets(&mut self, source: &str) -> String {
        if source.len() > 32 * 1024 * 1024 {
            return "[]".into();
        }
        let names = serde_json::from_str::<Request>(source)
            .map(|request| self.session.library_assets(&request))
            .unwrap_or_default();
        serde_json::to_string(&names).expect("finite image identities")
    }

    pub fn set_asset(&mut self, name: String, bytes: &[u8]) -> Result<(), JsValue> {
        let previous = self.assets.get(&name).map_or(0, |value| value.len());
        let total =
            self.assets.values().map(|value| value.len()).sum::<usize>() - previous + bytes.len();
        if name.len() > 4096 {
            return Err(JsValue::from_str("Authoring asset name is too long."));
        }
        crate::admit_asset(
            bytes.len(),
            total,
            self.assets.len(),
            self.assets.contains_key(&name),
        )
        .map_err(|message| JsValue::from_str(&message))?;
        self.assets.insert(name, Arc::from(bytes));
        Ok(())
    }

    /// Stages the model and packet before publishing either; failures retain the prior accepted model.
    pub fn build(&mut self, source: &str, revision: u32) -> String {
        let result = (|| {
            if source.len() > 32 * 1024 * 1024 {
                return Err(vec![Diagnostic::message(
                    "request",
                    "Authoring request allowance exceeded.",
                )]);
            }
            let request: Request = serde_json::from_str(source)
                .map_err(|e| vec![Diagnostic::message("request", e.to_string())])?;
            let output = self.session.build(
                &request,
                revision as u64,
                &|name| {
                    self.assets
                        .get(name)
                        .cloned()
                        .ok_or_else(|| format!("Asset {name} was not supplied."))
                },
                &|| false,
            )?;
            let scene = output
                .scene(revision)
                .map_err(|e| vec![Diagnostic::message(&request.entry, e)])?;
            Ok((output, scene.packet()))
        })();
        match result {
            Ok((output, packet)) => {
                let response =
                    serde_json::json!({"success":true,"metadata":output.metadata}).to_string();
                self.packet = packet;
                self.accepted = Some(output);
                response
            }
            Err(diagnostics) => {
                serde_json::json!({"success":false,"diagnostics":diagnostics}).to_string()
            }
        }
    }

    pub fn packet(&self) -> Vec<u8> {
        self.packet.clone()
    }

    /// Evaluates fields of a material request's program over its receiver atlas, for a GPU preview
    /// that binds them as textures. The reply lists each field's size; the texels (four `f32` per
    /// texel per field, in order) are then taken with [`Self::field_texels`]. Builds nothing and
    /// never replaces the accepted model.
    pub fn bake_fields(&mut self, source: &str, revision: u32, names: &str) -> String {
        let result = (|| {
            if source.len() > 32 * 1024 * 1024 {
                return Err(vec![Diagnostic::message(
                    "request",
                    "Authoring request allowance exceeded.",
                )]);
            }
            let request: Request = serde_json::from_str(source)
                .map_err(|e| vec![Diagnostic::message("request", e.to_string())])?;
            let names: Vec<String> = serde_json::from_str(names)
                .map_err(|e| vec![Diagnostic::message("request", e.to_string())])?;
            self.session.bake_fields(
                &request,
                revision as u64,
                &names,
                &|name| {
                    self.assets
                        .get(name)
                        .cloned()
                        .ok_or_else(|| format!("Asset {name} was not supplied."))
                },
                &|| false,
            )
        })();
        match result {
            Ok(baked) => {
                self.field_texels = baked.fields.into_iter().flatten().collect();
                serde_json::json!({"success":true,"width":baked.width,"height":baked.height})
                    .to_string()
            }
            Err(diagnostics) => {
                serde_json::json!({"success":false,"diagnostics":diagnostics}).to_string()
            }
        }
    }

    /// The texels of the last [`Self::bake_fields`], all fields end to end.
    pub fn field_texels(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.field_texels)
    }

    pub fn export_glb(&self) -> Result<Vec<u8>, JsValue> {
        let output = self
            .accepted
            .as_ref()
            .ok_or_else(|| JsValue::from_str("No valid authoring model is available."))?;
        superspec_export::instance_glb(&output.model, &Default::default())
            .map_err(|e| JsValue::from_str(&e))
    }

    pub fn project_library_assets(&mut self, source: &str) -> String {
        if source.len() > 32 * 1024 * 1024 {
            return "[]".into();
        }
        let Ok(project) = serde_json::from_str::<Project>(source) else {
            return "[]".into();
        };
        let mut names = std::collections::BTreeSet::new();
        if let Some(source) = project
            .base
            .source
            .as_ref()
            .filter(|_| project.base.kind == "aml")
        {
            names.extend(
                serde_json::from_str::<Vec<String>>(&superspec_engine::library_assets(source))
                    .unwrap_or_default(),
            );
        }
        for model in project.models.iter().filter(|model| model.kind == "aml") {
            if let Some(source) = &model.source {
                names.extend(
                    serde_json::from_str::<Vec<String>>(&superspec_engine::library_assets(source))
                        .unwrap_or_default(),
                );
            }
        }
        for material in &project.materials {
            if let Some(source) = project.modules.get(&material.entry) {
                names.extend(self.session.library_assets(&Request {
                    kind: "material".into(),
                    entry: material.entry.clone(),
                    source: source.clone(),
                    modules: project.modules.clone(),
                    ..Default::default()
                }));
            }
        }
        if project.kind == "world" {
            if let Some(source) = project.modules.get(&project.entry) {
                names.extend(
                    serde_json::from_str::<Vec<String>>(&superspec_engine::library_assets(source))
                        .unwrap_or_default(),
                );
            }
            if let Some(world) = &project.world {
                let mut entries = Vec::new();
                for item in &world.items {
                    entries.push(("item", item.entry.clone(), item.slots.clone()));
                }
                entries.extend(
                    world
                        .materials
                        .iter()
                        .map(|binding| ("material", binding.entry.clone(), BTreeMap::new())),
                );
                for (kind, entry, slot_overrides) in entries {
                    if let Some(source) = project.modules.get(&entry) {
                        names.extend(self.session.library_assets(&Request {
                            kind: kind.into(),
                            entry,
                            source: source.clone(),
                            modules: project.modules.clone(),
                            slot_overrides,
                            ..Default::default()
                        }));
                    }
                }
            }
        }
        if let Some(source) = project.modules.get(&project.entry) {
            names.extend(self.session.library_assets(&Request {
                kind: project.kind,
                entry: project.entry,
                source: source.clone(),
                modules: project.modules,
                ..Default::default()
            }));
        }
        serde_json::to_string(&names).expect("finite catalog identities")
    }

    /// Previews a whole project, including multi-model and multi-material projects, through the
    /// same `build_project` the export and native commands use. Stages the packet like `build`.
    pub fn preview_project(&mut self, source: &str, revision: u32) -> String {
        let result = (|| {
            if source.len() > 32 * 1024 * 1024 {
                return Err(vec![Diagnostic::message(
                    "project",
                    "Authoring project allowance exceeded.",
                )]);
            }
            let project: Project = serde_json::from_str(source)
                .map_err(|e| vec![Diagnostic::message("project", e.to_string())])?;
            let output = self.session.build_project(
                &project,
                revision as u64,
                &|name| {
                    self.assets
                        .get(name)
                        .cloned()
                        .ok_or_else(|| format!("Asset {name} was not supplied."))
                },
                &|| false,
            )?;
            let scene = output
                .scene(revision)
                .map_err(|e| vec![Diagnostic::message(&project.entry, e)])?;
            Ok((output.metadata, scene.packet()))
        })();
        match result {
            Ok((metadata, packet)) => {
                self.packet = packet;
                serde_json::json!({"success":true,"metadata":metadata}).to_string()
            }
            Err(diagnostics) => {
                serde_json::json!({"success":false,"diagnostics":diagnostics}).to_string()
            }
        }
    }

    /// Describes a project for a renderer that evaluates its materials in a shader: the composed
    /// models as the staged packet, and each material's preview shader in the metadata. No bake.
    pub fn shade_project(&mut self, source: &str, revision: u32) -> String {
        let result = (|| {
            if source.len() > 32 * 1024 * 1024 {
                return Err(vec![Diagnostic::message(
                    "project",
                    "Authoring project allowance exceeded.",
                )]);
            }
            let project: Project = serde_json::from_str(source)
                .map_err(|e| vec![Diagnostic::message("project", e.to_string())])?;
            let output = self.session.shade_project(
                &project,
                revision as u64,
                &|name| {
                    self.assets
                        .get(name)
                        .cloned()
                        .ok_or_else(|| format!("Asset {name} was not supplied."))
                },
                &|| false,
            )?;
            let scene = output
                .scene(revision)
                .map_err(|e| vec![Diagnostic::message(&project.entry, e)])?;
            Ok((output.metadata, scene.packet()))
        })();
        match result {
            Ok((metadata, packet)) => {
                self.packet = packet;
                serde_json::json!({"success":true,"metadata":metadata}).to_string()
            }
            Err(diagnostics) => {
                serde_json::json!({"success":false,"diagnostics":diagnostics}).to_string()
            }
        }
    }

    /// Export rebuilds the authored project through the same engine while leaving accepted preview state intact.
    /// `ktx2` stores textures as KTX2 (await `prepareTextures` first); otherwise PNG.
    pub fn export_project(
        &mut self,
        source: &str,
        revision: u32,
        ktx2: bool,
    ) -> Result<Vec<u8>, JsValue> {
        let output = self.export_build(source, revision)?;
        let options = superspec_export::Options {
            textures: crate::textures(ktx2),
            ..Default::default()
        };
        superspec_export::world_glb(&output.model, revision, &options)
            .map_err(|error| JsValue::from_str(&error))
    }

    /// Browser Wasm has no threads, so large material bakes are shared with helper workers. While
    /// enabled, a build that needs a large uncached bake fails with a `BAKE_PENDING` diagnostic
    /// and `take_pending_bake` describes it; helpers run `bake_share`, their results go to
    /// `provide_bake_share`, and the same build is called again. The output is byte-identical.
    pub fn distribute_bakes(&mut self, enabled: bool) {
        self.session.distribute_bakes(enabled);
    }

    /// JSON of the bake the last build stopped for (request, keys and image names), or empty.
    pub fn take_pending_bake(&mut self) -> String {
        self.session
            .take_pending_bake()
            .map_or(String::new(), |pending| {
                serde_json::to_string(&pending).expect("finite admitted request")
            })
    }

    /// One helper's rows of a pending bake, from the assets set on this helper's session.
    pub fn bake_share(
        &mut self,
        pending: &str,
        index: u32,
        count: u32,
    ) -> Result<Vec<u8>, JsValue> {
        let pending: superspec_engine::authoring::PendingBake =
            serde_json::from_str(pending).map_err(|error| JsValue::from_str(&error.to_string()))?;
        self.session
            .bake_share(
                &pending,
                index as usize,
                count as usize,
                &|name| {
                    self.assets
                        .get(name)
                        .cloned()
                        .ok_or_else(|| format!("Asset {name} was not supplied."))
                },
                &|| false,
            )
            .map_err(|errors| JsValue::from_str(&serde_json::to_string(&errors).unwrap()))
    }

    pub fn provide_bake_share(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
        self.session
            .provide_bake_share(bytes)
            .map_err(|error| JsValue::from_str(&error))
    }

    /// Stages a KTX2 project export so the host can encode its textures in parallel workers;
    /// returns the texture list. Then `staged_texture`, `encodeTexture` and `finish_project`.
    pub fn stage_project(&mut self, source: &str, revision: u32) -> Result<String, JsValue> {
        let output = self.export_build(source, revision)?;
        let scene = output
            .model
            .scene_cancellable(revision, &|| false)
            .map_err(|error| JsValue::from_str(&error))?;
        self.staged.stage(scene)
    }
    pub fn staged_texture(&self, index: usize) -> Result<Vec<u8>, JsValue> {
        self.staged.source(index)
    }
    pub fn finish_project(&mut self, encoded: &[u8], lengths: &[u32]) -> Result<Vec<u8>, JsValue> {
        self.staged.finish(encoded, lengths)
    }
}

impl Authoring {
    fn export_build(&mut self, source: &str, revision: u32) -> Result<ProjectOutput, JsValue> {
        if source.len() > 32 * 1024 * 1024 {
            return Err(JsValue::from_str("Authoring project allowance exceeded."));
        }
        let project: Project =
            serde_json::from_str(source).map_err(|error| JsValue::from_str(&error.to_string()))?;
        self.session
            .build_project(
                &project,
                revision as u64,
                &|name| {
                    self.assets
                        .get(name)
                        .cloned()
                        .ok_or_else(|| format!("Asset {name} was not supplied."))
                },
                &|| false,
            )
            .map_err(|errors| JsValue::from_str(&serde_json::to_string(&errors).unwrap()))
    }
}
