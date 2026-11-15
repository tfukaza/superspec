//! Inserts existing collinear edge vertices into flat receivers without welding nearby surfaces.
use super::mesh::{Mesh, RasterBinding, Vertex};
use std::collections::{BTreeMap, BTreeSet};
use superspec_geometry::obstructions::{Budget, Triangle};
use superspec_geometry::{
    V3,
    math::{cross, dot, sub},
};
const CELL: f64 = 0.05;
const MAX_POINTS: usize = 1_048_576;
const MAX_PATCH_BYTES: usize = 128 * 1024 * 1024;

// Includes the full retained index/owner tail and commit's remap/surface arrays.
fn patch_storage(
    vertices: usize,
    triangles: usize,
    original_vertices: usize,
    maximum: usize,
) -> Result<(), String> {
    let bytes = vertices
        .checked_mul(std::mem::size_of::<Vertex>() + 4)
        .and_then(|n| {
            triangles
                .checked_mul(12 + 2 * std::mem::size_of::<usize>())
                .and_then(|t| n.checked_add(t))
        })
        .and_then(|n| {
            original_vertices
                .checked_mul(4)
                .and_then(|v| n.checked_add(v))
        });
    if triangles > maximum || bytes.is_none_or(|n| n > MAX_PATCH_BYTES) {
        return Err("Receiver conformance exceeds triangle or temporary storage allowance.".into());
    }
    Ok(())
}

pub struct Edges {
    buckets: BTreeMap<[i32; 3], Vec<V3>>,
    parent: Option<std::sync::Arc<Edges>>,
}
impl Edges {
    /// Only points inside the drawing bounds can lie on one of its receiver edges.
    pub(crate) fn binding_hash(&self, hash: &mut sha2::Sha256, bounds: [V3; 2]) {
        use sha2::Digest;
        hash.update([u8::from(self.parent.is_some())]);
        if let Some(parent) = &self.parent {
            parent.binding_hash(hash, bounds);
        }
        let count = self
            .buckets
            .values()
            .flatten()
            .filter(|point| {
                (0..3).all(|i| point[i] >= bounds[0][i] - 1e-6 && point[i] <= bounds[1][i] + 1e-6)
            })
            .count();
        hash.update((count as u64).to_le_bytes());
        for points in self.buckets.values() {
            for point in points {
                if (0..3)
                    .all(|i| point[i] >= bounds[0][i] - 1e-6 && point[i] <= bounds[1][i] + 1e-6)
                {
                    for value in point {
                        hash.update(value.to_bits().to_le_bytes());
                    }
                }
            }
        }
    }
    pub fn new(
        points: impl IntoIterator<Item = V3>,
        budget: &mut Budget<'_>,
    ) -> Result<Self, String> {
        let mut buckets = BTreeMap::<_, Vec<_>>::new();
        let mut seen = BTreeSet::new();
        for point in points {
            budget.spend(1)?;
            if point.iter().any(|v| !v.is_finite() || v.abs() > 1e6) {
                return Err("Receiver edges require bounded finite points.".into());
            }
            let identity = point.map(|v| if v == 0. { 0 } else { v.to_bits() });
            if !seen.insert(identity) {
                continue;
            }
            if seen.len() > MAX_POINTS {
                return Err("Receiver edge index exceeds its point allowance.".into());
            }
            buckets
                .entry(point.map(|v| (v / CELL).floor() as i32))
                .or_default()
                .push(point);
        }
        Ok(Self {
            buckets,
            parent: None,
        })
    }
    fn contains(&self, point: V3) -> bool {
        self.buckets
            .get(&point.map(|v| (v / CELL).floor() as i32))
            .is_some_and(|points| points.contains(&point))
    }
    fn touches(a: V3, delta: V3, cell: [i32; 3], padding: f64) -> bool {
        let mut enter = 0f64;
        let mut leave = 1f64;
        for axis in 0..3 {
            let low = cell[axis] as f64 * CELL - padding;
            let high = (cell[axis] + 1) as f64 * CELL + padding;
            if delta[axis] == 0. {
                if a[axis] < low || a[axis] > high {
                    return false;
                }
                continue;
            }
            let first = (low - a[axis]) / delta[axis];
            let last = (high - a[axis]) / delta[axis];
            enter = enter.max(first.min(last));
            leave = leave.min(first.max(last));
            if enter > leave {
                return false;
            }
        }
        true
    }
    fn points(&self, a: V3, b: V3, budget: &mut Budget<'_>) -> Result<Vec<V3>, String> {
        let delta = sub(b, a);
        let squared = dot(delta, delta);
        if squared < 1e-20 {
            return Ok(vec![]);
        }
        let length = squared.sqrt();
        let scale = a
            .iter()
            .chain(&b)
            .fold(length.max(1.), |n, v| n.max(v.abs()));
        let tolerance = f64::EPSILON * scale * 64.;
        let stations = (length / CELL).ceil().max(1.) as usize;
        budget.spend(
            stations
                .checked_add(1)
                .and_then(|n| n.checked_mul(27))
                .ok_or("Receiver edge station overflow.")?,
        )?;
        let mut visited = BTreeSet::new();
        let mut found = vec![];
        for station in 0..=stations {
            budget.spend(0)?;
            let cell: [i32; 3] = std::array::from_fn(|i| {
                ((a[i] + (b[i] - a[i]) * station as f64 / stations as f64) / CELL).floor() as i32
            });
            for x in -1..=1 {
                for y in -1..=1 {
                    for z in -1..=1 {
                        let neighbor = [cell[0] + x, cell[1] + y, cell[2] + z];
                        if !Self::touches(a, delta, neighbor, tolerance * 4.)
                            || !visited.insert(neighbor)
                        {
                            continue;
                        }
                        let Some(points) = self.buckets.get(&neighbor) else {
                            continue;
                        };
                        budget.spend(points.len())?;
                        for &point in points {
                            let offset = sub(point, a);
                            let t = dot(offset, delta) / squared;
                            if t <= tolerance / length || t >= 1. - tolerance / length {
                                continue;
                            }
                            let distance: f64 = (0..3)
                                .map(|i| (point[i] - a[i] - t * delta[i]).powi(2))
                                .sum();
                            if distance <= tolerance * tolerance {
                                found.push((t, point));
                            }
                        }
                    }
                }
            }
        }
        if let Some(parent) = &self.parent {
            found.extend(
                parent
                    .points(a, b, budget)?
                    .into_iter()
                    .map(|point| (dot(sub(point, a), delta) / squared, point)),
            );
        }
        found.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut previous = f64::NEG_INFINITY;
        Ok(found
            .into_iter()
            .filter_map(|(t, point)| {
                if (t - previous) * length <= tolerance {
                    None
                } else {
                    previous = t;
                    Some(point)
                }
            })
            .collect())
    }
    pub fn triangles(
        &self,
        face: Triangle,
        budget: &mut Budget<'_>,
    ) -> Result<Vec<Triangle>, String> {
        budget.spend(1)?;
        if face
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 1e6)
        {
            return Err("Receiver face requires bounded finite points.".into());
        }
        let mut triangles = vec![face];
        for edge in 0..3 {
            let a = face[edge];
            let b = face[(edge + 1) % 3];
            let interior = self.points(a, b, budget)?;
            if interior.is_empty() {
                continue;
            }
            let index = triangles
                .iter()
                .position(|t| t.contains(&a) && t.contains(&b))
                .ok_or("Receiver lost an original boundary.")?;
            let opposite = *triangles[index]
                .iter()
                .find(|p| **p != a && **p != b)
                .ok_or("Receiver boundary is degenerate.")?;
            let mut previous = a;
            for (i, point) in interior.into_iter().chain([b]).enumerate() {
                budget.spend(1)?;
                let triangle = [opposite, previous, point];
                if i == 0 {
                    triangles[index] = triangle;
                } else {
                    triangles.push(triangle);
                }
                previous = point;
            }
        }
        Ok(triangles)
    }
}

