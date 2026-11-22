use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{self, ErrorKind, Read, Write},
    path::{Path, PathBuf},
};

const GENERATIONS: &str = "superspec-generations";

pub(super) struct Generation {
    pub(super) relative: String,
    unpublished: Option<PathBuf>,
}
impl Generation {
    pub(super) fn commit(&mut self) {
        self.unpublished = None;
    }
}
impl Drop for Generation {
    fn drop(&mut self) {
        if let Some(path) = &self.unpublished {
            let _ = fs::remove_dir_all(path);
        }
    }
}

fn transfer(path: &Path, writer: &mut impl Write, cancelled: &dyn Fn() -> bool) -> io::Result<()> {
    let mut source = File::open(path)?;
    let mut chunk = vec![0; 256 * 1024];
    loop {
        if cancelled() {
            return Err(io::Error::other("Build cancelled."));
        }
        let size = source.read(&mut chunk)?;
        if size == 0 {
            return Ok(());
        }
        writer.write_all(&chunk[..size])?;
    }
}

fn fingerprint(
    directory: &Path,
    names: &[String],
    cancelled: &dyn Fn() -> bool,
) -> io::Result<String> {
    let mut hash = Sha256::new();
    for name in names {
        let path = directory.join(name);
        let metadata = path.symlink_metadata()?;
        if !metadata.is_file() {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                format!("Expected an ordinary artifact: {}", path.display()),
            ));
        }
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        hash.update(metadata.len().to_le_bytes());
        transfer(&path, &mut hash, cancelled)?;
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn ordinary_directory(path: &Path) -> io::Result<()> {
    if path.symlink_metadata()?.file_type().is_dir() {
        Ok(())
    } else {
        Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("Expected an ordinary directory: {}", path.display()),
        ))
    }
}

/// Called under the output writer lock. Each generation contains a portable
/// game entry whose relative references remain valid after later publications.
pub(super) fn snapshot(
    staging: &Path,
    output: &Path,
    names: &[String],
    cancelled: &dyn Fn() -> bool,
) -> io::Result<Generation> {
    let digest = fingerprint(staging, names, cancelled)?;
    let generations = output.join(GENERATIONS);
    match fs::create_dir(&generations) {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::AlreadyExists => ordinary_directory(&generations)?,
        Err(e) => return Err(e),
    }
    let destination = generations.join(&digest);
    let unpublished = match ordinary_directory(&destination) {
        Ok(()) => {
            if fingerprint(&destination, names, cancelled)? != digest {
                return Err(io::Error::new(
                    ErrorKind::InvalidData,
                    format!(
                        "Published generation was modified: {}",
                        destination.display()
                    ),
                ));
            }
            None
        }
        Err(e) if e.kind() == ErrorKind::NotFound => {
            let pending = staging.join("generation");
            fs::create_dir(&pending)?;
            for name in names {
                // Legacy flat outputs may be edited in place. An independently
                // owned copy keeps their writes out of retained generations.
                transfer(
                    &staging.join(name),
                    &mut File::create_new(pending.join(name))?,
                    cancelled,
                )?;
            }
            fs::rename(pending, &destination)?;
            Some(destination)
        }
        Err(e) => return Err(e),
    };
    Ok(Generation {
        relative: format!("{GENERATIONS}/{digest}"),
        unpublished,
    })
}

pub(super) fn backup(path: &Path, destination: &Path) -> io::Result<()> {
    if !path.symlink_metadata()?.is_file() {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("Refusing to replace non-file artifact: {}", path.display()),
        ));
    }
    fs::hard_link(path, destination).or_else(|_| fs::copy(path, destination).map(|_| ()))
}

pub(super) fn game_entry(path: &Path, generation: &str, names: &[String]) -> Result<(), String> {
    let mut game: serde_json::Value =
        serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    for slot in ["visual", "collision"] {
        let name = game[slot].as_str().ok_or("Invalid staged game entry")?;
        game[slot] = format!("{generation}/{name}").into();
    }
    game["artifacts"] = names
        .iter()
        .map(|name| {
            (
                name.clone(),
                serde_json::Value::String(format!("{generation}/{name}")),
            )
        })
        .collect::<serde_json::Map<_, _>>()
        .into();
    fs::write(
        path,
        serde_json::to_vec_pretty(&game).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

pub(super) fn rollback(written: Vec<PathBuf>, backups: &[(PathBuf, PathBuf)]) -> Vec<String> {
    let mut errors = vec![];
    for path in written.into_iter().rev() {
        let result = if let Some((_, original)) = backups.iter().find(|(p, _)| *p == path) {
            fs::rename(original, &path)
        } else {
            fs::remove_file(&path)
        };
        if let Err(error) = result {
            errors.push(format!("{}: {error}", path.display()));
        }
    }
    errors
}
