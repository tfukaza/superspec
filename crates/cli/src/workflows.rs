//! Native generation and conversion entry points with shared transactional source publication.
mod bake;
mod generate;
mod migrate;
mod reconstruct;
mod text_export;
mod trace;
mod visibility;
use super::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

struct Arguments {
    paths: Vec<OsString>,
    values: BTreeMap<String, OsString>,
    flags: BTreeSet<String>,
}
impl Arguments {
    fn text(&self, name: &str) -> Result<Option<&str>, String> {
        self.values
            .get(name)
            .map(|v| v.to_str().ok_or_else(|| format!("{name} must be UTF-8.")))
            .transpose()
    }
    fn parse(values: &[OsString], named: &[&str], flags: &[&str]) -> Result<Self, String> {
        let mut result = Self {
            paths: vec![],
            values: BTreeMap::new(),
            flags: BTreeSet::new(),
        };
        let mut args = values.iter();
        let mut positional = false;
        while let Some(arg) = args.next() {
            if !positional && arg == "--" {
                positional = true;
                continue;
            }
            let name = arg.to_string_lossy();
            if !positional && flags.contains(&name.as_ref()) {
                if !result.flags.insert(name.into_owned()) {
                    return Err("Repeated flag.".into());
                }
                continue;
            }
            if !positional && named.contains(&name.as_ref()) {
                let value = args
                    .next()
                    .ok_or_else(|| format!("Missing value for {name}."))?
                    .clone();
                if result.values.insert(name.into_owned(), value).is_some() {
                    return Err("Repeated option.".into());
                }
                continue;
            }
            if !positional && name.starts_with('-') {
                return Err(format!("Unknown option {name}."));
            }
            result.paths.push(arg.clone());
        }
        Ok(result)
    }
    fn number(&self, name: &str, default: f64) -> Result<f64, String> {
        self.values
            .get(name)
            .map(|v| {
                v.to_str()
                    .and_then(|s| s.parse::<f64>().ok())
                    .filter(|n| n.is_finite())
                    .ok_or_else(|| format!("Invalid number for {name}."))
            })
            .unwrap_or(Ok(default))
    }
    fn integer(&self, name: &str, default: usize) -> Result<usize, String> {
        let n = self.number(name, default as f64)?;
        if n < 0. || n.fract() != 0. || n > usize::MAX as f64 {
            return Err(format!("Invalid integer for {name}."));
        }
        Ok(n as usize)
    }
}

pub fn dispatch(values: &[OsString]) -> Option<(CommandResult, bool, u8)> {
    let command = values.first()?.to_str()?;
    if ![
        "trace",
        "generate",
        "migrate",
        "reconstruct",
        "bake",
        "visibility",
        "obj",
        "plan",
    ]
    .contains(&command)
    {
        return None;
    }
    let json = values.iter().any(|v| v == "--json");
    let mut result = CommandResult::new(command);
    let named: &[&str] = if ["visibility", "obj", "plan"].contains(&command) {
        &["--out"]
    } else if command == "bake" {
        &["--out", "--blender", "--samples", "--resolution"]
    } else if command == "reconstruct" {
        &[
            "--source",
            "--evidence",
            "--out",
            "--max-dimension",
            "--model",
            "--api-url",
            "--max-revisions",
            "--max-source-characters",
        ]
    } else if command == "migrate" {
        &["--out"]
    } else if command == "generate" {
        &[
            "--out",
            "--seed",
            "--depth",
            "--min-routes",
            "--archetype",
            "--theme",
            "--attempts",
        ]
    } else {
        &[
            "--out",
            "--evidence",
            "--id",
            "--name",
            "--cells",
            "--bands",
            "--grid",
            "--span",
            "--fill",
            "--bg-tol",
            "--alpha-cut",
            "--smooth",
            "--min-band",
        ]
    };
    let flags: &[&str] = if command != "trace" {
        &["--json"]
    } else {
        &["--json", "--no-crop", "--raster"]
    };
    let arguments = match Arguments::parse(&values[1..], named, flags) {
        Ok(a) => a,
        Err(error) => {
            result.fail("arguments", error);
            return Some((result, json, 2));
        }
    };
    let correct_paths = if command == "visibility" {
        arguments.paths.len() == 2
    } else if command == "reconstruct" {
        arguments.paths.len() == usize::from(!arguments.values.contains_key("--source"))
    } else {
        arguments.paths.len() == usize::from(command != "generate")
    };
    if !correct_paths {
        result.fail(
            "arguments",
            "Trace requires one input path. Generate uses named options only.",
        );
        return Some((result, json, 2));
    }
    let outcome = if ["obj", "plan"].contains(&command) {
        text_export::run(command, &arguments, &mut result)
    } else if command == "visibility" {
        visibility::run(&arguments, &mut result)
    } else if command == "trace" {
        trace::run(&arguments, &mut result)
    } else if command == "migrate" {
        migrate::run(&arguments, &mut result)
    } else if command == "reconstruct" {
        reconstruct::run(&arguments, &mut result)
    } else if command == "bake" {
        bake::run(&arguments, &mut result)
    } else {
        generate::run(&arguments, &mut result)
    };
    match outcome {
        Ok(()) => result.success = true,
        Err(error) => result.fail("workflow", error),
    }
    let status = if cancelled() {
        result.success = false;
        result.fail("cancelled", "Operation cancelled.");
        130
    } else if result.success {
        0
    } else {
        1
    };
    Some((result, json, status))
}

