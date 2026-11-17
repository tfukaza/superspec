//! Generated foliage as portable instanced batches (`superspec_model::batches`): one batch per
//! clump variant of a grass patch, and a plant's leaf clusters as one batch beside its bark.
//!
//! What maps to what:
//! - A generated transform (three axes and a translation) becomes a placement: translation,
//!   rotation and scale. The generators emit perpendicular, right-handed axes, which is all a
//!   placement can state; anything else is refused ([`placement`]).
//! - A mesh's named vertex scalars and vectors become the batch's named per-vertex attributes,
//!   and a generated batch's per-instance scalars and vectors its named per-instance attributes,
//!   under the generator's names (`heightRatio`, `bendWeight`, `shadeNormal`, `pivot`, `root`,
//!   `groundNormal`, `variation`, `threshold`, ...). `root` is also the placement's translation.
//! - A mesh's normals are its shade normals where it has them per vertex, else its geometric
//!   ones. A two-sided mesh is delivered with both faces, sharing vertices, as a renderer that
//!   turns normals over on back faces would turn a shade normal into the plant.
//! - A level of detail draws the first instances of each generated batch (they are sorted by
//!   `threshold`), so the converter takes that prefix.
//!
//! Colours are not the generator's business. [`Paint`] lets a caller give vertex colours and
//! per-instance tints until painted materials read the named attributes themselves.
use super::{
    FoliageMesh, InstanceBatch as Generated, PIVOT, ROOT, SHADE_NORMAL, Transform,
    grass::{GrassPatch, GrassTile},
    tree::GeneratedTree,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use superspec_model::{
    batches::{InstanceBatch, Placement},
    instances::LocalMesh,
};

type V3 = [f64; 3];

/// One portable batch with the mesh it draws.
#[derive(Clone, Debug)]
pub struct FoliageBatch {
    pub batch: InstanceBatch,
    pub mesh: LocalMesh,
}

/// Stand-in colours: a linear RGB multiplier per vertex of a mesh, and a tint per instance.
pub struct Paint<'a> {
    pub vertex: &'a dyn Fn(&FoliageMesh, usize) -> V3,
    pub instance: &'a dyn Fn(&Generated, usize) -> [f32; 3],
}

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The placement that is a generated transform: its axes' lengths as the scale and their
/// directions as the rotation. Axes that are not perpendicular (shear), a left-handed frame
/// (mirroring) or a degenerate axis cannot be stated as a placement and are refused.
pub fn placement(transform: &Transform) -> Result<Placement, String> {
    let axes = [transform[0], transform[1], transform[2]];
    let scale = axes.map(|axis| dot(axis, axis).sqrt());
    if scale
        .iter()
        .any(|length| !length.is_finite() || *length < 1e-9)
    {
        return Err("A foliage transform has a degenerate axis.".into());
    }
    let r: [V3; 3] = std::array::from_fn(|i| axes[i].map(|v| v / scale[i]));
    let handed = dot(
        [
            r[0][1] * r[1][2] - r[0][2] * r[1][1],
            r[0][2] * r[1][0] - r[0][0] * r[1][2],
            r[0][0] * r[1][1] - r[0][1] * r[1][0],
        ],
        r[2],
    );
    if dot(r[0], r[1]).abs() > 1e-6
        || dot(r[0], r[2]).abs() > 1e-6
        || dot(r[1], r[2]).abs() > 1e-6
        || handed < 0.
    {
        return Err(
            "A foliage transform is sheared or mirrored, which a placement cannot state.".into(),
        );
    }
    // The rotation matrix's columns are `r`; element (row, column) is `r[column][row]`.
    let m = |row: usize, column: usize| r[column][row];
    let trace = m(0, 0) + m(1, 1) + m(2, 2);
    let rotation = if trace > 0. {
        let s = (trace + 1.).sqrt() * 2.;
        [
            (m(2, 1) - m(1, 2)) / s,
            (m(0, 2) - m(2, 0)) / s,
            (m(1, 0) - m(0, 1)) / s,
            s / 4.,
        ]
    } else if m(0, 0) > m(1, 1) && m(0, 0) > m(2, 2) {
        let s = (1. + m(0, 0) - m(1, 1) - m(2, 2)).sqrt() * 2.;
        [
            s / 4.,
            (m(0, 1) + m(1, 0)) / s,
            (m(0, 2) + m(2, 0)) / s,
            (m(2, 1) - m(1, 2)) / s,
        ]
    } else if m(1, 1) > m(2, 2) {
        let s = (1. + m(1, 1) - m(0, 0) - m(2, 2)).sqrt() * 2.;
        [
            (m(0, 1) + m(1, 0)) / s,
            s / 4.,
            (m(1, 2) + m(2, 1)) / s,
            (m(0, 2) - m(2, 0)) / s,
        ]
    } else {
        let s = (1. + m(2, 2) - m(0, 0) - m(1, 1)).sqrt() * 2.;
        [
            (m(0, 2) + m(2, 0)) / s,
            (m(1, 2) + m(2, 1)) / s,
            s / 4.,
            (m(1, 0) - m(0, 1)) / s,
        ]
    };
    Ok(Placement {
        translation: transform[3],
        rotation,
        scale,
    })
}

