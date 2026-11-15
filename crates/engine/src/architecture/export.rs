//! Explicit retention of closed source solids for brush-based formats.
use super::*;
use crate::{ExportFace, ExportSolid};

impl Compiler<'_> {
    pub(super) fn export_solids(
        &mut self,
        product: &Product,
    ) -> Result<Vec<ExportSolid>, Diagnostic> {
        let root = &self.resolution.document.root;
        if !product.patches.is_empty() {
            return Err(root.fail("BSP_SURFACE", "Brush delivery requires closed solids. Open collision or water surfaces require a mesh export."));
        }
        let mut result = vec![];
        for solid in &product.solids {
            if (self.cancelled)() {
                return Err(self
                    .resolution
                    .document
                    .root
                    .fail("CANCELLED", "Brush retention cancelled."));
            }
            let raw = owned_geometry(std::slice::from_ref(solid), &[]);
            let mut geometry = Geometry::default();
            let mut faces = vec![];
            // Invisible physical faces still close the brush and use its skip material.
            for (&owner, mesh) in &raw {
                let offset = geometry.points.len();
                geometry.points.extend_from_slice(&mesh.points);
                geometry
                    .triangles
                    .extend(mesh.triangles.iter().map(|t| t.map(|i| i + offset)));
                let surface = &self.surfaces[&owner];
                if !surface.visible {
                    for t in &mesh.triangles {
                        faces.push(ExportFace {
                            points: t.map(|i| mesh.points[i]),
                            material: surface.material.clone(),
                            visible: false,
                            collidable: surface.collidable,
                            dynamic: self.resolution.nodes[surface.node]
                                .source
                                .boolean("dynamic", false),
                            object: self.resolution.nodes[surface.node].id.clone(),
                        });
                    }
                }
            }
            for (owner, mesh) in self.paint(raw)? {
                let surface = &self.surfaces[&owner];
                for t in mesh.triangles {
                    faces.push(ExportFace {
                        points: t.map(|i| mesh.points[i]),
                        material: surface.material.clone(),
                        visible: surface.visible,
                        collidable: surface.collidable,
                        dynamic: self.resolution.nodes[surface.node]
                            .source
                            .boolean("dynamic", false),
                        object: self.resolution.nodes[surface.node].id.clone(),
                    });
                }
            }
            result.push(ExportSolid { geometry, faces });
        }
        Ok(result)
    }
}
