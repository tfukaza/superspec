//! Session lifetimes for bookkeeping supplied by compiled producer composition.
use std::{
    any::{Any, TypeId},
    cell::RefCell,
    collections::BTreeMap,
    sync::Arc,
};

/// Only compiled adapters supply state types; authored names cannot create entries.
/// Completed artifacts remain in the runtime store with their retention policies.
#[derive(Default)]
pub(crate) struct ProducerStates {
    entries: RefCell<BTreeMap<TypeId, Arc<dyn Any + Send + Sync>>>,
}
impl ProducerStates {
    pub(crate) fn with<T: Send + Sync + 'static, R>(
        &self,
        create: impl FnOnce() -> T,
        use_state: impl FnOnce(&T) -> R,
    ) -> R {
        self.try_with(|| Ok::<_, std::convert::Infallible>(create()), use_state)
            .unwrap_or_else(|never| match never {})
    }

    pub(crate) fn try_with<T: Send + Sync + 'static, E, R>(
        &self,
        create: impl FnOnce() -> Result<T, E>,
        use_state: impl FnOnce(&T) -> R,
    ) -> Result<R, E> {
        let state = if let Some(state) = self.get::<T>() {
            state
        } else {
            let created: Arc<dyn Any + Send + Sync> = Arc::new(create()?);
            // Factories may compose another state. No registry borrow crosses
            // a factory or producer call, including cancellation and clearing.
            self.entries
                .borrow_mut()
                .entry(TypeId::of::<T>())
                .or_insert(created)
                .clone()
                .downcast::<T>()
                .unwrap_or_else(|_| unreachable!("state key is its concrete type"))
        };
        Ok(use_state(&state))
    }

    pub(crate) fn inspect<T: Send + Sync + 'static, R>(
        &self,
        inspect: impl FnOnce(&T) -> R,
    ) -> Option<R> {
        self.get::<T>().map(|state| inspect(&state))
    }

    pub(crate) fn clear(&self) {
        let entries = std::mem::take(&mut *self.entries.borrow_mut());
        drop(entries);
    }

    fn get<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.entries
            .borrow()
            .get(&TypeId::of::<T>())
            .cloned()
            .map(|state| {
                state
                    .downcast::<T>()
                    .unwrap_or_else(|_| unreachable!("state key is its concrete type"))
            })
    }
}
