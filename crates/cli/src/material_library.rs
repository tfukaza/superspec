use std::path::{Path, PathBuf};

pub(crate) fn directory() -> Result<PathBuf, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let packaged = executable
        .parent()
        .ok_or("Executable has no directory")?
        .join("materials");
    if packaged.is_dir() {
        return Ok(packaged);
    }
    Ok(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/materials"))
}
