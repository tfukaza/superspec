//! Native input and publication adapter for portable item/map authoring.
use super::{Image, fs};
use std::path::Path;
use superspec_engine::{itemspec::embedded_images, worldspec::themes::kowloon::authoring::atlas};

pub(super) fn extract_maps(source: &Path, stem: &str, out: &Path) -> Result<(), String> {
    let bytes = fs::read(source).map_err(|e| e.to_string())?;
    embedded_images::extract(&bytes, stem, &mut |name, bytes| {
        let path = out.join(name);
        if let Some(bytes) = bytes {
            fs::write(path, bytes).map_err(|e| e.to_string())
        } else if path.is_file() {
            fs::remove_file(path).map_err(|e| e.to_string())
        } else {
            Ok(())
        }
    })
}
pub(super) fn balcony(source: &Path, cotton: &Image, out: &Path) -> Result<(), String> {
    super::publish(out, atlas::balcony(&super::inputs(), source, cotton)?)
}
pub(super) fn pack(source: &Path, out: &Path, food: bool) -> Result<(), String> {
    super::publish(out, atlas::pack(&super::inputs(), source, food)?)
}
