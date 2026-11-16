//! Instanced batches: one local mesh, one material binding and N placements, delivered unexpanded.
//!
//! A producer describes a batch with [`InstanceBatch`] and hands it, with its mesh, to
//! [`Scene::add_batch`] (or lists it in `instances::Model::batches`). The scene then holds the
//! mesh once in [`Buffers`], the placements and per-instance attributes beside it, and one
//! [`Batch`] record in `Manifest::batches` that names their ranges.
//!
//! What a batch contributes, each independently of the others:
//! - **Render**: `visible` batches are drawn, one instance per placement.
//! - **Collision**: only with [`Collision::Instances`], and then every instance collides as the
//!   mesh's triangles. The default is [`Collision::None`]. A batch is never listed in
//!   `Manifest::colliders`, which names meshes; consumers read the batch's own `collision`.
//! - **Navigation and validation**: none. A render-only batch leaves the scene's validation
//!   statuses as they are; adding a colliding one resets them to `not-run`, as any physical change
//!   does, because no validation has seen its instances.
//!
//! Ordering is deterministic: `Manifest::batches` is ascending by `id` and the buffers follow that
//! order, whatever order batches were added in. Instances keep the producer's order; an instance's
//! index in its batch is its identity (picking reports it).
//!
//! Per-instance attributes are a small fixed set, each given for every instance or for none:
//! `values` (four numbers, the material `instance` input: for example a random number, a phase, a
//! growth factor) and `tints` (linear RGB multipliers of the base color). Both are render-time
//! inputs: they are not part of the mesh, of the material or of any bake key.
//!
//! Beyond that fixed set a batch may carry **named attributes**, for producers whose materials
//! read more (foliage: a shade normal, a pivot, a height ratio): scalars and three-component
//! vectors, per instance or per vertex of the batch's mesh, at most [`MAX_ATTRIBUTES`] of each
//! domain. A name is 1 to 32 ASCII letters and digits starting with a letter, unique within its
//! domain. They are delivered in a fixed order (instance scalars, instance vectors, vertex
//! scalars, vertex vectors, each ascending by name) and, like values and tints, are render-time
//! inputs. A GLB writes them as underscore-prefixed custom attributes (`shadeNormal` becomes
//! `_SHADE_NORMAL`), per-instance ones in `EXT_mesh_gpu_instancing`.
use crate::{Scene, instances::LocalMesh};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
pub use superspec_types::batch::{
    Collision, Foliage, MAX_BATCH_INDICES, MAX_BATCH_VERTICES, MAX_BATCHES, MAX_INSTANCES,
    PLACEMENT_FLOATS, Placement, basis, place,
};
use superspec_types::{architecture::V3, diagnostic::SourceLocation};

/// A batch as a producer states it. `mesh` names the local mesh where the batch is part of an
/// `instances::Model` (a key of `Model::meshes`); [`Scene::add_batch`] takes the mesh itself.
#[derive(Clone, Debug)]
pub struct InstanceBatch {
    /// Unique among the scene's batches and meshes.
    pub id: String,
    /// Stable owner, as for meshes.
    pub object_id: String,
    pub source: SourceLocation,
    pub mesh: String,
    /// A material of the scene, by identity.
    pub material: Option<String>,
    /// The scene mesh the batch's mesh was taken from, when it was one (see [`add_project`]).
    pub template: Option<String>,
    pub visible: bool,
    pub collision: Collision,
    pub placements: Vec<Placement>,
    /// Empty, or one `instance` value per placement.
    pub values: Vec<[f32; 4]>,
    /// Empty, or one linear RGB multiplier per placement.
    pub tints: Vec<[f32; 3]>,
    /// Named per-instance scalars and vectors: one entry per placement under each name.
    pub scalars: BTreeMap<String, Vec<f32>>,
    pub vectors: BTreeMap<String, Vec<[f32; 3]>>,
    /// Named per-vertex scalars and vectors of the batch's mesh: one entry per vertex.
    pub vertex_scalars: BTreeMap<String, Vec<f32>>,
    pub vertex_vectors: BTreeMap<String, Vec<[f32; 3]>>,
    /// How a renderer draws the batch as foliage (shader family and parameters), when it is some.
    pub foliage: Option<Foliage>,
}

impl Default for InstanceBatch {
    /// A visible, render-only batch with nothing in it; a producer fills in the rest.
    fn default() -> Self {
        Self {
            id: String::new(),
            object_id: String::new(),
            source: SourceLocation {
                line: 1,
                column: 1,
                offset: 0,
                end: None,
            },
            mesh: String::new(),
            material: None,
            template: None,
            visible: true,
            collision: Collision::None,
            placements: vec![],
            values: vec![],
            tints: vec![],
            scalars: BTreeMap::new(),
            vectors: BTreeMap::new(),
            vertex_scalars: BTreeMap::new(),
            vertex_vectors: BTreeMap::new(),
            foliage: None,
        }
    }
}

/// Named attributes of one domain a batch may carry.
pub const MAX_ATTRIBUTES: usize = 12;

/// What a named attribute has one value for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Domain {
    Instance,
    Vertex,
}

/// One named attribute of a delivered batch: `components` floats (1 or 3) per instance or per
/// vertex, starting at float `start` of `Buffers::attributes`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attribute {
    pub name: String,
    pub domain: Domain,
    pub components: usize,
    pub start: usize,
}

/// One delivered batch: its identity and bindings, and where its data is in [`Buffers`].
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Batch {
    pub id: String,
    pub object_id: String,
    pub source: SourceLocation,
    pub material: Option<String>,
    /// The identity of the scene mesh this batch's mesh was taken from, when it was one: per-mesh
    /// metadata a producer keeps beside the scene (a receiver's atlas layout, its shading inputs)
    /// is found under it. The mesh itself is no longer in the scene.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    pub visible: bool,
    pub collision: Collision,
    /// The mesh, in vertices of the batch buffers; its indices are local to this range.
    pub vertex_start: usize,
    pub vertex_count: usize,
    pub index_start: usize,
    pub index_count: usize,
    /// The placements, in instances.
    pub instance_start: usize,
    pub instance_count: usize,
    /// Where the batch's `instance` values start in `Buffers::values`, in instances; absent
    /// without them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_start: Option<usize>,
    /// The same for `Buffers::tints`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tint_start: Option<usize>,
    /// Named attributes, in delivery order (see the module's description).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attributes: Vec<Attribute>,
    /// Bounds of the mesh in its own space.
    pub local_bounds: [V3; 2],
    /// Bounds of every instance in the scene: the union of the placed local bounds (their eight
    /// corners each), so conservative for rotated instances and never too small.
    pub bounds: [V3; 2],
    /// How a renderer draws the batch as foliage; absent for every other batch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub foliage: Option<Foliage>,
}

/// Lengths of [`Buffers`], stated in the manifest so a packet's payload can be measured.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub vertices: usize,
    pub indices: usize,
    pub instances: usize,
    /// Instances that carry an `instance` value.
    pub values: usize,
    /// Instances that carry a tint.
    pub tints: usize,
    /// Floats of named attributes.
    pub attributes: usize,
}

