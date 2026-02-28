//! Pipeline event handlers.
//!
//! Each handler receives a pipeline event, performs I/O via deps,
//! and returns child events that re-enter the dispatch loop.

pub(crate) mod bootstrap;
pub(crate) mod creation;
pub(crate) mod dedup;

#[cfg(test)]
mod creation_tests;
#[cfg(test)]
mod dedup_tests;
#[cfg(test)]
mod engine_tests;
