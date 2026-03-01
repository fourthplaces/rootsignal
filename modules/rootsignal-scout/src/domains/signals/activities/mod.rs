// Signal processing activities: dedup, creation, edge wiring.
pub(crate) mod creation;
pub(crate) mod dedup;
pub(crate) mod dedup_utils;

#[cfg(test)]
mod creation_tests;
#[cfg(test)]
mod dedup_tests;
#[cfg(test)]
mod engine_tests;
