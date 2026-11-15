//! Native process boundary for compilation, delivery and preview, without a Node backend.

mod assets;
mod benchmark;
mod build;
mod external;
mod material_library;
mod preview;
mod project;
mod workflows;

use serde::Serialize;
use std::{
    env,
    ffi::OsString,
    fs::File,
    io::{self, Read, Write},
    path::PathBuf,
    process::ExitCode,
    sync::atomic::{AtomicBool, Ordering},
};
use superspec_aml::{
    diagnostic::Diagnostic,
    source::{MAX_SOURCE_BYTES, SourceInspection, SourceLimits, parse_source},
};
static CANCELLED: AtomicBool = AtomicBool::new(false);
fn cancelled() -> bool {
    CANCELLED.load(Ordering::Relaxed)
}

const HELP: &str = concat!(
    "SuperSpec native prototype\nUsage: superspec <command> [options]\n",
    "  init <new-project-directory> [--json]\n",
    "  inspect <file.aml|file.xml> [--json]\n",
    "  trace <image.png|image.jpg> [--out <file.aml>] [--evidence <raster.json>] [--cells <n>] [--bands <n>] [--json]\n",
    "  generate [--seed <text>] [--depth <1-8>] [--archetype <name>] [--theme <name>] [--out <file.aml>] [--json]\n",
    "  visibility <file.aml> <queries.json> --out <new-directory> [--json]\n",
    "  obj <file.aml> --out <directory> [--json]\n",
    "  plan <file.aml> --out <plan.svg> [--json]\n",
    "  reconstruct --source <file.aml> [--evidence <annotations.json>] [--out <directory>] [--json]\n",
    "  bake <file.aml> --out <directory> [--blender <executable>] [--samples <n>] [--json]\n",
    "  game-export <file.aml> --out <directory> [--json]\n",
    "  scene <file.aml|file.xml> (binary LSP3 stdout)\n",
    "  glb <file.aml|file.xml> [--collision] [--no-uv] [--textures ktx2|png] (binary GLB stdout)\n",
    "  validate <file.aml|file.xml> [--json]\n",
    "  benchmark <file.aml|file.xml> [--edit <changed.aml>]... [--jobs <1-16>] [--json]\n",
    "  build <file.aml|file.xml> --out <directory> [--no-uv] [--textures ktx2|png] [--blender <executable>] [--json]\n",
    "    [--quake-palette <palette.lmp>] [--qbsp <executable>] [--vis <executable>] [--light <executable>]\n",
    "    [--jobs <1-16>]\n",
    "  preview <file.aml|file.xml> [--ui <prebuilt-directory>] [--port <0-65535>]\n",
    "  --help --version\n",
    "Preview is read-only and binds 127.0.0.1; the default port is selected automatically.\n",
    "Build emits GLB/glTF, runtime JSON, collision and validation data. --blender also creates binary FBX.\n",
    "Validation checks collision and sampled standing navigation. Unverified movement requirements remain warnings.\n",
    "  migrate <historical.level.svgx> --out <new-project-directory> [--json]\n",
    "Inspection checks XML only."
);

#[derive(Serialize)]
struct Failure {
    stage: &'static str,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CommandResult {
    schema_version: u32,
    command: String,
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    project: Option<PathBuf>,
    diagnostics: Vec<Diagnostic>,
    repairs: Vec<()>,
    suggestions: Vec<()>,
    artifacts: Vec<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    inspection: Option<SourceInspection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    report: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Failure>,
}

impl CommandResult {
    fn new(command: &str) -> Self {
        Self {
            schema_version: 1,
            command: command.to_owned(),
            success: false,
            source: None,
            project: None,
            diagnostics: Vec::new(),
            repairs: Vec::new(),
            suggestions: Vec::new(),
            artifacts: Vec::new(),
            inspection: None,
            report: None,
            error: None,
        }
    }

    fn fail(&mut self, stage: &'static str, message: impl Into<String>) {
        self.error = Some(Failure {
            stage,
            message: message.into(),
        });
    }
}

struct Arguments {
    command: String,
    input: PathBuf,
    json: bool,
    collision: bool,
    ui: Option<PathBuf>,
    port: u16,
    output: Option<PathBuf>,
    no_uv: bool,
    textures: superspec_export::Textures,
    blender: Option<PathBuf>,
    quake: Option<external::QuakeOptions>,
    edits: Vec<PathBuf>,
    workers: usize,
}

