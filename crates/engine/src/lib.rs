//! Core of the Dealers Choice engine: cards, randomness, and (in later PRs) hand evaluation,
//! betting, pots, the `Game` trait, and sessions.
//!
//! Design docs: `docs/tech/engine.md`, `docs/api/game-driver.md`.

pub mod card;
pub mod rng;

// Re-export the most used types so callers can write `engine::Card` instead of
// `engine::card::Card`.
pub use card::{Card, CardSet, Deck, Rank, Suit};
pub use rng::DetRng;
