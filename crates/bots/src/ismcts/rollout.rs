//! Rollout policies: how a simulated hand is finished below the search tree (`bots.md` §2.1).

use rand::Rng;
use rand::seq::IndexedRandom;

use engine::{ActionClass, Chips, DetRng, Game};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Rollout {
    /// Check or call whenever possible; declarations and other choices uniformly at random.
    /// It measures showdown equity cheaply. Known bias: nobody folds below the tree, so fold
    /// equity is only captured by decisions inside the tree.
    CheckCallDown,
    /// Uniformly random legal actions. Sanity checks only: random folding makes values noisy
    /// and biased.
    Random,
    /// Strength-aware (backlog #1): uses the game's `decision_context` hand-strength category.
    /// Facing a bet after the first round with nothing (high card), fold most of the time; with
    /// trips or better, raise half the time; with two pair or better and no bet to call, bet
    /// most of the time; otherwise check or call. Games without a strength signal fall back to
    /// check/call-down. This gives rollouts fold equity and value bets, so checks and folds are
    /// no longer undervalued in the tree above.
    Heuristic,
}

impl Rollout {
    pub fn choose<G: Game>(
        self,
        game: &G,
        s: &G::State,
        legal: &[G::Action],
        rng: &mut DetRng,
    ) -> G::Action {
        match self {
            Rollout::Random => *legal.choose(rng).unwrap(),
            Rollout::Heuristic => {
                let to_act = match game.to_act(s) {
                    engine::ToAct::Seat(p) => p,
                    _ => return Rollout::CheckCallDown.choose(game, s, legal, rng),
                };
                let Some(ctx) = game.decision_context(s, to_act) else {
                    return Rollout::CheckCallDown.choose(game, s, legal, rng);
                };
                let class = |a: &G::Action| game.classify(s, *a);
                let call = legal
                    .iter()
                    .find(|a| matches!(class(a), ActionClass::CheckOrCall { .. }));
                let owed = match call.map(class) {
                    Some(ActionClass::CheckOrCall { owed }) => owed,
                    _ => Chips::ZERO,
                };
                let fold = legal.iter().find(|a| matches!(class(a), ActionClass::Fold));
                let raise = legal
                    .iter()
                    .filter(|a| matches!(class(a), ActionClass::BetOrRaise { .. }))
                    .min_by_key(|a| match class(a) {
                        ActionClass::BetOrRaise { to } => to,
                        _ => Chips::ZERO,
                    });
                let strength = ctx.strength;
                let pick = if owed > Chips::ZERO {
                    if ctx.street >= 1 && strength == 0 && rng.random_bool(0.7) {
                        fold.or(call)
                    } else if strength >= 3 && rng.random_bool(0.5) {
                        raise.or(call)
                    } else {
                        call
                    }
                } else if strength >= 2 && rng.random_bool(0.6) {
                    raise.or(call)
                } else {
                    call
                };
                match pick {
                    Some(&a) => a,
                    None => Rollout::CheckCallDown.choose(game, s, legal, rng),
                }
            }
            Rollout::CheckCallDown => {
                if let Some(&a) = legal
                    .iter()
                    .find(|&&a| matches!(game.classify(s, a), ActionClass::CheckOrCall { .. }))
                {
                    return a;
                }
                // No check/call available (e.g. a declaration or a card choice): pick at random.
                *legal.choose(rng).unwrap()
            }
        }
    }
}