/// A generated mesh as a local mesh with its named vertex attributes.
#[allow(clippy::type_complexity)]
fn local(
    mesh: &FoliageMesh,
    paint: Option<&Paint<'_>>,
) -> (
    LocalMesh,
    BTreeMap<String, Vec<f32>>,
    BTreeMap<String, Vec<[f32; 3]>>,
) {
    let count = mesh.positions.len();
    let mut indices = mesh.indices.clone();
    if mesh.double_sided {
        indices.extend(
            mesh.indices
                .chunks_exact(3)
                .flat_map(|t| [t[0], t[2], t[1]]),
        );
    }
    let normals = mesh
        .vectors
        .get(SHADE_NORMAL)
        .filter(|normals| normals.iter().all(|n| dot(*n, *n) > 1e-12))
        .unwrap_or(&mesh.normals)
        .clone();
    let local = LocalMesh {
        positions: mesh.positions.clone(),
        normals,
        uvs: mesh.uvs.clone(),
        colors: (0..count)
            .map(|vertex| paint.map_or([1.; 3], |paint| (paint.vertex)(mesh, vertex)))
            .collect(),
        indices,
        tangents: vec![],
    };
    let scalars = mesh
        .scalars
        .iter()
        .map(|(name, values)| (name.clone(), values.iter().map(|v| *v as f32).collect()))
        .collect();
    let vectors = mesh
        .vectors
        .iter()
        .map(|(name, values)| {
            (
                name.clone(),
                values.iter().map(|v| v.map(|c| c as f32)).collect(),
            )
        })
        .collect();
    (local, scalars, vectors)
}

/// The per-instance scalar a grass batch carries with the index of its instance's tile in
/// `GrassPatch::tiles`: a tile's instances are consecutive, so a renderer can cull and choose a
/// level per tile.
pub const TILE: &str = "tile";

/// Appends the first `count` instances of a generated batch, moved by `offset`.
fn extend(
    target: &mut InstanceBatch,
    source: &Generated,
    count: usize,
    offset: V3,
    paint: Option<&Paint<'_>>,
) -> Result<(), String> {
    for instance in 0..count {
        let mut placed = placement(&source.transforms[instance])?;
        for axis in 0..3 {
            placed.translation[axis] += offset[axis];
        }
        target.placements.push(placed);
        if let Some(paint) = paint {
            target.tints.push((paint.instance)(source, instance));
        }
    }
    for (name, values) in &source.scalars {
        target
            .scalars
            .entry(name.clone())
            .or_default()
            .extend(values[..count].iter().map(|v| *v as f32));
    }
    for (name, values) in &source.vectors {
        // Points move with the plant; directions do not.
        let moved = if name == ROOT || name == PIVOT {
            offset
        } else {
            [0.; 3]
        };
        target
            .vectors
            .entry(name.clone())
            .or_default()
            .extend(values[..count].iter().map(|v| {
                [
                    (v[0] + moved[0]) as f32,
                    (v[1] + moved[1]) as f32,
                    (v[2] + moved[2]) as f32,
                ]
            }));
    }
    Ok(())
}

fn batch(id: String, object: &str, mesh: &FoliageMesh, paint: Option<&Paint<'_>>) -> FoliageBatch {
    let (local, vertex_scalars, vertex_vectors) = local(mesh, paint);
    FoliageBatch {
        batch: InstanceBatch {
            id,
            object_id: object.into(),
            mesh: mesh.id.clone(),
            vertex_scalars,
            vertex_vectors,
            ..Default::default()
        },
        mesh: local,
    }
}

