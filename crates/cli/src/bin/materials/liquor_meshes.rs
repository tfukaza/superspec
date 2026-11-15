use std::{fs, path::Path};
use superspec_engine::worldspec::themes::kowloon::authoring::liquor_meshes;

pub(super) fn finish(source: &Path, output: &Path) -> Result<(), String> {
    let bytes = liquor_meshes::finish(&fs::read(source).map_err(|e| e.to_string())?)?;
    fs::write(output, bytes).map_err(|e| e.to_string())
}
pub(super) fn prepare(source: &Path, output: &Path) -> Result<(), String> {
    let bytes = liquor_meshes::prepare(&fs::read(source).map_err(|e| e.to_string())?)?;
    fs::write(output, bytes).map_err(|e| e.to_string())
}
