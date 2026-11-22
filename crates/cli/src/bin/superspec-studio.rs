//! Native host for the same portable material/item projects accepted by the browser studios.
#[path = "studio/archive.rs"]
mod archive;
#[path = "studio/arguments.rs"]
mod arguments;
#[path = "../assets.rs"]
mod assets;
#[path = "studio/chart_delta.rs"]
mod chart_delta;
#[path = "studio/delivery.rs"]
mod delivery;
#[path = "studio/foliage.rs"]
mod foliage;
#[path = "studio/input.rs"]
mod input;
#[path = "studio/item.rs"]
mod item;
#[path = "../material_library.rs"]
mod material_library;
#[path = "studio/recipe.rs"]
mod recipe;

#[path = "studio/serve.rs"]
mod serve;
#[path = "studio/shading.rs"]
mod shading;
#[path = "studio/world.rs"]
mod world;

use serde_json::json;
use std::{
    io::{self, Read, Write},
    process::ExitCode,
    sync::atomic::{AtomicBool, Ordering},
};
use superspec_engine::authoring::{Diagnostic, Session};
static CANCELLED: AtomicBool = AtomicBool::new(false);
fn cancelled() -> bool {
    CANCELLED.load(Ordering::Relaxed)
}

fn failure(diagnostics: Vec<Diagnostic>, status: u8) -> ExitCode {
    eprintln!(
        "{}",
        json!({"schemaVersion":1,"success":false,"diagnostics":diagnostics})
    );
    ExitCode::from(status)
}

/// Native offline clients send one stored project archive and receive the same binary
/// shader/layout or field report as an editor session, without starting a web service.
fn bake_input(action: &str) -> Result<(), Vec<Diagnostic>> {
    let invalid = |e| vec![Diagnostic::message("bake-input", e)];
    if !matches!(
        action,
        "shade" | "shade-layouts" | "project-fields" | "aml-scene"
    ) {
        return Err(invalid(
            "Expected shade, shade-layouts, project-fields or aml-scene.".into(),
        ));
    }
    let limit = superspec_types::limits::MAX_PROJECT_FILE_BYTES;
    let mut bytes = Vec::new();
    io::stdin()
        .lock()
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| invalid(e.to_string()))?;
    if bytes.len() > limit {
        return Err(invalid("Project archive exceeds its allowance.".into()));
    }
    let input = archive::read(&bytes).map_err(invalid)?;
    drop(bytes);
    let mut session = Session::default();
    let output = if action == "aml-scene" {
        world::build(&session, input, action, &mut Default::default(), &cancelled)?
    } else {
        shading::build(
            &mut session,
            &mut Default::default(),
            &mut Default::default(),
            input,
            1,
            action,
            &cancelled,
        )?
    };
    io::stdout()
        .lock()
        .write_all(&output)
        .map_err(|e| invalid(e.to_string()))
}

fn main() -> ExitCode {
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "bake-input")
    {
        return match bake_input(&std::env::args().nth(2).unwrap_or_default()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(errors) => failure(errors, 1),
        };
    }
    if std::env::args_os().nth(1).is_some_and(|arg| arg == "serve") {
        return match serve::run(std::env::args_os().skip(2)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => failure(vec![Diagnostic::message("", error)], 1),
        };
    }
    let arguments = match arguments::Arguments::parse(std::env::args_os().skip(1)) {
        Ok(Some(arguments)) => arguments,
        Ok(None) => {
            print!("{}", arguments::HELP);
            return ExitCode::SUCCESS;
        }
        Err(message) => return failure(vec![Diagnostic::message("", message)], 2),
    };
    // An item definition is inspected by what it generates; a recipe by what it declares.
    if arguments.command == "inspect" && arguments.source.to_string_lossy().ends_with(".item.json")
    {
        return item::inspect(&arguments);
    }
    if arguments::RECIPE_COMMANDS.contains(&arguments.command.as_str()) {
        return recipe::run(&arguments);
    }
    let source = arguments.source.to_string_lossy();
    if source.ends_with(".tree.json") || source.ends_with(".grass.json") {
        let run = || -> Result<(), String> {
            ctrlc::set_handler(|| CANCELLED.store(true, Ordering::Relaxed))
                .map_err(|e| e.to_string())?;
            let bytes = foliage::read(
                &arguments.source,
                superspec_engine::treespec::MAX_DEFINITION_BYTES,
            )?;
            let site = arguments
                .site
                .as_ref()
                .map(|p| foliage::read(p, superspec_types::limits::MAX_FILE_BYTES))
                .transpose()?;
            let scene = foliage::scene(&source, &bytes, site.as_deref(), 0, &cancelled)?;
            let output =
                foliage::deliver(scene, &arguments.command, arguments.textures, &cancelled)?;
            io::stdout()
                .lock()
                .write_all(&output)
                .map_err(|e| e.to_string())
        };
        return match run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => failure(vec![Diagnostic::message(&source, e)], 1),
        };
    }
    let input = match input::Input::open(&arguments) {
        Ok(input) => input,
        Err(message) => return failure(vec![Diagnostic::message(&source, message)], 1),
    };
    if let Err(error) = ctrlc::set_handler(|| CANCELLED.store(true, Ordering::Relaxed)) {
        return failure(
            vec![Diagnostic::message(
                &source,
                format!("Cannot install cancellation handler: {error}"),
            )],
            1,
        );
    }
    let built = delivery::build(
        &mut Session::default(),
        &mut Default::default(),
        &input,
        0,
        &cancelled,
    );
    // Strict TypeScript runs beside the compiler's own checks so one report lists both.
    let mut diagnostics = match &built {
        Ok(_) => Vec::new(),
        Err(diagnostics) => diagnostics.clone(),
    };
    if arguments.types {
        match recipe::type_check(&arguments, &input) {
            Ok(found) => diagnostics.extend(found),
            Err(message) => return failure(vec![Diagnostic::message(&source, message)], 2),
        }
    }
    let (output, occlusion) = match built {
        Ok(output) if diagnostics.is_empty() => output,
        _ => return failure(diagnostics, 1),
    };
    let bytes = delivery::project(
        &output,
        &input,
        0,
        &arguments.command,
        occlusion.as_ref(),
        arguments.textures,
        &cancelled,
    );
    let bytes = match bytes {
        Ok(bytes) => bytes,
        Err(error) => return failure(vec![Diagnostic::message(&source, error)], 1),
    };
    // The export report: what a glTF importer without SuperSpec support skips and shows baked.
    if arguments.command == "glb" {
        if let Ok(scene) = output.scene(0) {
            for line in superspec_export::live_report(&scene.manifest.materials) {
                eprintln!("export: {line}");
            }
        }
    }
    if cancelled() {
        return failure(
            vec![Diagnostic::message(&source, "Authoring cancelled.")],
            1,
        );
    }
    match io::stdout().lock().write_all(&bytes) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => failure(
            vec![Diagnostic::message(
                &source,
                format!("Cannot write output: {error}"),
            )],
            1,
        ),
    }
}
