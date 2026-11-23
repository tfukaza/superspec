//! Theme planning state around the shared ItemSpec mesh.
use crate::itemspec::mesh as geometry;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use superspec_model::visual::VisualInput;

pub(crate) use geometry::BoundMesh;
pub use geometry::{Mode, RasterBinding, Surface, Vertex, rounded};

pub struct Mesh<'a> {
    geometry: geometry::Mesh<'a>,
    pub(super) identity: Arc<()>,
    pub(super) weathering: super::weathered::Collection,
    graphics: super::graphics_detail::GraphicsDetail,
}

impl VisualInput for Mesh<'_> {
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

impl<'a> Mesh<'a> {
    pub fn surveying(&self) -> bool {
        self.geometry.surveying()
    }
    #[cfg(test)]
    pub(crate) fn into_survey(self) -> Vec<superspec_geometry::V3> {
        self.geometry.survey
    }
    pub fn new(
        mode: Mode,
        maximum_triangles: usize,
        cancelled: &'a dyn Fn() -> bool,
    ) -> Result<Self, String> {
        Ok(Self {
            geometry: geometry::Mesh::new(mode, maximum_triangles, cancelled)?,
            identity: Arc::new(()),
            weathering: Default::default(),
            graphics: Default::default(),
        })
    }
    /// How much geometry fixtures draw into this mesh. A survey always reads level 0, so
    /// clearance and sign planning see the authored fixture at every level.
    pub fn graphics_detail(&self) -> super::graphics_detail::GraphicsDetail {
        if self.surveying() {
            Default::default()
        } else {
            self.graphics
        }
    }
    pub fn set_graphics_detail(&mut self, detail: super::graphics_detail::GraphicsDetail) {
        self.graphics = detail;
    }
}

impl<'a> Deref for Mesh<'a> {
    type Target = geometry::Mesh<'a>;
    fn deref(&self) -> &Self::Target {
        &self.geometry
    }
}
impl DerefMut for Mesh<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.geometry
    }
}
