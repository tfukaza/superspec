//! Producer costs and compatibility counters over the shared dependency runtime.
use serde::Serialize;

pub(crate) trait Stored {
    fn stored_bytes(&self) -> usize;
}

#[derive(Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub hits: u64,
    pub misses: u64,
    pub entries: usize,
    pub encoded_bytes: usize,
    /// Unique rasters completed by successfully bound batches; cache probes are counted separately.
    pub baked_jobs: u64,
    pub baked_channel_pixels: u64,
    pub coalesced_jobs: u64,
    /// Successful procedural source decodes, including admission work before a later failed batch.
    pub source_decodes: u64,
    pub source_decoded_pixels: u64,
    pub binding_hits: u64,
    pub binding_misses: u64,
    pub binding_entries: usize,
    pub binding_bytes: usize,
    pub plant_hits: u64,
    pub plant_misses: u64,
}
