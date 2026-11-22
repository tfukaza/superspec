use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
};

/// Planes of the occluder service, by the asset name projects bind them under
/// (`occlusion/<name>/<key>`: the key is the plane's content). They outlive a request's assets,
/// so a later request whose planes have the same keys casts no ray: a moved sun bakes `shadow`
/// again and finds `occlusion` and `sunExposure` here.
#[derive(Default)]
pub struct OcclusionStore {
    planes: BTreeMap<String, Arc<[u8]>>,
    /// Asset names, least recently used first.
    order: VecDeque<String>,
    bytes: usize,
}

impl OcclusionStore {
    pub fn get(&self, name: &str) -> Option<&Arc<[u8]>> {
        self.planes.get(name)
    }
    pub fn contains(&self, name: &str) -> bool {
        self.planes.contains_key(name)
    }

    /// Encoded planes kept at most; what one call uses is kept whatever it takes.
    const ALLOWANCE: usize = 96 * 1024 * 1024;

    pub fn holds(&self, name: &str, key: &str) -> bool {
        self.planes.contains_key(&format!("occlusion/{name}/{key}"))
    }

    /// Marks planes as used now and adds the new ones, then drops the least recently used
    /// beyond the allowance, never one of `used`.
    pub fn keep(&mut self, used: &[String], added: BTreeMap<String, Arc<[u8]>>) {
        for (asset, bytes) in added {
            self.bytes += bytes.len();
            if let Some(previous) = self.planes.insert(asset, bytes) {
                self.bytes -= previous.len();
            }
        }
        let protected: BTreeSet<_> = used.iter().collect();
        self.order.retain(|asset| !protected.contains(asset));
        let evictable = self.order.len();
        self.order.extend(protected.into_iter().cloned());
        for _ in 0..evictable {
            if self.bytes <= Self::ALLOWANCE {
                break;
            }
            let asset = self.order.pop_front().expect("counted evictable plane");
            if let Some(dropped) = self.planes.remove(&asset) {
                self.bytes -= dropped.len();
            }
        }
    }
}
