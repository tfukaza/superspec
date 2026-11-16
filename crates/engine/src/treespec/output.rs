//! Records every generator emits. Plain data: a later stage maps them to the portable
//! instanced-batch and vertex-attribute records (see `README.md`).
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use superspec_geometry::math::V3;

/// Four affine columns: three basis vectors and the translation.
pub type Transform = [[f64; 3]; 4];
pub type Bounds = [[f64; 3]; 2];

/// Vertex scalar names.
pub const HEIGHT_RATIO: &str = "heightRatio";
pub const BEND_WEIGHT: &str = "bendWeight";
pub const RANDOM: &str = "random";
pub const ELEMENT: &str = "element";
pub const LEVEL: &str = "level";
/// Vertex and instance vector names.
pub const SHADE_NORMAL: &str = "shadeNormal";
pub const PIVOT: &str = "pivot";
/// Instance names.
pub const ROOT: &str = "root";
pub const GROUND_NORMAL: &str = "groundNormal";
pub const THRESHOLD: &str = "threshold";
pub const VARIATION: &str = "variation";
pub const CLUSTER: &str = "cluster";

/// One mesh with the named attributes painted materials and wind read.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoliageMesh {
    pub id: String,
    pub geometry_key: String,
    pub positions: Vec<V3>,
    /// Geometric normals.
    pub normals: Vec<V3>,
    pub uvs: Vec<[f64; 2]>,
    pub indices: Vec<u32>,
    /// `blade`, `bark` or `leaf`.
    pub material_slot: String,
    pub double_sided: bool,
    /// One value per vertex under each name.
    pub scalars: BTreeMap<String, Vec<f64>>,
    pub vectors: BTreeMap<String, Vec<V3>>,
}
impl FoliageMesh {
    pub(super) fn new(id: String, geometry_key: String, slot: &str, double_sided: bool) -> Self {
        Self {
            id,
            geometry_key,
            positions: vec![],
            normals: vec![],
            uvs: vec![],
            indices: vec![],
            material_slot: slot.into(),
            double_sided,
            scalars: BTreeMap::new(),
            vectors: BTreeMap::new(),
        }
    }
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }
    pub(super) fn scalar(&mut self, name: &str, value: f64) {
        self.scalars.entry(name.into()).or_default().push(value);
    }
    pub(super) fn vector(&mut self, name: &str, value: V3) {
        self.vectors.entry(name.into()).or_default().push(value);
    }
    /// Counts agree, indices are in range and every value is finite.
    pub fn validate(&self) -> Result<(), String> {
        let count = self.positions.len();
        if self.normals.len() != count
            || self.uvs.len() != count
            || self.scalars.values().any(|v| v.len() != count)
            || self.vectors.values().any(|v| v.len() != count)
        {
            return Err(format!("Mesh {} has uneven attribute counts.", self.id));
        }
        if !self.indices.len().is_multiple_of(3)
            || self.indices.iter().any(|i| *i as usize >= count)
        {
            return Err(format!("Mesh {} has invalid indices.", self.id));
        }
        let finite = self
            .positions
            .iter()
            .chain(&self.normals)
            .chain(self.vectors.values().flatten())
            .flatten()
            .chain(self.uvs.iter().flatten())
            .chain(self.scalars.values().flatten())
            .all(|v| v.is_finite());
        if finite {
            Ok(())
        } else {
            Err(format!("Mesh {} has a nonfinite value.", self.id))
        }
    }
    fn digest(&self, hash: &mut Sha256) {
        text(hash, &self.id);
        text(hash, &self.geometry_key);
        text(hash, &self.material_slot);
        hash.update([self.double_sided as u8]);
        floats(hash, self.positions.iter().flatten());
        floats(hash, self.normals.iter().flatten());
        floats(hash, self.uvs.iter().flatten());
        hash.update((self.indices.len() as u64).to_le_bytes());
        for index in &self.indices {
            hash.update(index.to_le_bytes());
        }
        attributes(hash, &self.scalars, &self.vectors);
    }
}

