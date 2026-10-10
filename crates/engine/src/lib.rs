//! Core of the Dealers Choice engine: cards, randomness, hand evaluation, the `Game` trait,
//! replay, the conformance harness, betting rules, and pot splitting. Sessions arrive later.
//!
//! Design docs: `docs/tech/engine.md`, `docs/api/game-driver.md`.

pub mod betting;
pub mod card;
pub mod eval;
pub mod game;
pub mod pot;
pub mod replay;
pub mod rng;
pub mod testing;

// Re-export the most used types so callers can write `engine::Card` instead of
// `engine::card::Card`.
pub use card::{Card, CardSet, Deck, Rank, Suit};
pub use eval::{Category, HandValue};
pub use game::{
    API_VERSION, ActionClass, CardVec, Chips, Game, GameLength, GameMeta, HandSetup, HiddenSampler,
    IllegalAction, Seat, StakeModel, ToAct, UniformSampler,
};
pub use rng::DetRng;
