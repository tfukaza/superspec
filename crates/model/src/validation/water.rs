use serde::Serialize;
use superspec_geometry::navigation;
use superspec_types::diagnostic::Diagnostic;

#[derive(Debug, Serialize, Clone, Copy)]
pub struct Boat {
    pub radius: f64,
    pub height: f64,
    pub draft: f64,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Depth {
    pub id: String,
    pub unknown_area: f64,
    pub shallow_area: f64,
}
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub boat: Boat,
    pub navigation: navigation::Mesh,
    pub depth: Vec<Depth>,
    pub diagnostics: Vec<Diagnostic>,
}
