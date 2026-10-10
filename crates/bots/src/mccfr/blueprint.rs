//! A trained strategy table, and a bot that plays from it (FR-BT-3, FR-ST-3).
//! Saving to and loading from disk is added in the storage PR.

use std::collections::HashMap;
use std::sync::Arc;

use smallvec::SmallVec;

use engine::game::sample_weighted;
use engine::{DetRng, Game};

use crate::policy::Policy;
use crate::strategy::{Probs, Strategy};

/// Action → probability, for one information set.
pub type Distribution<A> = SmallVec<[(A, f64); 8]>;

/// The average strategy from MCCFR training, keyed by observation.
pub struct Blueprint<G: Game> {
    pub game_id: String,
    pub game_version: u32,
    pub iterations: u64,
    table: HashMap<G::Observation, Distribution<G::Action>>,
}

impl<G: Game> Blueprint<G> {
    pub fn new(game_id: &str, game_version: u32, iterations: u64) -> Blueprint<G> {
        Blueprint {
            game_id: game_id.to_string(),
            game_version,
            iterations,
            table: HashMap::new(),
        }
    }

    pub fn insert(&mut self, obs: G::Observation, dist: Distribution<G::Action>) {
        self.table.insert(obs, dist);
    }

    pub fn get(&self, obs: &G::Observation) -> Option<&Distribution<G::Action>> {
        self.table.get(obs)
    }

    pub fn len(&self) -> usize {
        self.table.len()
    }

    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&G::Observation, &Distribution<G::Action>)> {
        self.table.iter()
    }
}

impl<G: Game> Strategy<G> for Blueprint<G> {
    /// Unknown information sets (never reached in training) or a changed action menu fall
    /// back to uniform rather than failing.
    fn probs(&self, _game: &G, obs: &G::Observation, legal: &[G::Action]) -> Probs {
        if let Some(dist) = self.table.get(obs)
            && dist.len() == legal.len()
            && dist.iter().zip(legal).all(|((a, _), b)| a == b)
        {
            return dist.iter().map(|&(_, p)| p).collect();
        }
        let p = 1.0 / legal.len() as f64;
        legal.iter().map(|_| p).collect()
    }
}

/// A bot that samples its moves from a blueprint. `Arc` (a thread-safe shared pointer) lets
/// many bots and sim threads share one large table read-only without copying it.
pub struct BlueprintPolicy<G: Game> {
    blueprint: Arc<Blueprint<G>>,
}

impl<G: Game> BlueprintPolicy<G> {
    pub fn new(blueprint: Arc<Blueprint<G>>) -> BlueprintPolicy<G> {
        BlueprintPolicy { blueprint }
    }
}

impl<G: Game> Policy<G> for BlueprintPolicy<G> {
    fn act(
        &mut self,
        game: &G,
        obs: &G::Observation,
        legal: &[G::Action],
        rng: &mut DetRng,
    ) -> G::Action {
        let probs = self.blueprint.probs(game, obs, legal);
        let weighted: SmallVec<[(G::Action, f64); 8]> = legal.iter().copied().zip(probs).collect();
        sample_weighted(&weighted, rng)
    }

    fn name(&self) -> String {
        format!("mccfr:{}", self.blueprint.iterations)
    }
}
