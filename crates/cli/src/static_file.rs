use std::{
    fs::File,
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

/// Checks both URI spelling and canonical containment, including symlink/junction targets.
pub fn confined(root: &Path, uri: &str) -> io::Result<PathBuf> {
    let uri = uri.split('?').next().unwrap_or(uri);
    let name = if uri == "/" {
        "preview.html"
    } else {
        uri.strip_prefix('/').unwrap_or("")
    };
    if name.is_empty()
        || name.contains(['%', '\\', ':', '?', '#'])
        || name
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || !Path::new(name)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Invalid static path",
        ));
    }
    let path = root.join(name).canonicalize()?;
    if !path.starts_with(root) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Static path escapes UI",
        ));
    }
    Ok(path)
}

pub fn static_file(root: &Path, uri: &str) -> io::Result<(Vec<u8>, &'static str)> {
    let path = confined(root, uri)?;
    let mime = match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
    {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript",
        "css" => "text/css",
        "wasm" => "application/wasm",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Unsupported static file",
            ));
        }
    };
    let file = File::open(path)?;
    const MAX: u64 = 32 * 1024 * 1024;
    if !file.metadata()?.is_file() || file.metadata()?.len() > MAX {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Static file exceeds budget",
        ));
    }
    let mut bytes = vec![];
    file.take(MAX + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Static file exceeds budget",
        ));
    }
    Ok((bytes, mime))
}
