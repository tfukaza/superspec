use std::{fs, path::Path};
use superspec_engine::worldspec::themes::site_textures;

pub(super) fn bake(output: &Path) -> Result<(), String> {
    fs::create_dir(output).map_err(|e| e.to_string())?;
    for (style, kinds) in site_textures::COLLECTIONS {
        for kind in kinds {
            fs::write(
                output.join(format!("{style}-{kind}.png")),
                site_textures::texture(style, kind)?,
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
