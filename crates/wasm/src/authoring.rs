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
    /// The fields of the last field bake, each moved in from the engine and out to the host.
    field_texels: Vec<Vec<f32>>,
    layout_records: Vec<f32>,
    staged: crate::staged::StagedExport,
    /// Whether the host answers [`host_cancelled`] (see [`Authoring::set_cancellable`]).
    cancellable: bool,
}

#[wasm_bindgen]
extern "C" {
    /// The host's answer to "has the request this session is running been cancelled?", polled by
    /// the engine while it builds and bakes. A worker cannot receive a message while a call into
    /// this module runs, so the host answers from something it can read synchronously.
    #[wasm_bindgen(js_namespace = globalThis, js_name = superspecCancelled, catch)]
    fn host_cancelled() -> Result<JsValue, JsValue>;
}

/// Whether the host says the running request is cancelled; never, unless the host opted in.
fn cancelled_by_host(enabled: bool) -> bool {
    enabled
        && host_cancelled()
            .ok()
            .and_then(|answer| answer.as_bool())
            .unwrap_or(false)
}

#[wasm_bindgen]
impl Authoring {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Lets builds, bakes and exports of this session stop early when the host's
    /// `globalThis.superspecCancelled()` returns true: they then fail with a cancellation
    /// diagnostic and leave the session's retained artifacts usable for the next request. The
    /// host defines that function before enabling this, and keeps it cheap (it is called often).
    pub fn set_cancellable(&mut self, enabled: bool) {
        self.cancellable = enabled;
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
                &|| cancelled_by_host(self.cancellable),
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
    /// that binds them as textures. The reply gives the grid size and the number of fields; each
    /// field's texels (four `f32` per texel) are then taken with [`Self::field_texels`]. Builds
    /// nothing and never replaces the accepted model.
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
                &|| cancelled_by_host(self.cancellable),
            )
        })();
        match result {
            Ok(baked) => self.keep_fields(baked),
            Err(diagnostics) => {
                serde_json::json!({"success":false,"diagnostics":diagnostics}).to_string()
            }
        }
    }

    /// The texels of field `index` of the last field bake, moved out: each field is handed over
    /// once and never copied or joined inside the module. Empty for a field already taken.
    pub fn field_texels(&mut self, index: usize) -> Vec<f32> {
        let texels = self
            .field_texels
            .get_mut(index)
            .map(std::mem::take)
            .unwrap_or_default();
        if self.field_texels.iter().all(Vec::is_empty) {
            self.field_texels = Vec::new();
        }
        texels
    }

    /// [`Self::bake_fields`] for one mesh of a scene project: the fields of the material assigned
    /// to `mesh`, over that receiver's atlas as `preview_project` bakes it.
    pub fn bake_project_fields(
        &mut self,
        source: &str,
        revision: u32,
        mesh: &str,
        names: &str,
    ) -> String {
        let result = (|| {
            if source.len() > 32 * 1024 * 1024 {
                return Err(vec![Diagnostic::message(
                    "project",
                    "Authoring project allowance exceeded.",
                )]);
            }
            let project: Project = serde_json::from_str(source)
                .map_err(|e| vec![Diagnostic::message("project", e.to_string())])?;
            let names: Vec<String> = serde_json::from_str(names)
                .map_err(|e| vec![Diagnostic::message("project", e.to_string())])?;
            self.session.bake_project_fields(
                &project,
                mesh,
                &names,
                revision as u64,
                &|name| {
                    self.assets
                        .get(name)
                        .cloned()
                        .ok_or_else(|| format!("Asset {name} was not supplied."))
                },
                &|| cancelled_by_host(self.cancellable),
            )
        })();
        match result {
            Ok(baked) => self.keep_fields(baked),
            Err(diagnostics) => {
                serde_json::json!({"success":false,"diagnostics":diagnostics}).to_string()
            }
        }
    }

    /// The atlas layout of every assigned receiver of a scene project, as its CPU bake lays them
    /// out, for a renderer that bakes in UV space. The reply lists the receivers; their triangle
    /// records (see `superspec_engine::authoring::LAYOUT_RECORD`) are then taken with
    /// [`Self::layout_records`].
    pub fn layout_project(&mut self, source: &str, revision: u32) -> String {
        let result = (|| {
            if source.len() > 32 * 1024 * 1024 {
                return Err(vec![Diagnostic::message(
                    "project",
                    "Authoring project allowance exceeded.",
                )]);
            }
            let project: Project = serde_json::from_str(source)
                .map_err(|e| vec![Diagnostic::message("project", e.to_string())])?;
            self.session.project_layouts(
                &project,
                revision as u64,
                &|name| {
                    self.assets
                        .get(name)
                        .cloned()
                        .ok_or_else(|| format!("Asset {name} was not supplied."))
                },
                &|| cancelled_by_host(self.cancellable),
            )
        })();
        match result {
            Ok(mut layouts) => {
                self.layout_records = std::mem::take(&mut layouts.records);
                serde_json::json!({"success":true,"layouts":layouts}).to_string()
            }
            Err(diagnostics) => {
                serde_json::json!({"success":false,"diagnostics":diagnostics}).to_string()
            }
        }
    }

    /// The triangle records of the last [`Self::layout_project`], moved out.
    pub fn layout_records(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.layout_records)
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
                &|| cancelled_by_host(self.cancellable),
            )?;
            let mut scene = output
                .scene(revision)
                .map_err(|e| vec![Diagnostic::message(&project.entry, e)])?;
            add_batches(&mut scene, source)
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
                &|| cancelled_by_host(self.cancellable),
            )?;
            let mut scene = output
                .scene(revision)
                .map_err(|e| vec![Diagnostic::message(&project.entry, e)])?;
            add_batches(&mut scene, source)
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
        let lighting = export_lighting(source)?;
        let batches = export_batches(source)?;
        let options = superspec_export::Options {
            textures: crate::textures(ktx2),
            lighting: lighting.as_ref(),
            batches: &batches,
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
        let mut scene = output
            .model
            .scene_cancellable(revision, &|| false)
            .map_err(|error| JsValue::from_str(&error))?;
        if let Some(lighting) = export_lighting(source)? {
            scene.manifest.lighting = Some(lighting);
        }
        add_batches(&mut scene, source).map_err(|error| JsValue::from_str(&error))?;
        self.staged.stage(scene)
    }
    pub fn staged_texture(&self, index: usize) -> Result<Vec<u8>, JsValue> {
        self.staged.source(index)
    }
    pub fn finish_project(&mut self, encoded: &[u8], lengths: &[u32]) -> Result<Vec<u8>, JsValue> {
        self.staged.finish(encoded, lengths)
    }
}

