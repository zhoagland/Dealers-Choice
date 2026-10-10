//! Bots. Every bot sees only its seat's `Observation` and the public list of legal actions;
//! none ever touches a game's full `State`. Plan: `docs/tech/bots.md`.

pub mod coach;
pub mod harness;
pub mod ismcts;
pub mod mccfr;
pub mod policy;
pub mod random;
pub mod strategy;
pub mod strength;

pub use harness::{HandResult, play_hand};
pub use policy::{ActionValue, Budget, Evaluator, Policy};
pub use random::RandomPolicy;
pub use strategy::{Strategy, UniformStrategy};
pub use strength::{IsmctsBot, Strength, Style};
