//! Project the generator's explicit foliage levels into the portable render contract.
use crate::{
    Scene,
    batches::{Batch, Foliage, InstanceBatch},
};
use std::collections::{BTreeMap, BTreeSet};
use superspec_types::{
    batch::FoliageFamily,
    lod::{GeometryLodGroup, GeometryLodLevel},
};

impl Scene {
    /// Declare the already-authored foliage alternatives for portable delivery.
    pub fn project_foliage_lods(&mut self) -> Result<(), String> {
        project_foliage(self)
    }
}

/// Keep foliage's finer per-tile metadata unchanged while also describing whole
/// authored levels to exporters and consumers without the foliage renderer.
/// Shadow/painted-mass carriers and untagged ground are independent of these levels.
pub fn project_foliage(scene: &mut Scene) -> Result<(), String> {
    let groups = with_foliage(&scene.manifest.geometry_lods, scene.manifest.batches.iter())?;
    super::validate(
        &groups,
        scene
            .manifest
            .meshes
            .iter()
            .map(|m| (m.id.as_str(), m.visible)),
        scene
            .manifest
            .batches
            .iter()
            .map(|b| (b.id.as_str(), b.visible)),
    )?;
    scene.manifest.geometry_lods = groups;
    Ok(())
}

pub(crate) struct FoliageBatch<'a> {
    id: &'a str,
    visible: bool,
    foliage: Option<&'a Foliage>,
}

impl<'a> From<&'a Batch> for FoliageBatch<'a> {
    fn from(batch: &'a Batch) -> Self {
        Self {
            id: &batch.id,
            visible: batch.visible,
            foliage: batch.foliage.as_ref(),
        }
    }
}

impl<'a> From<&'a InstanceBatch> for FoliageBatch<'a> {
    fn from(batch: &'a InstanceBatch) -> Self {
        Self {
            id: &batch.id,
            visible: batch.visible,
            foliage: batch.foliage.as_ref(),
        }
    }
}

pub(crate) fn with_foliage<'a>(
    existing: &[GeometryLodGroup],
    batches: impl IntoIterator<Item = impl Into<FoliageBatch<'a>>>,
) -> Result<Vec<GeometryLodGroup>, String> {
    let batches: Vec<_> = batches.into_iter().map(Into::into).collect();
    let group_ids: BTreeSet<_> = existing.iter().map(|group| group.id.as_str()).collect();
    let owned: BTreeSet<_> = existing
        .iter()
        .flat_map(|g| &g.levels)
        .flat_map(|l| l.batches.iter().map(String::as_str))
        .collect();
    let owned_patches: BTreeSet<_> = batches
        .iter()
        .filter(|b| owned.contains(b.id))
        .filter_map(|b| b.foliage)
        .filter(|f| f.family != FoliageFamily::Shadow)
        .map(|f| f.patch.as_str())
        .collect();
    let mut patches: BTreeMap<&str, BTreeMap<u32, GeometryLodLevel>> = BTreeMap::new();
    for batch in &batches {
        let Some(foliage) = batch.foliage else {
            continue;
        };
        if !batch.visible || foliage.family == FoliageFamily::Shadow || owned.contains(batch.id) {
            continue;
        }
        let levels = patches.entry(&foliage.patch).or_default();
        let level = levels
            .entry(foliage.level)
            .or_insert_with(|| GeometryLodLevel {
                distance: foliage.distance,
                meshes: vec![],
                batches: vec![],
                screen_coverage: None,
            });
        if level.distance != foliage.distance {
            return Err(format!(
                "Foliage patch {} level {} has inconsistent authored distances (batch {}).",
                foliage.patch, foliage.level, batch.id
            ));
        }
        level.batches.push(batch.id.to_owned());
    }
    let mut additions = Vec::new();
    for (patch, levels) in patches {
        let id = format!("{patch}/foliage-lod");
        if group_ids.contains(id.as_str()) || (levels.len() < 2 && owned_patches.contains(patch)) {
            return Err(format!(
                "Foliage patch {patch} adds unowned batches to existing geometry LOD {id}; replace its complete authored chain."
            ));
        }
        // A deliberately fixed generator tier remains an ordinary render batch.
        if levels.len() < 2 {
            continue;
        }
        let mut levels: Vec<_> = levels.into_values().collect();
        for level in &mut levels {
            level.batches.sort();
        }
        additions.push(GeometryLodGroup { id, levels });
    }
    super::validate(&additions, [], batches.iter().map(|b| (b.id, b.visible)))?;
    let mut result = existing.to_vec();
    result.extend(additions);
    Ok(result)
}
