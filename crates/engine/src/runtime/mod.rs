//! Bounded artifact reuse and execution of producer-supplied indexed jobs.
pub(crate) mod batch;
pub(crate) mod cache;
pub mod dependencies;
pub(crate) mod dispatch;
pub(crate) mod producer_states;
#[cfg(test)]
mod producer_states_tests;
pub(crate) mod store;
#[cfg(test)]
mod tests;
