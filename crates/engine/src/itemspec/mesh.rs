//! Owned visual triangles for procedural producers; never collision or navigation geometry.
use std::collections::BTreeMap;
use superspec_geometry::V3;

pub use superspec_model::visual::{Surface, Vertex};

fn same_vertex(vertex: Vertex, other: Vertex) -> bool {
    vertex
        .position
        .into_iter()
        .chain(vertex.normal)
        .chain(vertex.uv)
        .chain(vertex.color)
        .zip(
            other
                .position
                .into_iter()
                .chain(other.normal)
                .chain(other.uv)
                .chain(other.color),
        )
        .all(|(a, b)| a.to_bits() == b.to_bits())
}
fn rounded_vertex(mut vertex: Vertex) -> Result<Vertex, String> {
    if vertex
        .position
        .iter()
        .chain(&vertex.normal)
        .chain(&vertex.uv)
        .chain(&vertex.color)
        .any(|v| !v.is_finite())
    {
        return Err("Procedural vertex attributes must be finite.".into());
    }
    vertex.position = vertex.position.map(|v| rounded(v, 1e5));
    vertex.normal = vertex.normal.map(|v| rounded(v, 1e4));
    vertex.color = vertex.color.map(|v| rounded(v, 1e3));
    if vertex
        .position
        .iter()
        .chain(&vertex.normal)
        .chain(&vertex.color)
        .any(|v| !v.is_finite())
    {
        return Err("Procedural vertex quantization overflow.".into());
    }
    Ok(vertex)
}
/// The procedural mesh's JS rounding contract differs from architectural quantization.
pub fn rounded(value: f64, scale: f64) -> f64 {
    let value = value * scale;
    let lower = value.floor();
    let result = if value - lower < 0.5 {
        lower
    } else {
        lower + 1.
    };
    if result == 0. {
        0.0f64.copysign(value)
    } else {
        result / scale
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    Render,
    Survey,
}

/// An exclusive triangle range receiving one completed raster. Ranges may share a slot/layout.
pub struct RasterBinding<'a> {
    pub first_triangle: usize,
    pub triangle_count: usize,
    pub slot: &'a str,
    pub layout: &'a crate::materialspec::surface::Layout,
}

/// A host's detached output. Survey mode stores only expanded, rounded triangle positions.
pub struct Mesh<'a> {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub surfaces: Vec<Surface>,
    pub triangle_surfaces: Vec<usize>,
    pub survey: Vec<V3>,
    mode: Mode,
    pub(crate) maximum_triangles: usize,
    cancelled: &'a dyn Fn() -> bool,
    surface_lookup: BTreeMap<String, BTreeMap<String, usize>>,
    source_materials: BTreeMap<String, String>,
    identity_bytes: usize,
    work: usize,
}
/// Completed receiver binding, excluding immutable producer planning state.
pub(crate) struct BoundMesh {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    surfaces: Vec<Surface>,
    triangle_surfaces: Vec<usize>,
    surface_lookup: BTreeMap<String, BTreeMap<String, usize>>,
    identity_bytes: usize,
}
impl BoundMesh {
    pub(crate) fn bytes(&self) -> usize {
        self.vertices.len() * std::mem::size_of::<Vertex>()
            + self.indices.len() * 4
            + self.triangle_surfaces.len() * std::mem::size_of::<usize>()
            + self.identity_bytes
            + 512 * self.surfaces.len()
    }
}
impl<'a> Mesh<'a> {
    pub(crate) fn binding_snapshot(&self) -> BoundMesh {
        BoundMesh {
            vertices: self.vertices.clone(),
            indices: self.indices.clone(),
            surfaces: self.surfaces.clone(),
            triangle_surfaces: self.triangle_surfaces.clone(),
            surface_lookup: self.surface_lookup.clone(),
            identity_bytes: self.identity_bytes,
        }
    }

    pub(crate) fn restore_binding(&mut self, bound: &BoundMesh) -> Result<(), String> {
        self.check_work(0)?;
        if bound.indices.len() / 3 > self.maximum_triangles {
            return Err("Retained binding exceeds the current drawing allowance.".into());
        }
        self.vertices.clone_from(&bound.vertices);
        self.indices.clone_from(&bound.indices);
        self.surfaces.clone_from(&bound.surfaces);
        self.triangle_surfaces.clone_from(&bound.triangle_surfaces);
        self.surface_lookup.clone_from(&bound.surface_lookup);
        self.identity_bytes = bound.identity_bytes;
        Ok(())
    }

