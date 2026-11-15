//! Shared immutable building placement, with independent collision and visible support inputs.
pub(crate) mod assembly;
pub mod draw;
pub(crate) mod drawings;
pub mod finish;
pub mod footprint;
pub mod materials;
mod placement;
pub(crate) mod reference_corner;
pub mod shell;
pub use placement::{Facade, Plan, Request, Shape, Site, Warning};
