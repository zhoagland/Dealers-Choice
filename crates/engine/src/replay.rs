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

/// Rebuild a state when the deals and the player actions are known separately, which is the usual
/// situation inside `determinize`: the observation has the public action list, and the driver has
/// just sampled the hidden cards.
///
/// Walks the game from `new_hand`. Whenever it's a chance node, the next item from `deals` is applied;
/// whenever a seat acts, the next item from `actions`. It stops at the terminal, or as soon as the next
/// needed item isn't available (the observation's "present moment").
///
/// Panics if items are left over, because that means the observation and the game disagree about the
/// order of events: a driver bug.
pub fn replay_interleaved<G: Game>(
    game: &G,
    setup: &HandSetup,
    deals: impl IntoIterator<Item = G::Action>,
    actions: impl IntoIterator<Item = G::Action>,
) -> G::State {
    // `.peekable()` lets us look at the next item without consuming it.
    let mut deals = deals.into_iter().peekable();
    let mut actions = actions.into_iter().peekable();
    let mut state = game.new_hand(setup);
    loop {
        let next = match game.to_act(&state) {
            ToAct::Terminal => break,
            ToAct::Chance => deals.next(),
            ToAct::Seat(_) => actions.next(),
        };
        match next {
            Some(a) => {
                debug_assert!(
                    game.validate(&state, a).is_ok(),
                    "replay_interleaved: {a:?} invalid"
                );
                game.apply(&mut state, a);
            }
            None => break,
        }
    }
    assert!(
        deals.peek().is_none() && actions.peek().is_none(),
        "replay_interleaved: unused deals or actions; observation and game disagree on event order"
    );
    state
}
