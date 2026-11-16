//! Procedural foliage geometry: grass patches, shrubs and trees. See `README.md`.
//!
//! Generic like `materialspec` and `itemspec`: no filesystem, no theme and no world
//! assembly. A caller supplies the ground and decides where plants go.
pub mod grass;
mod output;
mod random;
pub mod tree;

pub use output::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Hard caps no definition can raise.
pub const MAX_VERTICES: usize = 2_000_000;
pub const MAX_INSTANCES: usize = 1_000_000;
pub const MAX_DEFINITION_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TreeDiagnostic {
    pub code: String,
    pub message: String,
    pub source: String,
    pub pointer: String,
}
impl std::fmt::Display for TreeDiagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}: {}", self.source, self.pointer, self.message)
    }
}
impl std::error::Error for TreeDiagnostic {}

pub(crate) fn fail(
    source: &str,
    pointer: &str,
    code: &str,
    message: impl Into<String>,
) -> TreeDiagnostic {
    TreeDiagnostic {
        code: code.into(),
        message: message.into(),
        source: source.into(),
        pointer: pointer.into(),
    }
}

pub(crate) fn check_cancelled(
    source: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), TreeDiagnostic> {
    if cancelled() {
        Err(fail(
            source,
            "/",
            "TREE_CANCELLED",
            "Foliage generation cancelled.",
        ))
    } else {
        Ok(())
    }
}

/// Per-definition budgets. Exceeding one is a `TREE_BUDGET` error that names the count.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct Limits {
    /// Vertices of all meshes of the most detailed level.
    pub max_vertices: usize,
    /// Instances of one result: clumps of a patch, or leaf clusters of a tree.
    pub max_instances: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_vertices: 400_000,
            max_instances: 200_000,
        }
    }
}
impl Limits {
    pub(crate) fn check(&self, source: &str) -> Result<(), TreeDiagnostic> {
        if self.max_vertices == 0
            || self.max_vertices > MAX_VERTICES
            || self.max_instances == 0
            || self.max_instances > MAX_INSTANCES
        {
            return Err(fail(
                source,
                "/limits",
                "TREE_LIMIT",
                format!(
                    "Limits must be positive and at most {MAX_VERTICES} vertices and {MAX_INSTANCES} instances."
                ),
            ));
        }
        Ok(())
    }
}

pub(crate) fn header(
    source: &str,
    format: &str,
    version: u32,
    kind: &str,
    expected: &str,
    id: &str,
) -> Result<(), TreeDiagnostic> {
    if format != "treespec" || version != 1 {
        return Err(fail(
            source,
            "/format",
            "TREE_FORMAT",
            "Expected format \"treespec\" version 1.",
        ));
    }
    if kind != expected {
        return Err(fail(
            source,
            "/kind",
            "TREE_FORMAT",
            format!("Expected kind \"{expected}\"."),
        ));
    }
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(fail(
            source,
            "/id",
            "TREE_FORMAT",
            "An id is 1 to 128 letters, digits, hyphens or underscores.",
        ));
    }
    Ok(())
}

/// A finite value inside a closed range, or a `TREE_VALUE` error at `pointer`.
pub(crate) fn within(
    source: &str,
    pointer: &str,
    value: f64,
    low: f64,
    high: f64,
) -> Result<f64, TreeDiagnostic> {
    if value.is_finite() && value >= low && value <= high {
        Ok(value)
    } else {
        Err(fail(
            source,
            pointer,
            "TREE_VALUE",
            format!("Expected a value from {low} to {high}, found {value}."),
        ))
    }
}

/// An ordered `[low, high]` pair inside a closed range.
pub(crate) fn range(
    source: &str,
    pointer: &str,
    value: [f64; 2],
    low: f64,
    high: f64,
) -> Result<[f64; 2], TreeDiagnostic> {
    within(source, pointer, value[0], low, high)?;
    within(source, pointer, value[1], value[0], high)?;
    Ok(value)
}

/// A geometry key: the hash of a version tag and the JSON of every value the part reads.
pub(crate) fn key(tag: &str, value: &impl Serialize) -> String {
    let mut hash = Sha256::new();
    hash.update(tag.as_bytes());
    hash.update([0]);
    hash.update(serde_json::to_vec(value).expect("geometry key inputs serialize"));
    format!("{:x}", hash.finalize())
}
