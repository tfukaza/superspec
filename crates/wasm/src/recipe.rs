//! Recipe tooling for the browser. These wrap the engine's JSON contract so the web editor and
//! the command line behave identically; none of them keep state, so they are cheap to call from
//! a dedicated worker while a bake is running elsewhere.
use superspec_engine::authoring::recipe;
use wasm_bindgen::prelude::*;

/// Compile a recipe: `{success, metadata}` or `{success:false, diagnostics}`.
#[wasm_bindgen]
pub fn recipe_check(request: &str) -> String {
    recipe::check(request)
}

/// Parameters, fields, layers, groups and the spans of their values and uses.
#[wasm_bindgen]
pub fn recipe_inspect(request: &str) -> String {
    recipe::inspect(request)
}

/// Apply one structured edit and return minimal patches, or the diagnostics that blocked it.
#[wasm_bindgen]
pub fn recipe_edit(request: &str) -> String {
    recipe::edit(request)
}

/// The operation catalog as JSON.
#[wasm_bindgen]
pub fn recipe_catalog() -> String {
    recipe::catalog_json()
}

/// The generated SDK declarations, ports and reference as `{module, files}`.
#[wasm_bindgen]
pub fn recipe_sdk() -> String {
    recipe::sdk_json()
}
