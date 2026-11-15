//! Kowloon producer admission and compatibility counters; artifacts belong to the session runtime.
use super::{bake::BakedMaterial, mesh::BoundMesh};
pub use crate::runtime::cache::Stats;
use crate::{
    assets::identities::Identities,
    runtime::{
        cache::Stored,
        dependencies::{Runtime, producer_key},
        store::Retention,
    },
    session::Artifact,
};
use std::sync::Arc;

const RASTERS: Retention = Retention {
    name: "kowloon-raster",
    bytes: 256 * 1024 * 1024,
    entries: 16384,
};
const BINDINGS: Retention = Retention {
    name: "item-bound-mesh",
    bytes: 384 * 1024 * 1024,
    entries: 64,
};
impl Stored for BakedMaterial {
    fn stored_bytes(&self) -> usize {
        self.images
            .iter()
            .map(|(id, bytes)| id.len() + bytes.len())
            .sum::<usize>()
            + 4096
    }
}
impl Stored for BoundMesh {
    fn stored_bytes(&self) -> usize {
        self.bytes()
    }
}
pub struct Cache {
    runtime: Arc<Runtime<Artifact>>,
    images: Identities,
    stats: Stats,
}
impl Default for Cache {
    fn default() -> Self {
        Self::with_runtime(Arc::new(Runtime::new(128 * 1024 * 1024)))
    }
}
impl Cache {
    pub(crate) fn with_runtime(runtime: Arc<Runtime<Artifact>>) -> Self {
        Self {
            runtime,
            images: Identities::default(),
            stats: Stats::default(),
        }
    }
    pub fn stats(&self) -> Stats {
        let mut stats = self.stats;
        (stats.entries, stats.encoded_bytes) = self.runtime.retained(RASTERS);
        (stats.binding_entries, stats.binding_bytes) = self.runtime.retained(BINDINGS);
        stats
    }
    pub fn image(&mut self, bytes: &[u8]) -> u64 {
        self.images.image(bytes)
    }
    pub(crate) fn binding(&mut self, key: &[u8; 32]) -> Option<Arc<BoundMesh>> {
        let value = self
            .runtime
            .lookup(BINDINGS, &producer_key("item-bound-mesh-v1", key));
        if let Some(value) = value {
            if let Artifact::Binding(binding) = value.as_ref() {
                self.stats.binding_hits += 1;
                return Some(binding.clone());
            }
        }
        self.stats.binding_misses += 1;
        None
    }
    pub(crate) fn insert_binding(&mut self, key: [u8; 32], binding: BoundMesh) {
        let bytes = binding.stored_bytes();
        self.runtime.retain(
            BINDINGS,
            producer_key("item-bound-mesh-v1", &key),
            Artifact::Binding(Arc::new(binding)),
            bytes,
            true,
        );
    }
    pub(super) fn record_baking(&mut self, jobs: usize, pixels: usize, coalesced: usize) {
        self.stats.baked_jobs = self.stats.baked_jobs.saturating_add(jobs as u64);
        self.stats.baked_channel_pixels = self
            .stats
            .baked_channel_pixels
            .saturating_add(pixels as u64);
        self.stats.coalesced_jobs = self.stats.coalesced_jobs.saturating_add(coalesced as u64);
    }
    pub(crate) fn source_pixels(&self, identity: u64) -> Option<usize> {
        self.images.source_pixels(identity)
    }
    pub(crate) fn record_source_decode(&mut self, identity: u64, pixels: usize) {
        self.stats.source_decodes = self.stats.source_decodes.saturating_add(1);
        self.stats.source_decoded_pixels = self
            .stats
            .source_decoded_pixels
            .saturating_add(pixels as u64);
        self.images.record_pixels(identity, pixels);
    }
    pub(super) fn get(&mut self, key: &[u8]) -> Option<BakedMaterial> {
        let value = self
            .runtime
            .lookup(RASTERS, &producer_key("kowloon-raster-v3", key));
        if let Some(value) = value {
            if let Artifact::Raster(bake) = value.as_ref() {
                self.stats.hits += 1;
                return Some(bake.clone());
            }
        }
        self.stats.misses += 1;
        None
    }
    pub(super) fn insert(&mut self, key: Vec<u8>, bake: &BakedMaterial) {
        self.runtime.retain(
            RASTERS,
            producer_key("kowloon-raster-v3", &key),
            Artifact::Raster(bake.clone()),
            key.len() + bake.stored_bytes(),
            false,
        );
    }
}
