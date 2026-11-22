//! Transfer a changed chart against its exact previously delivered float words. The browser
//! advertises content keys, so missing or independently evicted baselines always fall back.
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use superspec_engine::authoring::ProjectLayouts;

const HISTORY_BYTES: usize = 48 * 1024 * 1024;

#[derive(Default)]
pub(super) struct ChartHistory {
    bindings: BTreeMap<String, String>,
    records: BTreeMap<String, Arc<[f32]>>,
}

impl ChartHistory {
    /// Each span stores its absolute byte offset in the payload, byte length, and, for a
    /// delta, the prior chart's content key. Both raw and delta spans are word aligned.
    pub(super) fn encode(
        &mut self,
        layout: &ProjectLayouts,
        bindings: &[(String, String, String)],
        held: &BTreeSet<String>,
        buffer: &mut Vec<u8>,
    ) -> Vec<Value> {
        let mut spans = Vec::with_capacity(layout.receivers.len());
        let mut fresh = BTreeMap::new();
        for receiver in &layout.receivers {
            let count = receiver.triangles * layout.record;
            let current = &layout.records[receiver.offset..receiver.offset + count];
            fresh.insert(receiver.id.as_str(), current);
            let start = buffer.len();
            let base = self
                .bindings
                .get(&receiver.id)
                .filter(|key| held.contains(*key));
            if let Some((key, prior)) = base.and_then(|key| Some((key, self.records.get(key)?)))
                && prior.len() == count
                && write_delta(buffer, prior, current)
            {
                spans.push(json!([start, buffer.len() - start, key]));
            } else {
                super::shading::floats(buffer, current);
                spans.push(json!([start, buffer.len() - start]));
            }
        }

        // Only current bindings survive a request. Several receivers with the same chart
        // share one allocation. A full cache merely prevents using those deltas next time.
        let mut next = Self::default();
        let mut used = 0;
        for (id, _, key) in bindings {
            if !next.records.contains_key(key) {
                let current = fresh.get(id.as_str());
                let previous = self.records.get(key);
                let count = current
                    .map(|v| v.len())
                    .or_else(|| previous.map(|v| v.len()));
                let Some(bytes) = count.map(|n| n * 4).filter(|n| used + n <= HISTORY_BYTES) else {
                    continue;
                };
                let records = previous
                    .cloned()
                    .unwrap_or_else(|| Arc::from(*current.unwrap()));
                next.records.insert(key.clone(), records);
                used += bytes;
            }
            next.bindings.insert(id.clone(), key.clone());
        }
        *self = next;
        spans
    }
}

/// One 32-bit change mask per group of 32 float words, followed by the changed words.
/// Dense replacements, unequal lengths and a missing base use the original raw encoding.
fn write_delta(out: &mut Vec<u8>, prior: &[f32], current: &[f32]) -> bool {
    let start = out.len();
    for (old, new) in prior.chunks(32).zip(current.chunks(32)) {
        let mut mask = 0u32;
        for (i, (a, b)) in old.iter().zip(new).enumerate() {
            mask |= u32::from(a.to_bits() != b.to_bits()) << i;
        }
        out.extend_from_slice(&mask.to_le_bytes());
        while mask != 0 {
            let i = mask.trailing_zeros() as usize;
            out.extend_from_slice(&new[i].to_le_bytes());
            mask &= mask - 1;
        }
    }
    if out.len() - start < std::mem::size_of_val(current) {
        true
    } else {
        out.truncate(start);
        false
    }
}
