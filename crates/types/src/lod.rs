//! Authored alternatives for one renderable object. Geometry and physical ownership are separate.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GeometryLodGroup {
    pub id: String,
    /// Nearest first. Level zero is the ordinary, portable visual fallback.
    pub levels: Vec<GeometryLodLevel>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GeometryLodLevel {
    /// Minimum viewing distance in domain metres; starts at zero and strictly increases.
    pub distance: f64,
    /// Delivered placement IDs, not reusable local mesh resource IDs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub meshes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub batches: Vec<String>,
    /// Optional decreasing screen coverage thresholds, authored for every level or none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen_coverage: Option<f64>,
}