/// The data of every batch of a scene, in `Manifest::batches` order. Single precision, as the
/// scene's mesh buffers are.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Buffers {
    /// XYZ per vertex, in the mesh's own space.
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub uvs: Vec<f32>,
    /// Linear RGB multipliers per vertex.
    pub colors: Vec<f32>,
    pub indices: Vec<u32>,
    /// `PLACEMENT_FLOATS` per instance: translation, rotation `x y z w`, scale.
    pub placements: Vec<f32>,
    /// Four per instance that has one.
    pub values: Vec<f32>,
    /// Three per instance that has one.
    pub tints: Vec<f32>,
    /// Named attributes of every batch, end to end in record and attribute order.
    pub attributes: Vec<f32>,
}

/// One batch's slices of [`Buffers`].
#[derive(Clone, Copy, Debug)]
pub struct View<'a> {
    pub positions: &'a [f32],
    pub normals: &'a [f32],
    pub uvs: &'a [f32],
    pub colors: &'a [f32],
    pub indices: &'a [u32],
    pub placements: &'a [f32],
    pub values: Option<&'a [f32]>,
    pub tints: Option<&'a [f32]>,
    /// The pool the record's named attributes index (their `start`), whole.
    pub attributes: &'a [f32],
}

impl Buffers {
    pub fn is_empty(&self) -> bool {
        self.placements.is_empty()
    }

    pub fn counts(&self) -> Counts {
        Counts {
            vertices: self.positions.len() / 3,
            indices: self.indices.len(),
            instances: self.placements.len() / PLACEMENT_FLOATS,
            values: self.values.len() / 4,
            tints: self.tints.len() / 3,
            attributes: self.attributes.len(),
        }
    }