fn validate_proposal(aml: &str) -> Result<superspec_engine::Scene, String> {
    let mut scene = superspec_engine::Session::default()
        .compile_cancellable(
            aml,
            0,
            &|_| Err("Generated proposal has no external assets.".into()),
            false,
            &cancelled,
        )
        .map_err(|e| e.to_string())?;
    superspec_engine::validation::audit(&mut scene, &cancelled)?;
    if !scene.manifest.success {
        return Err(scene
            .manifest
            .diagnostics
            .iter()
            .filter(|d| d.severity == "error")
            .map(|d| d.message.as_ref())
            .collect::<Vec<_>>()
            .join("\n"));
    }
    if scene
        .manifest
        .validation
        .as_ref()
        .is_none_or(|v| v.navigation.polygons.is_empty())
    {
        return Err("Generated proposal has no traversable navigation support.".into());
    }
    Ok(scene)
}

/// Every file is staged before replacing destinations. A failed commit restores prior files.
fn publish_files(files: Vec<(PathBuf, Vec<u8>)>) -> Result<Vec<PathBuf>, String> {
    use std::fs::{self, OpenOptions};
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let mut staged = vec![];
    let mut backups = vec![];
    let mut committed = vec![];
    let operation = (|| {
        let mut unique = BTreeSet::new();
        for (i, (path, bytes)) in files.iter().enumerate() {
            if super::cancelled() {
                return Err("Source publication cancelled.".into());
            }
            let path = std::path::absolute(path).map_err(|e| e.to_string())?;
            let identity = if cfg!(windows) {
                path.to_string_lossy().to_lowercase()
            } else {
                path.to_string_lossy().into_owned()
            };
            if !unique.insert(identity) {
                return Err("Output paths must be distinct.".into());
            }
            let parent = path.parent().ok_or("Output needs a parent directory.")?;
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            let temp = parent.join(format!(
                ".superspec-source-{}-{stamp}-{i}",
                std::process::id()
            ));
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
                .map_err(|e| e.to_string())?;
            staged.push((path, temp.clone()));
            file.write_all(bytes).map_err(|e| e.to_string())?;
        }
        for (i, (path, temp)) in staged.iter().enumerate() {
            if super::cancelled() {
                return Err("Source publication cancelled.".into());
            }
            if path.symlink_metadata().is_ok() {
                if path.is_dir() {
                    return Err("Source output must be a file.".into());
                }
                let backup = path.parent().unwrap().join(format!(
                    ".superspec-source-backup-{}-{stamp}-{i}",
                    std::process::id()
                ));
                fs::rename(path, &backup).map_err(|e| e.to_string())?;
                backups.push((path.clone(), backup));
            }
            fs::rename(temp, path).map_err(|e| e.to_string())?;
            committed.push(path.clone());
        }
        Ok(())
    })();
    if let Err(error) = operation {
        for path in committed.iter().rev() {
            let _ = fs::remove_file(path);
        }
        let mut failed = vec![];
        for (path, backup) in backups.iter().rev() {
            if fs::rename(backup, path).is_err() {
                failed.push(backup.display().to_string());
            }
        }
        for (_, temp) in &staged {
            let _ = fs::remove_file(temp);
        }
        return Err(if failed.is_empty() {
            error
        } else {
            format!("{error}. Recovery files remain at {}.", failed.join(", "))
        });
    }
    for (_, backup) in backups {
        let _ = fs::remove_file(backup);
    }
    Ok(committed)
}

fn path(value: &OsString) -> Result<PathBuf, String> {
    std::path::absolute(Path::new(value)).map_err(|e| e.to_string())
}
