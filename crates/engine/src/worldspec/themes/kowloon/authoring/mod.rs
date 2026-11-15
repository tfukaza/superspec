//! Offline authoring of Kowloon recipes and item catalogs.
pub mod intrinsic;
pub mod kit;
pub mod prop_textures;
use superspec_geometry::image::{self, Image};
pub mod bags;
pub mod liquor_meshes;

pub mod appearances;
pub mod clean_stock;
pub mod refine;
pub mod stock;

use std::path::Path;
pub type Files = Vec<(String, Vec<u8>)>;

/// Source access is supplied by the host; recipe code has no filesystem or environment access.
pub struct Inputs<'a> {
    pub read: &'a dyn Fn(&Path) -> Result<Vec<u8>, String>,
    pub exists: &'a dyn Fn(&Path) -> bool,
    pub catalog: &'a dyn Fn(&str) -> Result<Vec<u8>, String>,
}
impl Inputs<'_> {
    fn decode(&self, path: &Path) -> Result<Image, String> {
        image::decode(&(self.read)(path)?)
    }
}
use crate::materialspec::{
    authoring::sampling::mix,
    noise::{lattice_hash, smooth_step, value_noise},
};
pub mod atlas;
pub mod stock_meshes;
