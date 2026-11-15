//! Inspectable sign placement records from the existing theme contract.
use serde::{Deserialize, Serialize};
use superspec_types::architecture::V3;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Mount {
    Fascia,
    Roof,
    Blade,
    Hung,
    Wall,
    Balcony,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Form {
    Board,
    Lightbox,
    Plaque,
    Tiles,
    Painted,
    Ladder,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Built,
    Withheld,
    Crowded,
    Obstructed,
}
#[derive(Clone, Debug, Serialize)]
pub struct PlanBounds {
    pub min: V3,
    pub max: V3,
}
#[derive(Clone, Debug, Serialize)]
pub struct Plan {
    pub id: String,
    pub host: String,
    pub mount: Mount,
    pub form: Form,
    pub lines: Vec<String>,
    pub bounds: PlanBounds,
    pub status: Status,
    pub triangles: usize,
}
