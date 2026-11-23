//! Source geometry is counted once. Each node with a mesh reference also consumes
//! placed geometry, including when it reuses a previously counted source.
use superspec_types::limits::MAX_MODEL_VERTICES;

pub(super) const MAX_SOURCE_VERTICES: usize = 500_000;
pub(super) const MAX_SOURCE_TRIANGLES: usize = 250_000;
pub(super) const MAX_SOURCE_INDICES: usize = 3 * MAX_SOURCE_TRIANGLES;
const MAX_PLACED_TRIANGLES: usize = 1_000_000;

#[derive(Default)]
pub(super) struct PlacedGeometry {
    pub(super) vertices: usize,
    pub(super) triangles: usize,
}
impl PlacedGeometry {
    pub(super) fn add(
        &mut self,
        owner: &str,
        vertices: usize,
        triangles: usize,
    ) -> Result<(), String> {
        let next_vertices = self.vertices.checked_add(vertices).filter(|n| *n <= MAX_MODEL_VERTICES)
            .ok_or_else(|| format!("Placed item vertex allowance exceeded at {owner} ({} + {vertices} vertices, limit {MAX_MODEL_VERTICES}).", self.vertices))?;
        let next_triangles = self.triangles.checked_add(triangles).filter(|n| *n <= MAX_PLACED_TRIANGLES)
            .ok_or_else(|| format!("Placed item triangle allowance exceeded at {owner} ({} + {triangles} triangles, limit {MAX_PLACED_TRIANGLES}).", self.triangles))?;
        self.vertices = next_vertices;
        self.triangles = next_triangles;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::itemspec::definition::{ItemGeometry, ItemMesh, ItemNode, identity};

    #[test]
    fn reused_mesh_crosses_a_million_placed_vertices_in_bounds_and_flat_output() {
        let vertices = 333_975;
        let mut positions = vec![[0., 0., 0.]; vertices];
        positions[1] = [1., 0., 0.];
        positions[2] = [0., 1., 0.];
        let mut geometry = ItemGeometry {
            nodes: vec![ItemNode {
                id: "kit".into(),
                meshes: vec![0, 0, 0],
                transform: identity(),
                parent: None,
                name: None,
                extras: None,
            }],
            meshes: vec![ItemMesh {
                id: "panel".into(),
                positions,
                normals: vec![[0., 0., 1.]; vertices],
                uvs: vec![[0., 0.]; vertices],
                additional_uvs: Default::default(),
                colors: vec![],
                tangents: vec![],
                indices: vec![0, 1, 2],
                material_slot: "concrete".into(),
                roles: vec![],
            }],
            bounds: Default::default(),
            imported_materials: Default::default(),
            images: vec![],
            parts: vec![],
            features: vec![],
            anchors: vec![],
            prototypes: Default::default(),
            placements: vec![],
        };
        geometry.measure().unwrap();
        assert_eq!(geometry.bounds, [[0., 0., 0.], [1., 1., 0.]]);
        let flat = geometry.flattened().unwrap();
        assert_eq!(
            flat.iter().map(|part| part.positions.len()).sum::<usize>(),
            1_001_925
        );
        drop(flat);
        geometry.nodes[0].meshes.extend([0, 0]);
        assert!(geometry.measure().unwrap_err().contains("vertex allowance"));
    }

    #[test]
    fn all_item_placements_share_the_model_ceiling_and_keep_a_separate_triangle_budget() {
        let mut placed = PlacedGeometry::default();
        placed.add("previous floors", 999_553, 500_000).unwrap();
        placed.add("remaining pavement", 2_372, 1_000).unwrap();
        assert_eq!(placed.vertices, 1_001_925);
        placed
            .add(
                "final floor",
                MAX_MODEL_VERTICES - placed.vertices,
                MAX_PLACED_TRIANGLES - placed.triangles,
            )
            .unwrap();
        assert!(placed.add("tower", 1, 0).unwrap_err().contains("tower"));
        assert!(placed.add("trim", 0, 1).unwrap_err().contains("triangle"));
        assert!(placed.add("overflow", usize::MAX, 0).is_err());
        assert_eq!(
            (placed.vertices, placed.triangles),
            (MAX_MODEL_VERTICES, MAX_PLACED_TRIANGLES)
        );
    }
}
