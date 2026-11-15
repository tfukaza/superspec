//! Ordered cache probes and duplicate cold-request grouping over producer-defined keys.
pub(crate) struct Batch<T> {
    pub(crate) keys: Vec<Vec<u8>>,
    pub(crate) ordered: Vec<Option<T>>,
    pub(crate) missing: Vec<usize>,
    pub(crate) shared: Vec<(usize, usize)>,
}
impl<T> Batch<T> {
    pub(crate) fn missing(&self) -> &[usize] {
        &self.missing
    }
    pub(crate) fn prepare(
        keys: Vec<Vec<u8>>,
        mut lookup: impl FnMut(usize, &[u8]) -> Option<T>,
        cancelled: &dyn Fn() -> bool,
        cancellation: &str,
    ) -> Result<Self, String> {
        let mut ordered = Vec::with_capacity(keys.len());
        let mut missing = vec![];
        let mut shared = vec![];
        {
            let mut pending = std::collections::BTreeMap::new();
            for (i, key) in keys.iter().enumerate() {
                if cancelled() {
                    return Err(cancellation.into());
                }
                let value = lookup(i, key);
                if value.is_none() {
                    match pending.entry(key.as_slice()) {
                        std::collections::btree_map::Entry::Occupied(entry) => {
                            shared.push((i, *entry.get()));
                        }
                        std::collections::btree_map::Entry::Vacant(entry) => {
                            entry.insert(i);
                            missing.push(i);
                        }
                    }
                }
                ordered.push(value);
            }
        }
        Ok(Self {
            keys,
            ordered,
            missing,
            shared,
        })
    }
}
