//! Runtime game selection: turns a game id string into a concrete `Game` type (API doc §7).
//!
//! **This is the only file to edit when adding a game**, besides the game's own module.
//!
//! Why a visitor: `Game` has associated types, so there's no single type that can hold
//! "any game" (the trait isn't *object-safe*). Instead, the caller passes an object whose
//! `visit::<G>` method is generic. The `match` below calls it with the concrete type, and from
//! there on everything is ordinary generic code, compiled separately for each game.

use std::fmt;

use engine::Game;

use crate::{holdem::Holdem, kuhn::Kuhn, leduc::Leduc};

/// Something to do with a game once its concrete type is known.
pub trait GameVisitor<R> {
    fn visit<G: Game>(self, game: G) -> R;
}

/// All registered game ids, for help text and menus.
pub const GAME_IDS: &[&str] = &["kuhn", "leduc", "holdem_nl"];

/// Build game `id` from `config` (a TOML table of that game's `Config`; `None` = defaults)
/// and hand it to `visitor`.
pub fn with_game<R>(
    id: &str,
    config: Option<&toml::Table>,
    visitor: impl GameVisitor<R>,
) -> Result<R, RegistryError> {
    match id {
        "kuhn" => Ok(visitor.visit(Kuhn::new(parse_config(id, config)?))),
        "leduc" => Ok(visitor.visit(Leduc::new(parse_config(id, config)?))),
        "holdem_nl" => Ok(visitor.visit(Holdem::new(parse_config(id, config)?))),
        _ => Err(RegistryError::UnknownGame(id.to_string())),
    }
}

/// `C` is inferred from the call site: e.g. `Kuhn::new` takes a `kuhn::Config`, so that's what
/// gets parsed.
fn parse_config<C: Default + serde::de::DeserializeOwned>(
    id: &str,
    config: Option<&toml::Table>,
) -> Result<C, RegistryError> {
    match config {
        None => Ok(C::default()),
        Some(table) => {
            toml::Value::Table(table.clone())
                .try_into()
                .map_err(|e| RegistryError::BadConfig {
                    game: id.to_string(),
                    message: e.to_string(),
                })
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegistryError {
    UnknownGame(String),
    BadConfig { game: String, message: String },
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegistryError::UnknownGame(id) => {
                write!(
                    f,
                    "unknown game {id:?}; known games: {}",
                    GAME_IDS.join(", ")
                )
            }
            RegistryError::BadConfig { game, message } => {
                write!(f, "bad config for {game}: {message}")
            }
        }
    }
}

impl std::error::Error for RegistryError {}

#[cfg(test)]
mod tests {
    use super::*;

    /// A visitor that just reports the game's id.
    struct Id;
    impl GameVisitor<&'static str> for Id {
        fn visit<G: Game>(self, game: G) -> &'static str {
            game.meta().id
        }
    }

    #[test]
    fn every_registered_id_resolves_to_itself() {
        for &id in GAME_IDS {
            assert_eq!(with_game(id, None, Id), Ok(id));
        }
    }

    #[test]
    fn unknown_game_is_an_error() {
        assert!(matches!(
            with_game("nope", None, Id),
            Err(RegistryError::UnknownGame(_))
        ));
    }
}
