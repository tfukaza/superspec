//! Native source and deterministic generation contracts for the Rust compiler.
//! Source inspection is intentionally distinct from semantic or geometry validation.

pub mod architecture;
pub mod diagnostic;
pub mod extensions;
pub mod format;
pub mod material;
pub mod material_catalog;
pub mod migration;
pub mod rng;
pub mod source;