    pub(crate) fn binding_hash(&self, hash: &mut sha2::Sha256) {
        use sha2::Digest;
        hash.update((self.vertices.len() as u64).to_le_bytes());
        for vertex in &self.vertices {
            for value in vertex
                .position
                .iter()
                .chain(&vertex.normal)
                .chain(&vertex.uv)
                .chain(&vertex.color)
            {
                hash.update(value.to_bits().to_le_bytes());
            }
        }
        hash.update((self.indices.len() as u64).to_le_bytes());
        for value in &self.indices {
            hash.update(value.to_le_bytes());
        }
        hash.update((self.surfaces.len() as u64).to_le_bytes());
        for surface in &self.surfaces {
            for text in [&surface.part, &surface.slot] {
                hash.update((text.len() as u64).to_le_bytes());
                hash.update(text.as_bytes());
            }
        }
        hash.update((self.triangle_surfaces.len() as u64).to_le_bytes());
        for value in &self.triangle_surfaces {
            hash.update((*value as u64).to_le_bytes());
        }
        hash.update((self.source_materials.len() as u64).to_le_bytes());
        for (a, b) in &self.source_materials {
            for text in [a, b] {
                hash.update((text.len() as u64).to_le_bytes());
                hash.update(text.as_bytes());
            }
        }
    }

