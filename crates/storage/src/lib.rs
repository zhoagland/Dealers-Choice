//! Persistence (PRD FR-ST-1…4, FR-SS-1; `docs/tech/storage.md`).
//!
//! - [`db`]: SQLite app state: players, bankroll ledger, human hands, coach notes.
//! - [`files`]: the data directory: sim runs (zstd records + JSON summary) and blueprints.

pub mod db;
pub mod files;

pub use db::{Db, DbError, LedgerKind, StoredHand};
pub use files::{DataRoot, ZstdRecordSink};
