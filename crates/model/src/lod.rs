//! Shared validation for declared geometry levels, used by delivery and all exporters.
use std::collections::{BTreeMap, BTreeSet};
use superspec_types::limits::{MAX_GEOMETRY_LOD_GROUPS, MAX_GEOMETRY_LOD_LEVELS};
use superspec_types::lod::GeometryLodGroup;

pub(crate) fn prefix_placements(groups: &mut [GeometryLodGroup], prefix: &str) {
    for group in groups {
        group.id.insert_str(0, prefix);
        for level in &mut group.levels {
            for id in level.meshes.iter_mut().chain(&mut level.batches) {
                id.insert_str(0, prefix);
            }
        }
    }
}
mod foliage;
pub use foliage::project_foliage;
pub(crate) use foliage::with_foliage;

pub fn validate<'a>(
    groups: &[GeometryLodGroup],
    meshes: impl IntoIterator<Item = (&'a str, bool)>,
    batches: impl IntoIterator<Item = (&'a str, bool)>,
) -> Result<(), String> {
    if groups.is_empty() {
        return Ok(());
    }
    if groups.len() > MAX_GEOMETRY_LOD_GROUPS {
        return Err("Too many geometry LOD groups.".into());
    }
    let meshes: BTreeMap<_, _> = meshes.into_iter().collect();
    let batches: BTreeMap<_, _> = batches.into_iter().collect();
    let mut groups_seen = BTreeSet::new();
    let mut meshes_seen = BTreeSet::new();
    let mut batches_seen = BTreeSet::new();
    for group in groups {
        if group.id.is_empty() || group.id.len() > 4096 || !groups_seen.insert(&group.id) {
            return Err(format!(
                "Invalid or duplicate geometry LOD group {}.",
                group.id
            ));
        }
        if !(2..=MAX_GEOMETRY_LOD_LEVELS).contains(&group.levels.len()) {
            return Err(format!(
                "Geometry LOD {} needs 2 to {MAX_GEOMETRY_LOD_LEVELS} authored levels.",
                group.id
            ));
        }
        let coverage = group.levels[0].screen_coverage.is_some();
        for (index, level) in group.levels.iter().enumerate() {
            if !level.distance.is_finite()
                || level.distance < 0.
                || (index == 0 && level.distance != 0.)
                || (index > 0 && level.distance <= group.levels[index - 1].distance)
            {
                return Err(format!(
                    "Geometry LOD {} has invalid distance at level {index}.",
                    group.id
                ));
            }
            if level.screen_coverage.is_some() != coverage
                || level.screen_coverage.is_some_and(|n| {
                    !n.is_finite()
                        || !(0. ..=1.).contains(&n)
                        || (index > 0 && n >= group.levels[index - 1].screen_coverage.unwrap_or(1.))
                })
            {
                return Err(format!(
                    "Geometry LOD {} has invalid screen coverage at level {index}.",
                    group.id
                ));
            }
            if level.meshes.is_empty() && level.batches.is_empty() {
                return Err(format!(
                    "Geometry LOD {} has an empty level {index}.",
                    group.id
                ));
            }
            for (ids, available, seen, kind) in [
                (&level.meshes, &meshes, &mut meshes_seen, "mesh"),
                (&level.batches, &batches, &mut batches_seen, "batch"),
            ] {
                for id in ids {
                    if available.get(id.as_str()) != Some(&true) {
                        return Err(format!(
                            "Geometry LOD {} level {index} references missing or hidden {kind} {id}.",
                            group.id
                        ));
                    }
                    if !seen.insert(id) {
                        return Err(format!("Geometry LOD {} overlaps {kind} {id}.", group.id));
                    }
                }
            }
        }
    }
    Ok(())
}

pub fn check(scene: &crate::Scene) -> Result<(), String> {
    validate(
        &scene.manifest.geometry_lods,
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
    )
}

/// Additional render variants cannot expand the canonical collision selection.
pub fn alternate_ids(groups: &[GeometryLodGroup]) -> (BTreeSet<&str>, BTreeSet<&str>) {
    let mut meshes = BTreeSet::new();
    let mut batches = BTreeSet::new();
    for level in groups.iter().flat_map(|g| g.levels.iter().skip(1)) {
        meshes.extend(level.meshes.iter().map(String::as_str));
        batches.extend(level.batches.iter().map(String::as_str));
    }
    (meshes, batches)
}
