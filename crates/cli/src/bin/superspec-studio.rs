//! Native host for the same portable material/item projects accepted by the browser studios.
#[path = "studio/archive.rs"]
mod archive;
#[path = "studio/arguments.rs"]
mod arguments;
#[path = "../assets.rs"]
mod assets;
#[path = "studio/input.rs"]
mod input;
#[path = "studio/item.rs"]
mod item;
#[path = "../material_library.rs"]
mod material_library;
#[path = "studio/recipe.rs"]
mod recipe;

use serde_json::json;
use std::{
    io::{self, Write},
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

fn main() -> ExitCode {
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
    let built =
        Session::default().build_project(&input.project, 0, &|name| input.asset(name), &cancelled);
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
    let output = match built {
        Ok(output) if diagnostics.is_empty() => output,
        _ => return failure(diagnostics, 1),
    };
    let bytes = match arguments.command.as_str() {
        "scene" => output.scene(0).and_then(|mut scene| {
            superspec_export::add_project_batches(&mut scene, &input.batches)?;
            Ok(scene.packet())
        }),
        "glb" => superspec_export::world_glb(&output.model, 0, &superspec_export::Options { cancelled: &cancelled, textures: arguments.textures, lighting: input.lighting.as_ref(), batches: &input.batches, ..Default::default() }),
        "check" => output.scene(0).and_then(|scene| serde_json::to_vec(&json!({"schemaVersion":1,"success":true,"diagnostics":scene.manifest.diagnostics,
            "metadata":output.metadata,"scene":{"id":scene.manifest.id,"meshes":scene.manifest.meshes.len(),
            "vertices":scene.manifest.vertex_count,"colliders":scene.manifest.colliders.len()}})).map_err(|error| error.to_string())),
        _ => unreachable!(),
    };
    let bytes = match bytes {
        Ok(bytes) => bytes,
        Err(error) => return failure(vec![Diagnostic::message(&source, error)], 1),
    };
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
