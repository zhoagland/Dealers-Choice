//! Game drivers. Each module implements `engine::Game` for one game; `registry` maps game ids to
//! types. Plan: `docs/tech/games.md`. Contract: `docs/api/game-driver.md`.

pub mod kuhn;
pub mod leduc;
pub mod registry;