fn arguments(values: &[OsString]) -> Result<Arguments, String> {
    let command = values
        .first()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let command = if command == "game-export" {
        "build"
    } else {
        command
    };
    if ![
        "init",
        "inspect",
        "scene",
        "glb",
        "build",
        "validate",
        "preview",
        "migrate",
        "benchmark",
    ]
    .contains(&command)
    {
        return Err(
            "Expected init, inspect, validate, scene, glb, build, benchmark or preview.".to_owned(),
        );
    }
    let mut input = None;
    let mut json = false;
    let mut collision = false;
    let mut positional = false;
    let mut ui = None;
    let mut port = None;
    let mut output = None;
    let mut no_uv = false;
    let mut textures = None;
    let mut blender = None;
    let mut quake_palette = None;
    let mut quake_tools = std::collections::BTreeMap::new();
    let mut edits = vec![];
    let mut workers = None;
    let mut values = values[1..].iter();
    while let Some(value) = values.next() {
        if !positional && value == "--jobs" {
            if !["build", "benchmark"].contains(&command) || workers.is_some() {
                return Err("--jobs applies once to build or benchmark.".into());
            }
            workers = Some(
                values
                    .next()
                    .and_then(|v| v.to_str())
                    .and_then(|v| v.parse::<usize>().ok())
                    .filter(|v| (1..=16).contains(v))
                    .ok_or("--jobs requires 1–16.")?,
            );
            continue;
        }
        if !positional && value == "--edit" {
            if command != "benchmark" {
                return Err("--edit applies to benchmark.".into());
            }
            edits.push(PathBuf::from(
                values.next().ok_or("Missing edited source path.")?,
            ));
            continue;
        }
        if !positional
            && ["--quake-palette", "--qbsp", "--vis", "--light"]
                .iter()
                .any(|flag| value == *flag)
        {
            if command != "build" {
                return Err("Quake export options apply to build.".into());
            }
            let path = PathBuf::from(values.next().ok_or("Missing Quake export option value.")?);
            if value == "--quake-palette" {
                if quake_palette.replace(path).is_some() {
                    return Err("Repeated --quake-palette.".into());
                }
            } else if quake_tools
                .insert(value.to_string_lossy().into_owned(), path)
                .is_some()
            {
                return Err("Repeated Quake tool option.".into());
            }
            continue;
        }
        if !positional && value == "--" {
            positional = true;
            continue;
        }
        if !positional && value == "--json" {
            if json {
                return Err("Repeated --json flag.".to_owned());
            }
            json = true;
            continue;
        }
        if !positional && value == "--collision" {
            if collision || command != "glb" {
                return Err("--collision is supported once, on glb only.".into());
            }
            collision = true;
            continue;
        }
        if !positional && value == "--out" {
            if command != "build" || output.is_some() {
                return Err("--out is supported once, on build only.".into());
            }
            output = Some(PathBuf::from(
                values.next().ok_or("Missing --out directory.")?,
            ));
            continue;
        }
        if !positional && value == "--no-uv" {
            if !["build", "glb"].contains(&command) || no_uv {
                return Err("--no-uv is supported once, on build or glb.".into());
            }
            no_uv = true;
            continue;
        }
        if !positional && value == "--textures" {
            if !matches!(command, "build" | "glb") || textures.is_some() {
                return Err("--textures is supported once, on build or glb.".into());
            }
            textures = Some(match values.next().and_then(|v| v.to_str()) {
                Some("ktx2") => superspec_export::Textures::Ktx2,
                Some("png") => superspec_export::Textures::Png,
                _ => return Err("--textures requires ktx2 or png.".into()),
            });
            continue;
        }
        if !positional && value == "--blender" {
            if command != "build" || blender.is_some() {
                return Err("--blender is supported once, on build only.".into());
            }
            blender = Some(PathBuf::from(
                values.next().ok_or("Missing Blender executable.")?,
            ));
            continue;
        }
        if !positional && (value == "--ui" || value == "--port") {
            if command != "preview" {
                return Err("--ui and --port apply to preview only.".into());
            }
            let argument = values.next().ok_or("Missing preview option value.")?;
            if value == "--ui" {
                if ui.replace(PathBuf::from(argument)).is_some() {
                    return Err("Repeated --ui.".into());
                }
            } else {
                let number = argument
                    .to_str()
                    .and_then(|value| value.parse::<u16>().ok())
                    .ok_or("Invalid port; expected 0-65535.")?;
                if port.replace(number).is_some() {
                    return Err("Repeated --port.".into());
                }
            }
            continue;
        }
        if !positional && value.to_string_lossy().starts_with('-') {
            return Err(format!("Unknown option {}.", value.to_string_lossy()));
        }
        if input.replace(PathBuf::from(value)).is_some() {
            return Err("Expected exactly one path.".to_owned());
        }
    }
    if ["scene", "glb"].contains(&command) && json {
        return Err(format!(
            "{command} emits binary data; --json is not supported."
        ));
    }
    if command == "preview" && json {
        return Err("Preview is a long-running service; --json is not supported.".into());
    }
    if command == "build" && output.is_none() {
        return Err("Build requires --out <directory>.".into());
    }
    // Exports default to KTX2 textures, except where Blender (which cannot import them) reads the
    // GLB.
    if blender.is_some() && textures == Some(superspec_export::Textures::Ktx2) {
        return Err(
            "Blender cannot import KTX2 textures; use --textures png with --blender.".into(),
        );
    }
    let textures = textures.unwrap_or(if blender.is_some() {
        superspec_export::Textures::Png
    } else {
        superspec_export::Textures::Ktx2
    });
    if quake_palette.is_none() && !quake_tools.is_empty() {
        return Err("Quake tool options require --quake-palette.".into());
    }
    let quake = quake_palette.map(|palette| external::QuakeOptions {
        palette,
        qbsp: quake_tools
            .remove("--qbsp")
            .or_else(|| env::var_os("QBSP_PATH").map(PathBuf::from))
            .unwrap_or_else(|| "qbsp".into()),
        vis: quake_tools
            .remove("--vis")
            .or_else(|| env::var_os("VIS_PATH").map(PathBuf::from))
            .unwrap_or_else(|| "vis".into()),
        light: quake_tools
            .remove("--light")
            .or_else(|| env::var_os("LIGHT_PATH").map(PathBuf::from))
            .unwrap_or_else(|| "light".into()),
    });
    Ok(Arguments {
        command: command.to_owned(),
        input: input.ok_or("Expected exactly one path.")?,
        json,
        collision,
        ui,
        port: port.unwrap_or(0),
        output,
        no_uv,
        textures,
        blender,
        quake,
        edits,
        workers: workers.unwrap_or(4),
    })
}

