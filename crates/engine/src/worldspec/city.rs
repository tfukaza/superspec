//! Materialize local ItemSpec resources, then place their instances in AML source order.
//!
//! Buildings repeat: a playable level places a handful of designs dozens of times. The first copy
//! of each design stands in the scene as usual; later copies add only what cannot be instanced
//! (meshes with tangents, colliders, decals), and `Repeats::deliver` then turns the
//! first copy's other meshes into instanced batches that carry every copy's placement. The batch's
//! mesh is the first copy where it stands (its template), so its material is described and baked
//! once, there, and every copy shows that copy's look.
use crate::{Scene, Session, authoring::Request};
use std::sync::{Arc, Mutex};
use superspec_model::{
    batches::{InstanceBatch, Placement, ProjectBatch, ProjectMesh, add_project_batches},
    composition::WorldModel,
    instances::Model,
};
use superspec_types::{
    architecture::Node,
    diagnostic::{Diagnostic, SourceLocation},
};

/// A building design placed more than once: its first copy and every copy's placement.
struct Group {
    /// The placement's own attributes (`design_key`): a later node with the same key is the same
    /// design and, when the first copy is wholly instanced, is not built again.
    key: String,
    /// The first copy's local model, as placed: compared with later copies to find the design.
    design: Model,
    /// The first copy's owner and source.
    owner: String,
    source: SourceLocation,
    /// The first copy's placement, column-major.
    first: [f64; 16],
    /// Instance identities (`{node}/{mesh}`) of the first copy that are delivered as batches.
    batched: Vec<String>,
    /// Placements relative to the first copy, the first copy's (identity) included.
    placements: Vec<Placement>,
    /// Source placement IDs in the same order as the transforms.
    owners: Vec<String>,
}

/// The repeated designs `place` found; `deliver` makes their batches once the scene is finished.
#[derive(Default)]
pub(crate) struct Repeats {
    groups: Vec<Group>,
}

impl Repeats {
    /// Turns each repeated design's batchable meshes into instanced batches. Designs placed once
    /// stay ordinary meshes.
    pub(crate) fn deliver(self, scene: &mut Scene) -> Result<(), String> {
        let mut batches = Vec::new();
        let mut owners = std::collections::BTreeMap::new();
        for group in self.groups.into_iter().filter(|g| g.placements.len() > 1) {
            if !group.batched.is_empty()
                && !scene
                    .manifest
                    .batches
                    .iter()
                    .any(|b| b.template.is_some() && b.object_id == group.owner)
            {
                owners.insert(group.owner.clone(), group.owners);
            }
            for instance in &group.batched {
                let template = format!("{}/{instance}", group.owner);
                batches.push(ProjectBatch {
                    batch: InstanceBatch {
                        id: template.clone(),
                        object_id: group.owner.clone(),
                        source: group.source.clone(),
                        template: Some(template.clone()),
                        placements: group.placements.clone(),
                        ..Default::default()
                    },
                    mesh: ProjectMesh::Template(template),
                });
            }
        }
        add_project_batches(scene, &batches)
            .map_err(|e| format!("Repeated city buildings could not be instanced: {e}"))?;
        scene.manifest.batch_placement_owners.extend(owners);
        Ok(())
    }
}

