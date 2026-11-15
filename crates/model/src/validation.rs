//! Physical and sampled-navigation report records; analysis remains with the producer.
use serde::Serialize;
use superspec_geometry::navigation::{self, Agent};
use superspec_types::{architecture::V3, diagnostic::Diagnostic};
pub mod physical;
pub mod water;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
    pub id: String,
    pub reachable: bool,
    pub distance: Option<f64>,
    pub straight_distance: f64,
    pub path: Vec<V3>,
    pub requested_routes: usize,
    pub verified_routes: usize,
    pub maximum_routes: Option<usize>,
    pub traversals: Vec<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Coverage {
    pub object: String,
    pub samples: usize,
    pub covered: usize,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub schema_version: u32,
    pub success: bool,
    pub geometry_status: &'static str,
    pub navigation_status: &'static str,
    pub agent: Agent,
    pub navigation: navigation::Mesh,
    pub routes: Vec<Route>,
    pub coverage: Vec<Coverage>,
    pub diagnostics: Vec<Diagnostic>,
    pub reviews: Vec<crate::review::Finding>,
    pub physical: physical::Evidence,
    pub water: Option<water::Report>,
}
