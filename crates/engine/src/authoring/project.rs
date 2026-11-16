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

/// One imported model of a multi-model project: a GLB companion or an AML scene. Its mesh and
/// owner identities are placed under `<id>/` so several models compose without collisions.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ModelSource {
    pub id: String,
    /// `glb` (the companion named `name`) or `aml` (the scene text in `source`).
    pub kind: String,
    pub source: Option<String>,
    pub name: Option<String>,
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
                request.receivers = receivers(&base, &ids, &transforms)
                    .into_iter()
                    .map(|mut receiver| {
                        receiver.contacts = contacts(&base, &receiver);
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
                other => {
                    return Err(fail(format!(
                        "Model {} has unsupported kind {other}; use glb or aml.",
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
                        receiver.contacts = contacts(&scene, &receiver);
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
        Ok(ProjectOutput {
            model,
            metadata: serde_json::json!({
                "materials": materials,
                "meshes": meshes,
                "receiverTransforms": transforms,
            }),
        })
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
    Request {
        kind: "material".into(),
        entry: assignment.entry.clone(),
        source: source.to_owned(),
        modules: project.modules.clone(),
        parameters: assignment.parameters.clone(),
        seed: assignment.seed,
        resolution: project.resolution,
        inspection: (project.inspection != "final").then(|| project.inspection.clone()),
        mapping: project.mapping,
        domain: project.domain,
        edges: project.edges.clone(),
        gravity: project.gravity,
        profile: project.profile,
        texel_density: project.texel_density,
        receivers,
        ..Default::default()
    }
}

/// At most this many features of a receiver reach a preview shader, which visits every one of
/// them per pixel; the sharpest creases and the open edges are kept.
pub const PREVIEW_FEATURES: usize = 1024;

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
        self.begin_authoring(revision, &project.entry)?;
        let (model, transforms) = self.compose_models(project, revision, assets, cancelled)?;
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
            // A program with CPU-only operations (runoff) also gets a plan: a shader that reads the
            // CPU-baked fields as textures, with its own seeds (it holds less of the program).
            if program
                .metadata()
                .required_inputs
                .iter()
                .any(|input| input == "edges")
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
            materials.insert(assignment.id.clone(), metadata);
            let ids: BTreeSet<String> = assignment.meshes.iter().cloned().collect();
            if let Some(missing) = ids.iter().find(|id| !visible.contains(id.as_str())) {
                return Err(fail(format!(
                    "Material {} names mesh {missing}, which is not a visible mesh of the project's models.",
                    assignment.id
                )));
            }
            for mut receiver in receivers(&scene, &ids, &transforms) {
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
                let mut features = Vec::new();
                if wants_features {
                    receiver.contacts = contacts(&scene, &receiver);
                    let mut found = crate::materialspec::program::receiver_features(
                        &receiver.positions,
                        &receiver.indices,
                    );
                    // Sharpest first, so a receiver with more edges than a shader visits loses
                    // only its shallowest creases.
                    found.sort_by(|a, b| b.angle.total_cmp(&a.angle));
                    found.truncate(
                        PREVIEW_FEATURES - receiver.contacts.len().min(PREVIEW_FEATURES / 2),
                    );
                    use crate::materialspec::program::FeatureKind as Kind;
                    for feature in &found {
                        let kind = match feature.kind {
                            Kind::Convex => 0.,
                            Kind::Concave => 1.,
                            Kind::Boundary => 2.,
                            Kind::Seam => 3.,
                            Kind::Contact => 4.,
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
                }
                meshes.insert(
                    receiver.id.clone(),
                    serde_json::json!({
                        "material": assignment.id, "boundsMin": low, "boundsMax": high, "features": features,
                        "objectToWorld": receiver.object_to_world,
                    }),
                );
            }
        }
        let ids: Vec<&str> = visible.into_iter().collect();
        Ok(ProjectOutput {
            model,
            metadata: serde_json::json!({
                "materials": materials,
                "meshes": ids,
                "shading": meshes,
                "gravity": project.gravity,
                "domain": project.domain,
            }),
        })
    }
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

/// Crease and rim edges of `receiver` that lie on another visible mesh of `scene`: both ends and
/// the midpoint within [`CONTACT_TOLERANCE`] of its triangles. A wall's base on a slab or a
/// crate's bottom edges on the ground are contacts.
pub(super) fn contacts(scene: &Scene, receiver: &Receiver) -> Vec<super::Contact> {
    let tolerance = CONTACT_TOLERANCE;
    let others: Vec<_> = scene
        .manifest
        .meshes
        .iter()
        .filter(|mesh| mesh.visible && mesh.id != receiver.id)
        .collect();
    let point = |index: usize| -> [f64; 3] {
        std::array::from_fn(|axis| f64::from(scene.positions[index * 3 + axis]))
    };
    let touches = |p: [f64; 3]| {
        others.iter().any(|mesh| {
            let [low, high] = mesh.bounds;
            let outside = (0..3).any(|i| p[i] < low[i] - tolerance || p[i] > high[i] + tolerance);
            !outside
                && scene.indices[mesh.index_start..mesh.index_start + mesh.index_count]
                    .chunks_exact(3)
                    .any(|triangle| {
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
