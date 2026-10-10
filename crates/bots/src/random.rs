//! Uniform random bot: the baseline every other bot should crush (FR-BT-4).

use rand::seq::IndexedRandom;

use engine::{DetRng, Game};

use crate::policy::Policy;

#[derive(Copy, Clone, Debug, Default)]
pub struct RandomPolicy;

impl<G: Game> Policy<G> for RandomPolicy {
    fn act(
        &mut self,
        _game: &G,
        _obs: &G::Observation,
        legal: &[G::Action],
        rng: &mut DetRng,
    ) -> G::Action {
        *legal.choose(rng).expect("act called with no legal actions")
    }

    fn name(&self) -> String {
        "random".to_string()
    }
}
