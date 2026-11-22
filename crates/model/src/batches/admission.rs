//! Batch validation shared by persistent world admission and buffer delivery.
use super::{
    Domain, InstanceBatch, LocalMesh, MAX_ATTRIBUTES, MAX_BATCH_INDICES, MAX_BATCH_VERTICES,
    MAX_BATCHES, MAX_INSTANCES, Scene, V3, bounds, placed_corners,
};
use std::collections::BTreeMap;
use superspec_types::limits::MAX_BATCH_ATTRIBUTE_FLOATS;

#[derive(Clone, Copy, Default, Debug)]
pub(crate) struct BatchUsage {
    batches: usize,
    instances: usize,
    pub(crate) vertices: usize,
    pub(crate) indices: usize,
    attribute_floats: usize,
}

impl BatchUsage {
    pub(crate) fn scene(scene: &Scene) -> Self {
        let counts = scene.batches.counts();
        Self {
            batches: scene.manifest.batches.len(),
            instances: counts.instances,
            vertices: counts.vertices,
            indices: counts.indices,
            attribute_floats: counts.attributes,
        }
    }

    pub(crate) fn source(batch: &InstanceBatch, mesh: &LocalMesh) -> Self {
        Self {
            batches: 1,
            instances: batch.placements.len(),
            vertices: mesh.positions.len(),
            indices: mesh.indices.len(),
            attribute_floats: batch
                .named_attributes()
                .fold(0usize, |total, (_, _, _, values)| {
                    total.saturating_add(values.len())
                }),
        }
    }

    pub(crate) fn plus(self, other: Self) -> Self {
        Self {
            batches: self.batches.saturating_add(other.batches),
            instances: self.instances.saturating_add(other.instances),
            vertices: self.vertices.saturating_add(other.vertices),
            indices: self.indices.saturating_add(other.indices),
            attribute_floats: self.attribute_floats.saturating_add(other.attribute_floats),
        }
    }

    pub(crate) fn fits(self) -> bool {
        self.batches <= MAX_BATCHES
            && self.instances <= MAX_INSTANCES
            && self.vertices <= MAX_BATCH_VERTICES
            && self.indices <= MAX_BATCH_INDICES
            && self.attribute_floats <= MAX_BATCH_ATTRIBUTE_FLOATS
    }
}

type Attributes<'a> = (
    Domain,
    &'a BTreeMap<String, Vec<f32>>,
    &'a BTreeMap<String, Vec<[f32; 3]>>,
);

impl InstanceBatch {
    fn domains(&self) -> [Attributes<'_>; 2] {
        [
            (Domain::Instance, &self.scalars, &self.vectors),
            (Domain::Vertex, &self.vertex_scalars, &self.vertex_vectors),
        ]
    }

    pub(super) fn named_attributes(
        &self,
    ) -> impl Iterator<Item = (Domain, &String, usize, &[f32])> {
        self.domains()
            .into_iter()
            .flat_map(|(domain, scalars, vectors)| {
                scalars
                    .iter()
                    .map(move |(name, v)| (domain, name, 1, v.as_slice()))
                    .chain(
                        vectors
                            .iter()
                            .map(move |(name, v)| (domain, name, 3, v.as_flattened())),
                    )
            })
    }

    /// The caller validates the local mesh resource. Bounds use the delivered single precision.
    pub(crate) fn validate_for_mesh(&self, mesh: &LocalMesh) -> Result<([V3; 2], [V3; 2]), String> {
        let count = self.placements.len();
        if self.id.is_empty()
            || self.id.len() > 4096
            || self.object_id.is_empty()
            || self.object_id.len() > 4096
        {
            return Err(format!(
                "Batch identity {:?} is invalid or already used by a batch or mesh.",
                self.id
            ));
        }
        let fail = |message: &str| format!("Batch {}: {message}", self.id);
        if self
            .template
            .as_ref()
            .is_some_and(|id| id.is_empty() || id.len() > 4096)
        {
            return Err(fail("its template mesh identity is invalid."));
        }
        if let Some(foliage) = &self.foliage {
            foliage.validate().map_err(|message| fail(&message))?;
        }
        for (_, scalars, vectors) in self.domains() {
            if scalars.len() + vectors.len() > MAX_ATTRIBUTES
                || scalars.keys().any(|name| vectors.contains_key(name))
            {
                return Err(fail(
                    "it has too many named attributes, or one name for a scalar and a vector.",
                ));
            }
        }
        for (domain, name, components, values) in self.named_attributes() {
            let (expected, element) = match domain {
                Domain::Instance => (count, "instance"),
                Domain::Vertex => (mesh.positions.len(), "vertex"),
            };
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
                    "attribute {name} needs one finite value per {element}."
                )));
            }
        }
        if count == 0 {
            return Err(fail("it has no instance."));
        }
        if (!self.values.is_empty() && self.values.len() != count)
            || (!self.tints.is_empty() && self.tints.len() != count)
        {
            return Err(fail(
                "per-instance attributes must be given for every instance or for none.",
            ));
        }
        if self.values.iter().flatten().any(|v| !v.is_finite())
            || self
                .tints
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || *v < 0.)
        {
            return Err(fail(
                "instance values must be finite and tints finite and nonnegative.",
            ));
        }
        for (index, placement) in self.placements.iter().enumerate() {
            placement
                .validate()
                .map_err(|message| fail(&format!("instance {index}: {message}")))?;
        }
        let local_bounds = bounds(
            mesh.positions
                .iter()
                .map(|p| p.map(|v| f64::from(v as f32))),
        );
        let world = bounds(
            self.placements
                .iter()
                .flat_map(|p| placed_corners(&local_bounds, &p.delivered())),
        );
        if world
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 1e9)
        {
            return Err(fail("an instance leaves the supported coordinate range."));
        }
        Ok((local_bounds, world))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combined_batch_limits_include_templates_and_all_placements() {
        let full = BatchUsage {
            batches: MAX_BATCHES,
            instances: MAX_INSTANCES,
            vertices: MAX_BATCH_VERTICES,
            indices: MAX_BATCH_INDICES,
            attribute_floats: MAX_BATCH_ATTRIBUTE_FLOATS,
        };
        assert!(full.fits());
        for extra in [
            BatchUsage {
                batches: 1,
                ..Default::default()
            },
            BatchUsage {
                instances: 1,
                ..Default::default()
            },
            BatchUsage {
                vertices: 1,
                ..Default::default()
            },
            BatchUsage {
                indices: 1,
                ..Default::default()
            },
            BatchUsage {
                attribute_floats: 1,
                ..Default::default()
            },
        ] {
            assert!(!full.plus(extra).fits());
        }
    }
}
