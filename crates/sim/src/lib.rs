//! Bot-vs-bot simulation: parallel, reproducible, with duplicate deals and confidence intervals.
//! PRD FR-SM-1…4; plan: `docs/tech/sim.md`.

pub mod analytics;
pub mod encoding;
pub mod runner;
pub mod spec;
pub mod stats;

pub use runner::{NullSink, RecordSink, RunReport, VecSink, run};
pub use spec::{BotSpec, RunSpec};
