//! Rollout policies: how a simulated hand is finished below the search tree (`bots.md` §2.1).

use rand::seq::IndexedRandom;

use engine::{ActionClass, DetRng, Game};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Rollout {
    /// Check or call whenever possible; declarations and other choices uniformly at random.
    /// It measures showdown equity cheaply. Known bias: nobody folds below the tree, so fold
    /// equity is only captured by decisions inside the tree.
    CheckCallDown,
    /// Uniformly random legal actions. Sanity checks only: random folding makes values noisy
    /// and biased.
    Random,
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
            Rollout::CheckCallDown => {
                if let Some(&a) = legal
                    .iter()
                    .find(|&&a| game.classify(s, a) == ActionClass::CheckOrCall)
                {
                    return a;
                }
                // No check/call available (e.g. a declaration or a card choice): pick at random.
                *legal.choose(rng).unwrap()
            }
        }
    }
}
