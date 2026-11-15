//! Existing pavement layout and finish delivery records.
use serde::{Deserialize, Serialize};
use superspec_types::architecture::V3;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Floor,
    Wall,
    FloorConnection,
    Connection,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Anchor {
    pub point: V3,
    pub kind: Kind,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Finish {
    Concrete,
    Repair,
    Tile,
    Timber,
    Grout,
    Substrate,
    Asphalt,
    ManholeRim,
    ManholeCover,
    ManholeSlot,
    Crack,
    Joint,
    Spall,
    Chip,
    TileGrime,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct Region {
    pub id: String,
    pub kind: Finish,
    pub bounds: Vec<V3>,
    pub footprint: Vec<Vec<V3>>,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct Plan {
    pub host: String,
    pub anchors: Vec<Anchor>,
    pub regions: Vec<Region>,
}
