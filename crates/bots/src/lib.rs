//! Bots. Every bot sees only its seat's `Observation` and the public list of legal actions;
//! none ever touches a game's full `State`. Plan: `docs/tech/bots.md`.

pub mod harness;
pub mod mccfr;
pub mod policy;
pub mod random;
pub mod strategy;

pub use harness::{HandResult, play_hand};
pub use policy::{ActionValue, Budget, Evaluator, Policy};
pub use random::RandomPolicy;
pub use strategy::{Strategy, UniformStrategy};
