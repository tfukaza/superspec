use std::path::Path;
use superspec_engine::worldspec::themes::kowloon::authoring::clean_stock;

pub(super) fn bake(root: &Path, textures: &Path, out: &Path) -> Result<(), String> {
    super::publish(out, clean_stock::bake(&super::inputs(), root, textures)?)
}
