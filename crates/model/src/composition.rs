//! Owned scene composition shared by editor hosts and WorldSpec adapters.
mod admission;
mod world;
use crate::{Asset, Mesh, Scene};
pub use admission::Limits;
use std::collections::{BTreeMap, BTreeSet};
pub use world::{Layer, WorldModel};

fn append_geometry(output: &mut Scene, source: &Scene, mesh: &Mesh) -> Mesh {
    let vertex_start = output.positions.len() / 3;
    let index_start = output.indices.len();
    let end = mesh.vertex_start + mesh.vertex_count;
    output
        .positions
        .extend_from_slice(&source.positions[mesh.vertex_start * 3..end * 3]);
    output
        .normals
        .extend_from_slice(&source.normals[mesh.vertex_start * 3..end * 3]);
    output
        .colors
        .extend_from_slice(&source.colors[mesh.vertex_start * 3..end * 3]);
    output
        .uvs
        .extend_from_slice(&source.uvs[mesh.vertex_start * 2..end * 2]);
    output
        .indices
        .extend_from_slice(&source.indices[mesh.index_start..mesh.index_start + mesh.index_count]);
    Mesh {
        id: mesh.id.clone(),
        object_id: mesh.object_id.clone(),
        source: mesh.source.clone(),
        material: mesh.material.clone(),
        visible: mesh.visible,
        vertex_start,
        vertex_count: mesh.vertex_count,
        index_start,
        index_count: mesh.index_count,
        bounds: mesh.bounds,
    }
}

fn ranges(scene: &Scene) -> Result<(), String> {
    let count = scene.positions.len() / 3;
    if scene.positions.len() % 3 != 0
        || scene.normals.len() != count * 3
        || scene.colors.len() != count * 3
        || scene.uvs.len() != count * 2
        || scene.manifest.vertex_count != count
        || scene.manifest.index_count != scene.indices.len()
    {
        return Err("Invalid scene composition buffers.".into());
    }
    for mesh in &scene.manifest.meshes {
        if mesh
            .vertex_start
            .checked_add(mesh.vertex_count)
            .is_none_or(|end| end > count)
            || mesh
                .index_start
                .checked_add(mesh.index_count)
                .is_none_or(|end| end > scene.indices.len())
        {
            return Err("Invalid scene composition mesh range.".into());
        }
    }
    for asset in &scene.manifest.assets {
        if asset
            .byte_start
            .checked_add(asset.byte_length)
            .is_none_or(|end| end > scene.asset_bytes.len())
        {
            return Err("Invalid scene composition asset range.".into());
        }
    }
    crate::batches::check(scene)
}

fn image_pixels(scene: &Scene, asset: &Asset) -> Result<usize, String> {
    if let Some(pixels) = scene.image_sizes.get(&asset.id) {
        return Ok(*pixels);
    }
    // The header gives the base level size; admission already validated the image itself.
    let info = superspec_geometry::image::info(
        &scene.asset_bytes[asset.byte_start..asset.byte_start + asset.byte_length],
    )?;
    Ok(info.width * info.height)
}

fn same_triangles(a: &Scene, am: &Mesh, b: &Scene, bm: &Mesh) -> bool {
    if am.index_count != bm.index_count {
        return false;
    }
    a.indices[am.index_start..am.index_start + am.index_count]
        .iter()
        .zip(&b.indices[bm.index_start..bm.index_start + bm.index_count])
        .all(|(&ai, &bi)| {
            if ai as usize >= am.vertex_count || bi as usize >= bm.vertex_count {
                return false;
            }
            let ap = (am.vertex_start + ai as usize) * 3;
            let bp = (bm.vertex_start + bi as usize) * 3;
            a.positions[ap..ap + 3] == b.positions[bp..bp + 3]
        })
}

