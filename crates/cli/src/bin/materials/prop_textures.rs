//! Filesystem publication of the theme's authored prop atlases.
use std::{fs, path::Path};
use superspec_engine::worldspec::themes::kowloon::authoring::prop_textures;

pub(super) fn bake(output: &Path) -> Result<(), String> {
    fs::create_dir(output).map_err(|error| error.to_string())?;
    fs::write(output.join("cloth.png"), prop_textures::cloth()?)
        .map_err(|error| error.to_string())?;
    fs::write(output.join("aircon.png"), prop_textures::aircon()?)
        .map_err(|error| error.to_string())
}
