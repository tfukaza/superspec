use std::path::Path;
use superspec_engine::worldspec::themes::kowloon::authoring::appearances;

pub(super) fn bake(
    root: &Path,
    textures: &Path,
    out: &Path,
    pouches_only: bool,
) -> Result<(), String> {
    super::publish(
        out,
        appearances::bake(&super::inputs(), root, textures, pouches_only)?,
    )
}
