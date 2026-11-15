//! JSON contract for recipe tooling, shared by the browser worker and the command line.
//!
//! Every function takes and returns JSON text and never panics on bad input, so the Wasm
//! wrappers and the native CLI are thin adapters over identical behavior. Responses carry
//! `success` and, on failure, `diagnostics` in the same shape the authoring session reports.
use super::Diagnostic;
use crate::materialspec::{
    catalog,
    program::{self, EditFailure, EditOp, Sources},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

const MAX_REQUEST_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RecipeRequest {
    /// The material's entry module.
    pub entry: String,
    /// Text of the entry module; overrides `modules[entry]` when present.
    pub source: Option<String>,
    pub modules: BTreeMap<String, String>,
    /// The module to inspect or edit; defaults to the entry.
    pub module: Option<String>,
    pub op: Option<EditOp>,
}

impl RecipeRequest {
    fn sources(&self) -> Sources {
        let mut modules = self.modules.clone();
        if let Some(source) = &self.source {
            modules.insert(self.entry.clone(), source.clone());
        }
        Sources {
            entry: self.entry.clone(),
            modules,
        }
    }
    fn module(&self) -> &str {
        self.module.as_deref().unwrap_or(&self.entry)
    }
}

fn diagnostics(errors: impl IntoIterator<Item = program::ProgramDiagnostic>) -> Value {
    json!({
        "success": false,
        "diagnostics": errors.into_iter().map(Diagnostic::from).collect::<Vec<_>>(),
    })
}

fn rejected(source: &str, message: impl Into<String>) -> Value {
    json!({ "success": false, "diagnostics": [Diagnostic::message(source, message)] })
}

fn request(text: &str) -> Result<RecipeRequest, Value> {
    if text.len() > MAX_REQUEST_BYTES {
        return Err(rejected("request", "Recipe request allowance exceeded."));
    }
    serde_json::from_str(text).map_err(|error| rejected("request", error.to_string()))
}

/// Compile the recipe and report its metadata, or every diagnostic found.
pub fn check(text: &str) -> String {
    let result = request(text).map(|request| match program::compile(&request.sources()) {
        Ok(program) => json!({ "success": true, "metadata": program.metadata() }),
        Err(errors) => diagnostics(errors),
    });
    result.unwrap_or_else(|value| value).to_string()
}

/// Parameters, fields, layers, groups and the spans of their values and uses.
pub fn inspect(text: &str) -> String {
    let result = request(text).map(|request| {
        match program::inspect_recipe(&request.sources(), request.module()) {
            Ok(inspection) => json!({ "success": true, "inspection": inspection }),
            Err(EditFailure {
                diagnostics: errors,
            }) => diagnostics(errors),
        }
    });
    result.unwrap_or_else(|value| value).to_string()
}

/// Apply one structured edit, returning minimal patches or the diagnostics that blocked it.
pub fn edit(text: &str) -> String {
    let result = request(text).map(|request| {
        let Some(op) = &request.op else {
            return rejected(request.module(), "An edit request needs an op.");
        };
        match program::apply_edit(&request.sources(), request.module(), op) {
            Ok(edited) => {
                json!({ "success": true, "patches": edited.patches, "source": edited.source })
            }
            Err(EditFailure {
                diagnostics: errors,
            }) => diagnostics(errors),
        }
    });
    result.unwrap_or_else(|value| value).to_string()
}

/// The operation catalog: operations, operators, groups, renderer operations and inputs.
pub fn catalog_json() -> String {
    catalog::sdk::ports_json()
}

/// The generated SDK files by name.
pub fn sdk_json() -> String {
    let files: BTreeMap<&str, String> = catalog::sdk::files()
        .into_iter()
        .map(|file| (file.name, file.contents))
        .collect();
    json!({ "module": catalog::SDK_MODULE, "files": files }).to_string()
}
