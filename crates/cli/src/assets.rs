use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

/// Resolves shipped images outside the author's namespace. Installed releases use sibling assets.
fn library_path(name: &str) -> Result<PathBuf, String> {
    let file = superspec_catalog::image_name(name)
        .ok_or_else(|| format!("Unknown library image {name}."))?;
    Ok(crate::material_library::directory()?.join(file))
}

/// Preview dependencies must stay inside the source directory, including resolved links.
pub fn read_confined(parent: &Path, name: &str) -> Result<Vec<u8>, String> {
    if name.starts_with("superspec-material:") {
        return read(parent, name);
    }
    if name.is_empty()
        || name.contains(['\\', ':'])
        || name
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(format!("Invalid companion path {name}."));
    }
    let root = parent.canonicalize().map_err(|error| error.to_string())?;
    let path = parent
        .join(name)
        .canonicalize()
        .map_err(|error| format!("Cannot read asset {name}: {error}"))?;
    if !path.starts_with(&root) {
        return Err(format!("Companion {name} escapes the source directory."));
    }
    read_path(&path, name)
}

/// A single bounded read for every native companion. Library IDs cannot be shadowed by local files.
pub fn read(parent: &Path, name: &str) -> Result<Vec<u8>, String> {
    let path = if name.starts_with("superspec-material:") {
        library_path(name)?
    } else {
        parent.join(name)
    };
    read_path(&path, name)
}

/// Reads the already-resolved target; callers choose the namespace and containment policy.
fn read_path(path: &Path, name: &str) -> Result<Vec<u8>, String> {
    let mut bytes = vec![];
    File::open(path)
        .and_then(|file| {
            file.take(superspec_types::limits::MAX_FILE_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
        })
        .map_err(|e| format!("Cannot read asset {name}: {e}"))?;
    if bytes.len() > superspec_types::limits::MAX_FILE_BYTES {
        return Err(format!(
            "Asset {name} exceeds the {} MiB file limit.",
            superspec_types::limits::MAX_FILE_BYTES >> 20
        ));
    }
    Ok(bytes)
}
