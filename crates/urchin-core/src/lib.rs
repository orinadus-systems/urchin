//! urchin-core: canonical event model, journal, identity, config.
//! All other crates depend on this. No I/O here — pure data types and logic.

pub mod config;
pub mod ephemeral;
pub mod event;
pub mod governance;
pub mod identity;
pub mod index;
pub mod journal;
pub mod query;
