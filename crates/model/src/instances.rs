//! Shared local geometry and independent placements; compatibility delivery expands owned buffers.
use crate::{Asset, Mesh, Scene};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use superspec_types::{diagnostic::SourceLocation, material::Material};

pub const IDENTITY: [f64; 16] = [
    1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
];

#[derive(Clone, Debug, PartialEq)]
pub struct LocalMesh {
    pub positions: Vec<[f64; 3]>,
    pub normals: Vec<[f64; 3]>,
    pub uvs: Vec<[f64; 2]>,
    pub colors: Vec<[f64; 3]>,
    pub indices: Vec<u32>,
    pub tangents: Vec<[f64; 4]>,
}

#[derive(Clone, Debug)]
pub struct Instance {
    pub id: String,
    pub object_id: String,
    pub mesh: String,
    pub material: Option<String>,
    /// Column-major affine transform in the model's right-handed Z-up space.
    pub transform: [f64; 16],
    pub source: SourceLocation,
    pub visible: bool,
    pub collidable: bool,
}

#[derive(Clone, Debug)]
pub struct ImageResource {
    pub mime: &'static str,
    pub bytes: Arc<[u8]>,
    /// Supplied by the image admission boundary, never inferred from encoded byte length.
    pub pixels: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub meshes: BTreeMap<String, Arc<LocalMesh>>,
    /// Optional producer identities for immutable local artifacts, independent of placements.
    /// A repeated identity must represent every attribute exactly, not merely the same shape.
    pub mesh_artifacts: BTreeMap<String, String>,
    pub instances: Vec<Instance>,
    /// Instanced batches (`crate::batches`): each names a mesh of `meshes` and is delivered
    /// unexpanded, beside the placements above.
    pub batches: Vec<crate::batches::InstanceBatch>,
    pub materials: BTreeMap<String, Material>,
    pub images: BTreeMap<String, ImageResource>,
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.into_iter().zip(b).map(|(a, b)| a * b).sum()
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Checks affine invertibility before either points or normals are transformed.
pub fn determinant(matrix: &[f64; 16]) -> Result<f64, String> {
    if matrix.iter().any(|n| !n.is_finite() || n.abs() > 1e12)
        || matrix[3] != 0.
        || matrix[7] != 0.
        || matrix[11] != 0.
        || matrix[15] != 1.
    {
        return Err("Instance transform must be a finite affine matrix.".into());
    }
    let value = dot(
        [matrix[0], matrix[1], matrix[2]],
        cross(
            [matrix[4], matrix[5], matrix[6]],
            [matrix[8], matrix[9], matrix[10]],
        ),
    );
    if !value.is_finite() || value.abs() < 1e-12 {
        return Err("Instance transform is singular.".into());
    }
    Ok(value)
}

pub fn transform_point(matrix: &[f64; 16], point: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|axis| {
        matrix[axis] * point[0]
            + matrix[4 + axis] * point[1]
            + matrix[8 + axis] * point[2]
            + matrix[12 + axis]
    })
}

pub fn transform_normal(matrix: &[f64; 16], normal: [f64; 3]) -> Result<[f64; 3], String> {
    let determinant = determinant(matrix)?;
    let a = [matrix[0], matrix[1], matrix[2]];
    let b = [matrix[4], matrix[5], matrix[6]];
    let c = [matrix[8], matrix[9], matrix[10]];
    let cofactors = [cross(b, c), cross(c, a), cross(a, b)];
    let result: [f64; 3] = std::array::from_fn(|axis| {
        (cofactors[0][axis] * normal[0]
            + cofactors[1][axis] * normal[1]
            + cofactors[2][axis] * normal[2])
            / determinant
    });
    let length = dot(result, result).sqrt();
    if !length.is_finite() || length < 1e-15 {
        return Err("Instance normal must have a finite nonzero length.".into());
    }
    Ok(result.map(|v| v / length))
}