/// Replaces selected visual receivers while retaining structural metadata, collision IDs and source ranges.
pub fn replace_receivers(
    mut base: Scene,
    replacement: Scene,
    ids: &BTreeSet<String>,
) -> Result<Scene, String> {
    ranges(&base)?;
    ranges(&replacement)?;
    let updated: BTreeMap<_, _> = replacement
        .manifest
        .meshes
        .iter()
        .filter(|mesh| mesh.visible)
        .map(|mesh| (mesh.id.as_str(), mesh))
        .collect();
    for id in ids {
        if !updated.contains_key(id.as_str())
            || !base
                .manifest
                .meshes
                .iter()
                .any(|mesh| mesh.visible && &mesh.id == id)
        {
            return Err(format!("Unknown replacement receiver {id}."));
        }
    }
    // Atlas bindings may duplicate vertices without changing a single physical corner.
    let physical_unchanged = base
        .manifest
        .colliders
        .iter()
        .filter(|id| ids.contains(*id))
        .all(|id| {
            base.manifest
                .meshes
                .iter()
                .find(|mesh| &mesh.id == id)
                .is_some_and(|original| {
                    same_triangles(&base, original, &replacement, updated[id.as_str()])
                })
        });
    let mut output = Scene::empty(replacement.manifest.revision, "superspec-composition-v1");
    for original in &base.manifest.meshes {
        let changed = original.visible && ids.contains(&original.id);
        let mut mesh = if changed {
            append_geometry(&mut output, &replacement, updated[original.id.as_str()])
        } else {
            append_geometry(&mut output, &base, original)
        };
        mesh.object_id = original.object_id.clone();
        mesh.source = original.source.clone();
        output.manifest.meshes.push(mesh);
        if changed {
            base.manifest.mesh_tangents.remove(&original.id);
        }
    }
    base.manifest
        .mesh_tangents
        .extend(replacement.manifest.mesh_tangents.clone());
    if output.positions.len() / 3 > 6_000_000 || output.indices.len() > 18_000_000 {
        return Err("Composed scene exceeds its geometry allowance.".into());
    }
    for material in &replacement.manifest.materials {
        if let Some(existing) = base
            .manifest
            .materials
            .iter_mut()
            .find(|value| value.id == material.id)
        {
            if existing != material {
                return Err(format!("Conflicting replacement material {}.", material.id));
            }
        } else {
            base.manifest.materials.push(material.clone());
        }
    }
    // Batches are not receivers: they stay as the base has them, with the materials they use.
    let used: BTreeSet<_> = output
        .manifest
        .meshes
        .iter()
        .filter_map(|mesh| mesh.material.clone())
        .chain(
            base.manifest
                .batches
                .iter()
                .filter_map(|batch| batch.material.clone()),
        )
        .collect();
    base.manifest
        .materials
        .retain(|material| used.contains(&material.id));
    let required: BTreeSet<String> = base
        .manifest
        .materials
        .iter()
        .flat_map(|material| material.companions().into_iter().map(str::to_owned))
        .collect();
    let mut images = Vec::<(String, &'static str, Vec<u8>, usize)>::new();
    for scene in [&base, &replacement] {
        for asset in &scene.manifest.assets {
            if !required.contains(&asset.id) {
                continue;
            }
            let entry = (
                asset.id.clone(),
                asset.mime,
                scene.asset_bytes[asset.byte_start..asset.byte_start + asset.byte_length].to_vec(),
                image_pixels(scene, asset)?,
            );
            if let Some(existing) = images.iter_mut().find(|image| image.0 == asset.id) {
                if existing.1 != entry.1 || existing.2 != entry.2 {
                    return Err(format!("Conflicting replacement image {}.", asset.id));
                }
            } else {
                images.push(entry);
            }
        }
    }
    if images.iter().map(|image| image.2.len()).sum::<usize>() > crate::MAX_SCENE_IMAGE_BYTES {
        return Err("Composed scene image byte allowance exceeded.".into());
    }
    for (id, mime, bytes, pixels) in images {
        output.reserve_image_pixels(pixels)?;
        output.image_sizes.insert(id.clone(), pixels);
        output.manifest.assets.push(Asset {
            id,
            mime,
            byte_start: output.asset_bytes.len(),
            byte_length: bytes.len(),
        });
        output.asset_bytes.extend(bytes);
    }
    let meshes = std::mem::take(&mut output.manifest.meshes);
    let assets = std::mem::take(&mut output.manifest.assets);
    output.batches = std::mem::take(&mut base.batches);
    output.manifest = base.manifest;
    output.manifest.revision = replacement.manifest.revision;
    output.manifest.meshes = meshes;
    output.manifest.assets = assets;
    output.manifest.vertex_count = output.positions.len() / 3;
    output.manifest.index_count = output.indices.len();
    output.manifest.asset_byte_count = output.asset_bytes.len();
    output.manifest.lightmap = None;
    if !physical_unchanged {
        output.manifest.validation = None;
        output.manifest.geometry_validation = "not-run";
        output.manifest.navigation_validation = "not-run";
    }
    output.export_solids = base.export_solids;
    Ok(output)
}

/// Places a scene's identities under `prefix` so several independently authored models can be
/// composed: mesh, owner, collider and tangent identities become `<prefix><id>`. Material and
/// image identities are kept; `append` accepts equal duplicates and rejects conflicting ones.
pub fn prefixed(mut scene: Scene, prefix: &str) -> Scene {
    let rename = |id: &mut String| *id = format!("{prefix}{id}");
    for mesh in &mut scene.manifest.meshes {
        rename(&mut mesh.id);
        rename(&mut mesh.object_id);
    }
    for batch in &mut scene.manifest.batches {
        rename(&mut batch.id);
        rename(&mut batch.object_id);
    }
    scene.manifest.colliders.iter_mut().for_each(rename);
    scene.manifest.decals.iter_mut().for_each(rename);
    scene.manifest.mesh_tangents = std::mem::take(&mut scene.manifest.mesh_tangents)
        .into_iter()
        .map(|(id, tangents)| (format!("{prefix}{id}"), tangents))
        .collect();
    if let Some(context) = &mut scene.manifest.architecture {
        for object in &mut context.objects {
            rename(&mut object.id);
            object.instances.iter_mut().for_each(rename);
            object.contributors.iter_mut().for_each(rename);
        }
    }
    for solid in &mut scene.export_solids {
        for face in &mut solid.faces {
            rename(&mut face.object);
        }
    }
    scene
}

/// Appends an already admitted scene and its provenance without changing the input producer's resources.
pub fn append(mut base: Scene, addition: Scene) -> Result<Scene, String> {
    ranges(&base)?;
    ranges(&addition)?;
    if let (Some(base), Some(addition)) =
        (&base.manifest.architecture, &addition.manifest.architecture)
    {
        if addition
            .objects
            .iter()
            .any(|object| base.objects.iter().any(|old| old.id == object.id))
        {
            return Err("Composed scenes contain duplicate source owner identities.".into());
        }
    }
    if addition
        .manifest
        .meshes
        .iter()
        .any(|added| base.manifest.meshes.iter().any(|mesh| mesh.id == added.id))
    {
        return Err("Composed scenes contain duplicate mesh identities.".into());
    }
    if addition
        .manifest
        .batches
        .iter()
        .any(|added| base.manifest.meshes.iter().any(|mesh| mesh.id == added.id))
        || base.manifest.batches.iter().any(|batch| {
            addition
                .manifest
                .meshes
                .iter()
                .any(|mesh| mesh.id == batch.id)
        })
    {
        return Err("Composed scenes use one identity for a mesh and a batch.".into());
    }
    base.append_batches(&addition)?;
    if base.positions.len() / 3 + addition.positions.len() / 3 > 6_000_000
        || base.indices.len() + addition.indices.len() > 18_000_000
    {
        return Err("Composed scene exceeds its geometry allowance.".into());
    }
    for material in &addition.manifest.materials {
        if let Some(existing) = base.manifest.materials.iter().find(|m| m.id == material.id) {
            if existing != material {
                return Err(format!("Conflicting composed material {}.", material.id));
            }
        } else {
            base.manifest.materials.push(material.clone());
        }
    }
    for asset in &addition.manifest.assets {
        let bytes = &addition.asset_bytes[asset.byte_start..asset.byte_start + asset.byte_length];
        if let Some(existing) = base.manifest.assets.iter().find(|a| a.id == asset.id) {
            if existing.mime != asset.mime
                || &base.asset_bytes
                    [existing.byte_start..existing.byte_start + existing.byte_length]
                    != bytes
            {
                return Err(format!("Conflicting composed image {}.", asset.id));
            }
        } else {
            if base.asset_bytes.len() + bytes.len() > crate::MAX_SCENE_IMAGE_BYTES {
                return Err("Composed image byte allowance exceeded.".into());
            }
            let pixels = image_pixels(&addition, asset)?;
            base.reserve_image_pixels(pixels)?;
            base.image_sizes.insert(asset.id.clone(), pixels);
            base.manifest.assets.push(Asset {
                id: asset.id.clone(),
                mime: asset.mime,
                byte_start: base.asset_bytes.len(),
                byte_length: bytes.len(),
            });
            base.asset_bytes.extend_from_slice(bytes);
        }
    }
    for mesh in &addition.manifest.meshes {
        let mesh = append_geometry(&mut base, &addition, mesh);
        base.manifest.meshes.push(mesh);
    }
    base.manifest.colliders.extend(addition.manifest.colliders);
    base.manifest.decals.extend(addition.manifest.decals);
    base.manifest
        .mesh_tangents
        .extend(addition.manifest.mesh_tangents);
    if let Some(context) = addition.manifest.architecture {
        if let Some(existing) = &mut base.manifest.architecture {
            existing.objects.extend(context.objects);
        } else {
            base.manifest.architecture = Some(context);
        }
    }
    base.manifest.vertex_count = base.positions.len() / 3;
    base.manifest.index_count = base.indices.len();
    base.manifest.asset_byte_count = base.asset_bytes.len();
    base.lightmap_uvs = None;
    base.lightmap_charts = None;
    base.manifest.lightmap = None;
    base.manifest.validation = None;
    base.manifest.geometry_validation = "not-run";
    base.manifest.navigation_validation = "not-run";
    Ok(base)
}
