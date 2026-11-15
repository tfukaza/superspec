//! Local item geometry operations and bounded premade-mesh ingestion.
mod binding;
pub mod definition;
pub mod imports;
pub mod mesh;
pub mod primitives;

#[cfg(feature = "authoring")]
pub mod embedded_images;
