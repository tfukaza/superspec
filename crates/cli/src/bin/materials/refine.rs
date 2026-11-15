use std::path::Path;
use superspec_engine::worldspec::themes::kowloon::authoring::refine;

pub(super) fn bake(root: &Path, source: &Path, out: &Path) -> Result<(), String> {
    super::publish(out, refine::bake(&super::inputs(), root, source)?)
}
