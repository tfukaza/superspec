use std::{fs, io::Write, path::Path};
use superspec_engine::worldspec::themes::kowloon::authoring::bags;

pub(super) fn bake(source: &Path, out: &Path) -> Result<(), String> {
    let json = bags::bake(&fs::read(source).map_err(|e| e.to_string())?)?;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)
        .and_then(|mut file| file.write_all(&json))
        .map_err(|e| e.to_string())
}