    /// The slices a record names. The record must be one of this scene's (see [`check`]).
    pub fn view(&self, batch: &Batch) -> View<'_> {
        let vertices = batch.vertex_start..batch.vertex_start + batch.vertex_count;
        let range = |components: usize| vertices.start * components..vertices.end * components;
        let instances = |start: usize, components: usize| {
            start * components..(start + batch.instance_count) * components
        };
        View {
            positions: &self.positions[range(3)],
            normals: &self.normals[range(3)],
            uvs: &self.uvs[range(2)],
            colors: &self.colors[range(3)],
            indices: &self.indices[batch.index_start..batch.index_start + batch.index_count],
            placements: &self.placements[instances(batch.instance_start, PLACEMENT_FLOATS)],
            values: batch
                .value_start
                .map(|start| &self.values[instances(start, 4)]),
            tints: batch
                .tint_start
                .map(|start| &self.tints[instances(start, 3)]),
            attributes: &self.attributes,
        }
    }

    /// Appends a batch's data and returns its record with the ranges it has here.
    fn push(&mut self, mut batch: Batch, view: View<'_>) -> Batch {
        let counts = self.counts();
        batch.vertex_start = counts.vertices;
        batch.index_start = counts.indices;
        batch.instance_start = counts.instances;
        batch.value_start = view.values.map(|_| counts.values);
        batch.tint_start = view.tints.map(|_| counts.tints);
        self.positions.extend_from_slice(view.positions);
        self.normals.extend_from_slice(view.normals);
        self.uvs.extend_from_slice(view.uvs);
        self.colors.extend_from_slice(view.colors);
        self.indices.extend_from_slice(view.indices);
        self.placements.extend_from_slice(view.placements);
        self.values
            .extend_from_slice(view.values.unwrap_or_default());
        self.tints.extend_from_slice(view.tints.unwrap_or_default());
        let (instances, vertices) = (batch.instance_count, batch.vertex_count);
        for attribute in &mut batch.attributes {
            let length = attribute.components
                * match attribute.domain {
                    Domain::Instance => instances,
                    Domain::Vertex => vertices,
                };
            let start = self.attributes.len();
            self.attributes
                .extend_from_slice(&view.attributes[attribute.start..attribute.start + length]);
            attribute.start = start;
        }
        batch
    }

    /// The values of one named attribute of a batch: `components` floats per instance or vertex.
    pub fn attribute(&self, batch: &Batch, attribute: &Attribute) -> &[f32] {
        let count = match attribute.domain {
            Domain::Instance => batch.instance_count,
            Domain::Vertex => batch.vertex_count,
        };
        &self.attributes[attribute.start..attribute.start + attribute.components * count]
    }

    /// The payload a packet carries after its mesh indices: every array end to end, little-endian.
    pub(crate) fn write(&self, bytes: &mut Vec<u8>) {
        for value in self
            .positions
            .iter()
            .chain(&self.normals)
            .chain(&self.uvs)
            .chain(&self.colors)
        {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in &self.indices {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in self
            .placements
            .iter()
            .chain(&self.values)
            .chain(&self.tints)
            .chain(&self.attributes)
        {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }

    pub(crate) fn byte_length(&self) -> usize {
        4 * (self.positions.len()
            + self.normals.len()
            + self.uvs.len()
            + self.colors.len()
            + self.indices.len()
            + self.placements.len()
            + self.values.len()
            + self.tints.len()
            + self.attributes.len())
    }
}

/// Whether a scene's batch records and buffers agree: contiguous ranges in record order, records
/// ascending by identity, counts as stated. Composition and export check this before indexing.
pub fn check(scene: &Scene) -> Result<(), String> {
    let buffers = &scene.batches;
    let counts = buffers.counts();
    let mut expected = Counts::default();
    let mut previous: Option<&str> = None;
    if buffers.positions.len() % 3 != 0
        || buffers.normals.len() != buffers.positions.len()
        || buffers.colors.len() != buffers.positions.len()
        || buffers.uvs.len() != counts.vertices * 2
        || buffers.placements.len() % PLACEMENT_FLOATS != 0
        || buffers.values.len() % 4 != 0
        || buffers.tints.len() % 3 != 0
        || scene.manifest.batch_counts != (!buffers.is_empty()).then_some(counts)
    {
        return Err("Invalid scene batch buffers.".into());
    }
    for batch in &scene.manifest.batches {
        if previous.is_some_and(|id| id >= batch.id.as_str())
            || batch.vertex_start != expected.vertices
            || batch.index_start != expected.indices
            || batch.instance_start != expected.instances
            || batch
                .value_start
                .is_some_and(|start| start != expected.values)
            || batch
                .tint_start
                .is_some_and(|start| start != expected.tints)
            || batch.vertex_count == 0
            || batch.index_count == 0
            || !batch.index_count.is_multiple_of(3)
            || batch.instance_count == 0
        {
            return Err(format!("Invalid scene batch range: {}", batch.id));
        }
        previous = Some(&batch.id);
        expected.vertices += batch.vertex_count;
        expected.indices += batch.index_count;
        expected.instances += batch.instance_count;
        if batch.value_start.is_some() {
            expected.values += batch.instance_count;
        }
        if batch.tint_start.is_some() {
            expected.tints += batch.instance_count;
        }
        for attribute in &batch.attributes {
            if attribute.start != expected.attributes || !matches!(attribute.components, 1 | 3) {
                return Err(format!("Invalid scene batch attribute: {}", batch.id));
            }
            expected.attributes += attribute.components
                * match attribute.domain {
                    Domain::Instance => batch.instance_count,
                    Domain::Vertex => batch.vertex_count,
                };
        }
    }
    if expected != counts {
        return Err("Scene batch records do not cover their buffers.".into());
    }
    for batch in &scene.manifest.batches {
        if buffers
            .view(batch)
            .indices
            .iter()
            .any(|index| *index as usize >= batch.vertex_count)
        {
            return Err(format!("Batch index exceeds its mesh: {}", batch.id));
        }
    }
    Ok(())
}

/// Bounds of points, as `[minimum, maximum]`.
fn bounds(points: impl Iterator<Item = [f64; 3]>) -> [V3; 2] {
    let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
    for point in points {
        for axis in 0..3 {
            bounds[0][axis] = bounds[0][axis].min(point[axis]);
            bounds[1][axis] = bounds[1][axis].max(point[axis]);
        }
    }
    bounds
}

/// Bounds of every placed copy of `local`: the union of its eight corners under each placement.
pub fn placed_bounds(local: &[V3; 2], placements: &[f32]) -> [V3; 2] {
    bounds(
        placements
            .chunks_exact(PLACEMENT_FLOATS)
            .flat_map(|placement| {
                (0..8).map(move |corner| {
                    place(
                        placement,
                        std::array::from_fn(|axis| local[(corner >> axis) & 1][axis]),
                    )
                })
            }),
    )
}

/// Puts batch records in ascending identity order, with the buffers in the same order.
fn sort(scene: &mut Scene) {
    let records = &scene.manifest.batches;
    if records.windows(2).all(|pair| pair[0].id < pair[1].id) {
        return;
    }
    let mut order: Vec<usize> = (0..records.len()).collect();
    order.sort_by(|a, b| records[*a].id.cmp(&records[*b].id));
    let mut buffers = Buffers::default();
    let sorted = order
        .into_iter()
        .map(|index| buffers.push(records[index].clone(), scene.batches.view(&records[index])))
        .collect();
    scene.batches = buffers;
    scene.manifest.batches = sorted;
}

impl Scene {
    /// Adds an instanced batch to a delivered scene. The mesh is stored once; nothing is expanded.
    /// A failure leaves the scene as it was.
    pub fn add_batch(&mut self, batch: &InstanceBatch, mesh: &LocalMesh) -> Result<(), String> {
        let count = batch.placements.len();
        if batch.id.is_empty()
            || batch.id.len() > 4096
            || batch.object_id.is_empty()
            || batch.object_id.len() > 4096
            || self.manifest.batches.iter().any(|old| old.id == batch.id)
            || self.manifest.meshes.iter().any(|old| old.id == batch.id)
        {
            return Err(format!(
                "Batch identity {:?} is invalid or already used by a batch or mesh.",
                batch.id
            ));
        }
        let fail = |message: &str| format!("Batch {}: {message}", batch.id);
        if batch
            .material
            .as_ref()
            .is_some_and(|id| !self.manifest.materials.iter().any(|m| &m.id == id))
        {
            return Err(fail("it references a missing material."));
        }
        if batch
            .template
            .as_ref()
            .is_some_and(|id| id.is_empty() || id.len() > 4096)
        {
            return Err(fail("its template mesh identity is invalid."));
        }
        mesh.validate().map_err(|message| fail(&message))?;
        if let Some(foliage) = &batch.foliage {
            foliage.validate().map_err(|message| fail(&message))?;
        }
        // Named attributes, in delivery order, into a pool of their own.
        let mut attributes = vec![];
        let mut pool: Vec<f32> = vec![];
        for (domain, expected, scalars, vectors) in [
            (Domain::Instance, count, &batch.scalars, &batch.vectors),
            (
                Domain::Vertex,
                mesh.positions.len(),
                &batch.vertex_scalars,
                &batch.vertex_vectors,
            ),
        ] {
            if scalars.len() + vectors.len() > MAX_ATTRIBUTES
                || scalars.keys().any(|name| vectors.contains_key(name))
            {
                return Err(fail(
                    "it has too many named attributes, or one name for a scalar and a vector.",
                ));
            }
            let named =
                scalars
                    .iter()
                    .map(|(name, values)| (name, 1, values.clone()))
                    .chain(vectors.iter().map(|(name, values)| {
                        (name, 3, values.iter().flatten().copied().collect())
                    }));
            for (name, components, values) in named {
                if name.is_empty()
                    || name.len() > 32
                    || !name.as_bytes()[0].is_ascii_alphabetic()
                    || !name.bytes().all(|b| b.is_ascii_alphanumeric())
                    || ["instance", "tint"].contains(&name.as_str())
                {
                    return Err(fail(&format!(
                        "attribute name {name:?} must be 1 to 32 letters and digits, starting with a letter, and not `instance` or `tint`."
                    )));
                }
                if values.len() != expected * components || values.iter().any(|v| !v.is_finite()) {
                    return Err(fail(&format!(
                        "attribute {name} needs one finite value per {}.",
                        if domain == Domain::Instance {
                            "instance"
                        } else {
                            "vertex"
                        }
                    )));
                }
                attributes.push(Attribute {
                    name: name.clone(),
                    domain,
                    components,
                    start: pool.len(),
                });
                pool.extend(values);
            }
        }
        if count == 0 {
            return Err(fail("it has no instance."));
        }
        if (!batch.values.is_empty() && batch.values.len() != count)
            || (!batch.tints.is_empty() && batch.tints.len() != count)
        {
            return Err(fail(
                "per-instance attributes must be given for every instance or for none.",
            ));
        }
        if batch.values.iter().flatten().any(|v| !v.is_finite())
            || batch
                .tints
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || *v < 0.)
        {
            return Err(fail(
                "instance values must be finite and tints finite and nonnegative.",
            ));
        }
        let counts = self.batches.counts();
        if self.manifest.batches.len() >= MAX_BATCHES
            || counts.instances + count > MAX_INSTANCES
            || counts.vertices + mesh.positions.len() > MAX_BATCH_VERTICES
            || counts.indices + mesh.indices.len() > MAX_BATCH_INDICES
        {
            return Err(fail("the scene's batch allowance is exceeded."));
        }
        let mut placements = Vec::with_capacity(count * PLACEMENT_FLOATS);
        for (index, placement) in batch.placements.iter().enumerate() {
            placement
                .validate()
                .map_err(|message| fail(&format!("instance {index}: {message}")))?;
            placements.extend(placement.delivered());
        }
        let flat = |values: &[[f64; 3]]| -> Vec<f32> {
            values.iter().flatten().map(|v| *v as f32).collect()
        };
        let positions = flat(&mesh.positions);
        let normals = flat(&mesh.normals);
        let colors = flat(&mesh.colors);
        let uvs: Vec<f32> = mesh.uvs.iter().flatten().map(|v| *v as f32).collect();
        let values: Vec<f32> = batch.values.iter().flatten().copied().collect();
        let tints: Vec<f32> = batch.tints.iter().flatten().copied().collect();
        // Bounds describe the delivered single-precision data, so a consumer's own agree exactly.
        let local_bounds = bounds(
            positions
                .chunks_exact(3)
                .map(|point| [point[0], point[1], point[2]].map(f64::from)),
        );
        let world = placed_bounds(&local_bounds, &placements);
        if world
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 1e9)
        {
            return Err(fail("an instance leaves the supported coordinate range."));
        }
        let record = Batch {
            id: batch.id.clone(),
            object_id: batch.object_id.clone(),
            source: batch.source.clone(),
            material: batch.material.clone(),
            template: batch.template.clone(),
            visible: batch.visible,
            collision: batch.collision,
            vertex_start: 0,
            vertex_count: mesh.positions.len(),
            index_start: 0,
            index_count: mesh.indices.len(),
            instance_start: 0,
            instance_count: count,
            value_start: None,
            tint_start: None,
            attributes,
            local_bounds,
            bounds: world,
            foliage: batch.foliage.clone(),
        };
        let record = self.batches.push(
            record,
            View {
                positions: &positions,
                normals: &normals,
                uvs: &uvs,
                colors: &colors,
                indices: &mesh.indices,
                placements: &placements,
                values: (!values.is_empty()).then_some(&values),
                tints: (!tints.is_empty()).then_some(&tints),
                attributes: &pool,
            },
        );
        self.manifest.batches.push(record);
        sort(self);
        self.manifest.batch_counts = Some(self.batches.counts());
        if batch.collision == Collision::Instances {
            // Physical content no validation has seen.
            self.manifest.validation = None;
            self.manifest.geometry_validation = "not-run";
            self.manifest.navigation_validation = "not-run";
        }
        Ok(())
    }

    /// Appends every batch of `addition` (composition). Identities must not repeat.
    pub(crate) fn append_batches(&mut self, addition: &Scene) -> Result<(), String> {
        if addition.manifest.batches.is_empty() {
            return Ok(());
        }
        let ids: BTreeSet<&str> = self
            .manifest
            .batches
            .iter()
            .map(|batch| batch.id.as_str())
            .chain(self.manifest.meshes.iter().map(|mesh| mesh.id.as_str()))
            .collect();
        if addition
            .manifest
            .batches
            .iter()
            .any(|batch| ids.contains(batch.id.as_str()))
        {
            return Err("Composed scenes contain duplicate batch identities.".into());
        }
        let counts = self.batches.counts();
        let added = addition.batches.counts();
        if self.manifest.batches.len() + addition.manifest.batches.len() > MAX_BATCHES
            || counts.instances + added.instances > MAX_INSTANCES
            || counts.vertices + added.vertices > MAX_BATCH_VERTICES
            || counts.indices + added.indices > MAX_BATCH_INDICES
        {
            return Err("Composed scene exceeds its batch allowance.".into());
        }
        for batch in &addition.manifest.batches {
            let record = self
                .batches
                .push(batch.clone(), addition.batches.view(batch));
            self.manifest.batches.push(record);
        }
        sort(self);
        self.manifest.batch_counts = Some(self.batches.counts());
        Ok(())
    }
}

