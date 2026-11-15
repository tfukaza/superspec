//! Recipe tooling commands: the same JSON contract the browser worker calls.
use super::{arguments::Arguments, input};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
    sync::atomic::{AtomicU64, Ordering},
};
use superspec_engine::authoring::{Diagnostic, recipe};

fn print(value: Value) -> ExitCode {
    let mut object = match value {
        Value::Object(object) => object,
        other => return print(json!({ "result": other })),
    };
    let success = object
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    object.insert("schemaVersion".into(), json!(1));
    let text = Value::Object(object).to_string();
    if success {
        println!("{text}");
        ExitCode::SUCCESS
    } else {
        eprintln!("{text}");
        ExitCode::from(1)
    }
}

fn fail(source: &str, message: impl Into<String>) -> ExitCode {
    print(json!({ "success": false, "diagnostics": [Diagnostic::message(source, message)] }))
}

/// The entry file name and every sibling recipe module, keyed relative to the entry's folder.
pub fn project(path: &Path) -> Result<(PathBuf, String, BTreeMap<String, String>), String> {
    let path = path
        .canonicalize()
        .map_err(|error| format!("Cannot open {}: {error}", path.display()))?;
    let parent = path
        .parent()
        .ok_or("Input has no containing folder.")?
        .to_path_buf();
    let entry = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Source filename must be UTF-8.")?
        .to_owned();
    if !entry.ends_with(".material.ts") {
        return Err("Recipe tooling reads .material.ts files.".into());
    }
    Ok((parent.clone(), entry, input::modules(&parent)?))
}

pub fn run(arguments: &Arguments) -> ExitCode {
    let source = arguments.source.to_string_lossy().into_owned();
    match arguments.command.as_str() {
        "catalog" => match serde_json::from_str::<Value>(&recipe::catalog_json()) {
            Ok(catalog) => print(json!({ "success": true, "catalog": catalog })),
            Err(error) => fail("catalog", error.to_string()),
        },
        "sdk" => {
            let out = arguments.out.as_ref().expect("parser requires --out");
            match write_sdk(out) {
                Ok(paths) => print(
                    json!({ "success": true, "module": superspec_engine::materialspec::catalog::SDK_MODULE, "files": paths }),
                ),
                Err(error) => fail(&out.to_string_lossy(), error),
            }
        }
        "inspect" | "edit" => {
            let (parent, entry, modules) = match project(&arguments.source) {
                Ok(project) => project,
                Err(error) => return fail(&source, error),
            };
            let mut request = json!({ "entry": entry, "modules": modules });
            if arguments.command == "edit" {
                let op = match operation(arguments.op.as_deref().unwrap_or_default()) {
                    Ok(op) => op,
                    Err(error) => return fail(&source, error),
                };
                request["op"] = op;
            }
            let text = request.to_string();
            let response = if arguments.command == "inspect" {
                recipe::inspect(&text)
            } else {
                recipe::edit(&text)
            };
            let Ok(mut response) = serde_json::from_str::<Value>(&response) else {
                return fail(&source, "The recipe tools returned invalid JSON.");
            };
            if arguments.command == "edit" && arguments.write && response["success"] == true {
                let edited = response["source"].as_str().unwrap_or_default().to_owned();
                match write_atomically(&parent.join(&entry), &edited) {
                    Ok(()) => response["written"] = json!(parent.join(&entry)),
                    Err(error) => return fail(&source, error),
                }
            }
            print(response)
        }
        _ => unreachable!("only recipe commands are dispatched here"),
    }
}

fn operation(text: &str) -> Result<Value, String> {
    let json = match text.strip_prefix('@') {
        Some(path) => {
            fs::read_to_string(path).map_err(|error| format!("Cannot read {path}: {error}"))?
        }
        None => text.to_owned(),
    };
    serde_json::from_str(&json).map_err(|error| format!("--op is not valid JSON: {error}"))
}

fn write_atomically(path: &Path, text: &str) -> Result<(), String> {
    static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);
    let filename = path
        .file_name()
        .ok_or("Edit target has no filename.")?
        .to_string_lossy();
    let permissions = match fs::metadata(path) {
        Ok(metadata) => Some(metadata.permissions()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("Cannot inspect {}: {error}", path.display())),
    };
    for _ in 0..128 {
        let serial = NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed);
        let temporary = path.with_file_name(format!(
            ".{filename}.{}.{serial}.superspec-edit",
            std::process::id()
        ));
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("Cannot create {}: {error}", temporary.display())),
        };
        let result = file.write_all(text.as_bytes()).and_then(|()| {
            permissions.as_ref().map_or(Ok(()), |permissions| {
                file.set_permissions(permissions.clone())
            })
        });
        drop(file);
        let result = result.and_then(|()| fs::rename(&temporary, path));
        if let Err(error) = result {
            let _ = fs::remove_file(&temporary);
            return Err(format!("Cannot replace {}: {error}", path.display()));
        }
        return Ok(());
    }
    Err(format!(
        "Cannot allocate a temporary file beside {}.",
        path.display()
    ))
}

pub fn write_sdk(out: &Path) -> Result<Vec<PathBuf>, String> {
    fs::create_dir_all(out).map_err(|error| format!("Cannot create {}: {error}", out.display()))?;
    let mut written = Vec::new();
    for file in superspec_engine::materialspec::catalog::sdk::files() {
        let path = out.join(file.name);
        write_atomically(&path, &file.contents)?;
        written.push(path);
    }
    Ok(written)
}