impl LocalMesh {
    pub fn validate(&self) -> Result<(), String> {
        let count = self.positions.len();
        if count == 0
            || count > 750_000
            || self.normals.len() != count
            || self.uvs.len() != count
            || self.colors.len() != count
            || self.indices.is_empty()
            || !self.indices.len().is_multiple_of(3)
            || self.indices.len() > 4_500_000
            || self.indices.iter().any(|i| *i as usize >= count)
        {
            return Err(
                "Local mesh requires bounded, complete triangle attributes and valid indices."
                    .into(),
            );
        }
        if !self.tangents.is_empty()
            && (self.tangents.len() != count
                || self.tangents.iter().any(|t| {
                    t.iter().any(|v| !v.is_finite())
                        || !matches!(t[3], -1. | 1.)
                        || dot([t[0], t[1], t[2]], [t[0], t[1], t[2]]) < 1e-20
                }))
        {
            return Err("Local mesh tangents must contain a finite nonzero direction and signed handedness per vertex.".into());
        }
        if self
            .positions
            .iter()
            .flatten()
            .chain(self.normals.iter().flatten())
            .chain(self.uvs.iter().flatten())
            .chain(self.colors.iter().flatten())
            .any(|n| !n.is_finite() || n.abs() > 1e9)
        {
            return Err("Local mesh contains invalid numeric attributes.".into());
        }
        if self.normals.iter().any(|n| dot(*n, *n) < 1e-20)
            || self.colors.iter().flatten().any(|n| *n < 0.)
        {
            return Err(
                "Local mesh requires nonzero normals and nonnegative linear colors.".into(),
            );
        }
        Ok(())
    }
}