/// The lighting record the project document carries (its optional `lighting` field, which the
/// engine's project ignores), for the export to deliver. An invalid record refuses the export.
fn export_lighting(source: &str) -> Result<Option<superspec_export::Lighting>, JsValue> {
    let document: serde_json::Value =
        serde_json::from_str(source).map_err(|error| JsValue::from_str(&error.to_string()))?;
    superspec_export::project_lighting(&document).map_err(|findings| {
        let findings: Vec<String> = findings.iter().map(ToString::to_string).collect();
        JsValue::from_str(&format!(
            "Invalid lighting record: {}.",
            findings.join("; ")
        ))
    })
}

/// The instanced batches the project document carries (its optional `batches` field, which the
/// engine's project ignores as it does `lighting`), for the export to deliver.
fn export_batches(source: &str) -> Result<Vec<superspec_export::ProjectBatch>, JsValue> {
    let document: serde_json::Value =
        serde_json::from_str(source).map_err(|error| JsValue::from_str(&error.to_string()))?;
    superspec_export::project_batches(&document).map_err(|error| JsValue::from_str(&error))
}

/// Adds the project document's instanced batches to the scene delivered for it, so the preview
/// packet and the native `scene` command hold the same batches.
fn add_batches(scene: &mut superspec_model::Scene, source: &str) -> Result<(), String> {
    let document: serde_json::Value =
        serde_json::from_str(source).map_err(|error| error.to_string())?;
    superspec_model::batches::add_project(scene, &document)
}

impl Authoring {
    /// Holds a field bake's texels for [`Self::field_texels`] without copying them, and answers
    /// with the grid size and field count.
    fn keep_fields(&mut self, baked: superspec_engine::authoring::FieldBake) -> String {
        let reply = serde_json::json!({
            "success": true, "width": baked.width, "height": baked.height, "fields": baked.fields.len(),
        });
        self.field_texels = baked.fields;
        reply.to_string()
    }

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
                &|| cancelled_by_host(self.cancellable),
            )
            .map_err(|errors| JsValue::from_str(&serde_json::to_string(&errors).unwrap()))
    }
}