/// Bounded byte loading precedes UTF-8/XML decoding; no companion files are read here.
fn read_source(path: &PathBuf) -> io::Result<String> {
    let file = File::open(path)?;
    let mut bytes = Vec::new();
    file.take(MAX_SOURCE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_SOURCE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "AML source byte budget exceeded.",
        ));
    }
    String::from_utf8(bytes).map_err(|failure| io::Error::new(io::ErrorKind::InvalidData, failure))
}

fn execute(arguments: &Arguments) -> CommandResult {
    let mut result = CommandResult::new(&arguments.command);
    if !["init", "inspect", "build", "validate", "benchmark"].contains(&arguments.command.as_str())
    {
        result.fail(
            "unsupported",
            format!(
                "Native {} is not implemented yet; no compilation or validation was performed.",
                arguments.command
            ),
        );
        return result;
    }
    let path = match std::path::absolute(&arguments.input) {
        Ok(path) => path,
        Err(failure) => {
            result.fail("arguments", failure.to_string());
            return result;
        }
    };
    if arguments.command == "init" {
        result.project = Some(path.clone());
        match project::initialize(&path) {
            Ok(artifacts) => {
                result.artifacts = artifacts;
                result.success = true;
            }
            Err(failure) => result.fail("initialize", failure.to_string()),
        }
        return result;
    }
    result.source = Some(path.clone());
    if arguments.command == "benchmark" {
        match benchmark::run(&path, &arguments.edits, arguments.workers) {
            Ok(report) => {
                result.report = Some(report);
                result.success = true;
            }
            Err(error) => result.fail("benchmark", error),
        }
        return result;
    }
    let text = match read_source(&path) {
        Ok(text) => text,
        Err(failure) => {
            result.fail("load", failure.to_string());
            return result;
        }
    };
    if ["build", "validate"].contains(&arguments.command.as_str()) {
        let output =
            match std::path::absolute(arguments.output.as_ref().unwrap_or(&arguments.input)) {
                Ok(output) => output,
                Err(error) => {
                    result.fail("arguments", error.to_string());
                    return result;
                }
            };
        eprintln!("Compiling AML with Rust…");
        let parent = path.parent().expect("absolute source parent");
        let mut session = superspec_engine::Session::default();
        if let Err(error) = session.set_workers(arguments.workers) {
            result.fail("arguments", error);
            return result;
        }
        let compiled = if arguments.quake.is_some() {
            session.compile_for_brushes(&text, 0, &|name| assets::read(parent, name), &cancelled)
        } else {
            session.compile_cancellable(
                &text,
                0,
                &|name| assets::read(parent, name),
                arguments.command == "build",
                &cancelled,
            )
        };
        match compiled {
            Ok(mut scene) => {
                eprintln!("Checking collision and navigation…");
                if let Err(error) = superspec_engine::validation::audit(&mut scene, &cancelled) {
                    result.fail("validation", error);
                    return result;
                }
                result.diagnostics = scene.manifest.diagnostics.clone();
                result.report = serde_json::to_value(&scene.manifest.validation).ok();
                if !scene.manifest.success {
                    result.fail("validation", "Declared requirements failed.");
                    return result;
                }
                if arguments.command == "validate" {
                    result.success = true;
                    return result;
                }
                let scene = if !arguments.no_uv {
                    eprintln!("Unwrapping lightmap UVs…");
                    match superspec_export::prepare_lightmap(&scene, &cancelled) {
                        Ok(prepared) => prepared,
                        Err(error) => {
                            result.fail("lightmap", error);
                            return result;
                        }
                    }
                } else {
                    scene
                };
                eprintln!(
                    "Prepared {} vertices, {} triangles and {} encoded image bytes.",
                    scene.manifest.vertex_count,
                    scene.manifest.index_count / 3,
                    scene.asset_bytes.len()
                );
                match build::publish(
                    &scene,
                    &output,
                    arguments.textures,
                    arguments.blender.as_deref(),
                    arguments.quake.as_ref(),
                    &cancelled,
                ) {
                    Ok(artifacts) => {
                        result.artifacts = artifacts;
                        result.success = true;
                    }
                    Err(error) => result.fail("export", error),
                }
            }
            Err(diagnostic) => {
                result.fail("compile", diagnostic.to_string());
                result.diagnostics.push(diagnostic);
            }
        }
        return result;
    }
    match parse_source(&text, SourceLimits::default()) {
        Ok(source) => {
            result.inspection = Some(source.inspect());
            result.success = true;
        }
        Err(diagnostic) => {
            result.fail("parse", diagnostic.to_string());
            result.diagnostics.push(diagnostic);
        }
    }
    result
}