pub(crate) fn place<'a>(
    scene: Scene,
    item_session: impl FnOnce() -> Arc<Mutex<Session>>,
    placements: impl Iterator<Item = &'a Node>,
    item: impl Fn(&Node) -> Result<Request, String>,
    assets: impl Fn(&Node, &str) -> Result<Arc<[u8]>, String>,
    cancelled: &dyn Fn() -> bool,
) -> Result<(Scene, Repeats), Diagnostic> {
    let revision = scene.manifest.revision;
    let mut placements = placements.peekable();
    let Some(first) = placements.peek() else {
        return Ok((scene, Repeats::default()));
    };
    let fail = |s: String| first.fail("AML_CITY", s);
    let mut model = WorldModel::from_scene(scene).map_err(fail)?;
    let items = item_session();
    // An asset provider may itself request a world build. Never wait on a session that its
    // caller has borrowed. Such a build gets the same independent compiler as a cold request.
    let mut shared = items.try_lock().ok();
    let mut isolated;
    let session = if let Some(session) = shared.as_deref_mut() {
        session
    } else {
        isolated = Session::default();
        &mut isolated
    };
    // Compilation callers may restart their revision sequence in a persistent world session.
    if session
        .authoring_revision
        .get()
        .is_some_and(|last| u64::from(revision) < last)
    {
        session.clear();
    }
    let mut repeats = Repeats::default();
    for node in placements {
        let fail = |s: String| node.fail("AML_CITY", s);
        let mut request = item(node).map_err(fail)?;
        let origin = node.vector("position", [0.; 3]);
        let step = node.vector("step", [0.; 3]);
        let (sin, cos) = node.number("heading", 0.).to_radians().sin_cos();
        let scale = node.number("scale", 1.);
        request.placement_transform = Some([
            [scale * cos, scale * sin, 0.],
            [-scale * sin, scale * cos, 0.],
            [0., 0., scale],
            origin,
        ]);
        let count = node.number("count", 1.) as usize;
        let copy_of = |i: usize| -> [f64; 16] {
            [
                scale * cos,
                scale * sin,
                0.,
                0.,
                -scale * sin,
                scale * cos,
                0.,
                0.,
                0.,
                0.,
                scale,
                0.,
                origin[0] + i as f64 * step[0],
                origin[1] + i as f64 * step[1],
                origin[2] + i as f64 * step[2],
                1.,
            ]
        };
        let key = design_key(node);
        // A known design whose first copy is wholly instanced: its copies are placements only.
        if node.kind == "building"
            && let Some(group) = repeats.groups.iter_mut().find(|g| {
                g.key == key
                    && g.design.batches.is_empty()
                    && g.batched.len() == g.design.instances.len()
            })
            && let Some(relatives) = (0..count)
                .map(|i| relative(&group.first, &copy_of(i)))
                .collect::<Option<Vec<_>>>()
        {
            if cancelled() {
                return Err(node.fail("CANCELLED", "City placement cancelled."));
            }
            group.placements.extend(relatives);
            group.owners.extend(
                (1..=count).map(|i| superspec_profiles::city::placement_id(&node.id, count, i)),
            );
            continue;
        }
        let local = session
            .build(
                &request,
                revision.into(),
                &|path| assets(node, path),
                cancelled,
            )
            .map_err(|errors| {
                fail(
                    errors
                        .into_iter()
                        .map(|e| format!("{} {}", e.source_name, e.message))
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
            })?
            .model;
        for i in 0..count {
            if cancelled() {
                return Err(node.fail("CANCELLED", "City placement cancelled."));
            }
            let mut placed = local.clone();
            let owner = superspec_profiles::city::placement_id(&node.id, count, i + 1);
            let offset: [f64; 3] = std::array::from_fn(|axis| i as f64 * step[axis]);
            placed.place_under(&owner);
            for instance in &mut placed.instances {
                instance.source = node.source.clone();
                for (axis, delta) in offset.iter().enumerate() {
                    instance.transform[12 + axis] += delta;
                }
            }
            for batch in &mut placed.batches {
                batch.source = node.source.clone();
                for placement in &mut batch.placements {
                    for (coordinate, delta) in placement.translation.iter_mut().zip(offset) {
                        *coordinate += delta;
                    }
                }
            }
            let copy = copy_of(i);
            if node.kind == "building" {
                if let Some(group) = repeats
                    .groups
                    .iter_mut()
                    .find(|g| same_design(&g.design, &local))
                    && let Some(relative) = relative(&group.first, &copy)
                {
                    // A later copy: only what cannot be instanced is placed as meshes.
                    group.placements.push(relative);
                    group.owners.push(owner.clone());
                    let batched: std::collections::BTreeSet<_> = group
                        .batched
                        .iter()
                        .map(|id| format!("{owner}/{id}"))
                        .collect();
                    placed
                        .instances
                        .retain(|instance| !batched.contains(&instance.id));
                    let used: std::collections::BTreeSet<_> =
                        placed.instances.iter().map(|i| i.mesh.clone()).collect();
                    placed.meshes.retain(|id, _| used.contains(id));
                    placed.mesh_artifacts.retain(|id, _| used.contains(id));
                    if !placed.instances.is_empty() || !placed.batches.is_empty() {
                        model.append(placed).map_err(fail)?;
                    }
                    continue;
                }
                repeats.groups.push(Group {
                    key: key.clone(),
                    batched: batchable(&local),
                    design: local.clone(),
                    owner: owner.clone(),
                    owners: vec![owner],
                    source: node.source.clone(),
                    first: copy,
                    placements: vec![Placement {
                        translation: [0.; 3],
                        rotation: [0., 0., 0., 1.],
                        scale: [1.; 3],
                    }],
                });
            }
            model.append(placed).map_err(fail)?;
        }
    }
    let scene = model.scene_cancellable(revision, cancelled).map_err(fail)?;
    Ok((scene, repeats))
}

/// A placement's design: its kind, attributes and children, less where it stands (`id`,
/// `position`, `heading`, `count`, `step`, `scale`). The item request is made of nothing else, so
/// two nodes with one key build the same item.
fn design_key(node: &Node) -> String {
    let attributes: Vec<_> = node
        .attributes
        .iter()
        .filter(|(name, _)| {
            !matches!(
                name.as_str(),
                "id" | "position" | "heading" | "count" | "step" | "scale"
            )
        })
        .collect();
    let children: Vec<_> = node
        .children
        .iter()
        .map(|child| (&child.kind, &child.attributes))
        .collect();
    format!("{}|{attributes:?}|{children:?}", node.kind)
}

/// Whether two placements of an item are one design: the same geometry artifacts, placements of
/// the same meshes and the same materials.
fn same_design(a: &Model, b: &Model) -> bool {
    a.mesh_artifacts == b.mesh_artifacts
        && a.mesh_artifacts.len() == a.meshes.len()
        && a.materials == b.materials
        && a.geometry_lods == b.geometry_lods
        && a.images.keys().eq(b.images.keys())
        && a.decals == b.decals
        && a.batches.is_empty()
        && b.batches.is_empty()
        && a.instances.len() == b.instances.len()
        && a.instances.iter().zip(&b.instances).all(|(a, b)| {
            a.id == b.id
                && a.mesh == b.mesh
                && a.material == b.material
                && a.visible == b.visible
                && a.collidable == b.collidable
        })
}

/// The first copy's instances an instanced batch can carry: visible, not colliding, not a decal
/// and without tangents (batches deliver none). Vertex tints are kept: a batch's mesh carries them.
fn batchable(model: &Model) -> Vec<String> {
    let authored: std::collections::BTreeSet<_> = model
        .geometry_lods
        .iter()
        .flat_map(|g| &g.levels)
        .flat_map(|level| &level.meshes)
        .collect();
    model
        .instances
        .iter()
        .filter(|instance| {
            instance.visible
                && !instance.collidable
                && !model.decals.contains(&instance.id)
                && !authored.contains(&instance.id)
                && model
                    .meshes
                    .get(&instance.mesh)
                    .is_some_and(|mesh| mesh.tangents.is_empty())
        })
        .map(|instance| instance.id.clone())
        .collect()
}

/// `copy · first⁻¹` as a placement, when it is one: a turn about Z, a uniform scale and a
/// translation (city placements are no more). None for anything else, which is then placed as
/// ordinary meshes.
fn relative(first: &[f64; 16], copy: &[f64; 16]) -> Option<Placement> {
    let turn = |m: &[f64; 16]| m[1].atan2(m[0]);
    let size = |m: &[f64; 16]| m[0].hypot(m[1]);
    for m in [first, copy] {
        let s = size(m);
        if !(s > 0.)
            || m[2].abs() > 1e-12
            || m[6].abs() > 1e-12
            || m[8].abs() > 1e-12
            || m[9].abs() > 1e-12
            || (m[10] - s).abs() > 1e-9 * s
            || (m[5] - m[0]).abs() > 1e-9 * s
            || (m[4] + m[1]).abs() > 1e-9 * s
        {
            return None;
        }
    }
    let angle = turn(copy) - turn(first);
    let scale = size(copy) / size(first);
    let (sin, cos) = angle.sin_cos();
    let t = [first[12], first[13], first[14]];
    // The copy's origin minus the first copy's origin carried by the relative turn and scale.
    let translation = [
        copy[12] - scale * (cos * t[0] - sin * t[1]),
        copy[13] - scale * (sin * t[0] + cos * t[1]),
        copy[14] - scale * t[2],
    ];
    let (half_sin, half_cos) = (angle / 2.).sin_cos();
    Some(Placement {
        translation,
        rotation: [0., 0., half_sin, half_cos],
        scale: [scale; 3],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placement(heading: f64, scale: f64, origin: [f64; 3]) -> [f64; 16] {
        let (sin, cos) = heading.to_radians().sin_cos();
        [
            scale * cos,
            scale * sin,
            0.,
            0.,
            -scale * sin,
            scale * cos,
            0.,
            0.,
            0.,
            0.,
            scale,
            0.,
            origin[0],
            origin[1],
            origin[2],
            1.,
        ]
    }

    /// A point of the first copy, carried by the relative placement, lands where the copy puts the
    /// same local point.
    #[test]
    fn relative_placement_maps_the_first_copy_onto_the_copy() {
        let first = placement(90., 1., [21.15, 31.45, 0.]);
        let copy = placement(180., 1.5, [-1.35, -31.35, 0.2]);
        let relative = relative(&first, &copy).unwrap();
        let mut flat = [0f32; 10];
        flat[..3].copy_from_slice(&relative.translation.map(|v| v as f32));
        flat[3..7].copy_from_slice(&relative.rotation.map(|v| v as f32));
        flat[7..].copy_from_slice(&relative.scale.map(|v| v as f32));
        for local in [[0., 0., 0.], [9.8, 0., 0.], [3., 7., 12.]] {
            let at = |m: &[f64; 16]| superspec_model::instances::transform_point(m, local);
            let placed = superspec_model::batches::place(&flat, at(&first));
            let expected = at(&copy);
            for axis in 0..3 {
                assert!(
                    (placed[axis] - expected[axis]).abs() < 1e-3,
                    "{placed:?} {expected:?}"
                );
            }
        }
    }

    #[test]
    fn a_tilted_placement_is_not_a_batch_placement() {
        let first = placement(0., 1., [0.; 3]);
        let mut tilted = placement(0., 1., [1., 2., 3.]);
        tilted[2] = 0.3;
        assert!(relative(&first, &tilted).is_none());
    }
}