/// One level of a grass patch: a batch per clump variant that has instances, every tile's
/// instances of the variant together, in tile order, each with its tile's index ([`TILE`]).
/// Identities are `<patch>/clump-<variant>/lod<level>`.
pub fn grass(
    patch: &GrassPatch,
    level: usize,
    offset: V3,
    paint: Option<&Paint<'_>>,
) -> Result<Vec<FoliageBatch>, String> {
    let lod = patch
        .lods
        .get(level)
        .ok_or_else(|| format!("Grass patch {} has no level {level}.", patch.id))?;
    let mut batches = vec![];
    for (variant, mesh) in lod.meshes.iter().enumerate() {
        let mesh = &patch.meshes[*mesh];
        let mut converted = batch(
            format!("{}/clump-{variant}/lod{level}", patch.id),
            &patch.id,
            mesh,
            paint,
        );
        for (tile, source) in patch.tiles.iter().enumerate().flat_map(|(index, tile)| {
            tile.batches
                .iter()
                .filter(|batch| batch.mesh == variant)
                .map(move |batch| (index, batch))
        }) {
            let drawn = GrassTile::drawn(source, lod.keep);
            extend(&mut converted.batch, source, drawn, offset, paint)?;
            converted
                .batch
                .scalars
                .entry(TILE.into())
                .or_default()
                .extend(std::iter::repeat_n(tile as f32, drawn));
        }
        if !converted.batch.placements.is_empty() {
            batches.push(converted);
        }
    }
    Ok(batches)
}

/// Every level of a grass patch ([`grass`]), each batch carrying `look` as its foliage look with
/// the level's own `level`, `distance` and `keep` and the patch's identity: what a renderer needs
/// to draw the patch with its foliage shader families, choosing a level per tile.
pub fn grass_levels(
    patch: &GrassPatch,
    offset: V3,
    look: &superspec_model::batches::Foliage,
    paint: Option<&Paint<'_>>,
) -> Result<Vec<FoliageBatch>, String> {
    let mut all = vec![];
    for (level, lod) in patch.lods.iter().enumerate() {
        let mut found = grass(patch, level, offset, paint)?;
        for converted in &mut found {
            converted.batch.foliage = Some(superspec_model::batches::Foliage {
                patch: patch.id.clone(),
                level: level as u32,
                distance: lod.distance,
                keep: lod.keep,
                ..look.clone()
            });
        }
        all.extend(found);
    }
    Ok(all)
}

/// One level of a plant: its bark meshes as batches of one instance each (`<plant>/trunk`,
/// `<plant>/branches`) and its leaf clusters as one batch (`<plant>/leaves`).
pub fn tree(
    tree: &GeneratedTree,
    level: usize,
    offset: V3,
    paint: Option<&Paint<'_>>,
) -> Result<Vec<FoliageBatch>, String> {
    let lod = tree
        .lods
        .get(level)
        .ok_or_else(|| format!("Plant {} has no level {level}.", tree.id))?;
    let mut batches = vec![];
    for (name, mesh) in [("trunk", Some(lod.trunk)), ("branches", lod.branches)] {
        let Some(mesh) = mesh.map(|index| &tree.meshes[index]) else {
            continue;
        };
        if mesh.indices.is_empty() {
            continue;
        }
        let mut converted = batch(format!("{}/{name}", tree.id), &tree.id, mesh, paint);
        converted.batch.placements.push(Placement {
            translation: offset,
            ..Default::default()
        });
        batches.push(converted);
    }
    if lod.leaf_instances > 0 {
        let mut leaves = batch(
            format!("{}/leaves", tree.id),
            &tree.id,
            &tree.meshes[lod.leaf],
            paint,
        );
        extend(
            &mut leaves.batch,
            &tree.leaves,
            lod.leaf_instances,
            offset,
            paint,
        )?;
        batches.push(leaves);
    }
    Ok(batches)
}

/// Two meshes as one (the trunk and the branches of a level, which draw together), each named
/// attribute end to end; a name only one of them has reads 0 on the other.
fn merged(first: &FoliageMesh, second: &FoliageMesh, id: String) -> FoliageMesh {
    let mut mesh = first.clone();
    mesh.id = id;
    let offset = first.positions.len() as u32;
    let (a, b) = (first.positions.len(), second.positions.len());
    mesh.positions.extend(&second.positions);
    mesh.normals.extend(&second.normals);
    mesh.uvs.extend(&second.uvs);
    mesh.indices
        .extend(second.indices.iter().map(|index| index + offset));
    for name in first.scalars.keys().chain(second.scalars.keys()) {
        let mut values = first.scalars.get(name).cloned().unwrap_or(vec![0.; a]);
        values.extend(second.scalars.get(name).cloned().unwrap_or(vec![0.; b]));
        mesh.scalars.insert(name.clone(), values);
    }
    for name in first.vectors.keys().chain(second.vectors.keys()) {
        let mut values = first.vectors.get(name).cloned().unwrap_or(vec![[0.; 3]; a]);
        values.extend(
            second
                .vectors
                .get(name)
                .cloned()
                .unwrap_or(vec![[0.; 3]; b]),
        );
        mesh.vectors.insert(name.clone(), values);
    }
    mesh
}

