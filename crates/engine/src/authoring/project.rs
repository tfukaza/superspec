use super::{Diagnostic, Mapping, Receiver, Request, Session};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use superspec_model::{Scene, composition::WorldModel};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Base {
    pub kind: String,
    pub source: Option<String>,
    pub name: Option<String>,
}
impl Default for Base {
    fn default() -> Self {
        Self {
            kind: "swatch".into(),
            source: None,
            name: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Target {
    pub mesh: Option<String>,
    pub binding: Option<String>,
    pub scope: String,
    pub enabled: bool,
}
impl Default for Target {
    fn default() -> Self {
        Self {
            mesh: None,
            binding: None,
            scope: "mesh".into(),
            enabled: false,
        }
    }
}

/// One model of a multi-model project: a GLB companion, an AML scene or an item. Its mesh and
/// owner identities are placed under `<id>/` so several models compose without collisions.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ModelSource {
    pub id: String,
    /// `glb` (the companion named `name`), `aml` (the scene text in `source`) or `item` (the
    /// ItemSpec definition in the project module `entry`, or in `source`).
    pub kind: String,
    pub source: Option<String>,
    pub name: Option<String>,
    /// `item`: the project module holding the definition.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
    /// `item`: values of the definition's controls.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub parameters: BTreeMap<String, serde_json::Value>,
    /// `item`: where the item stands, as four affine columns (three basis vectors and the
    /// translation) in meters, Z up.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transform: Option<crate::itemspec::definition::Transform>,
    /// `item`: the project material (a `MaterialAssignment` identity) each of the item's
    /// slots shows. Every mesh of a part's face group with that slot is a receiver of that
    /// material, as if the assignment listed it; a slot left out keeps a plain surface.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub slots: BTreeMap<String, String>,
    /// `item`: texels per meter for the meshes of a slot of this item, where it is not the
    /// density of the slot's material or the project's (`Project::texel_densities`, which this
    /// fills per mesh; a density the project states for a mesh itself stays). It comes before
    /// the item definition's own `densities`, which come before a material's and the project's.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub slot_densities: BTreeMap<String, f64>,
}

/// A scene project with its item models resolved (`Session::resolve_items`).
pub(super) struct ResolvedItems {
    /// The project with every item slot's meshes added to the material assignment it names
    /// and each of those meshes given its part's named features.
    pub project: Project,
    /// What each item reports about itself, by model identity (see `PlacedItem::metadata`).
    pub metadata: serde_json::Map<String, serde_json::Value>,
}

/// One material applied to a set of meshes. Every assignment's entry module comes from the
/// project's shared module map, so several materials are edited side by side.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MaterialAssignment {
    pub id: String,
    pub entry: String,
    pub parameters: BTreeMap<String, serde_json::Value>,
    pub seed: u32,
    /// Visible mesh identities of the composed scene; disjoint from every other assignment.
    pub meshes: Vec<String>,
    /// Weathering layers over this material, the lowest first (`Request::layers`): each with
    /// its parameters and the feature it follows.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<super::Layer>,
}

/// Portable authored inputs. Encoded companion bytes are decoded and supplied by the host adapter.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Project {
    pub schema_version: u32,
    pub kind: String,
    pub entry: String,
    pub modules: BTreeMap<String, String>,
    pub parameters: BTreeMap<String, serde_json::Value>,
    pub seed: u32,
    pub resolution: usize,
    pub inspection: String,
    pub mapping: Mapping,
    pub domain: [f64; 2],
    pub base: Base,
    pub target: Target,
    pub world: Option<crate::worldspec::authored::World>,
    pub edges: Vec<crate::materialspec::program::Edge>,
    pub gravity: [f64; 3],
    /// Imported models composed into one scene; when present they replace `base`.
    pub models: Vec<ModelSource>,
    /// Materials applied in order to disjoint mesh sets; when present they replace `target`.
    pub materials: Vec<MaterialAssignment>,
    /// Bake scheduling; "production" admits larger textures and bakes rows on native threads.
    pub profile: super::Profile,
    /// Texels per meter for every receiver that can reach it within `resolution`; smaller
    /// receivers then get smaller textures. Unset, every receiver fills `resolution`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub texel_density: Option<f64>,
    /// Texels per meter where it is not the project's: by material assignment id, for every
    /// receiver of that material (an item's weathered slot is baked at its bound material's), or
    /// by mesh id, for that receiver alone, which comes first. A sign's lettering or a notice can
    /// so be baked finer than the walls. Like `texel_density`, a receiver that cannot reach its
    /// density within `resolution` keeps the resolution at the density that fits.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub texel_densities: BTreeMap<String, f64>,
    /// Planes bound to the external inputs of the materials baked on each mesh, by mesh id and
    /// then input name (`Request::planes`): the layers over a base, per receiver.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub planes: BTreeMap<String, BTreeMap<String, super::PlaneSource>>,
    /// The materials planes are baked from (`Request::materials`), by the name `planes` uses.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub plane_materials: BTreeMap<String, crate::materialspec::program::MaterialRequest>,
    /// Named features of each mesh, by mesh id, in world meters (`Request::features`).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub features: BTreeMap<String, Vec<crate::materialspec::program::NamedFeature>>,
    /// The mapping of a material assignment where it is not the project's, by assignment id:
    /// `uv`, `object`, `world` or `anchored`, slot by slot.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub mappings: BTreeMap<String, Mapping>,
    /// The pattern anchor of each mesh an anchored material is baked on, by mesh id
    /// (`Request::anchors`).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub anchors: BTreeMap<String, super::Anchor>,
}
impl Default for Project {
    fn default() -> Self {
        Self {
            schema_version: 1,
            kind: "material".into(),
            entry: "main.ms".into(),
            modules: BTreeMap::new(),
            parameters: BTreeMap::new(),
            seed: 0,
            resolution: 128,
            inspection: "final".into(),
            mapping: Mapping::Object,
            domain: [1., 1.],
            base: Base::default(),
            target: Target::default(),
            world: None,
            edges: vec![],
            gravity: [0., 0., -1.],
            models: vec![],
            materials: vec![],
            profile: super::Profile::Preview,
            texel_density: None,
            texel_densities: BTreeMap::new(),
            planes: BTreeMap::new(),
            plane_materials: BTreeMap::new(),
            features: BTreeMap::new(),
            mappings: BTreeMap::new(),
            anchors: BTreeMap::new(),
        }
    }
}
pub struct ProjectOutput {
    pub model: WorldModel,
    pub metadata: serde_json::Value,
}

