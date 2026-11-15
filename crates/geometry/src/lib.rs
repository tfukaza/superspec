//! Portable architectural geometry in right-handed Z-up meters.
pub mod math;
pub mod profile;
pub mod solid;
pub use math::{Matrix, V3};
pub use profile::{Profile, triangulate};
pub use solid::{Geometry, Operation, Solid};
pub mod clipping;
pub mod grading;
pub mod heightmap;
pub mod image;
pub mod ktx2;
pub mod navigation;
pub mod obstructions;
