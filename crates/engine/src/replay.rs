//! Rebuild a state from a hand's setup and action log (PRD FR-EC-6, API doc §5 and §8).
//!
//! A hand record is just `(setup, actions)`, chance outcomes included. Replaying folds
//! `apply` over the log from `new_hand`. Drivers also use this inside `determinize`: substitute
//! sampled cards for the hidden deals and replay, so a determinized state is built by exactly
//! the same code path as a real one.

use std::fmt;

use crate::game::{Game, HandSetup, ToAct};

/// Why a log couldn't be replayed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayError {
    /// Index into the action log of the offending action.
    pub step: usize,
    pub reason: String,
}

impl fmt::Display for ReplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "replay failed at step {}: {}", self.step, self.reason)
    }
}

impl std::error::Error for ReplayError {}

/// Apply `actions` in order to a fresh hand. Every action is checked with
/// [`Game::validate`] first, so a corrupt or mismatched log fails with an error instead of
/// silently producing a nonsense state.
pub fn replay<G: Game>(
    game: &G,
    setup: &HandSetup,
    actions: &[G::Action],
) -> Result<G::State, ReplayError> {
    let mut state = game.new_hand(setup);
    for (step, &action) in actions.iter().enumerate() {
        if game.to_act(&state) == ToAct::Terminal {
            return Err(ReplayError {
                step,
                reason: "hand already over".into(),
            });
        }
        game.validate(&state, action).map_err(|e| ReplayError {
            step,
            reason: format!("{action:?}: {e}"),
        })?;
        game.apply(&mut state, action);
    }
    Ok(state)
}
