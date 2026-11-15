//! One completed-artifact ownership and retention contract for every producer.
use std::{collections::BTreeMap, sync::Arc};
pub type ContentKey = [u8; 32];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Retention {
    pub(crate) name: &'static str,
    pub(crate) bytes: usize,
    pub(crate) entries: usize,
}
struct Entry<T> {
    value: Arc<T>,
    bytes: usize,
    used: u64,
}
struct Partition<T> {
    policy: Retention,
    entries: BTreeMap<ContentKey, Entry<T>>,
    bytes: usize,
}
pub(crate) struct Store<T> {
    partitions: BTreeMap<&'static str, Partition<T>>,
    tick: u64,
}
impl<T> Default for Store<T> {
    fn default() -> Self {
        Self {
            partitions: BTreeMap::new(),
            tick: 0,
        }
    }
}
impl<T> Store<T> {
    pub(crate) fn get(&mut self, policy: Retention, key: &ContentKey) -> Option<Arc<T>> {
        self.tick = self.tick.saturating_add(1);
        let partition = self.partitions.get_mut(policy.name)?;
        assert_eq!(
            partition.policy, policy,
            "a retention partition has one fixed policy"
        );
        let entry = partition.entries.get_mut(key)?;
        entry.used = self.tick;
        Some(entry.value.clone())
    }
    pub(crate) fn insert(
        &mut self,
        policy: Retention,
        key: ContentKey,
        value: Arc<T>,
        bytes: usize,
        replace: bool,
    ) {
        if bytes > policy.bytes || policy.entries == 0 {
            return;
        }
        let partition = self
            .partitions
            .entry(policy.name)
            .or_insert_with(|| Partition {
                policy,
                entries: BTreeMap::new(),
                bytes: 0,
            });
        assert_eq!(
            partition.policy, policy,
            "a retention partition has one fixed policy"
        );
        if partition.entries.contains_key(&key) && !replace {
            return;
        }
        if let Some(old) = partition.entries.remove(&key) {
            partition.bytes -= old.bytes;
        }
        while partition.bytes.saturating_add(bytes) > policy.bytes
            || partition.entries.len() >= policy.entries
        {
            let Some(oldest) = partition
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| *key)
            else {
                break;
            };
            partition.bytes -= partition.entries.remove(&oldest).unwrap().bytes;
        }
        self.tick = self.tick.saturating_add(1);
        partition.entries.insert(
            key,
            Entry {
                value,
                bytes,
                used: self.tick,
            },
        );
        partition.bytes += bytes;
    }
    pub(crate) fn stats(&self, policy: Retention) -> (usize, usize) {
        self.partitions
            .get(policy.name)
            .map_or((0, 0), |partition| {
                (partition.entries.len(), partition.bytes)
            })
    }
}