/// The mesh of a batch a project document carries.
#[derive(Clone, Debug)]
pub enum ProjectMesh {
    /// A mesh of the project's delivered scene, by identity: the batch's template. It leaves the
    /// scene and becomes the batch's mesh, with the material it has there.
    Template(String),
    /// Geometry stated in the document.
    Inline(LocalMesh),
}

/// A batch a project document carries.
#[derive(Clone, Debug)]
pub struct ProjectBatch {
    pub batch: InstanceBatch,
    pub mesh: ProjectMesh,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BatchDocument {
    id: String,
    object_id: Option<String>,
    #[serde(default)]
    collision: Collision,
    visible: Option<bool>,
    /// Named per-instance scalars, and vectors as three numbers per instance.
    #[serde(default)]
    scalars: BTreeMap<String, Vec<f32>>,
    #[serde(default)]
    vectors: BTreeMap<String, Vec<f32>>,
    mesh: MeshReference,
    instances: Vec<InstanceDocument>,
    /// How a renderer draws the batch as foliage ([`Foliage`]).
    foliage: Option<Foliage>,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum MeshReference {
    Template(String),
    Inline(MeshDocument),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MeshDocument {
    positions: Vec<f64>,
    normals: Vec<f64>,
    uvs: Option<Vec<f64>>,
    colors: Option<Vec<f64>>,
    /// Named per-vertex scalars, and vectors as three numbers per vertex.
    #[serde(default)]
    scalars: BTreeMap<String, Vec<f32>>,
    #[serde(default)]
    vectors: BTreeMap<String, Vec<f32>>,
    indices: Vec<u32>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InstanceDocument {
    translation: [f64; 3],
    rotation: Option<[f64; 4]>,
    scale: Option<[f64; 3]>,
    value: Option<[f32; 4]>,
    tint: Option<[f32; 3]>,
}

/// The instanced batches a project document carries in its optional `batches` field:
///
/// ```json
/// {"id": "bolts", "mesh": "bolt/node/0/mesh/0/primitive/0", "collision": "none",
///  "instances": [{"translation": [x, y, z], "rotation": [x, y, z, w], "scale": [x, y, z],
///                 "value": [a, b, c, d], "tint": [r, g, b]}]}
/// ```
///
/// `mesh` names a mesh of the project's composed scene, the batch's **template**: an ordinary
/// mesh of one of the project's models, with a material assignment like any other, so the project
/// bakes its material once, for the template where it stands, as it does for every receiver. The
/// delivered scene then holds the batch in its place ([`add_project_batches`]): the template's
/// geometry (with its baked UVs) and material become the batch's, and placements apply to the
/// template's positions as the scene has them. The template should stand clear of the scene, so
/// that it touches nothing and shades nothing. `mesh` may instead state geometry inline
/// (`{"positions": [...], "normals": [...], "uvs": [...], "colors": [...], "indices": [...]}`, UVs
/// defaulting to zero and colors to white), which has no material.
///
/// `objectId` defaults to the template's owner (or `id`); `rotation`, `scale`, `value` and `tint`
/// are optional, the last two for every instance or none. An optional `foliage` states how a
/// renderer draws the batch as foliage ([`Foliage`]). Unknown fields are refused. The
/// authoring engine's project ignores the field: a host that delivers the project reads it here,
/// as it does the lighting record.
pub fn of_project(document: &serde_json::Value) -> Result<Vec<ProjectBatch>, String> {
    let Some(list) = document.get("batches") else {
        return Ok(vec![]);
    };
    let documents: Vec<BatchDocument> = serde_json::from_value(list.clone())
        .map_err(|error| format!("Invalid project batches: {error}"))?;
    if documents.len() > MAX_BATCHES {
        return Err("Project batches exceed their allowance.".into());
    }
    documents
        .into_iter()
        .map(|document| {
            let fail = |message: &str| format!("Batch {}: {message}", document.id);
            let instances = &document.instances;
            let all_or_none = |given: usize| given == 0 || given == instances.len();
            if !all_or_none(instances.iter().filter(|i| i.value.is_some()).count())
                || !all_or_none(instances.iter().filter(|i| i.tint.is_some()).count())
            {
                return Err(fail(
                    "`value` and `tint` must be given for every instance or for none.",
                ));
            }
            let mut template = None;
            let triples = |named: BTreeMap<String, Vec<f32>>| -> Result<BTreeMap<String, Vec<[f32; 3]>>, String> {
                named
                    .into_iter()
                    .map(|(name, values)| {
                        if values.len() % 3 != 0 {
                            return Err(format!("Batch vector attribute {name} needs three numbers per entry."));
                        }
                        Ok((name, values.chunks_exact(3).map(|v| [v[0], v[1], v[2]]).collect()))
                    })
                    .collect()
            };
            let (mut vertex_scalars, mut vertex_vectors) = (BTreeMap::new(), BTreeMap::new());
            let mesh = match document.mesh {
                MeshReference::Template(id) => {
                    template = Some(id.clone());
                    ProjectMesh::Template(id)
                }
                MeshReference::Inline(mut mesh) => {
                    vertex_scalars = std::mem::take(&mut mesh.scalars);
                    vertex_vectors = triples(std::mem::take(&mut mesh.vectors))?;
                    let count = mesh.positions.len() / 3;
                    let triples = |values: &[f64]| -> Vec<[f64; 3]> {
                        values.chunks_exact(3).map(|v| [v[0], v[1], v[2]]).collect()
                    };
                    if !mesh.positions.len().is_multiple_of(3)
                        || mesh.normals.len() != count * 3
                        || mesh.uvs.as_ref().is_some_and(|uvs| uvs.len() != count * 2)
                        || mesh
                            .colors
                            .as_ref()
                            .is_some_and(|colors| colors.len() != count * 3)
                    {
                        return Err(fail(
                            "mesh attributes must have one entry per vertex (positions and normals three numbers, uvs two, colors three).",
                        ));
                    }
                    ProjectMesh::Inline(LocalMesh {
                        positions: triples(&mesh.positions),
                        normals: triples(&mesh.normals),
                        uvs: mesh.uvs.as_ref().map_or_else(
                            || vec![[0.; 2]; count],
                            |uvs| uvs.chunks_exact(2).map(|v| [v[0], v[1]]).collect(),
                        ),
                        colors: mesh
                            .colors
                            .as_ref()
                            .map_or_else(|| vec![[1.; 3]; count], |colors| triples(colors)),
                        indices: mesh.indices,
                        tangents: vec![],
                    })
                }
            };
            Ok(ProjectBatch {
                batch: InstanceBatch {
                    // A template's owner is known when the batch meets its scene.
                    object_id: document.object_id.unwrap_or_default(),
                    id: document.id,
                    source: SourceLocation {
                        line: 1,
                        column: 1,
                        offset: 0,
                        end: None,
                    },
                    mesh: String::new(),
                    material: None,
                    template,
                    visible: document.visible.unwrap_or(true),
                    collision: document.collision,
                    placements: instances
                        .iter()
                        .map(|instance| Placement {
                            translation: instance.translation,
                            rotation: instance.rotation.unwrap_or([0., 0., 0., 1.]),
                            scale: instance.scale.unwrap_or([1.; 3]),
                        })
                        .collect(),
                    values: instances.iter().filter_map(|i| i.value).collect(),
                    tints: instances.iter().filter_map(|i| i.tint).collect(),
                    scalars: document.scalars,
                    vectors: triples(document.vectors)?,
                    vertex_scalars,
                    vertex_vectors,
                    foliage: document.foliage,
                },
                mesh,
            })
        })
        .collect()
}

/// A mesh of the scene as a local mesh, with the record it has there.
fn template(scene: &Scene, id: &str) -> Result<(LocalMesh, crate::Mesh), String> {
    let record = scene
        .manifest
        .meshes
        .iter()
        .find(|mesh| mesh.id == id)
        .ok_or_else(|| format!("the scene has no mesh {id} to take as its template."))?;
    let vertices = record.vertex_start..record.vertex_start + record.vertex_count;
    let triples = |values: &[f32]| -> Vec<[f64; 3]> {
        values[vertices.start * 3..vertices.end * 3]
            .chunks_exact(3)
            .map(|v| [v[0], v[1], v[2]].map(f64::from))
            .collect()
    };
    Ok((
        LocalMesh {
            positions: triples(&scene.positions),
            normals: triples(&scene.normals),
            uvs: scene.uvs[vertices.start * 2..vertices.end * 2]
                .chunks_exact(2)
                .map(|v| [v[0], v[1]].map(f64::from))
                .collect(),
            colors: triples(&scene.colors),
            indices: scene.indices[record.index_start..record.index_start + record.index_count]
                .to_vec(),
            tangents: vec![],
        },
        record.clone(),
    ))
}

/// Removes meshes from a delivered scene: their geometry, records, tangents and collider entries.
fn remove_meshes(scene: &mut Scene, ids: &BTreeSet<String>) {
    let kept: Vec<crate::Mesh> = std::mem::take(&mut scene.manifest.meshes);
    let (positions, normals, uvs, colors, indices) = (
        std::mem::take(&mut scene.positions),
        std::mem::take(&mut scene.normals),
        std::mem::take(&mut scene.uvs),
        std::mem::take(&mut scene.colors),
        std::mem::take(&mut scene.indices),
    );
    for mut mesh in kept {
        if ids.contains(&mesh.id) {
            continue;
        }
        let vertices = mesh.vertex_start..mesh.vertex_start + mesh.vertex_count;
        let elements = mesh.index_start..mesh.index_start + mesh.index_count;
        mesh.vertex_start = scene.positions.len() / 3;
        mesh.index_start = scene.indices.len();
        scene
            .positions
            .extend_from_slice(&positions[vertices.start * 3..vertices.end * 3]);
        scene
            .normals
            .extend_from_slice(&normals[vertices.start * 3..vertices.end * 3]);
        scene
            .uvs
            .extend_from_slice(&uvs[vertices.start * 2..vertices.end * 2]);
        scene
            .colors
            .extend_from_slice(&colors[vertices.start * 3..vertices.end * 3]);
        scene.indices.extend_from_slice(&indices[elements]);
        scene.manifest.meshes.push(mesh);
    }
    scene.manifest.colliders.retain(|id| !ids.contains(id));
    scene
        .manifest
        .mesh_tangents
        .retain(|id, _| !ids.contains(id));
    scene.manifest.vertex_count = scene.positions.len() / 3;
    scene.manifest.index_count = scene.indices.len();
    // Per-vertex lightmap data no longer lines up; a lightmap is prepared after batches are added.
    scene.lightmap_uvs = None;
    scene.lightmap_charts = None;
    scene.manifest.lightmap = None;
}

/// Adds a project's batches to the scene delivered for it. A batch with a template takes that
/// mesh out of the scene: its geometry as delivered (baked UVs included) and its material become
/// the batch's, and placements apply to its positions as they are. Several batches may share a
/// template. Everything is checked before the scene changes.
pub fn add_project_batches(scene: &mut Scene, batches: &[ProjectBatch]) -> Result<(), String> {
    if batches.is_empty() {
        return Ok(());
    }
    let mut prepared = Vec::with_capacity(batches.len());
    let mut templates = BTreeSet::new();
    // Checked against a scene that holds nothing but the batches, so a refusal changes nothing.
    let mut trial = Scene::empty(scene.manifest.revision, "superspec-batch-admission");
    for ProjectBatch { batch, mesh } in batches {
        let mut batch = batch.clone();
        let mesh = match mesh {
            ProjectMesh::Inline(mesh) => mesh.clone(),
            ProjectMesh::Template(id) => {
                let (mesh, record) = template(scene, id)
                    .map_err(|message| format!("Batch {}: {message}", batch.id))?;
                if !record.visible {
                    return Err(format!(
                        "Batch {}: template mesh {id} is not a visible mesh.",
                        batch.id
                    ));
                }
                batch.material = record.material;
                if batch.object_id.is_empty() {
                    batch.object_id = record.object_id;
                }
                templates.insert(id.clone());
                mesh
            }
        };
        if batch.object_id.is_empty() {
            batch.object_id = batch.id.clone();
        }
        if scene
            .manifest
            .meshes
            .iter()
            .any(|old| old.id == batch.id && !templates.contains(&old.id))
        {
            return Err(format!(
                "Batch identity {:?} is already used by a mesh.",
                batch.id
            ));
        }
        let material = batch.material.take();
        trial.add_batch(&batch, &mesh)?;
        batch.material = material;
        prepared.push((batch, mesh));
    }
    let counts = scene.batches.counts();
    let added = trial.batches.counts();
    if scene.manifest.batches.len() + prepared.len() > MAX_BATCHES
        || counts.instances + added.instances > MAX_INSTANCES
        || counts.vertices + added.vertices > MAX_BATCH_VERTICES
        || counts.indices + added.indices > MAX_BATCH_INDICES
        || prepared
            .iter()
            .any(|(batch, _)| scene.manifest.batches.iter().any(|old| old.id == batch.id))
    {
        return Err("Project batches exceed the scene's allowance or repeat an identity.".into());
    }
    remove_meshes(scene, &templates);
    for (batch, mesh) in &prepared {
        scene.add_batch(batch, mesh)?;
    }
    Ok(())
}

/// Adds a project document's batches ([`of_project`]) to the scene delivered for it.
pub fn add_project(scene: &mut Scene, document: &serde_json::Value) -> Result<(), String> {
    add_project_batches(scene, &of_project(document)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn triangle() -> LocalMesh {
        LocalMesh {
            positions: vec![[0., 0., 0.], [1., 0., 0.], [0., 2., 0.]],
            normals: vec![[0., 0., 1.]; 3],
            uvs: vec![[0., 0.], [1., 0.], [0., 1.]],
            colors: vec![[1.; 3]; 3],
            indices: vec![0, 1, 2],
            tangents: vec![],
        }
    }

    fn batch(id: &str, placements: Vec<Placement>) -> InstanceBatch {
        InstanceBatch {
            id: id.into(),
            object_id: id.into(),
            source: SourceLocation {
                line: 1,
                column: 1,
                offset: 0,
                end: None,
            },
            mesh: "triangle".into(),
            material: None,
            template: None,
            visible: true,
            collision: Collision::None,
            placements,
            values: vec![],
            tints: vec![],
            ..Default::default()
        }
    }

    fn at(x: f64) -> Placement {
        Placement {
            translation: [x, 0., 0.],
            ..Default::default()
        }
    }

    #[test]
    fn a_batch_stores_its_mesh_once_with_bounds_of_every_instance() {
        let mut scene = Scene::empty(1, "test");
        let half = std::f64::consts::FRAC_1_SQRT_2;
        let mut turned = at(10.);
        turned.rotation = [0., 0., half, half];
        scene
            .add_batch(&batch("b", vec![at(0.), at(4.), turned]), &triangle())
            .unwrap();
        let record = &scene.manifest.batches[0];
        assert_eq!((record.vertex_count, record.index_count), (3, 3));
        assert_eq!(record.instance_count, 3);
        assert_eq!(scene.batches.positions.len(), 9);
        assert_eq!(scene.batches.placements.len(), 3 * PLACEMENT_FLOATS);
        assert_eq!(record.local_bounds, [[0., 0., 0.], [1., 2., 0.]]);
        // Derived by hand: the quarter turn about Z at x = 10 reaches x from 8 to 10 and y to 1.
        for (found, expected) in record
            .bounds
            .iter()
            .flatten()
            .zip([0., 0., 0., 10., 2., 0.])
        {
            assert!((found - expected).abs() < 1e-6, "{:?}", record.bounds);
        }
        assert_eq!(
            scene.manifest.batch_counts,
            Some(Counts {
                vertices: 3,
                indices: 3,
                instances: 3,
                values: 0,
                tints: 0,
                attributes: 0
            })
        );
        // Nothing is expanded into the scene's own meshes.
        assert!(scene.positions.is_empty() && scene.manifest.meshes.is_empty());
        check(&scene).unwrap();
    }

    #[test]
    fn records_are_ordered_by_identity_whatever_the_order_added() {
        let build = |order: &[&str]| {
            let mut scene = Scene::empty(1, "test");
            for id in order {
                let mut b = batch(id, vec![at(id.len() as f64), at(1.)]);
                if *id == "b" {
                    b.values = vec![[1., 2., 3., 4.], [5., 6., 7., 8.]];
                }
                if *id != "a" {
                    b.tints = vec![[0.5, 0.5, 0.5], [1., 0., 0.]];
                }
                scene.add_batch(&b, &triangle()).unwrap();
            }
            scene
        };
        let forward = build(&["a", "b", "cc"]);
        let backward = build(&["cc", "b", "a"]);
        assert_eq!(forward.manifest.batches, backward.manifest.batches);
        assert_eq!(forward.batches, backward.batches);
        assert_eq!(forward.packet(), backward.packet());
        let ids: Vec<_> = forward
            .manifest
            .batches
            .iter()
            .map(|b| b.id.as_str())
            .collect();
        assert_eq!(ids, ["a", "b", "cc"]);
        let b = &forward.manifest.batches[1];
        assert_eq!((b.value_start, b.tint_start), (Some(0), Some(0)));
        assert_eq!(forward.manifest.batches[2].tint_start, Some(2));
        assert_eq!(
            forward.batches.view(b).values.unwrap(),
            [1., 2., 3., 4., 5., 6., 7., 8.]
        );
        check(&forward).unwrap();
    }

    #[test]
    fn invalid_batches_are_refused_and_leave_the_scene_unchanged() {
        let mut scene = Scene::empty(1, "test");
        scene
            .add_batch(&batch("kept", vec![at(0.)]), &triangle())
            .unwrap();
        let before = scene.packet();
        let mut cases: Vec<(InstanceBatch, LocalMesh, &str)> = vec![];
        cases.push((batch("kept", vec![at(0.)]), triangle(), "already used"));
        cases.push((batch("", vec![at(0.)]), triangle(), "invalid"));
        cases.push((batch("n", vec![]), triangle(), "no instance"));
        let mut b = batch("m", vec![at(0.)]);
        b.material = Some("missing".into());
        cases.push((b, triangle(), "missing material"));
        let mut b = batch("v", vec![at(0.), at(1.)]);
        b.values = vec![[0.; 4]];
        cases.push((b, triangle(), "every instance or for none"));
        let mut b = batch("t", vec![at(0.)]);
        b.tints = vec![[-1., 0., 0.]];
        cases.push((b, triangle(), "nonnegative"));
        let mut p = at(0.);
        p.scale = [1., -1., 1.];
        cases.push((batch("s", vec![p]), triangle(), "instance 0"));
        let mut mesh = triangle();
        mesh.indices = vec![0, 1, 7];
        cases.push((batch("i", vec![at(0.)]), mesh, "valid indices"));
        for (batch, mesh, expected) in cases {
            let error = scene.add_batch(&batch, &mesh).unwrap_err();
            assert!(error.contains(expected), "{error}");
            assert_eq!(scene.packet(), before);
        }
    }

    #[test]
    fn only_colliding_batches_reset_validation() {
        let mut scene = Scene::empty(1, "test");
        scene.manifest.geometry_validation = "checked";
        scene.manifest.navigation_validation = "sampled";
        scene
            .add_batch(&batch("render", vec![at(0.)]), &triangle())
            .unwrap();
        assert_eq!(scene.manifest.geometry_validation, "checked");
        assert_eq!(scene.manifest.navigation_validation, "sampled");
        assert!(scene.manifest.colliders.is_empty());
        let mut solid = batch("solid", vec![at(0.)]);
        solid.collision = Collision::Instances;
        scene.add_batch(&solid, &triangle()).unwrap();
        assert_eq!(scene.manifest.geometry_validation, "not-run");
        assert_eq!(scene.manifest.navigation_validation, "not-run");
        // Colliders name meshes; a batch states its own collision.
        assert!(scene.manifest.colliders.is_empty());
    }

    #[test]
    fn project_batches_are_read_strictly() {
        assert!(of_project(&json!({"kind": "material"})).unwrap().is_empty());
        let document = json!({"batches": [{
            "id": "bolts",
            "mesh": {"positions": [0, 0, 0, 1, 0, 0, 0, 1, 0], "normals": [0, 0, 1, 0, 0, 1, 0, 0, 1], "indices": [0, 1, 2]},
            "instances": [{"translation": [1, 2, 3], "value": [0.5, 0, 0, 1]}, {"translation": [4, 5, 6], "scale": [2, 2, 2], "value": [0.25, 0, 0, 1]}],
        }]});
        let found = of_project(&document).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].batch.collision, Collision::None);
        assert_eq!(
            found[0].batch.values,
            [[0.5, 0., 0., 1.], [0.25, 0., 0., 1.]]
        );
        let ProjectMesh::Inline(mesh) = &found[0].mesh else {
            panic!()
        };
        assert_eq!(mesh.colors, vec![[1.; 3]; 3]);
        let mut scene = Scene::empty(1, "test");
        add_project(&mut scene, &document).unwrap();
        assert_eq!(scene.manifest.batches[0].object_id, "bolts");
        assert_eq!(scene.manifest.batches[0].value_start, Some(0));
        assert_eq!(scene.manifest.batches[0].template, None);

        let mut unknown = document.clone();
        unknown["batches"][0]["shear"] = json!(1);
        assert!(of_project(&unknown).unwrap_err().contains("shear"));
        let mut partial = document.clone();
        partial["batches"][0]["instances"][1]
            .as_object_mut()
            .unwrap()
            .remove("value");
        assert!(of_project(&partial).unwrap_err().contains("every instance"));
        let mut short = document;
        short["batches"][0]["mesh"]["normals"] = json!([0, 0, 1]);
        assert!(of_project(&short).unwrap_err().contains("per vertex"));
    }

    #[test]
    fn a_foliage_look_is_read_with_its_defaults_and_delivered_beside_the_batch() {
        let document = |foliage: serde_json::Value| {
            json!({"batches": [{
                "id": "grass",
                "mesh": {"positions": [0, 0, 0, 1, 0, 0, 0, 1, 0], "normals": [0, 0, 1, 0, 0, 1, 0, 0, 1], "indices": [0, 1, 2]},
                "instances": [{"translation": [1, 2, 3]}],
                "foliage": foliage,
            }]})
        };
        let given = document(
            json!({"family": "grass", "patch": "lawn", "level": 1, "distance": 4, "keep": 0.5, "far": 30, "material": "painted-grass"}),
        );
        let found = of_project(&given).unwrap();
        let foliage = found[0].batch.foliage.clone().unwrap();
        assert_eq!(foliage.keep, 0.5);
        assert_eq!(foliage.wind, superspec_types::batch::FoliageWind::default());
        assert_eq!(
            foliage.profile,
            superspec_types::batch::FoliageProfile::Painted
        );
        let mut scene = Scene::empty(1, "test");
        add_project(&mut scene, &given).unwrap();
        assert_eq!(scene.manifest.batches[0].foliage.as_ref(), Some(&foliage));
        let manifest = serde_json::to_value(&scene.manifest.batches[0]).unwrap();
        assert_eq!(manifest["foliage"]["material"], "painted-grass");
        assert_eq!(manifest["foliage"]["rejection"]["grazingKeep"], 0.5);
        // A batch without a look delivers the record it always did.
        let mut plain = Scene::empty(1, "test");
        let mut bare = given.clone();
        bare["batches"][0]
            .as_object_mut()
            .unwrap()
            .remove("foliage");
        add_project(&mut plain, &bare).unwrap();
        assert!(
            serde_json::to_value(&plain.manifest.batches[0])
                .unwrap()
                .get("foliage")
                .is_none()
        );
        // Families and fields are a closed set: shader code is not a parameter.
        for refused in [
            json!({"family": "custom", "patch": "lawn", "far": 30}),
            json!({"family": "grass", "patch": "lawn", "far": 30, "shader": "void main() {}"}),
            json!({"family": "grass", "patch": "lawn", "far": 30, "wind": {"gust": 1}}),
        ] {
            assert!(of_project(&document(refused)).is_err());
        }
        // Values out of range are refused when the batch meets its scene, and change nothing.
        let mut scene = Scene::empty(1, "test");
        let far = document(json!({"family": "grass", "patch": "lawn", "distance": 40, "far": 30}));
        assert!(add_project(&mut scene, &far).unwrap_err().contains("far"));
        assert!(scene.manifest.batches.is_empty());
    }

    /// A scene of two placed triangles, the second a collider with tangents.
    fn two_meshes() -> Scene {
        use crate::instances::{IDENTITY, Instance, Model};
        let instance = |id: &str, x: f64, collidable: bool| {
            let mut transform = IDENTITY;
            transform[12] = x;
            Instance {
                id: id.into(),
                object_id: format!("owner-{id}"),
                mesh: "triangle".into(),
                material: None,
                transform,
                source: SourceLocation {
                    line: 1,
                    column: 1,
                    offset: 0,
                    end: None,
                },
                visible: true,
                collidable,
            }
        };
        let mut mesh = triangle();
        mesh.tangents = vec![[1., 0., 0., 1.]; 3];
        Model {
            id: "scene".into(),
            name: "Scene".into(),
            meshes: std::collections::BTreeMap::from([(
                "triangle".into(),
                std::sync::Arc::new(mesh),
            )]),
            instances: vec![instance("template", 100., true), instance("kept", 5., true)],
            ..Default::default()
        }
        .scene(1)
        .unwrap()
    }

    #[test]
    fn a_template_mesh_leaves_the_scene_and_becomes_the_batch() {
        let mut scene = two_meshes();
        let document = json!({"batches": [
            {"id": "b", "mesh": "template", "instances": [{"translation": [-100, 0, 0]}, {"translation": [-99, 0, 0]}]},
            {"id": "a", "mesh": "template", "instances": [{"translation": [0, 0, 1]}]},
        ]});
        add_project(&mut scene, &document).unwrap();
        // The other mesh is all that is left, with its ranges and collision; the template collides nowhere.
        assert_eq!(scene.manifest.meshes.len(), 1);
        assert_eq!(scene.manifest.meshes[0].id, "kept");
        assert_eq!(
            (
                scene.manifest.meshes[0].vertex_start,
                scene.manifest.vertex_count
            ),
            (0, 3)
        );
        assert_eq!(scene.positions[..3], [5., 0., 0.]);
        assert_eq!(scene.manifest.colliders, ["kept"]);
        assert!(!scene.manifest.mesh_tangents.contains_key("template"));
        crate::composition::WorldModel::from_scene(scene.clone()).unwrap();
        let ids: Vec<_> = scene
            .manifest
            .batches
            .iter()
            .map(|b| b.id.as_str())
            .collect();
        assert_eq!(ids, ["a", "b"]);
        let b = &scene.manifest.batches[1];
        assert_eq!(b.template.as_deref(), Some("template"));
        assert_eq!(b.object_id, "owner-template");
        // The template stood at x = 100: its positions are the batch's, and placements apply to them.
        assert_eq!(scene.batches.view(b).positions[..3], [100., 0., 0.]);
        assert_eq!(b.bounds, [[0., 0., 0.], [2., 2., 0.]]);

        // A refusal leaves the scene as it was.
        let mut scene = two_meshes();
        let before = scene.packet();
        for (document, expected) in [
            (
                json!({"batches": [{"id": "x", "mesh": "missing", "instances": [{"translation": [0, 0, 0]}]}]}),
                "no mesh missing",
            ),
            (
                json!({"batches": [{"id": "kept", "mesh": "template", "instances": [{"translation": [0, 0, 0]}]}]}),
                "already used",
            ),
            (
                json!({"batches": [{"id": "x", "mesh": "template", "instances": [{"translation": [0, 0, 0], "scale": [0, 1, 1]}]}]}),
                "instance 0",
            ),
        ] {
            assert!(
                add_project(&mut scene, &document)
                    .unwrap_err()
                    .contains(expected)
            );
            assert_eq!(scene.packet(), before);
        }
    }
    #[test]
    fn named_attributes_are_delivered_in_a_fixed_order_and_checked() {
        let mut scene = Scene::empty(1, "test");
        let mut b = batch("b", vec![at(0.), at(1.)]);
        b.vectors
            .insert("shadeNormal".into(), vec![[0., 0., 1.], [0., 1., 0.]]);
        b.scalars.insert("variation".into(), vec![0.25, 0.75]);
        b.vertex_scalars
            .insert("heightRatio".into(), vec![0., 0.5, 1.]);
        b.vertex_vectors
            .insert("pivot".into(), vec![[1., 2., 3.]; 3]);
        scene
            .add_batch(&batch("a", vec![at(5.)]), &triangle())
            .unwrap();
        scene.add_batch(&b, &triangle()).unwrap();
        // Added first, sorted after: the pool follows the records.
        let mut first = batch("0", vec![at(2.)]);
        first.scalars.insert("cluster".into(), vec![7.]);
        scene.add_batch(&first, &triangle()).unwrap();
        check(&scene).unwrap();
        let record = &scene.manifest.batches[2];
        let listed: Vec<_> = record
            .attributes
            .iter()
            .map(|a| (a.name.as_str(), a.domain, a.components, a.start))
            .collect();
        assert_eq!(
            listed,
            [
                ("variation", Domain::Instance, 1, 1),
                ("shadeNormal", Domain::Instance, 3, 3),
                ("heightRatio", Domain::Vertex, 1, 9),
                ("pivot", Domain::Vertex, 3, 12),
            ]
        );
        assert_eq!(
            scene.batches.attribute(record, &record.attributes[1]),
            [0., 0., 1., 0., 1., 0.]
        );
        assert_eq!(
            scene.batches.attribute(record, &record.attributes[2]),
            [0., 0.5, 1.]
        );
        assert_eq!(scene.batches.attributes[0], 7.);
        assert_eq!(scene.manifest.batch_counts.unwrap().attributes, 21);
        let before = scene.packet();
        let mut bad = batch("c", vec![at(0.)]);
        bad.scalars.insert("shade normal".into(), vec![1.]);
        assert!(
            scene
                .add_batch(&bad, &triangle())
                .unwrap_err()
                .contains("letters and digits")
        );
        let mut bad = batch("c", vec![at(0.)]);
        bad.vertex_scalars.insert("heightRatio".into(), vec![1.]);
        assert!(
            scene
                .add_batch(&bad, &triangle())
                .unwrap_err()
                .contains("per vertex")
        );
        let mut bad = batch("c", vec![at(0.)]);
        for index in 0..13 {
            bad.scalars.insert(format!("s{index}"), vec![1.]);
        }
        assert!(
            scene
                .add_batch(&bad, &triangle())
                .unwrap_err()
                .contains("too many")
        );
        assert_eq!(scene.packet(), before);
        // A project document states them by name.
        let document = json!({"batches": [{"id": "p",
            "mesh": {"positions": [0, 0, 0, 1, 0, 0, 0, 1, 0], "normals": [0, 0, 1, 0, 0, 1, 0, 0, 1], "indices": [0, 1, 2],
                "scalars": {"heightRatio": [0, 1, 1]}, "vectors": {"shadeNormal": [0, 0, 1, 0, 0, 1, 0, 0, 1]}},
            "instances": [{"translation": [0, 0, 0]}], "scalars": {"variation": [0.5]}, "vectors": {"pivot": [1, 2, 3]}}]});
        add_project(&mut scene, &document).unwrap();
        let record = scene.manifest.batches.iter().find(|b| b.id == "p").unwrap();
        let names: Vec<_> = record.attributes.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["variation", "pivot", "heightRatio", "shadeNormal"]);
    }
}
