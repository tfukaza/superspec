use super::image;
use std::{fs, path::Path};

#[path = "kit/lettering.rs"]
mod lettering;
use superspec_engine::worldspec::themes::kowloon::authoring::kit::surfaces;

const RECIPES: [&str; 6] = ["lettering", "metal", "sheet", "timber", "signboard", "rust"];

pub(super) fn bake(
    output: &Path,
    font_dir: Option<&Path>,
    names: &[String],
    extra: &str,
    extend: Option<&Path>,
) -> Result<(), String> {
    let mut selected = if names.is_empty() {
        RECIPES.to_vec()
    } else {
        names.iter().map(String::as_str).collect()
    };
    for name in &selected {
        if !RECIPES.contains(name) {
            return Err(format!(
                "Unknown Kowloon kit texture {name:?}. Choose from {}.",
                RECIPES.join(", ")
            ));
        }
    }
    if (!extra.is_empty() || extend.is_some()) && !selected.contains(&"lettering") {
        return Err("Extra glyphs require the lettering recipe.".into());
    }
    // Preserve the requested order but write each image only once.
    let mut seen = std::collections::BTreeSet::new();
    selected.retain(|name| seen.insert(*name));
    if output.exists() {
        return Err(format!("Output already exists: {}", output.display()));
    }
    let lettering = selected
        .contains(&"lettering")
        .then(|| {
            lettering::bake(
                font_dir.unwrap_or_else(|| Path::new("generated/kowloon-kit-fonts")),
                extra,
                extend,
            )
        })
        .transpose()?;
    fs::create_dir(output).map_err(|e| format!("{}: {e}", output.display()))?;
    for name in selected {
        if name == "lettering" {
            let (png, layout) = lettering.as_ref().expect("lettering was selected");
            fs::write(output.join("lettering.png"), png).map_err(|e| e.to_string())?;
            fs::write(output.join("atlas.json"), layout).map_err(|e| e.to_string())?;
        } else {
            let rendered = surfaces::bake(name);
            fs::write(
                output.join(format!("{name}.png")),
                image::encode(&rendered)?,
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