/// Finite JSON commands publish exactly one result on stdout, including usage errors.
fn publish(result: &CommandResult, json: bool) -> io::Result<()> {
    if json {
        let stdout = io::stdout();
        let mut output = stdout.lock();
        serde_json::to_writer(&mut output, result)?;
        writeln!(output)
    } else {
        if let Some(failure) = &result.error {
            eprintln!("{}: {}", failure.stage, failure.message);
        } else if let Some(inspection) = &result.inspection {
            println!(
                "{}: {} XML elements. Semantic, geometry and navigation validation not run.",
                inspection.id, inspection.element_count
            );
        } else if let Some(project) = &result.project {
            println!("Created {}", project.display());
        } else if result.command == "build" {
            println!(
                "Built {} artifacts with physical and sampled navigation findings.",
                result.artifacts.len()
            );
        } else if result.command == "validate" {
            println!(
                "Validation completed with {} findings. Navigation uses sampled walkable space.",
                result.diagnostics.len()
            );
            for diagnostic in &result.diagnostics {
                eprintln!(
                    "{} {}: {}",
                    diagnostic.severity, diagnostic.code, diagnostic.message
                );
            }
        } else if ["trace", "generate"].contains(&result.command.as_str()) {
            if result.artifacts.is_empty() {
                if let Some(aml) = result.report.as_ref().and_then(|r| r["aml"].as_str()) {
                    print!("{aml}");
                }
            } else {
                for artifact in &result.artifacts {
                    println!("{}", artifact.display());
                }
            }
        } else if ["bake", "reconstruct"].contains(&result.command.as_str()) {
            for artifact in &result.artifacts {
                println!("{}", artifact.display());
            }
            if result.artifacts.is_empty() {
                println!("{} completed.", result.command);
            }
        } else if result.command == "benchmark" {
            if let Some(samples) = result.report.as_ref().and_then(|r| r["samples"].as_array()) {
                for sample in samples {
                    println!(
                        "{} {:.1} ms preview-ready",
                        sample["case"].as_str().unwrap_or("sample"),
                        sample["previewReadyMs"].as_f64().unwrap_or(0.)
                    );
                }
            }
        }
        Ok(())
    }
}