fn weights(point: V3, face: Triangle) -> V3 {
    let a = sub(face[1], face[0]);
    let b = sub(face[2], face[0]);
    let p = sub(point, face[0]);
    let n = cross(a, b).map(f64::abs);
    let dropped = (0..3).find(|&i| n[i] == n[0].max(n[1]).max(n[2])).unwrap();
    let x = (dropped + 1) % 3;
    let y = (dropped + 2) % 3;
    let determinant = a[x] * b[y] - a[y] * b[x];
    if determinant.abs() < 1e-24 {
        return [1., 0., 0.];
    }
    let u = (p[x] * b[y] - p[y] * b[x]) / determinant;
    let v = (a[x] * p[y] - a[y] * p[x]) / determinant;
    [1. - u - v, u, v]
}

/// Prepared before raster binding; committing cannot fail after the drawing's slots change.
pub(super) struct Patch {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    owners: Vec<usize>,
}
impl Patch {
    pub fn prepare(
        mesh: &Mesh<'_>,
        bindings: &[RasterBinding<'_>],
        selected: &[usize],
        extra: &[V3],
        shared: Option<std::sync::Arc<Edges>>,
        budget: &mut Budget<'_>,
    ) -> Result<Option<Self>, String> {
        if selected.is_empty() {
            return Ok(None);
        }
        if mesh.surveying()
            || mesh.indices.len() != mesh.triangle_surfaces.len() * 3
            || mesh.triangle_count() > mesh.maximum_triangles
            || mesh.vertices.len() > 6_000_000
            || extra.len() > 6_000_000
        {
            return Err("Receiver conformance requires bounded complete render geometry.".into());
        }
        let mut admitted_triangles = mesh.triangle_count();
        patch_storage(
            0,
            admitted_triangles,
            mesh.vertices.len(),
            mesh.maximum_triangles,
        )?;
        let edges = if let Some(parent) = shared {
            let mut added = vec![];
            for point in mesh
                .vertices
                .iter()
                .map(|v| v.position)
                .chain(extra.iter().copied())
            {
                budget.spend(1)?;
                if !parent.contains(point) {
                    added.push(point);
                }
            }
            let mut index = Edges::new(added, budget)?;
            index.parent = Some(parent);
            index
        } else {
            Edges::new(
                extra
                    .iter()
                    .copied()
                    .chain(mesh.vertices.iter().map(|v| v.position)),
                budget,
            )?
        };
        let mut targets = BTreeMap::new();
        for &id in selected {
            let binding = bindings
                .get(id)
                .ok_or("Unknown conformed raster binding.")?;
            let end = binding
                .first_triangle
                .checked_add(binding.triangle_count)
                .filter(|n| *n <= mesh.triangle_count())
                .ok_or("Conformed raster range exceeds drawing.")?;
            for t in binding.first_triangle..end {
                if targets.insert(t, binding.layout).is_some() {
                    return Err("Conformed raster ranges overlap.".into());
                }
            }
        }
        let mut patch = Self {
            vertices: vec![],
            indices: vec![],
            owners: vec![],
        };
        for (source, indices) in mesh.indices.as_chunks::<3>().0.iter().enumerate() {
            budget.spend(1)?;
            let Some(layout) = targets.get(&source) else {
                patch.indices.extend(indices);
                patch.owners.push(source);
                continue;
            };
            let corners = indices.map(|i| {
                mesh.vertices
                    .get(i as usize)
                    .copied()
                    .ok_or("Receiver index exceeds drawing.")
            });
            let [a, b, c] = corners;
            let corners = [a?, b?, c?];
            let face = corners.map(|v| v.position);
            let triangles = edges.triangles(face, budget)?;
            admitted_triangles += triangles.len().saturating_sub(1);
            patch_storage(
                patch.vertices.len() + triangles.len() * 3,
                admitted_triangles,
                mesh.vertices.len(),
                mesh.maximum_triangles,
            )?;
            for triangle in triangles {
                let normal = cross(sub(triangle[1], triangle[0]), sub(triangle[2], triangle[0]));
                if dot(normal, normal).sqrt() <= 1e-13 {
                    continue;
                }
                for position in triangle {
                    let w = weights(position, face);
                    let normal: V3 = std::array::from_fn(|axis| {
                        (0..3).map(|i| w[i] * corners[i].normal[axis]).sum()
                    });
                    let length = dot(normal, normal).sqrt();
                    let normal = normal.map(|v| v / if length == 0. { 1. } else { length });
                    let uv = layout.uv(std::array::from_fn(|axis| {
                        (0..3).map(|i| w[i] * corners[i].uv[axis]).sum()
                    }));
                    if uv.iter().chain(&normal).any(|v| !v.is_finite()) {
                        return Err("Receiver conformance produced nonfinite attributes.".into());
                    }
                    patch
                        .indices
                        .push((mesh.vertices.len() + patch.vertices.len()) as u32);
                    patch.vertices.push(Vertex {
                        position,
                        normal,
                        uv,
                        color: [1.; 3],
                    });
                }
                patch.owners.push(source);
            }
        }
        if patch.owners.len() > mesh.maximum_triangles {
            return Err("Receiver conformance exceeds triangle allowance.".into());
        }
        budget.spend(mesh.vertices.len() + patch.vertices.len() + patch.indices.len())?;
        Ok(Some(patch))
    }
    pub fn commit(self, mesh: &mut Mesh<'_>) {
        let surfaces = self
            .owners
            .iter()
            .map(|i| mesh.triangle_surfaces[*i])
            .collect();
        mesh.vertices.extend(self.vertices);
        mesh.indices = self.indices;
        mesh.triangle_surfaces = surfaces;
        let mut remap = vec![u32::MAX; mesh.vertices.len()];
        for &index in &mesh.indices {
            remap[index as usize] = 0;
        }
        let mut source = 0;
        let mut kept = 0;
        mesh.vertices.retain(|_| {
            let used = remap[source] != u32::MAX;
            if used {
                remap[source] = kept;
                kept += 1;
            }
            source += 1;
            used
        });
        for index in &mut mesh.indices {
            *index = remap[*index as usize];
        }
    }
}

#[cfg(test)]
#[path = "conform_tests.rs"]
mod tests;