/// A complete checker snapshot, owned by one invocation and never scanned as project input.
struct CheckDirectory(PathBuf);
impl CheckDirectory {
    fn create() -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir()
            .canonicalize()
            .map_err(|error| error.to_string())?;
        for _ in 0..128 {
            let path = root.join(format!(
                "superspec-check-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(format!("Cannot create checker snapshot: {error}")),
            }
        }
        Err("Cannot allocate a checker snapshot.".into())
    }
}
impl Drop for CheckDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn find_tsc(start: &Path) -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("SUPERSPEC_TSC") {
        return Some(explicit.into());
    }
    let names: &[&str] = if cfg!(windows) {
        &["tsc.cmd", "tsc.exe"]
    } else {
        &["tsc"]
    };
    let mut roots: Vec<PathBuf> = start.ancestors().map(Path::to_path_buf).collect();
    if let Ok(current) = std::env::current_dir() {
        roots.extend(current.ancestors().map(Path::to_path_buf));
    }
    for root in roots {
        for name in names {
            let candidate = root.join("node_modules").join(".bin").join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|directory| {
        names
            .iter()
            .map(|name| directory.join(name))
            .find(|candidate| candidate.is_file())
    })
}

/// Run strict TypeScript against the generated SDK.
pub fn type_check(arguments: &Arguments, input: &input::Input) -> Result<Vec<Diagnostic>, String> {
    let (parent, modules) = input.recipe_sources()?;
    let sdk = arguments
        .sdk
        .clone()
        .unwrap_or_else(|| PathBuf::from("generated/material-sdk"));
    write_sdk(&sdk)?;
    let snapshot = CheckDirectory::create()?;
    let source_root = snapshot.0.join("sources");
    fs::create_dir(&source_root).map_err(|error| error.to_string())?;
    let checker_sdk = snapshot.0.join("sdk");
    write_sdk(&checker_sdk)?;
    for (name, source) in modules {
        let path = source_root.join(name);
        fs::create_dir_all(path.parent().unwrap()).map_err(|error| error.to_string())?;
        fs::write(&path, source).map_err(|error| format!("Cannot stage {name}: {error}"))?;
    }
    let files: Vec<String> = modules
        .keys()
        .filter(|name| name.ends_with(".material.ts"))
        .map(|name| source_root.join(name).to_string_lossy().into_owned())
        .collect();
    let config = json!({
        "extends": "./tsconfig.json",
        "include": [],
        "files": files.iter().chain(std::iter::once(&checker_sdk.join("index.d.ts").to_string_lossy().into_owned())).collect::<Vec<_>>(),
    });
    let config_path = checker_sdk.join("recipes.tsconfig.json");
    fs::write(
        &config_path,
        serde_json::to_vec_pretty(&config).map_err(|e| e.to_string())?,
    )
    .map_err(|error| format!("Cannot write {}: {error}", config_path.display()))?;
    let tsc = find_tsc(&parent)
        .ok_or("--types needs TypeScript: install it (npm i typescript) or set SUPERSPEC_TSC.")?;
    let tsc = if tsc.is_file() {
        tsc.canonicalize().map_err(|error| error.to_string())?
    } else {
        tsc
    };
    let output = Command::new(&tsc)
        .args(["-p"])
        .arg(&config_path)
        .args(["--pretty", "false", "--noEmit"])
        .current_dir(&source_root)
        .output()
        .map_err(|error| format!("Cannot run {}: {error}", tsc.display()))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let source = arguments.source.to_string_lossy();
    let mut diagnostics = Vec::new();
    let mut other = Vec::new();
    for line in stdout
        .lines()
        .chain(stderr.lines())
        .filter(|line| !line.trim().is_empty())
    {
        if let Some(diagnostic) = parse_tsc(line, &parent, &source_root, &modules) {
            diagnostics.push(diagnostic);
        } else if let Some((code, message)) = line
            .strip_prefix("error ")
            .and_then(|line| line.split_once(": "))
        {
            let mut diagnostic = Diagnostic::message(&source, message);
            diagnostic.code = code.into();
            diagnostics.push(diagnostic);
        } else {
            other.push(line);
        }
    }
    if !output.status.success() && (diagnostics.is_empty() || !other.is_empty()) {
        let mut diagnostic = Diagnostic::message(
            &source,
            format!(
                "TypeScript exited with {}. {}",
                output.status,
                other.join("\n")
            ),
        );
        diagnostic.code = "TSC_PROCESS".into();
        diagnostics.push(diagnostic);
    }
    Ok(diagnostics)
}

/// `path(line,col): error TS2322: message`
fn parse_tsc(
    line: &str,
    parent: &Path,
    snapshot: &Path,
    modules: &BTreeMap<String, String>,
) -> Option<Diagnostic> {
    let (location, rest) = line.split_once("): error ")?;
    let (path, position) = location.rsplit_once('(')?;
    let (line_number, column) = position.split_once(',')?;
    let (code, message) = rest.split_once(": ")?;
    let (line_number, column): (usize, usize) = (line_number.parse().ok()?, column.parse().ok()?);
    let relative = Path::new(path)
        .strip_prefix(snapshot)
        .or_else(|_| Path::new(path).strip_prefix(parent))
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.replace('\\', "/"));
    let offset = modules.get(&relative).map_or(0, |text| {
        text.split_inclusive('\n')
            .take(line_number.saturating_sub(1))
            .map(|l| l.encode_utf16().count())
            .sum::<usize>()
            + column.saturating_sub(1)
    });
    Some(Diagnostic {
        code: code.to_owned(),
        source_name: relative,
        line: line_number,
        column,
        offset,
        end: None,
        message: message.to_owned(),
    })
}