fn run() -> ExitCode {
    let values: Vec<_> = env::args_os().skip(1).collect();
    if values
        .iter()
        .any(|value| value == "--help" || value == "-h")
    {
        println!("{HELP}");
        return ExitCode::SUCCESS;
    }
    if values.len() == 1 && values[0] == "--version" {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    if let Some((result, json, status)) = workflows::dispatch(&values) {
        if let Err(error) = publish(&result, json) {
            eprintln!("output: {error}");
            return ExitCode::FAILURE;
        }
        return ExitCode::from(status);
    }
    let (result, json, status) = match arguments(&values) {
        Ok(arguments) => {
            if arguments.command == "preview" {
                return match preview::serve(
                    &arguments.input,
                    &arguments.ui.unwrap_or_else(preview::default_ui),
                    arguments.port,
                ) {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(error) => {
                        eprintln!("preview: {error}");
                        ExitCode::FAILURE
                    }
                };
            }
            if ["scene", "glb"].contains(&arguments.command.as_str()) {
                return match read_source(&arguments.input) {
                    Ok(source) => {
                        let parent = arguments
                            .input
                            .parent()
                            .unwrap_or(std::path::Path::new("."));
                        let compile = if arguments.collision {
                            superspec_engine::compile_collision_with_assets
                        } else {
                            superspec_engine::compile_with_assets
                        };
                        let result = compile(&source, 0, &|name| assets::read(parent, name));
                        if arguments.command == "glb" {
                            let options = superspec_export::Options {
                                purpose: if arguments.collision {
                                    superspec_export::Purpose::Collision
                                } else {
                                    superspec_export::Purpose::Visual
                                },
                                textures: arguments.textures,
                                ..Default::default()
                            };
                            return match result.map_err(|e| e.to_string()).and_then(|mut scene| {
                                superspec_engine::validation::audit(&mut scene, &cancelled)?;
                                if !scene.manifest.success {
                                    return Err("Declared validation requirements failed.".into());
                                }
                                let scene = if !arguments.collision && !arguments.no_uv {
                                    superspec_export::prepare_lightmap(&scene, &cancelled)?
                                } else {
                                    scene
                                };
                                superspec_export::glb(&scene, &options)
                            }) {
                                Ok(bytes) => match io::stdout().lock().write_all(&bytes) {
                                    Ok(()) => ExitCode::SUCCESS,
                                    Err(error) => {
                                        eprintln!("output: {error}");
                                        ExitCode::FAILURE
                                    }
                                },
                                Err(error) => {
                                    eprintln!("export: {error}");
                                    ExitCode::FAILURE
                                }
                            };
                        }
                        let packet = superspec_engine::result_packet(result, 0);
                        let length =
                            u32::from_le_bytes(packet[4..8].try_into().expect("packet header"))
                                as usize;
                        let manifest: serde_json::Value =
                            serde_json::from_slice(&packet[8..8 + length])
                                .expect("packet manifest");
                        if let Err(error) = io::stdout().lock().write_all(&packet) {
                            eprintln!("output: {error}");
                            return ExitCode::FAILURE;
                        }
                        if manifest["success"] == true {
                            ExitCode::SUCCESS
                        } else {
                            ExitCode::FAILURE
                        }
                    }
                    Err(error) => {
                        eprintln!("load: {error}");
                        ExitCode::FAILURE
                    }
                };
            }
            let result = execute(&arguments);
            let status = if result.success { 0 } else { 1 };
            (result, arguments.json, status)
        }
        Err(message) => {
            let mut result = CommandResult::new(
                &values
                    .first()
                    .map(|value| value.to_string_lossy())
                    .unwrap_or_default(),
            );
            result.fail("arguments", message);
            (result, values.iter().any(|value| value == "--json"), 2)
        }
    };
    if let Err(failure) = publish(&result, json) {
        eprintln!("output: {failure}");
        return ExitCode::FAILURE;
    }
    if cancelled() {
        ExitCode::from(130)
    } else {
        ExitCode::from(status)
    }
}

fn main() -> ExitCode {
    if let Err(error) = ctrlc::set_handler(|| CANCELLED.store(true, Ordering::Relaxed)) {
        eprintln!("Cannot install cancellation handler: {error}");
        return ExitCode::FAILURE;
    }
    let result = run();
    if cancelled() {
        ExitCode::from(130)
    } else {
        result
    }
}
