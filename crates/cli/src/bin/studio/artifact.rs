use std::{fs::File, io, path::Path};

pub fn artifact(root: &Path, uri: &str) -> io::Result<(File, &'static str)> {
    let path = super::files::confined(root, uri)?;
    let mime = match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "glb" => "model/gltf-binary",
        "gltf" => "model/gltf+json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ktx2" => "image/ktx2",
        "bin" | "packet" | "scene" => "application/octet-stream",
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Unsupported artifact",
            ));
        }
    };
    let file = File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Expected an artifact file",
        ));
    }
    Ok((file, mime))
}
