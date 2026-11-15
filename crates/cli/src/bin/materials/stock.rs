use std::path::Path;
use superspec_engine::worldspec::themes::kowloon::authoring::stock;

pub(super) fn bake(carton: bool, root: &Path, out: &Path) -> Result<(), String> {
    bake_from(carton, root, &super::material_library::directory()?, out)
}
pub(super) fn bake_from(
    carton: bool,
    root: &Path,
    textures: &Path,
    out: &Path,
) -> Result<(), String> {
    super::publish(
        out,
        stock::bake_from(&super::inputs(), carton, root, textures)?,
    )
}
