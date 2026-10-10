//! Core of the Dealers Choice engine: cards, randomness, the `Game` trait, replay, and the
//! conformance harness. Hand evaluation, betting, pots, and sessions arrive in later PRs.
//!
//! Design docs: `docs/tech/engine.md`, `docs/api/game-driver.md`.

pub mod card;
pub mod game;
pub mod replay;
pub mod rng;
pub mod testing;

// Re-export the most used types so callers can write `engine::Card` instead of
// `engine::card::Card`.
pub use card::{Card, CardSet, Deck, Rank, Suit};
pub use game::{
    API_VERSION, ActionClass, CardVec, Chips, Game, GameLength, GameMeta, HandSetup, HiddenSampler,
    IllegalAction, Seat, StakeModel, ToAct, UniformSampler,
};
pub use rng::DetRng;