/// One mesh drawn many times: transforms and typed per-instance attributes.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceBatch {
    pub id: String,
    /// Which mesh the batch draws; each owner says what the index refers to.
    pub mesh: usize,
    pub transforms: Vec<Transform>,
    /// One value per instance under each name.
    pub scalars: BTreeMap<String, Vec<f64>>,
    pub vectors: BTreeMap<String, Vec<V3>>,
}
impl InstanceBatch {
    pub(super) fn new(id: String, mesh: usize) -> Self {
        Self {
            id,
            mesh,
            transforms: vec![],
            scalars: BTreeMap::new(),
            vectors: BTreeMap::new(),
        }
    }
    pub fn len(&self) -> usize {
        self.transforms.len()
    }
    pub fn is_empty(&self) -> bool {
        self.transforms.is_empty()
    }
    pub(super) fn scalar(&mut self, name: &str, value: f64) {
        self.scalars.entry(name.into()).or_default().push(value);
    }
    pub(super) fn vector(&mut self, name: &str, value: V3) {
        self.vectors.entry(name.into()).or_default().push(value);
    }
    /// Reorders every per-instance list by `order`, a permutation of the instances.
    pub(super) fn reorder(&mut self, order: &[usize]) {
        fn pick<T: Copy>(values: &mut Vec<T>, order: &[usize]) {
            *values = order.iter().map(|i| values[*i]).collect();
        }
        pick(&mut self.transforms, order);
        self.scalars.values_mut().for_each(|v| pick(v, order));
        self.vectors.values_mut().for_each(|v| pick(v, order));
    }
    fn digest(&self, hash: &mut Sha256) {
        text(hash, &self.id);
        hash.update((self.mesh as u64).to_le_bytes());
        floats(hash, self.transforms.iter().flatten().flatten());
        attributes(hash, &self.scalars, &self.vectors);
    }
}

/// Where an effect may go, in the shape ItemSpec reports its features: a named polyline.
/// A plant reports `base`, the ring where a trunk meets the ground.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Feature {
    pub name: String,
    pub part: String,
    pub face: String,
    pub points: Vec<V3>,
    pub closed: bool,
    pub normal: V3,
    pub width: f64,
}

fn text(hash: &mut Sha256, value: &str) {
    hash.update((value.len() as u64).to_le_bytes());
    hash.update(value.as_bytes());
}
fn floats<'a>(hash: &mut Sha256, values: impl Iterator<Item = &'a f64>) {
    let mut count = 0u64;
    for value in values {
        hash.update(value.to_bits().to_le_bytes());
        count += 1;
    }
    hash.update(count.to_le_bytes());
}
fn attributes(
    hash: &mut Sha256,
    scalars: &BTreeMap<String, Vec<f64>>,
    vectors: &BTreeMap<String, Vec<V3>>,
) {
    for (name, values) in scalars {
        text(hash, name);
        floats(hash, values.iter());
    }
    for (name, values) in vectors {
        text(hash, name);
        floats(hash, values.iter().flatten());
    }
}

/// A hash of every bit of the given meshes and batches, for byte-parity checks.
pub fn digest<'a>(
    meshes: impl IntoIterator<Item = &'a FoliageMesh>,
    batches: impl IntoIterator<Item = &'a InstanceBatch>,
) -> String {
    let mut hash = Sha256::new();
    hash.update(b"treespec-digest-v1\0");
    for mesh in meshes {
        mesh.digest(&mut hash);
    }
    hash.update([0xff]);
    for batch in batches {
        batch.digest(&mut hash);
    }
    format!("{:x}", hash.finalize())
}

pub(super) fn grow(bounds: &mut Bounds, point: V3) {
    for axis in 0..3 {
        bounds[0][axis] = bounds[0][axis].min(point[axis]);
        bounds[1][axis] = bounds[1][axis].max(point[axis]);
    }
}
pub(super) const EMPTY_BOUNDS: Bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
