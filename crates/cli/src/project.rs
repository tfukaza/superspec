use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

const STARTER: &str = include_str!("../assets/starter.aml");

/// Removes only paths this initialization created, never recursively removing user files.
struct Initialization {
    files: Vec<PathBuf>,
    directories: Vec<PathBuf>,
    committed: bool,
}

impl Initialization {
    fn directory(&mut self, path: &Path) -> io::Result<()> {
        fs::create_dir(path)?;
        self.directories.push(path.to_owned());
        Ok(())
    }

    fn file(&mut self, path: &Path, contents: &[u8]) -> io::Result<()> {
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        self.files.push(path.to_owned());
        file.write_all(contents)
    }
}

impl Drop for Initialization {
    fn drop(&mut self) {
        if !self.committed {
            for file in self.files.iter().rev() {
                let _ = fs::remove_file(file);
            }
            for directory in self.directories.iter().rev() {
                let _ = fs::remove_dir(directory);
            }
        }
    }
}

/// Claims a new project exclusively. Existing files, directories and symlinks are not replaced.
pub fn initialize(project: &Path) -> io::Result<Vec<PathBuf>> {
    initialize_source(project, STARTER, &[])
}

pub fn initialize_source(
    project: &Path,
    source: &str,
    companions: &[(String, Vec<u8>)],
) -> io::Result<Vec<PathBuf>> {
    if let Some(parent) = project.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut transaction = Initialization {
        files: Vec::new(),
        directories: Vec::new(),
        committed: false,
    };
    transaction.directory(project)?;
    for name in ["references", "schematics", "reviews"] {
        transaction.directory(&project.join(name))?;
    }
    transaction.file(&project.join("level.aml"), source.as_bytes())?;
    transaction.file(&project.join("BRIEF.md"), concat!(
        "# Architectural project\n\nAML is the editable source. Record references, dimensions and circulation requirements here.\n\n",
        "From this project, inspect the source with `superspec inspect level.aml --json`.\n",
        "Use `superspec validate level.aml --json` for geometry and navigation findings.\n",
        "Keep reference and review evidence beside the source.\n"
    ).as_bytes())?;
    for (name, bytes) in companions {
        let relative = Path::new(name);
        if relative
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err(io::Error::other(
                "Companion paths must remain inside the project.",
            ));
        }
        let mut folder = project.to_owned();
        if let Some(parent) = relative.parent() {
            for component in parent.components() {
                folder.push(component);
                if !folder.exists() {
                    transaction.directory(&folder)?;
                }
            }
        }
        transaction.file(&project.join(relative), bytes)?;
    }
    transaction.committed = true;
    Ok([
        "level.aml",
        "BRIEF.md",
        "references",
        "schematics",
        "reviews",
    ]
    .into_iter()
    .map(|name| project.join(name))
    .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rollback_keeps_unowned_files_and_allows_a_later_initialization() {
        let directory = tempfile::tempdir().unwrap();
        let project = directory.path().join("project");
        {
            let mut transaction = Initialization {
                files: Vec::new(),
                directories: Vec::new(),
                committed: false,
            };
            transaction.directory(&project).unwrap();
            transaction.file(&project.join("ours"), b"partial").unwrap();
            fs::write(project.join("user-note"), "keep").unwrap();
            assert!(
                transaction
                    .file(&project.join("user-note"), b"overwrite")
                    .is_err()
            );
        }
        assert!(!project.join("ours").exists());
        assert_eq!(
            fs::read_to_string(project.join("user-note")).unwrap(),
            "keep"
        );
        assert!(initialize(&project).is_err());
        assert_eq!(
            initialize(&directory.path().join("retry")).unwrap().len(),
            5
        );
    }
}
