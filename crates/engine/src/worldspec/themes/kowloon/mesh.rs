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

pub(crate) struct OwnedMesh {
    geometry: geometry::OwnedMesh,
    identity: Arc<()>,
    weathering: super::weathered::Collection,
}

impl OwnedMesh {
    pub(crate) fn bind(self, cancelled: &dyn Fn() -> bool) -> Mesh<'_> {
        Mesh {
            geometry: self.geometry.bind(cancelled),
            identity: self.identity,
            weathering: self.weathering,
        }
    }
}

impl<'a> Mesh<'a> {
    pub fn surveying(&self) -> bool {
        self.geometry.surveying()
    }
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
        })
    }
    pub(crate) fn into_owned(self) -> OwnedMesh {
        OwnedMesh {
            geometry: self.geometry.into_owned(),
            identity: self.identity,
            weathering: self.weathering,
        }
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