    /// Work already charged to this detached drawing, for aggregate scene admission.
    pub(crate) fn work_spent(&self) -> usize {
        self.work
    }
    /// Rebind all receivers in one triangle traversal. Nothing changes until the full plan passes.
    pub fn bind_rasters(&mut self, bindings: &[RasterBinding<'_>]) -> Result<(), String> {
        self.check_work(0)?;
        if self.surveying()
            || !self.indices.len().is_multiple_of(3)
            || self.indices.len() / 3 != self.triangle_surfaces.len()
            || bindings.len() > 32768
        {
            return Err("Raster binding requires bounded, complete render triangles.".into());
        }
        let mut ranges = vec![None; self.triangle_count()];
        for (binding_id, binding) in bindings.iter().enumerate() {
            binding.layout.validate()?;
            if binding.slot.is_empty() || binding.slot.len() > 4096 || binding.triangle_count == 0 {
                return Err("Raster binding requires an occupied range and bounded slot.".into());
            }
            let end = binding
                .first_triangle
                .checked_add(binding.triangle_count)
                .filter(|end| *end <= ranges.len())
                .ok_or("Raster range exceeds the drawing.")?;
            for target in &mut ranges[binding.first_triangle..end] {
                if target.replace(binding_id).is_some() {
                    return Err("Raster ranges overlap.".into());
                }
            }
        }
        let mut surfaces = Vec::<Surface>::new();
        let mut lookup = BTreeMap::<String, BTreeMap<String, usize>>::new();
        let mut identity_bytes = 0;
        let mut triangle_surfaces = Vec::with_capacity(ranges.len());
        let mut vertex_owner = vec![None; self.vertices.len()];
        let mut coordinates = Vec::new();
        for (i, triangle) in self.indices.as_chunks::<3>().0.iter().enumerate() {
            if i.is_multiple_of(2048) {
                self.check_work(0)?;
            }
            let old = self
                .surfaces
                .get(self.triangle_surfaces[i])
                .ok_or("Unknown drawing surface.")?;
            let binding = ranges[i].map(|id| &bindings[id]);
            let slot = binding.map_or(old.slot.as_str(), |b| b.slot);
            let surface = if let Some(id) = lookup.get(&old.part).and_then(|s| s.get(slot)) {
                *id
            } else {
                if old.part.len() > 4096 || slot.len() > 4096 {
                    return Err("Raster surface identity exceeds its budget.".into());
                }
                identity_bytes += 2 * (old.part.len() + slot.len()) + 512;
                if surfaces.len() >= 16384 || identity_bytes > 4 * 1024 * 1024 {
                    return Err("Raster surface identity budget exceeded.".into());
                }
                let id = surfaces.len();
                surfaces.push(Surface {
                    part: old.part.clone(),
                    slot: slot.into(),
                });
                lookup
                    .entry(old.part.clone())
                    .or_default()
                    .insert(slot.into(), id);
                id
            };
            triangle_surfaces.push(surface);
            for index in triangle {
                let index = *index as usize;
                let vertex = self
                    .vertices
                    .get(index)
                    .ok_or("Raster binding index exceeds the vertex range.")?;
                // Even equal-looking charts must not share vertices across ownership boundaries.
                let owner = (self.triangle_surfaces[i], ranges[i]);
                match vertex_owner[index] {
                    Some(previous) if previous != owner => {
                        return Err("Raster ranges need exclusive vertices.".into());
                    }
                    Some(_) => {}
                    None => {
                        vertex_owner[index] = Some(owner);
                        if let Some(binding) = binding {
                            let uv = binding.layout.uv(vertex.uv);
                            if uv
                                .iter()
                                .any(|v| !v.is_finite() || !(*v as f32).is_finite())
                            {
                                return Err("Raster binding has nonfinite coordinates.".into());
                            }
                            coordinates.push((index, uv));
                        }
                    }
                }
            }
        }
        self.check_work(0)?;
        for (index, uv) in coordinates {
            self.vertices[index].uv = uv;
        }
        self.surfaces = surfaces;
        self.surface_lookup = lookup;
        self.triangle_surfaces = triangle_surfaces;
        self.identity_bytes = identity_bytes;
        Ok(())
    }
    pub fn new(
        mode: Mode,
        maximum_triangles: usize,
        cancelled: &'a dyn Fn() -> bool,
    ) -> Result<Self, String> {
        if maximum_triangles > 2_000_000 {
            return Err("Procedural triangle limit exceeds 2000000.".into());
        }
        Ok(Self {
            vertices: vec![],
            indices: vec![],
            surfaces: vec![],
            triangle_surfaces: vec![],
            survey: vec![],
            mode,
            maximum_triangles,
            cancelled,
            surface_lookup: BTreeMap::new(),
            source_materials: BTreeMap::new(),
            identity_bytes: 0,
            work: 0,
        })
    }
    pub fn surveying(&self) -> bool {
        self.mode == Mode::Survey
    }
    /// Fixes authored identities before drawing; local slots remain sampling and geometry keys.
    pub fn bind_source_materials(
        &mut self,
        bindings: &BTreeMap<String, String>,
    ) -> Result<(), String> {
        if self.triangle_count() != 0
            || !self.source_materials.is_empty()
            || bindings.len() > 1024
            || bindings.iter().any(|(slot, id)| {
                slot.is_empty() || id.is_empty() || slot.len() > 8192 || id.len() > 4096
            })
            || bindings
                .iter()
                .map(|(slot, id)| slot.len() + id.len())
                .sum::<usize>()
                > 1024 * 1024
        {
            return Err(
                "Source material identities require a fresh drawing and bounded bindings.".into(),
            );
        }
        self.source_materials = bindings.clone();
        Ok(())
    }
    pub fn source_material<'b>(&'b self, slot: &'b str) -> &'b str {
        self.source_materials.get(slot).map_or(slot, String::as_str)
    }
    pub(crate) fn has_source_material(&self, slot: &str) -> bool {
        self.source_materials.contains_key(slot)
    }
    pub fn triangle_count(&self) -> usize {
        if self.surveying() {
            self.survey.len() / 3
        } else {
            self.indices.len() / 3
        }
    }
    /// Bounds attempted facets too: a degenerate producer may emit no triangles at all.
    pub(crate) fn check_work(&self, count: usize) -> Result<(), String> {
        if (self.cancelled)() {
            return Err("Dressing cancelled.".into());
        }
        if self
            .work
            .checked_add(count)
            .is_none_or(|n| n > self.maximum_triangles.max(1) * 8)
        {
            return Err("Dressing attempted-facet work budget exceeded.".into());
        }
        Ok(())
    }
    pub(crate) fn spend_work(&mut self, count: usize) -> Result<(), String> {
        self.check_work(count)?;
        self.work += count;
        Ok(())
    }
    fn admit(&self) -> Result<(), String> {
        if self.triangle_count() >= self.maximum_triangles {
            return Err("Dressing triangle budget exceeded.".into());
        }
        if self.triangle_count().is_multiple_of(2048) && (self.cancelled)() {
            return Err("Dressing cancelled.".into());
        }
        Ok(())
    }
    pub(crate) fn survey_triangle(&mut self, positions: [V3; 3]) -> Result<(), String> {
        self.admit()?;
        let rounded = positions.map(|p| p.map(|v| rounded(v, 1e5)));
        if rounded.iter().flatten().any(|v| !v.is_finite()) {
            return Err("Nonfinite survey triangle.".into());
        }
        self.survey.extend(rounded);
        Ok(())
    }
    /// All three corners are validated before any part of a triangle becomes observable.
    pub(crate) fn triangle(
        &mut self,
        part: &str,
        slot: &str,
        vertices: [Vertex; 3],
        source_indices: [usize; 3],
        shared: Option<&mut [Option<usize>]>,
    ) -> Result<(), String> {
        self.admit()?;
        let [a, b, c] = vertices;
        let vertices = [rounded_vertex(a)?, rounded_vertex(b)?, rounded_vertex(c)?];
        self.append_triangle(part, slot, vertices, source_indices, shared)
    }
    /// Native ground cuts retain their interpolated precision until the render-packet boundary.
    pub(crate) fn triangle_precise(
        &mut self,
        part: &str,
        slot: &str,
        vertices: [Vertex; 3],
    ) -> Result<(), String> {
        self.admit()?;
        if vertices.iter().any(|v| {
            v.position
                .iter()
                .chain(&v.normal)
                .chain(&v.uv)
                .chain(&v.color)
                .any(|v| !v.is_finite())
        }) {
            return Err("Procedural vertex attributes must be finite.".into());
        }
        self.append_triangle(part, slot, vertices, [0, 1, 2], None)
    }
    fn append_triangle(
        &mut self,
        part: &str,
        slot: &str,
        vertices: [Vertex; 3],
        source_indices: [usize; 3],
        shared: Option<&mut [Option<usize>]>,
    ) -> Result<(), String> {
        if part.len() > 4096 || slot.len() > 4096 {
            return Err("Procedural part/slot identity exceeds 4096 bytes.".into());
        }
        let existing = self
            .surface_lookup
            .get(part)
            .and_then(|slots| slots.get(slot))
            .copied();
        let surface = if let Some(index) = existing {
            index
        } else {
            // Includes the owned lookup/output strings and conservative tree/vector entry overhead.
            let charged = 2 * (part.len() + slot.len()) + 512;
            if self.surfaces.len() >= 16384 || self.identity_bytes + charged > 4 * 1024 * 1024 {
                return Err("Dressing surface identity budget exceeded.".into());
            }
            let index = self.surfaces.len();
            self.surfaces.push(Surface {
                part: part.into(),
                slot: slot.into(),
            });
            self.surface_lookup
                .entry(part.into())
                .or_default()
                .insert(slot.into(), index);
            self.identity_bytes += charged;
            index
        };
        let mut shared = shared;
        for (vertex, source) in vertices.into_iter().zip(source_indices) {
            let previous = shared.as_ref().and_then(|s| s[source]);
            if let Some(index) = previous.filter(|i| same_vertex(self.vertices[*i], vertex)) {
                self.indices.push(index as u32);
            } else {
                let index = self.vertices.len();
                self.vertices.push(vertex);
                self.indices.push(index as u32);
                if let Some(ref mut lookup) = shared {
                    lookup[source] = Some(index);
                }
            }
        }
        self.triangle_surfaces.push(surface);
        Ok(())
    }
}

impl superspec_model::visual::VisualInput for Mesh<'_> {
    fn vertices(&self) -> &[Vertex] {
        &self.vertices
    }
    fn indices(&self) -> &[u32] {
        &self.indices
    }
    fn surfaces(&self) -> &[Surface] {
        &self.surfaces
    }
    fn triangle_surfaces(&self) -> &[usize] {
        &self.triangle_surfaces
    }
    fn surveying(&self) -> bool {
        self.surveying()
    }
}
