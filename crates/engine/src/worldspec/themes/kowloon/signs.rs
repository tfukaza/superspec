//! Owned sign artwork, reader-relative typography and procedural panel finishes.
pub mod book;
pub mod copy;
pub mod designs;
pub mod frontage;
pub(super) mod graphics;
pub mod lettering;
pub mod mounts;
pub mod panel;
pub mod street;
pub mod survey;
pub mod weathering;
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Construction {
    Sheet,
    Lightbox,
    Timber,
    Enamel,
    Tile,
    Cloth,
    Painted,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Outline {
    Rectangle,
    Diamond,
    Round,
    Clipped,
    Rounded,
    Shield,
}
