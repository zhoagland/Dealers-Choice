//! Persistence (PRD FR-ST-1…4, FR-SS-1; `docs/tech/storage.md`).
//!
//! - [`db`]: SQLite app state: players, bankroll ledger, human hands, coach notes.
//! - [`files`]: the data directory: sim runs (zstd records + JSON summary) and blueprints.
//! - [`profiles`]: named sim/play settings as TOML files under `config/`.

pub mod db;
pub mod files;
pub mod profiles;

pub use db::{Db, DbError, HandRow, LedgerKind, Note, StoredHand};
pub use files::{DataRoot, ZstdRecordSink};
pub use profiles::{ProfileKind, ProfileStore};