impl Model {
    /// Creates fresh target buffers. The shared meshes, placements and maps are never modified.
    pub fn scene(&self, revision: u32) -> Result<Scene, String> {
        if self.meshes.len() > 16_384
            || self.instances.len() > 16_384
            || self.batches.len() > crate::batches::MAX_BATCHES
            || self.materials.len() > 16_384
            || self.images.len() > 16_384
        {
            return Err("Instance model exceeds its resource/placement allowance.".into());
        }
        if self.mesh_artifacts.iter().any(|(mesh, artifact)| {
            !self.meshes.contains_key(mesh) || artifact.is_empty() || artifact.len() > 4096
        }) {
            return Err("Invalid local mesh artifact identity.".into());
        }
        let mut scene = Scene::empty(revision, "superspec-instance-model-v1");
        scene.manifest.id = self.id.clone();
        scene.manifest.name = self.name.clone();
        let mut image_pixels = 0usize;
        for (id, image) in &self.images {
            if id.is_empty()
                || id.len() > 4096
                || !matches!(image.mime, "image/png" | "image/jpeg" | "image/ktx2")
                || image.bytes.is_empty()
            {
                return Err("Instance model image has invalid identity, encoding or bytes.".into());
            }
            image_pixels = image_pixels
                .checked_add(image.pixels)
                .ok_or("Image pixel allowance overflow.")?;
            scene.image_sizes.insert(id.clone(), image.pixels);
            if scene.asset_bytes.len().saturating_add(image.bytes.len())
                > crate::MAX_SCENE_IMAGE_BYTES
            {
                return Err("Instance model image byte allowance exceeded.".into());
            }
            scene.manifest.assets.push(Asset {
                id: id.clone(),
                mime: image.mime,
                byte_start: scene.asset_bytes.len(),
                byte_length: image.bytes.len(),
            });
            scene.asset_bytes.extend_from_slice(&image.bytes);
        }
        scene.reserve_image_pixels(image_pixels)?;
        for (id, material) in &self.materials {
            if id != &material.id || id.is_empty() || id.len() > 4096 {
                return Err("Material identity disagrees with its resource key.".into());
            }
            material.validate()?;
            for image in material.companions() {
                if !self.images.contains_key(image) {
                    return Err(format!("Material {id} references missing image {image}."));
                }
            }
            scene.manifest.materials.push(material.clone());
        }
        for mesh in self.meshes.values() {
            mesh.validate()?;
        }
        let mut ids = BTreeSet::new();
        for instance in &self.instances {
            if instance.id.is_empty()
                || instance.id.len() > 4096
                || instance.object_id.is_empty()
                || instance.object_id.len() > 4096
                || !ids.insert(&instance.id)
            {
                return Err("Instance identity is invalid or duplicated.".into());
            }
            let mesh = self.meshes.get(&instance.mesh).ok_or_else(|| {
                format!(
                    "Instance {} references missing mesh {}.",
                    instance.id, instance.mesh
                )
            })?;
            if instance
                .material
                .as_ref()
                .is_some_and(|id| !self.materials.contains_key(id))
            {
                return Err(format!(
                    "Instance {} references a missing material.",
                    instance.id
                ));
            }
            let mirrored = determinant(&instance.transform)? < 0.;
            if scene.positions.len() / 3 + mesh.positions.len() > 1_500_000
                || scene.indices.len() + mesh.indices.len() > 9_000_000
            {
                return Err(
                    "Expanded instance model exceeds its delivery geometry allowance.".into(),
                );
            }
            let vertex_start = scene.positions.len() / 3;
            let index_start = scene.indices.len();
            let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
            let mut tangents = Vec::new();
            for i in 0..mesh.positions.len() {
                let point = transform_point(&instance.transform, mesh.positions[i]);
                if point.iter().any(|n| !n.is_finite() || n.abs() > 1e9) {
                    return Err(
                        "Transformed instance leaves the supported coordinate range.".into(),
                    );
                }
                let normal = transform_normal(&instance.transform, mesh.normals[i])?;
                for axis in 0..3 {
                    bounds[0][axis] = bounds[0][axis].min(point[axis]);
                    bounds[1][axis] = bounds[1][axis].max(point[axis]);
                }
                scene.positions.extend(point.map(|n| n as f32));
                scene.normals.extend(normal.map(|n| n as f32));
                scene.uvs.extend(mesh.uvs[i].map(|n| n as f32));
                scene.colors.extend(mesh.colors[i].map(|n| n as f32));
                if !mesh.tangents.is_empty() {
                    let t = mesh.tangents[i];
                    let m = &instance.transform;
                    let direction: [f64; 3] = std::array::from_fn(|axis| {
                        m[axis] * t[0] + m[4 + axis] * t[1] + m[8 + axis] * t[2]
                    });
                    let direction: [f64; 3] = std::array::from_fn(|axis| {
                        direction[axis] - normal[axis] * dot(direction, normal)
                    });
                    let length = dot(direction, direction).sqrt();
                    if !length.is_finite() || length < 1e-12 {
                        return Err("Instance tangent is parallel to its normal.".into());
                    }
                    tangents.push([
                        (direction[0] / length) as f32,
                        (direction[1] / length) as f32,
                        (direction[2] / length) as f32,
                        (t[3] * if mirrored { -1. } else { 1. }) as f32,
                    ]);
                }
            }
            for triangle in mesh.indices.chunks_exact(3) {
                scene.indices.extend(if mirrored {
                    [triangle[0], triangle[2], triangle[1]]
                } else {
                    [triangle[0], triangle[1], triangle[2]]
                });
            }
            scene.manifest.meshes.push(Mesh {
                id: instance.id.clone(),
                object_id: instance.object_id.clone(),
                source: instance.source.clone(),
                material: instance.material.clone(),
                visible: instance.visible,
                vertex_start,
                vertex_count: mesh.positions.len(),
                index_start,
                index_count: mesh.indices.len(),
                bounds,
            });
            if !tangents.is_empty() {
                scene
                    .manifest
                    .mesh_tangents
                    .insert(instance.id.clone(), tangents);
            }
            if instance.collidable {
                scene.manifest.colliders.push(instance.id.clone());
            }
        }
        scene.manifest.vertex_count = scene.positions.len() / 3;
        scene.manifest.index_count = scene.indices.len();
        scene.manifest.asset_byte_count = scene.asset_bytes.len();
        for batch in &self.batches {
            let mesh = self.meshes.get(&batch.mesh).ok_or_else(|| {
                format!("Batch {} references missing mesh {}.", batch.id, batch.mesh)
            })?;
            scene.add_batch(batch, mesh)?;
        }
        scene.manifest.success = true;
        let mut objects = BTreeMap::new();
        for instance in &self.instances {
            let m = instance.transform;
            objects
                .entry(instance.object_id.clone())
                .or_insert_with(|| crate::architecture::Object {
                    id: instance.object_id.clone(),
                    source_id: instance.object_id.clone(),
                    kind: "item".into(),
                    source: instance.source.clone(),
                    matrix: [
                        [m[0], m[1], m[2]],
                        [m[4], m[5], m[6]],
                        [m[8], m[9], m[10]],
                        [m[12], m[13], m[14]],
                    ],
                    instances: vec![],
                    assemblies: vec![],
                    contributors: vec![],
                    attributes: Default::default(),
                });
        }
        // A batch's owner stands at the origin: its instances carry their own placements.
        for batch in &self.batches {
            objects
                .entry(batch.object_id.clone())
                .or_insert_with(|| crate::architecture::Object {
                    id: batch.object_id.clone(),
                    source_id: batch.object_id.clone(),
                    kind: "item".into(),
                    source: batch.source.clone(),
                    matrix: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [0., 0., 0.]],
                    instances: vec![],
                    assemblies: vec![],
                    contributors: vec![],
                    attributes: Default::default(),
                });
        }
        scene.manifest.architecture = Some(crate::architecture::Context {
            metadata: vec![],
            declarations: vec![],
            objects: objects.into_values().collect(),
            floors: vec![],
            water: vec![],
            water_obstacles: Default::default(),
            connections: vec![],
            connection_obstructions: Default::default(),
            volumes: vec![],
        });
        Ok(scene)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn model() -> Model {
        let mesh = LocalMesh {
            positions: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
            normals: vec![[0., 0., 1.]; 3],
            uvs: vec![[0., 0.], [1., 0.], [0., 1.]],
            colors: vec![[1.; 3]; 3],
            indices: vec![0, 1, 2],
            tangents: vec![],
        };
        let source = SourceLocation {
            line: 1,
            column: 1,
            offset: 0,
            end: Some(1),
        };
        let a = Instance {
            id: "a".into(),
            object_id: "prop-a".into(),
            mesh: "triangle".into(),
            material: None,
            transform: IDENTITY,
            source,
            visible: true,
            collidable: false,
        };
        let mut b = a.clone();
        b.id = "b".into();
        b.object_id = "prop-b".into();
        b.transform[0] = -2.;
        b.transform[12] = 4.;
        b.collidable = true;
        Model {
            id: "shared".into(),
            name: "Shared".into(),
            meshes: BTreeMap::from([("triangle".into(), Arc::new(mesh))]),
            instances: vec![a, b],
            ..Default::default()
        }
    }
    #[test]
    fn repeated_placements_keep_source_mesh_and_independent_collision() {
        let model = model();
        let before = model.meshes["triangle"].positions.clone();
        let scene = model.scene(4).unwrap();
        assert_eq!(scene.positions[9..], [4., 0., 0., 2., 0., 0., 4., 1., 0.]);
        assert_eq!(scene.indices, [0, 1, 2, 0, 2, 1]);
        assert_eq!(scene.manifest.colliders, ["b"]);
        assert_eq!(scene.manifest.meshes[1].object_id, "prop-b");
        assert_eq!(model.meshes["triangle"].positions, before);
        assert_eq!(model.scene(4).unwrap().packet(), scene.packet());
    }
    #[test]
    fn nonuniform_transform_preserves_normal_perpendicularity() {
        let mut transform = IDENTITY;
        transform[0] = 2.;
        transform[5] = 3.;
        transform[8] = 1.;
        let normal = transform_normal(&transform, [1., 0., 1.]).unwrap();
        let tangent = [1., 0., -1.];
        let tangent = transform_point(&transform, tangent);
        assert!(dot(normal, tangent).abs() < 1e-12);
        transform[10] = 0.;
        assert!(transform_normal(&transform, [0., 0., 1.]).is_err());
    }
    #[test]
    fn invalid_reference_and_transform_fail_without_changing_resources() {
        let mut model = model();
        model.instances[0].material = Some("missing".into());
        assert!(model.scene(0).unwrap_err().contains("missing material"));
        model.instances[0].material = None;
        model.instances[0].transform[15] = 2.;
        assert!(model.scene(0).unwrap_err().contains("affine"));
        assert_eq!(model.meshes["triangle"].indices, [0, 1, 2]);
    }
}