/// Every level of a plant as batches with foliage looks (roadmap stage 7), each carrying `look`
/// with the family of its part, the level's `level`, `distance` and `keep`, and the plant's
/// identity as its `patch`:
///
/// - `<plant>/bark/lod<n>`: the trunk and branches of the level as one mesh, one instance
///   (family `bark`);
/// - `<plant>/leaves/lod<n>`: its leaf clusters (`leaves`, `keep` the level's `leafKeep`), unless
///   the level is an impostor;
/// - `<plant>/hull/lod<n>`: its hull shell or impostor (`hull`), when it has one;
/// - `<plant>/shadow`: the shadow proxy (`shadow`, level 0), drawn at every level.
pub fn tree_levels(
    tree: &GeneratedTree,
    offset: V3,
    look: &superspec_model::batches::Foliage,
) -> Result<Vec<FoliageBatch>, String> {
    use superspec_types::batch::FoliageFamily;
    let mut all = vec![];
    let one = |id: String, mesh: &FoliageMesh| {
        let mut converted = batch(id, &tree.id, mesh, None);
        converted.batch.placements.push(Placement {
            translation: offset,
            ..Default::default()
        });
        converted
    };
    let with = |mut converted: FoliageBatch, family: FoliageFamily, level: usize, keep: f64| {
        converted.batch.foliage = Some(superspec_model::batches::Foliage {
            family,
            patch: tree.id.clone(),
            level: level as u32,
            distance: tree.lods[level].distance,
            keep,
            ..look.clone()
        });
        converted
    };
    for (level, lod) in tree.lods.iter().enumerate() {
        let trunk = &tree.meshes[lod.trunk];
        let bark = match lod.branches {
            Some(branches) => merged(trunk, &tree.meshes[branches], format!("bark/lod{level}")),
            None => trunk.clone(),
        };
        all.push(with(
            one(format!("{}/bark/lod{level}", tree.id), &bark),
            FoliageFamily::Bark,
            level,
            1.,
        ));
        if lod.leaf_instances > 0 {
            let mut leaves = batch(
                format!("{}/leaves/lod{level}", tree.id),
                &tree.id,
                &tree.meshes[lod.leaf],
                None,
            );
            extend(
                &mut leaves.batch,
                &tree.leaves,
                lod.leaf_instances,
                offset,
                None,
            )?;
            all.push(with(leaves, FoliageFamily::Leaves, level, lod.leaf_keep));
        }
        if let Some(hull) = lod.hull_mesh {
            all.push(with(
                one(format!("{}/hull/lod{level}", tree.id), &tree.meshes[hull]),
                FoliageFamily::Hull,
                level,
                1.,
            ));
        }
    }
    if let Some(shadow) = tree.shadow {
        all.push(with(
            one(format!("{}/shadow", tree.id), &tree.meshes[shadow]),
            FoliageFamily::Shadow,
            0,
            1.,
        ));
    }
    Ok(all)
}

/// The batches as a project document's `batches` list (`superspec_model::batches::of_project`),
/// their meshes inline. Numbers are written in single precision, as they are delivered.
pub fn project_document(batches: &[FoliageBatch]) -> Value {
    let flat = |values: &[V3]| -> Vec<f32> { values.iter().flatten().map(|v| *v as f32).collect() };
    let named = |values: &BTreeMap<String, Vec<[f32; 3]>>| -> BTreeMap<String, Vec<f32>> {
        values
            .iter()
            .map(|(name, values)| (name.clone(), values.iter().flatten().copied().collect()))
            .collect()
    };
    Value::Array(
        batches
            .iter()
            .map(|FoliageBatch { batch, mesh }| {
                let instances: Vec<Value> = batch
                    .placements
                    .iter()
                    .enumerate()
                    .map(|(index, placement)| {
                        let mut instance = json!({
                            "translation": placement.translation.map(|v| v as f32),
                            "rotation": placement.rotation.map(|v| v as f32),
                            "scale": placement.scale.map(|v| v as f32),
                        });
                        if let Some(tint) = batch.tints.get(index) {
                            instance["tint"] = json!(tint);
                        }
                        if let Some(value) = batch.values.get(index) {
                            instance["value"] = json!(value);
                        }
                        instance
                    })
                    .collect();
                let mut document = json!({
                    "id": batch.id, "objectId": batch.object_id, "collision": batch.collision,
                    "mesh": {
                        "positions": flat(&mesh.positions), "normals": flat(&mesh.normals),
                        "uvs": mesh.uvs.iter().flatten().map(|v| *v as f32).collect::<Vec<_>>(),
                        "colors": flat(&mesh.colors), "indices": mesh.indices,
                        "scalars": batch.vertex_scalars, "vectors": named(&batch.vertex_vectors),
                    },
                    "instances": instances,
                    "scalars": batch.scalars, "vectors": named(&batch.vectors),
                });
                if let Some(foliage) = &batch.foliage {
                    document["foliage"] = json!(foliage);
                }
                document
            })
            .collect(),
    )
}
