//! Scene composition from saved material and item definitions, with explicit project overrides.
use crate::{
    authoring::{Diagnostic, Mapping, Output, Receiver, Request, Session},
    itemspec::definition,
    materialspec::program::MaterialRequest,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use superspec_model::composition::WorldModel;

const MAX_WORLD_ITEM_PLACEMENTS: usize = 1024;
const MAX_WORLD_MATERIAL_ASSIGNMENTS: usize = 256;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct World {
    pub items: Vec<Placement>,
    pub materials: Vec<Assignment>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub material_lods: crate::materialspec::program::MaterialLodSources,
    pub theme_presets: BTreeMap<String, BTreeMap<String, serde_json::Value>>,
    pub project_presets: BTreeMap<String, BTreeMap<String, serde_json::Value>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Placement {
    pub id: String,
    pub entry: String,
    #[serde(default)]
    pub parameters: BTreeMap<String, serde_json::Value>,
    /// Explicit slot replacements; otherwise the item's saved assignments remain authoritative.
    #[serde(default)]
    pub slots: BTreeMap<String, MaterialRequest>,
    #[serde(default = "definition::identity")]
    pub transform: definition::Transform,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Assignment {
    pub objects: Vec<String>,
    pub entry: String,
    #[serde(default)]
    pub parameters: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub seed: u32,
    #[serde(default)]
    pub mapping: Mapping,
}

/// Physical compilation precedes item placement and visual overrides. Call validation on the result
/// when the host needs navigation or declared route checks over the added collision contributions.
pub fn build_model(
    session: &mut Session,
    source_name: &str,
    source: &str,
    modules: &BTreeMap<String, String>,
    world: &World,
    resolution: usize,
    revision: u64,
    assets: &dyn Fn(&str) -> Result<Arc<[u8]>, String>,
    cancelled: &dyn Fn() -> bool,
) -> Result<WorldModel, Vec<Diagnostic>> {
    let fail = |message: String| vec![Diagnostic::message(source_name, message)];
    crate::materialspec::program::check_lod_owners(&world.material_lods, modules).map_err(&fail)?;
    if world.items.len() > MAX_WORLD_ITEM_PLACEMENTS
        || world.materials.len() > MAX_WORLD_MATERIAL_ASSIGNMENTS
    {
        return Err(fail("World authoring placement allowance exceeded.".into()));
    }
    if cancelled() {
        return Err(fail("World authoring cancelled.".into()));
    }
    let scene = session
        .compile(source, revision as u32, &|name| {
            assets(name).map(|bytes| bytes.to_vec())
        })
        .map_err(|error| {
            vec![Diagnostic {
                code: error.code.into(),
                source_name: source_name.into(),
                line: error.source.line,
                column: error.source.column,
                offset: error.source.offset,
                end: error.source.end,
                message: error.message.to_string(),
            }]
        })?;
    let mut model = WorldModel::from_scene(scene).map_err(&fail)?;
    let mut ids = BTreeSet::new();
    let mut transforms = BTreeMap::<String, [f64; 16]>::new();
    for placement in &world.items {
        if placement.id.is_empty() || placement.id.len() > 256 || !ids.insert(&placement.id) {
            return Err(fail(
                "World item placement identities must be unique and bounded.".into(),
            ));
        }
        if cancelled() {
            return Err(fail("World authoring cancelled.".into()));
        }
        let text = modules
            .get(&placement.entry)
            .ok_or_else(|| fail(format!("Missing saved item {}.", placement.entry)))?;
        let mut parameters = world
            .theme_presets
            .get(&placement.entry)
            .cloned()
            .unwrap_or_default();
        parameters.extend(
            world
                .project_presets
                .get(&placement.entry)
                .cloned()
                .unwrap_or_default(),
        );
        parameters.extend(placement.parameters.clone());
        let request = Request {
            kind: "item".into(),
            entry: placement.entry.clone(),
            source: text.clone(),
            material_lods: world.material_lods.clone(),
            modules: modules.clone(),
            parameters,
            resolution,
            placement_transform: Some(placement.transform),
            slot_overrides: placement.slots.clone(),
            ..Default::default()
        };
        let mut output = session.build(&request, revision, assets, cancelled)?;
        let prefix = format!("item/{}/", placement.id);
        let receiver_transforms: BTreeMap<String, [f64; 16]> =
            serde_json::from_value(output.metadata["receiverTransforms"].clone())
                .map_err(|e| fail(e.to_string()))?;
        for (id, matrix) in receiver_transforms {
            transforms.insert(format!("{prefix}{id}"), matrix);
        }
        output.model.place_under(&format!("item/{}", placement.id));
        model.append(output.model).map_err(&fail)?;
    }
    for assignment in &world.materials {
        if cancelled() {
            return Err(fail("World authoring cancelled.".into()));
        }
        let scene = model
            .scene_cancellable(revision as u32, cancelled)
            .map_err(&fail)?;
        let objects: BTreeSet<_> = assignment.objects.iter().cloned().collect();
        if objects.is_empty() || objects.len() != assignment.objects.len() {
            return Err(fail(
                "Material assignments need distinct object identities.".into(),
            ));
        }
        for object in &objects {
            if !scene
                .manifest
                .meshes
                .iter()
                .any(|mesh| mesh.visible && &mesh.object_id == object)
            {
                return Err(fail(format!(
                    "Unknown material assignment object {object}."
                )));
            }
        }
        let selected: BTreeSet<_> = scene
            .manifest
            .meshes
            .iter()
            .filter(|mesh| mesh.visible && objects.contains(&mesh.object_id))
            .map(|mesh| mesh.id.clone())
            .collect();
        let receivers = scene
            .manifest
            .meshes
            .iter()
            .filter(|mesh| selected.contains(&mesh.id))
            .map(|mesh| Receiver {
                id: mesh.id.clone(),
                object_id: mesh.object_id.clone(),
                positions: scene.positions
                    [mesh.vertex_start * 3..(mesh.vertex_start + mesh.vertex_count) * 3]
                    .iter()
                    .map(|v| *v as f64)
                    .collect(),
                normals: scene.normals
                    [mesh.vertex_start * 3..(mesh.vertex_start + mesh.vertex_count) * 3]
                    .iter()
                    .map(|v| *v as f64)
                    .collect(),
                uvs: scene.uvs[mesh.vertex_start * 2..(mesh.vertex_start + mesh.vertex_count) * 2]
                    .iter()
                    .map(|v| *v as f64)
                    .collect(),
                indices: scene.indices[mesh.index_start..mesh.index_start + mesh.index_count]
                    .to_vec(),
                object_to_world: transforms.get(&mesh.id).copied(),
                contacts: Vec::new(),
            })
            .collect();
        let source = modules
            .get(&assignment.entry)
            .ok_or_else(|| fail(format!("Missing saved material {}.", assignment.entry)))?;
        let request = Request {
            kind: "material".into(),
            entry: assignment.entry.clone(),
            source: source.clone(),
            material_lods: world.material_lods.clone(),
            modules: modules.clone(),
            parameters: assignment.parameters.clone(),
            seed: assignment.seed,
            resolution,
            receivers,
            mapping: assignment.mapping,
            ..Default::default()
        };
        let Output {
            model: replacement,
            metadata: _,
        } = session.build(&request, revision, assets, cancelled)?;
        model
            .replace_receivers(replacement, selected)
            .map_err(&fail)?;
    }
    if cancelled() {
        return Err(fail("World authoring cancelled.".into()));
    }
    Ok(model)
}
