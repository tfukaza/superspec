//! Host-resolved bytes and source admission, independent of recipes and placement.
pub(crate) mod identities;
pub(crate) mod image_admission;
use std::{cell::RefCell, collections::BTreeMap, sync::Arc};

pub(crate) struct Sources<'a> {
    load: &'a dyn Fn(&str) -> Result<Vec<u8>, String>,
    bytes: RefCell<BTreeMap<String, Arc<[u8]>>>,
    limit: usize,
}
impl<'a> Sources<'a> {
    pub(crate) fn new(load: &'a dyn Fn(&str) -> Result<Vec<u8>, String>, limit: usize) -> Self {
        Self {
            load,
            bytes: Default::default(),
            limit,
        }
    }
    /// A revision resolves each admitted source once; the next revision receives fresh bytes.
    pub(crate) fn read(&self, path: &str) -> Result<Arc<[u8]>, String> {
        if let Some(bytes) = self.bytes.borrow().get(path) {
            return Ok(bytes.clone());
        }
        let bytes: Arc<[u8]> = (self.load)(path)?.into();
        let mut cached = self.bytes.borrow_mut();
        if cached.values().map(|b| b.len()).sum::<usize>() + bytes.len() <= self.limit {
            cached.insert(path.into(), bytes.clone());
        }
        Ok(bytes)
    }
}
#[cfg(test)]
mod tests;
