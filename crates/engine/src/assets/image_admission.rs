//! Bounded image validation and exact-byte admission metadata.
use serde::Serialize;
use std::{
    collections::{VecDeque, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
};

#[derive(Clone, Copy)]
pub(crate) struct ImageInfo {
    pub width: usize,
    pub height: usize,
    pub mime: &'static str,
}
pub(crate) fn validate_image(bytes: &[u8]) -> Result<ImageInfo, String> {
    // KTX2 is validated structurally: transcoding every texture only to discard it would cost far
    // more than the renderer's own upload. PNG and JPEG are fully decoded.
    if bytes.starts_with(&superspec_geometry::ktx2::MAGIC) {
        let (width, height) = superspec_geometry::ktx2::inspect(bytes)?;
        return Ok(ImageInfo {
            width,
            height,
            mime: superspec_geometry::ktx2::MIME,
        });
    }
    let image = superspec_geometry::image::decode(bytes)?;
    Ok(ImageInfo {
        width: image.width,
        height: image.height,
        mime: image.mime,
    })
}
struct Entry {
    hash: u64,
    bytes: Box<[u8]>,
    info: ImageInfo,
}
#[derive(Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub entries: usize,
    pub encoded_bytes: usize,
}
pub(crate) struct ImageCache {
    entries: VecDeque<Entry>,
    limit: usize,
    stats: CacheStats,
}
impl ImageCache {
    pub(crate) fn new(limit: usize) -> Result<Self, String> {
        if limit > 128 * 1024 * 1024 {
            return Err("Image admission cache exceeds 128 MiB.".into());
        }
        Ok(Self {
            entries: VecDeque::new(),
            limit,
            stats: CacheStats::default(),
        })
    }
    pub(crate) fn stats(&self) -> CacheStats {
        self.stats
    }
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.stats = CacheStats::default();
    }

    pub(crate) fn validate(&mut self, bytes: &[u8]) -> Result<ImageInfo, String> {
        let mut hasher = DefaultHasher::new();
        bytes.hash(&mut hasher);
        let hash = hasher.finish();
        // Hashes only accelerate lookup: exact owned bytes establish identity, even on collisions.
        if let Some(index) = self
            .entries
            .iter()
            .position(|e| e.hash == hash && e.bytes.as_ref() == bytes)
        {
            let entry = self.entries.remove(index).unwrap();
            let info = entry.info;
            self.entries.push_back(entry);
            self.stats.hits = self.stats.hits.saturating_add(1);
            return Ok(info);
        }
        self.stats.misses = self.stats.misses.saturating_add(1);
        let info = validate_image(bytes)?;
        // Oversized entries are valid inputs, just not cache residents. Failures are never cached.
        if bytes.len() <= self.limit {
            while self.stats.encoded_bytes + bytes.len() > self.limit || self.entries.len() >= 256 {
                let old = self.entries.pop_front().unwrap();
                self.stats.encoded_bytes -= old.bytes.len();
            }
            self.entries.push_back(Entry {
                hash,
                bytes: bytes.into(),
                info,
            });
            self.stats.encoded_bytes += bytes.len();
            self.stats.entries = self.entries.len();
        }
        Ok(info)
    }
}
