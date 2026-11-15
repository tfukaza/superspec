//! Material sampling, composition and baked artifacts independent of theme and receiver ownership.
pub mod bake;
pub mod catalog;
pub mod color;
pub mod noise;
pub mod program;
pub mod surface;

#[cfg(feature = "authoring")]
pub mod authoring;