impl ProjectOutput {
    /// Expands a fresh delivery view; persistent inputs and other consumers stay unchanged.
    pub fn scene(&self, revision: u32) -> Result<Scene, String> {
        self.model.scene(revision)
    }
}

impl Session {
    pub fn build_project(
        &mut self,
        project: &Project,
        revision: u64,
        assets: &dyn Fn(&str) -> Result<Arc<[u8]>, String>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<ProjectOutput, Vec<Diagnostic>> {
        let fail = |message: String| vec![Diagnostic::message(&project.entry, message)];
        // Version 2 adds `models` and `materials`; version 1 projects are read unchanged.
        if !matches!(project.schema_version, 1 | 2)
            || !matches!(project.kind.as_str(), "material" | "item" | "world")
            || !matches!(project.target.scope.as_str(), "mesh" | "binding")
        {
            return Err(fail(
                "Unsupported studio project version, kind or target scope.".into(),
            ));
        }
        self.begin_authoring(revision, &project.entry)?;
        let source = project
            .modules
            .get(&project.entry)
            .ok_or_else(|| fail("Project entry source is missing.".into()))?;
        if project.kind == "world" {
            let world = project
                .world
                .as_ref()
                .ok_or_else(|| fail("World project requires its composition settings.".into()))?;
            let model = crate::worldspec::authored::build_model(
                self,
                &project.entry,
                source,
                &project.modules,
                world,
                project.resolution,
                revision,
                assets,
                cancelled,
            )?;
            return Ok(ProjectOutput {
                model,
                metadata: serde_json::json!({"items":world.items.len(),"materialAssignments":world.materials.len()}),
            });
        }
        if project.kind == "material"
            && (!project.models.is_empty() || !project.materials.is_empty())
        {
            return self.build_materials(project, revision, assets, cancelled);
        }
        let mut request = Request {
            kind: project.kind.clone(),
            entry: project.entry.clone(),
            source: source.clone(),
            modules: project.modules.clone(),
            parameters: project.parameters.clone(),
            seed: project.seed,
            resolution: project.resolution,
            inspection: (project.inspection != "final").then(|| project.inspection.clone()),
            mapping: project.mapping,
            domain: project.domain,
            edges: project.edges.clone(),
            gravity: project.gravity,
            profile: project.profile,
            texel_density: project.texel_density,
            planes: project.planes.clone(),
            materials: project.plane_materials.clone(),
            features: project.features.clone(),
            ..Default::default()
        };
        let mut transforms = BTreeMap::<String, [f64; 16]>::new();
        let base = if project.kind == "item" {
            None
        } else {
            match project.base.kind.as_str() {
                "swatch" => None,
                "aml" => {
                    let source = project
                        .base
                        .source
                        .as_ref()
                        .ok_or_else(|| fail("Project AML scene is missing.".into()))?;
                    let scene = self
                        .compile(source, revision as u32, &|name| {
                            assets(name).map(|bytes| bytes.to_vec())
                        })
                        .map_err(|error| {
                            vec![Diagnostic {
                                code: error.code.into(),
                                source_name: "scene.aml".into(),
                                line: error.source.line,
                                column: error.source.column,
                                offset: error.source.offset,
                                end: error.source.end,
                                message: error.message.to_string(),
                            }]
                        })?;
                    Some(WorldModel::from_scene(scene).map_err(&fail)?)
                }
                "glb" => {
                    let name = project
                        .base
                        .name
                        .as_ref()
                        .ok_or_else(|| fail("Project GLB asset name is missing.".into()))?;
                    let imported = self.build(
                        &Request {
                            kind: "import-glb".into(),
                            entry: name.clone(),
                            ..Default::default()
                        },
                        revision,
                        assets,
                        cancelled,
                    )?;
                    transforms =
                        serde_json::from_value(imported.metadata["receiverTransforms"].clone())
                            .map_err(|e| fail(e.to_string()))?;
                    Some(WorldModel::from_instances(imported.model).map_err(&fail)?)
                }
                _ => return Err(fail("Unsupported project base scene.".into())),
            }
        };
        let mut ids = BTreeSet::new();
        if let Some(base) = &base {
            if project.target.enabled {
                let base = base
                    .scene_cancellable(revision as u32, cancelled)
                    .map_err(&fail)?;
                let target = base.manifest.meshes.iter().find(|mesh|mesh.visible && match (&project.target.mesh,&project.target.binding) {
                    (Some(id),_)=>&mesh.id==id,
                    (None,Some(binding))=>mesh.material.as_ref()==Some(binding),
                    _=>true,
                })
                    .ok_or_else(|| {
                        fail("The saved material receiver or binding is missing from the project scene.".into())
                    })?;
                ids = base
                    .manifest
                    .meshes
                    .iter()
                    .filter(|mesh| {
                        mesh.visible
                            && if project.target.scope == "binding" {
                                mesh.material == target.material
                            } else {
                                mesh.id == target.id
                            }
                    })
                    .map(|mesh| mesh.id.clone())
                    .collect();
                // Contacts with the rest of the scene become `contact` features, as in scene projects.
                let touching = ContactScene::new(&base);
                request.receivers = receivers(&base, &ids, &transforms)
                    .into_iter()
                    .map(|mut receiver| {
                        receiver.contacts = touching.contacts(&base, &receiver);
                        receiver
                    })
                    .collect();
            }
        }
        let output = self.build(&request, revision, assets, cancelled)?;
        let model = match base {
            Some(mut base) if project.target.enabled => {
                base.replace_receivers(output.model, ids).map_err(&fail)?;
                base
            }
            Some(base) => base,
            None => WorldModel::from_instances(output.model).map_err(&fail)?,
        };
        if cancelled() {
            return Err(fail("Project authoring cancelled.".into()));
        }
        Ok(ProjectOutput {
            model,
            metadata: output.metadata,
        })
    }
}

/// At most this many models compose one project scene.
pub const MAX_MODELS: usize = 16;
/// At most this many material assignments apply to one project scene.
pub const MAX_ASSIGNMENTS: usize = 64;

fn valid_identity(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

impl Session {
    /// Composes every project model into one scene, each under its own `<id>/` identities, with
    /// the object-to-world transforms of imported GLB receivers under the same identities.
    pub(super) fn compose_models(
        &mut self,
        project: &Project,
        revision: u64,
        assets: &dyn Fn(&str) -> Result<Arc<[u8]>, String>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(WorldModel, BTreeMap<String, [f64; 16]>), Vec<Diagnostic>> {
        let fail = |message: String| vec![Diagnostic::message(&project.entry, message)];
        if project.models.is_empty() || project.models.len() > MAX_MODELS {
            return Err(fail(format!(
                "A material project composes 1 to {MAX_MODELS} models."
            )));
        }
        let mut seen = BTreeSet::new();
        let mut composed: Option<Scene> = None;
        let mut transforms = BTreeMap::new();
        for model in &project.models {
            if !valid_identity(&model.id) || !seen.insert(model.id.as_str()) {
                return Err(fail(format!(
                    "Model identities are unique, 1-64 characters of letters, digits, '-', '_' or '.': {}.",
                    model.id
                )));
            }
            if cancelled() {
                return Err(fail("Project authoring cancelled.".into()));
            }
            let scene = match model.kind.as_str() {
                "aml" => {
                    let source = model
                        .source
                        .as_ref()
                        .ok_or_else(|| fail(format!("Model {} has no AML scene.", model.id)))?;
                    self.compile(source, revision as u32, &|name| {
                        assets(name).map(|bytes| bytes.to_vec())
                    })
                    .map_err(|error| {
                        vec![Diagnostic {
                            code: error.code.into(),
                            source_name: format!("{}.aml", model.id),
                            line: error.source.line,
                            column: error.source.column,
                            offset: error.source.offset,
                            end: error.source.end,
                            message: error.message.to_string(),
                        }]
                    })?
                }
                "glb" => {
                    let name = model.name.as_ref().ok_or_else(|| {
                        fail(format!("Model {} names no GLB companion.", model.id))
                    })?;
                    let imported = self.build(
                        &Request {
                            kind: "import-glb".into(),
                            entry: name.clone(),
                            ..Default::default()
                        },
                        revision,
                        assets,
                        cancelled,
                    )?;
                    let imported_transforms: BTreeMap<String, [f64; 16]> =
                        serde_json::from_value(imported.metadata["receiverTransforms"].clone())
                            .map_err(|e| fail(e.to_string()))?;
                    for (id, matrix) in imported_transforms {
                        transforms.insert(format!("{}/{id}", model.id), matrix);
                    }
                    WorldModel::from_instances(imported.model)
                        .and_then(|model| model.scene_cancellable(revision as u32, cancelled))
                        .map_err(&fail)?
                }
                "item" => {
                    let prefix = format!("{}/", model.id);
                    let placed = self.place_item(project, model, &prefix, assets, cancelled)?;
                    for (id, matrix) in placed.transforms {
                        transforms.insert(format!("{prefix}{id}"), matrix);
                    }
                    WorldModel::from_instances(placed.model)
                        .and_then(|model| model.scene_cancellable(revision as u32, cancelled))
                        .map_err(&fail)?
                }
                other => {
                    return Err(fail(format!(
                        "Model {} has unsupported kind {other}; use glb, aml or item.",
                        model.id
                    )));
                }
            };
            let scene = superspec_model::composition::prefixed(scene, &format!("{}/", model.id));
            composed = Some(match composed {
                None => scene,
                Some(base) => superspec_model::composition::append(base, scene).map_err(&fail)?,
            });
        }
        let scene = composed.expect("at least one model");
        Ok((WorldModel::from_scene(scene).map_err(&fail)?, transforms))
    }

    /// Resolves the item models of a scene project: generates each one (parts whose geometry
    /// key did not change since the session last placed it are kept), adds the meshes of every
    /// slot the model binds to the material assignment it names, and gives each of those
    /// meshes the named features of its part, so a material can follow a sill, a base, a top
    /// or a joint. `None` for a project without item models, which is used as it is.
    pub(super) fn resolve_items(
        &mut self,
        project: &Project,
        assets: &dyn Fn(&str) -> Result<Arc<[u8]>, String>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Option<ResolvedItems>, Vec<Diagnostic>> {
        if !project.models.iter().any(|model| model.kind == "item") {
            return Ok(None);
        }
        let fail = |message: String| vec![Diagnostic::message(&project.entry, message)];
        let mut resolved = ResolvedItems {
            project: project.clone(),
            metadata: serde_json::Map::new(),
        };
        // Per item: its features that reach the scene, and the world bounds of its meshes.
        let mut scene_reach = Vec::new();
        for model in project.models.iter().filter(|model| model.kind == "item") {
            if !valid_identity(&model.id) {
                return Err(fail(format!("Invalid model identity {}.", model.id)));
            }
            let prefix = format!("{}/", model.id);
            let mut placed = self.place_item(project, model, &prefix, assets, cancelled)?;
            for (slot, material) in &model.slots {
                // An item's slots depend on its controls (a seed or a condition can leave a
                // fixture out), so a binding for a slot it does not emit this time is unused.
                if !placed.slots.values().any(|used| used == slot) {
                    continue;
                }
                let base = resolved
                    .project
                    .materials
                    .iter()
                    .position(|assignment| &assignment.id == material)
                    .ok_or_else(|| {
                        fail(format!(
                            "Model {} binds slot {slot} to {material}, which is not a material of the project.",
                            model.id
                        ))
                    })?;
                // A slot the item weathers is its own assignment: the bound material with the
                // item's layers over the material's own, so the base bake is still shared.
                let index = match placed.layers.get(slot).filter(|layers| !layers.is_empty()) {
                    None => base,
                    Some(layers) => {
                        let id = format!("{material}.{}.{slot}", model.id);
                        if !valid_identity(&id) {
                            return Err(fail(format!(
                                "Model {} weathers slot {slot}, whose assignment {id} is not a valid identity.",
                                model.id
                            )));
                        }
                        match resolved.project.materials.iter().position(|a| a.id == id) {
                            Some(index) => index,
                            None => {
                                let mut derived = resolved.project.materials[base].clone();
                                // It is mapped as the material it derives from.
                                if let Some(mapping) =
                                    resolved.project.mappings.get(material).cloned()
                                {
                                    resolved.project.mappings.insert(id.clone(), mapping);
                                }
                                // And baked at its texel density, where it has one of its own.
                                if let Some(density) =
                                    resolved.project.texel_densities.get(material).copied()
                                {
                                    resolved.project.texel_densities.insert(id.clone(), density);
                                }
                                derived.id = id;
                                derived.meshes.clear();
                                derived
                                    .layers
                                    .extend(layers.iter().map(|layer| super::Layer {
                                        entry: layer.entry.clone(),
                                        parameters: layer.parameters.clone(),
                                        seed: layer.seed,
                                        follow: layer.follow.clone(),
                                    }));
                                resolved.project.materials.push(derived);
                                resolved.project.materials.len() - 1
                            }
                        }
                    }
                };
                let assignment = &mut resolved.project.materials[index];
                for (mesh, _) in placed.slots.iter().filter(|(_, used)| *used == slot) {
                    let id = format!("{prefix}{mesh}");
                    if !assignment.meshes.contains(&id) {
                        assignment.meshes.push(id);
                    }
                }
            }
            for (slot, density) in &model.slot_densities {
                for (mesh, _) in placed.slots.iter().filter(|(_, used)| *used == slot) {
                    resolved
                        .project
                        .texel_densities
                        .entry(format!("{prefix}{mesh}"))
                        .or_insert(*density);
                }
            }
            // Then what the item itself asks for its slots, under what the project says of a
            // mesh or the model of a slot, and over a material's or the project's density. It
            // reaches each receiver's layout, so every bake of the receiver is keyed by it.
            for (slot, density) in &placed.densities {
                for (mesh, _) in placed.slots.iter().filter(|(_, used)| *used == slot) {
                    resolved
                        .project
                        .texel_densities
                        .entry(format!("{prefix}{mesh}"))
                        .or_insert(*density);
                }
            }
            for (mesh, features) in placed.features {
                resolved
                    .project
                    .features
                    .entry(format!("{prefix}{mesh}"))
                    .or_default()
                    .extend(features);
            }
            scene_reach.push((
                model.id.clone(),
                std::mem::take(&mut placed.scene_features),
                std::mem::take(&mut placed.bounds),
            ));
            // Each mesh is anchored to its part's pattern anchor (read by `Mapping::Anchored`),
            // unless the project states an anchor of its own for it.
            for (mesh, anchor) in placed.anchors {
                resolved
                    .project
                    .anchors
                    .entry(format!("{prefix}{mesh}"))
                    .or_insert(super::Anchor {
                        origin: anchor.origin,
                        u: anchor.u,
                        v: anchor.v,
                    });
            }
            resolved.metadata.insert(model.id.clone(), placed.metadata);
        }
        // Features of scene reach: the receivers of every other item that come near enough are
        // given them (an item's own receivers have them already, by its item reach).
        if scene_reach
            .iter()
            .any(|(_, features, _)| !features.is_empty())
        {
            for (id, _, bounds) in &scene_reach {
                let others: Vec<super::item::Reaching> = scene_reach
                    .iter()
                    .filter(|(other, _, _)| other != id)
                    .flat_map(|(_, features, _)| features.iter().cloned())
                    .collect();
                let mut dropped = 0;
                for (mesh, bounds) in bounds {
                    let (near, left) = super::item::reached(&others, None, bounds);
                    dropped += left;
                    if !near.is_empty() {
                        resolved
                            .project
                            .features
                            .entry(format!("{id}/{mesh}"))
                            .or_default()
                            .extend(near);
                    }
                }
                if let Some(metadata) = resolved.metadata.get_mut(id) {
                    metadata["sceneReachDropped"] = dropped.into();
                }
            }
        }
        Ok(Some(resolved))
    }

    /// Builds a multi-model, multi-material project: the composed models, then each material
    /// baked onto its own meshes in order. A material without meshes is still compiled, so its
    /// diagnostics and parameters are reported while it waits to be applied.
    fn build_materials(
        &mut self,
        project: &Project,
        revision: u64,
        assets: &dyn Fn(&str) -> Result<Arc<[u8]>, String>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<ProjectOutput, Vec<Diagnostic>> {
        // Item models bind their slots' meshes to materials and name their features.
        let resolved = self.resolve_items(project, assets, cancelled)?;
        let items = resolved.as_ref().map(|resolved| &resolved.metadata);
        let project = resolved
            .as_ref()
            .map_or(project, |resolved| &resolved.project);
        let fail = |message: String| vec![Diagnostic::message(&project.entry, message)];
        if project.materials.len() > MAX_ASSIGNMENTS {
            return Err(fail(format!(
                "At most {MAX_ASSIGNMENTS} materials apply to one project."
            )));
        }
        let (mut model, transforms) = self.compose_models(project, revision, assets, cancelled)?;
        let scene = model
            .scene_cancellable(revision as u32, cancelled)
            .map_err(&fail)?;
        let visible: BTreeSet<&str> = scene
            .manifest
            .meshes
            .iter()
            .filter(|mesh| mesh.visible)
            .map(|mesh| mesh.id.as_str())
            .collect();
        let mut identities = BTreeSet::new();
        let mut claimed = BTreeSet::new();
        let mut materials = serde_json::Map::new();
        let mut distributed = None;
        // Made for the first receiver that asks, and used by every one after it.
        let touching = std::cell::OnceCell::new();
        for assignment in &project.materials {
            if !valid_identity(&assignment.id) || !identities.insert(assignment.id.as_str()) {
                return Err(fail(format!(
                    "Material identities are unique, 1-64 characters of letters, digits, '-', '_' or '.': {}.",
                    assignment.id
                )));
            }
            let source = project.modules.get(&assignment.entry).ok_or_else(|| {
                fail(format!(
                    "Material {} uses missing module {}.",
                    assignment.id, assignment.entry
                ))
            })?;
            for mesh in &assignment.meshes {
                if !visible.contains(mesh.as_str()) {
                    return Err(fail(format!(
                        "Material {} names mesh {mesh}, which is not a visible mesh of the project's models.",
                        assignment.id
                    )));
                }
                if !claimed.insert(mesh.as_str()) {
                    return Err(fail(format!(
                        "Mesh {mesh} is assigned more than one material."
                    )));
                }
            }
            if cancelled() {
                return Err(fail("Project authoring cancelled.".into()));
            }
            let ids: BTreeSet<String> = assignment.meshes.iter().cloned().collect();
            let request = material_request(
                project,
                assignment,
                source,
                // Contacts with the rest of the composed scene become `contact` features.
                receivers(&scene, &ids, &transforms)
                    .into_iter()
                    .map(|mut receiver| {
                        receiver.contacts = touching
                            .get_or_init(|| ContactScene::new(&scene))
                            .contacts(&scene, &receiver);
                        receiver
                    })
                    .collect(),
            );
            if ids.is_empty() {
                let program = self.program(&request)?;
                let mut metadata = serde_json::to_value(program.metadata()).unwrap();
                metadata["mapping"] = serde_json::to_value(project.mapping).unwrap();
                materials.insert(assignment.id.clone(), metadata);
                continue;
            }
            let output = match self.build(&request, revision, assets, cancelled) {
                Ok(output) => output,
                // A bake handed to other sessions: go on, so one build reports every such bake
                // of the project and the host can have them all evaluated before it builds again.
                Err(stopped) if stopped.iter().all(|d| d.code == "BAKE_PENDING") => {
                    distributed = Some(stopped);
                    continue;
                }
                Err(failed) => return Err(failed),
            };
            model.replace_receivers(output.model, ids).map_err(&fail)?;
            materials.insert(assignment.id.clone(), output.metadata);
        }
        if let Some(stopped) = distributed {
            return Err(stopped);
        }
        if cancelled() {
            return Err(fail("Project authoring cancelled.".into()));
        }
        let meshes: Vec<&str> = visible.into_iter().collect();
        let mut metadata = serde_json::json!({
            "materials": materials,
            "meshes": meshes,
            "receiverTransforms": transforms,
        });
        if let Some(items) = items {
            metadata["items"] = serde_json::Value::Object(items.clone());
            metadata["bindings"] = bindings(project);
        }
        Ok(ProjectOutput { model, metadata })
    }
}

/// The bake request of one material assignment of a scene project over `receivers`: what
/// `build_project` bakes, and what a layout or field bake of the same project must describe.
pub(super) fn material_request(
    project: &Project,
    assignment: &MaterialAssignment,
    source: &str,
    receivers: Vec<Receiver>,
) -> Request {
    // Features and anchors are part of a receiver's layout, which is also asked for one
    // receiver at a time (`project_layouts`), so every mesh's are passed along.
    request_with(
        project,
        assignment,
        source,
        receivers,
        project.features.clone(),
        project.anchors.clone(),
        project.texel_densities.clone(),
    )
}

/// [`material_request`] over one receiver alone, with that receiver's named features and
/// pattern anchor and no other mesh's: a bake or a layout reads them by the receiver's own
/// identity, so the request describes the same bake. A host that bakes a scene's receivers one
/// at a time (field bakes) would otherwise copy every mesh's features for each of them.
pub(super) fn receiver_request(
    project: &Project,
    assignment: &MaterialAssignment,
    source: &str,
    receiver: Receiver,
) -> Request {
    let features = project
        .features
        .get_key_value(&receiver.id)
        .map(|(id, features)| (id.clone(), features.clone()))
        .into_iter()
        .collect();
    let anchors = project
        .anchors
        .get_key_value(&receiver.id)
        .map(|(id, anchor)| (id.clone(), *anchor))
        .into_iter()
        .collect();
    let densities = project
        .texel_densities
        .get_key_value(&receiver.id)
        .map(|(id, density)| (id.clone(), *density))
        .into_iter()
        .collect();
    request_with(
        project,
        assignment,
        source,
        vec![receiver],
        features,
        anchors,
        densities,
    )
}

fn request_with(
    project: &Project,
    assignment: &MaterialAssignment,
    source: &str,
    receivers: Vec<Receiver>,
    features: BTreeMap<String, Vec<crate::materialspec::program::NamedFeature>>,
    anchors: BTreeMap<String, super::Anchor>,
    densities: BTreeMap<String, f64>,
) -> Request {
    // What the project binds to these receivers: their planes and named features.
    let of = |receiver: &Receiver| receiver.id.clone();
    let planes: BTreeMap<_, _> = receivers
        .iter()
        .filter_map(|receiver| Some((of(receiver), project.planes.get(&receiver.id)?.clone())))
        .collect();
    Request {
        kind: "material".into(),
        entry: assignment.entry.clone(),
        source: source.to_owned(),
        modules: project.modules.clone(),
        parameters: assignment.parameters.clone(),
        seed: assignment.seed,
        resolution: project.resolution,
        inspection: (project.inspection != "final").then(|| project.inspection.clone()),
        mapping: *project
            .mappings
            .get(&assignment.id)
            .unwrap_or(&project.mapping),
        domain: project.domain,
        edges: project.edges.clone(),
        gravity: project.gravity,
        profile: project.profile,
        // The material's own density, where it has one; a receiver's own comes before it.
        texel_density: project
            .texel_densities
            .get(&assignment.id)
            .copied()
            .or(project.texel_density),
        texel_densities: densities,
        receivers,
        anchors,
        materials: if planes.is_empty() {
            BTreeMap::new()
        } else {
            project.plane_materials.clone()
        },
        planes,
        features,
        layers: assignment.layers.clone(),
        ..Default::default()
    }
}

impl Session {
    /// A layered slot's layers for a renderer that composes them itself (the GPU preview's pass
    /// graph), lowest first: each layer's program as a shader plan in which every
    /// `base.<channel>` input is a CPU field (`below.<channel>`) the renderer binds to the
    /// texture of that channel beneath, with the seed values, the features it follows and the
    /// material each of those inputs reads (`stack.base`, `stack.1`, ...). Also the material
    /// each channel of the result comes from.
    fn layer_shaders(
        &mut self,
        project: &Project,
        assignment: &MaterialAssignment,
        base: &crate::materialspec::program::Program,
    ) -> Result<(serde_json::Value, serde_json::Value), Vec<Diagnostic>> {
        use crate::materialspec::program::{self as program, seed_value};
        let fail = |message: String| vec![Diagnostic::message(&assignment.entry, message)];
        let mut latest: BTreeMap<String, String> = base
            .metadata()
            .outputs
            .iter()
            .map(|output| (output.clone(), "stack.base".to_owned()))
            .collect();
        let mut layers = Vec::with_capacity(assignment.layers.len());
        for (k, layer) in assignment.layers.iter().enumerate() {
            let source = project.modules.get(&layer.entry).ok_or_else(|| {
                fail(format!(
                    "Material {} uses missing module {}.",
                    assignment.id, layer.entry
                ))
            })?;
            let program = self.program(&Request {
                kind: "material".into(),
                entry: layer.entry.clone(),
                source: source.clone(),
                modules: project.modules.clone(),
                ..Default::default()
            })?;
            let shader = program.shader().map_err(&fail)?;
            let seeds = |names: Vec<&str>| -> Vec<u32> {
                names
                    .into_iter()
                    .map(|name| seed_value(layer.seed, name))
                    .collect()
            };
            let reads: BTreeMap<&str, &String> = program
                .metadata()
                .externals
                .iter()
                .filter_map(|external| {
                    let channel = external.name.strip_prefix(program::BASE_PLANE)?;
                    Some((external.name.as_str(), latest.get(channel)?))
                })
                .collect();
            let mut entry = serde_json::json!({
                "id": format!("stack.{}", k + 1),
                "entry": layer.entry,
                "parameters": layer.parameters,
                "follow": layer.follow,
                "reads": reads,
                "metadata": program.metadata(),
                "seedValues": seeds(shader.seeds.iter().map(|seed| seed.name.as_str()).collect()),
            });
            match program.shader_plan() {
                Ok(plan) => {
                    let values = seeds(plan.seeds.iter().map(|seed| seed.name.as_str()).collect());
                    entry["plan"] = serde_json::json!({ "seedValues": values, "shader": plan });
                }
                Err(reason) => entry["planError"] = serde_json::json!(reason),
            }
            entry["shader"] = serde_json::to_value(&shader).unwrap();
            layers.push(entry);
            for output in &program.metadata().outputs {
                latest.insert(output.clone(), format!("stack.{}", k + 1));
            }
        }
        Ok((
            serde_json::Value::Array(layers),
            serde_json::to_value(latest).unwrap(),
        ))
    }
}

/// At most this many features of a receiver reach a preview shader, which visits every one of
/// them per pixel; the sharpest creases and the open edges are kept.
pub const PREVIEW_FEATURES: usize = 1024;

/// A receiver's features as a preview shader reads them, twelve numbers each (see
/// [`Session::shade_project`]): its derived creases, open edges and contacts (the receiver's
/// `contacts` must be found), then its named features that `names` (a shader's
/// `named_features`) queries, with kind 5 plus the name's place in `names`. A shader that follows
/// (`follow`, a layer's own name to the receiver's) sees a receiver's feature by the name it
/// follows it as, and the receiver's other names as they are, as a CPU bake renames them.
fn preview_features(
    receiver: &Receiver,
    named: Option<&[crate::materialspec::program::NamedFeature]>,
    names: &[String],
    follow: &BTreeMap<String, crate::materialspec::program::Follow>,
) -> Vec<f64> {
    let mut features = Vec::new();
    let mut found =
        crate::materialspec::program::receiver_features(&receiver.positions, &receiver.indices);
    // Sharpest first, so a receiver with more edges than a shader visits loses only its
    // shallowest creases.
    found.sort_by(|a, b| b.angle.total_cmp(&a.angle));
    found.truncate(PREVIEW_FEATURES - receiver.contacts.len().min(PREVIEW_FEATURES / 2));
    use crate::materialspec::program::FeatureKind as Kind;
    for feature in &found {
        let kind = match feature.kind {
            Kind::Convex => 0.,
            Kind::Concave => 1.,
            Kind::Boundary => 2.,
            Kind::Seam => 3.,
            Kind::Contact => 4.,
            // Derived features are never named.
            Kind::Named => continue,
        };
        let normal = feature.normal.unwrap_or([0.; 3]);
        features.extend(feature.start);
        features.push(kind);
        features.extend(feature.end);
        features.push(feature.angle);
        features.extend(normal);
        features.push(f64::from(u8::from(feature.normal.is_some())));
    }
    for contact in receiver.contacts.iter().take(PREVIEW_FEATURES / 2) {
        features.extend(contact.start);
        features.push(4.);
        features.extend(contact.end);
        features.extend([0., 0., 0., 0., 0.]);
    }
    for feature in named.into_iter().flatten() {
        // The names the shader sees this feature by: a name that follows several features
        // holds the segments of them all, under its one kind.
        for seen in crate::materialspec::program::followed_names(&feature.name, follow) {
            let Some(place) = names.iter().position(|name| name == seen) else {
                continue;
            };
            let last = feature
                .points
                .first()
                .filter(|_| feature.closed && feature.points.len() > 2);
            let ends = feature.points.iter().skip(1).chain(last);
            for (start, end) in feature.points.iter().zip(ends) {
                features.extend(start);
                features.push(5. + place as f64);
                features.extend(end);
                features.extend([0., 0., 0., 0., 0.]);
            }
        }
    }
    features
}

impl Session {
    /// Describes a multi-model, multi-material project for a renderer that evaluates materials
    /// itself: the composed models without any bake, and for each material its program as a
    /// preview shader (see `materialspec::program::Shader`) with the values of its named seeds.
    /// Each assigned mesh lists its material, world bounds and, when the program queries them,
    /// its receiver features as twelve numbers each: start and kind (0 convex, 1 concave,
    /// 2 boundary, 3 seam, 4 contact), end and angle in degrees, middle normal and whether it has
    /// one. Nothing is baked, so this answers in the time it takes to compile the recipes.
    pub fn shade_project(
        &mut self,
        project: &Project,
        revision: u64,
        assets: &dyn Fn(&str) -> Result<Arc<[u8]>, String>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<ProjectOutput, Vec<Diagnostic>> {
        let fail = |message: String| vec![Diagnostic::message(&project.entry, message)];
        if project.schema_version != 2 || project.kind != "material" {
            return Err(fail(
                "Only version 2 material projects have a shader preview.".into(),
            ));
        }
        let prepared = self.prepare_project(project, revision, assets, cancelled)?;
        let metadata = self.shade_prepared(&prepared, cancelled)?;
        Ok(ProjectOutput {
            model: prepared.model,
            metadata,
        })
    }

    /// [`Session::shade_project`]'s description of a prepared project
    /// ([`Session::prepare_project`]); the composed models are the prepared project's.
    pub fn shade_prepared(
        &mut self,
        prepared: &super::PreparedProject,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<serde_json::Value, Vec<Diagnostic>> {
        let super::PreparedProject {
            project,
            items,
            scene,
            transforms,
            touching,
            ..
        } = prepared;
        let items = items.as_ref();
        let fail = |message: String| vec![Diagnostic::message(&project.entry, message)];
        if project.schema_version != 2 || project.kind != "material" {
            return Err(fail(
                "Only version 2 material projects have a shader preview.".into(),
            ));
        }
        let visible: BTreeSet<&str> = scene
            .manifest
            .meshes
            .iter()
            .filter(|mesh| mesh.visible)
            .map(|mesh| mesh.id.as_str())
            .collect();
        let mut materials = serde_json::Map::new();
        let mut meshes = serde_json::Map::new();
        for assignment in &project.materials {
            let source = project.modules.get(&assignment.entry).ok_or_else(|| {
                fail(format!(
                    "Material {} uses missing module {}.",
                    assignment.id, assignment.entry
                ))
            })?;
            let program = self.program(&Request {
                kind: "material".into(),
                entry: assignment.entry.clone(),
                source: source.clone(),
                modules: project.modules.clone(),
                ..Default::default()
            })?;
            let shader = program.shader().map_err(&fail)?;
            let mut metadata = serde_json::to_value(program.metadata()).unwrap();
            metadata["mapping"] = serde_json::to_value(project.mapping).unwrap();
            metadata["seedValues"] = shader
                .seeds
                .iter()
                .map(|seed| crate::materialspec::program::seed_value(assignment.seed, &seed.name))
                .collect();
            let wants_features = shader.features;
            // A program with CPU-only operations (runoff, zones) or external planes also gets a
            // plan: a shader that reads the CPU-baked fields as textures, with its own seeds (it
            // holds less of the program).
            if program
                .metadata()
                .required_inputs
                .iter()
                .any(|input| input == "edges" || input == "zones" || input.starts_with("plane:"))
            {
                match program.shader_plan() {
                    Ok(plan) if !plan.cpu_fields.is_empty() => {
                        let seeds: Vec<_> = plan
                            .seeds
                            .iter()
                            .map(|seed| {
                                crate::materialspec::program::seed_value(
                                    assignment.seed,
                                    &seed.name,
                                )
                            })
                            .collect();
                        metadata["plan"] =
                            serde_json::json!({ "shader": plan, "seedValues": seeds });
                    }
                    Ok(_) => {}
                    Err(reason) => metadata["planError"] = serde_json::json!(reason),
                }
            }
            metadata["shader"] = serde_json::to_value(shader).unwrap();
            // What each layer's shader queries of a receiver: whether it reads derived features,
            // the named features it reads, and which of the receiver's names it follows.
            let mut layer_queries: Vec<(
                bool,
                Vec<String>,
                BTreeMap<String, crate::materialspec::program::Follow>,
            )> = Vec::new();
            if !assignment.layers.is_empty() {
                // A slot its bake would refuse is refused here too, with the same diagnostics.
                self.check_layered(&material_request(project, assignment, source, Vec::new()))?;
                let (layers, channels) = self.layer_shaders(project, assignment, &program)?;
                for (layer, described) in assignment
                    .layers
                    .iter()
                    .zip(layers.as_array().into_iter().flatten())
                {
                    // The plan is what a GPU host bakes; its queries are the program's.
                    let shader = described
                        .get("plan")
                        .map_or(&described["shader"], |plan| &plan["shader"]);
                    let names = shader["namedFeatures"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|name| name.as_str().map(str::to_owned))
                        .collect();
                    layer_queries.push((
                        shader["features"].as_bool().unwrap_or(false),
                        names,
                        layer.follow.clone(),
                    ));
                }
                metadata["layers"] = layers;
                metadata["channels"] = channels;
            }
            materials.insert(assignment.id.clone(), metadata);
            let ids: BTreeSet<String> = assignment.meshes.iter().cloned().collect();
            if let Some(missing) = ids.iter().find(|id| !visible.contains(id.as_str())) {
                return Err(fail(format!(
                    "Material {} names mesh {missing}, which is not a visible mesh of the project's models.",
                    assignment.id
                )));
            }
            for mut receiver in receivers(scene, &ids, transforms) {
                if cancelled() {
                    return Err(fail("Project authoring cancelled.".into()));
                }
                let mut low = [f64::INFINITY; 3];
                let mut high = [f64::NEG_INFINITY; 3];
                for point in receiver.positions.chunks_exact(3) {
                    for axis in 0..3 {
                        low[axis] = low[axis].min(point[axis]);
                        high[axis] = high[axis].max(point[axis]);
                    }
                }
                if wants_features || layer_queries.iter().any(|(derived, _, _)| *derived) {
                    receiver.contacts = touching
                        .get_or_init(|| ContactScene::new(scene))
                        .contacts(scene, &receiver);
                }
                let named = project.features.get(&receiver.id).map(Vec::as_slice);
                let features = if wants_features {
                    preview_features(
                        &receiver,
                        named,
                        &program.metadata().named_features,
                        &BTreeMap::new(),
                    )
                } else {
                    Vec::new()
                };
                let mut entry = serde_json::json!({
                    "material": assignment.id, "boundsMin": low, "boundsMax": high, "features": features,
                    "objectToWorld": receiver.object_to_world,
                });
                if !layer_queries.is_empty() {
                    // Each layer of the slot queries the receiver's features by its own names
                    // (and follows the slot's): a GPU host binds these to that layer's shader.
                    entry["layerFeatures"] = layer_queries
                        .iter()
                        .map(|(derived, names, follow)| {
                            if *derived {
                                preview_features(&receiver, named, names, follow)
                            } else {
                                Vec::new()
                            }
                        })
                        .collect();
                }
                meshes.insert(receiver.id.clone(), entry);
            }
        }
        let ids: Vec<&str> = visible.into_iter().collect();
        let mut metadata = serde_json::json!({
            "materials": materials,
            "meshes": ids,
            "shading": meshes,
            "gravity": project.gravity,
            "domain": project.domain,
        });
        if let Some(items) = items {
            metadata["items"] = serde_json::Value::Object(items.clone());
            metadata["bindings"] = bindings(project);
        }
        Ok(metadata)
    }
}

/// The material each assigned mesh of a (resolved) project shows, by mesh identity: with item
/// models, the assignments a host wrote plus the meshes of the slots the items bind.
fn bindings(project: &Project) -> serde_json::Value {
    project
        .materials
        .iter()
        .flat_map(|assignment| {
            assignment
                .meshes
                .iter()
                .map(|mesh| (mesh.clone(), serde_json::Value::from(assignment.id.clone())))
        })
        .collect::<serde_json::Map<_, _>>()
        .into()
}

/// Points this close to another surface, in meters, touch it.
pub const CONTACT_TOLERANCE: f64 = 0.001;

fn closest_on_triangle(p: [f64; 3], a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> [f64; 3] {
    let sub = |u: [f64; 3], v: [f64; 3]| [u[0] - v[0], u[1] - v[1], u[2] - v[2]];
    let dot = |u: [f64; 3], v: [f64; 3]| u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
    let at = |s: f64, t: f64| std::array::from_fn(|i| a[i] + (b[i] - a[i]) * s + (c[i] - a[i]) * t);
    // Real-Time Collision Detection 5.1.5: regions of the triangle's Voronoi diagram.
    let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    if d1 <= 0. && d2 <= 0. {
        return a;
    }
    let bp = sub(p, b);
    let (d3, d4) = (dot(ab, bp), dot(ac, bp));
    if d3 >= 0. && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0. && d1 >= 0. && d3 <= 0. {
        return at(d1 / (d1 - d3), 0.);
    }
    let cp = sub(p, c);
    let (d5, d6) = (dot(ab, cp), dot(ac, cp));
    if d6 >= 0. && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0. && d2 >= 0. && d6 <= 0. {
        return at(0., d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0. && d4 - d3 >= 0. && d5 - d6 >= 0. {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return std::array::from_fn(|i| b[i] + (c[i] - b[i]) * w);
    }
    let denominator = 1. / (va + vb + vc);
    at(vb * denominator, vc * denominator)
}

/// A scene prepared for finding the contacts of its receivers ([`ContactScene::contacts`]): the
/// bounds of every triangle of its visible meshes, so a point is measured only against the
/// triangles it can touch. Measuring every edge end of every receiver against every triangle
/// of the meshes near it was most of the time a scene of many receivers took to describe.
pub(super) struct ContactScene {
    /// Each visible mesh, by its place in the scene's manifest, with its triangles' bounds
    /// (low corner, then high) in index order.
    meshes: Vec<(usize, Vec<[f32; 6]>)>,
}

impl ContactScene {
    pub(super) fn new(scene: &Scene) -> Self {
        let meshes = scene
            .manifest
            .meshes
            .iter()
            .enumerate()
            .filter(|(_, mesh)| mesh.visible)
            .map(|(at, mesh)| {
                let boxes = scene.indices[mesh.index_start..mesh.index_start + mesh.index_count]
                    .chunks_exact(3)
                    .map(|triangle| {
                        let mut bounds = [f32::INFINITY; 6];
                        bounds[3..].fill(f32::NEG_INFINITY);
                        for corner in triangle {
                            let at = (mesh.vertex_start + *corner as usize) * 3;
                            for axis in 0..3 {
                                let value = scene.positions[at + axis];
                                bounds[axis] = bounds[axis].min(value);
                                bounds[axis + 3] = bounds[axis + 3].max(value);
                            }
                        }
                        bounds
                    })
                    .collect();
                (at, boxes)
            })
            .collect();
        Self { meshes }
    }

    /// Crease and rim edges of `receiver` that lie on another visible mesh of `scene` (the scene
    /// this was made for): both ends and the midpoint within [`CONTACT_TOLERANCE`] of its
    /// triangles. A wall's base on a slab or a crate's bottom edges on the ground are contacts.
    pub(super) fn contacts(&self, scene: &Scene, receiver: &Receiver) -> Vec<super::Contact> {
        let tolerance = CONTACT_TOLERANCE;
        // A triangle whose bounds a point is farther from than this cannot be within the tolerance
        // of it, whatever the rounding of the exact measure below; the measure decides the rest.
        let reach = tolerance * 1.001 + 1e-9;
        let point = |index: usize| -> [f64; 3] {
            std::array::from_fn(|axis| f64::from(scene.positions[index * 3 + axis]))
        };
        let touches = |p: [f64; 3]| {
            self.meshes.iter().any(|(mesh, boxes)| {
                let mesh = &scene.manifest.meshes[*mesh];
                let [low, high] = mesh.bounds;
                let outside =
                    (0..3).any(|i| p[i] < low[i] - tolerance || p[i] > high[i] + tolerance);
                !outside
                    && mesh.id != receiver.id
                    && scene.indices[mesh.index_start..mesh.index_start + mesh.index_count]
                        .chunks_exact(3)
                        .zip(boxes)
                        .any(|(triangle, bounds)| {
                            if (0..3).any(|i| {
                                p[i] < f64::from(bounds[i]) - reach
                                    || p[i] > f64::from(bounds[i + 3]) + reach
                            }) {
                                return false;
                            }
                            let [a, b, c] =
                                [0, 1, 2].map(|k| point(mesh.vertex_start + triangle[k] as usize));
                            let q = closest_on_triangle(p, a, b, c);
                            let gap: f64 = (0..3).map(|i| (p[i] - q[i]).powi(2)).sum::<f64>();
                            gap <= tolerance * tolerance
                        })
            })
        };
        crate::materialspec::program::receiver_features(&receiver.positions, &receiver.indices)
            .into_iter()
            .filter(|feature| {
                let middle = std::array::from_fn(|i| (feature.start[i] + feature.end[i]) * 0.5);
                touches(feature.start) && touches(feature.end) && touches(middle)
            })
            .map(|feature| super::Contact {
                start: feature.start,
                end: feature.end,
            })
            .collect()
    }
}

pub(super) fn receivers(
    scene: &Scene,
    ids: &BTreeSet<String>,
    transforms: &BTreeMap<String, [f64; 16]>,
) -> Vec<Receiver> {
    scene
        .manifest
        .meshes
        .iter()
        .filter(|mesh| mesh.visible && ids.contains(&mesh.id))
        .map(|mesh| Receiver {
            id: mesh.id.clone(),
            object_id: mesh.object_id.clone(),
            positions: scene.positions
                [mesh.vertex_start * 3..(mesh.vertex_start + mesh.vertex_count) * 3]
                .iter()
                .map(|n| *n as f64)
                .collect(),
            normals: scene.normals
                [mesh.vertex_start * 3..(mesh.vertex_start + mesh.vertex_count) * 3]
                .iter()
                .map(|n| *n as f64)
                .collect(),
            uvs: scene.uvs[mesh.vertex_start * 2..(mesh.vertex_start + mesh.vertex_count) * 2]
                .iter()
                .map(|n| *n as f64)
                .collect(),
            indices: scene.indices[mesh.index_start..mesh.index_start + mesh.index_count].to_vec(),
            object_to_world: transforms.get(&mesh.id).copied(),
            contacts: Vec::new(),
        })
        .collect()
}
