//! Applies completed material charts to item receiver meshes.
use super::mesh::Mesh;
use crate::materialspec::bake::BakedMaterial;

impl BakedMaterial {
    /// Maps chart coordinates into padded texture coordinates without changing physical vertices.
    /// Validate the complete slot before changing any UV; malformed drawings remain untouched.
    pub fn map_uvs(&self, drawing: &mut Mesh<'_>, slot: &str) -> Result<(), String> {
        if drawing.surveying()
            || drawing.indices.len() / 3 != drawing.triangle_surfaces.len()
            || !drawing.indices.len().is_multiple_of(3)
        {
            return Err("Raster binding requires complete render triangles.".into());
        }
        let mut selected = std::collections::BTreeSet::new();
        let mut other = std::collections::BTreeSet::new();
        for (triangle, surface) in drawing
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .zip(&drawing.triangle_surfaces)
        {
            let surface = drawing
                .surfaces
                .get(*surface)
                .ok_or("Unknown drawing surface.")?;
            for index in triangle {
                if *index as usize >= drawing.vertices.len() {
                    return Err("Raster binding index exceeds the vertex range.".into());
                }
                if surface.slot == slot {
                    selected.insert(*index as usize);
                } else {
                    other.insert(*index as usize);
                }
            }
        }
        if selected.is_empty() || selected.iter().any(|i| other.contains(i)) {
            return Err("Raster binding needs an occupied slot with exclusive vertices.".into());
        }
        let mut mapped = Vec::with_capacity(selected.len());
        for index in selected {
            let uv = self.layout().uv(drawing.vertices[index].uv);
            if uv
                .iter()
                .any(|v| !v.is_finite() || !(*v as f32).is_finite())
            {
                return Err("Raster binding has nonfinite coordinates.".into());
            }
            mapped.push((index, uv));
        }
        for (index, uv) in mapped {
            drawing.vertices[index].uv = uv;
        }
        Ok(())
    }
}
