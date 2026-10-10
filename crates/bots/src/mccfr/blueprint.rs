//! A trained strategy table, and a bot that plays from it (FR-BT-3, FR-ST-3).
//!
//! On disk (`to_bytes` / `from_bytes`): postcard of [`BlueprintFile`]. It carries the game id and
//! version, so a blueprint trained under different rules refuses to load (API doc §8). The storage
//! crate decides where files live and compresses them.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
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

/// The serialized form. Entries are sorted by their encoded bytes so the same blueprint always
/// produces the same file (HashMap order would differ between runs).
#[derive(Serialize, Deserialize)]
#[serde(
    bound = "O: Serialize + serde::de::DeserializeOwned, A: Serialize + serde::de::DeserializeOwned"
)]
pub struct BlueprintFile<O, A> {
    pub game_id: String,
    pub game_version: u32,
    pub iterations: u64,
    pub entries: Vec<(O, Vec<(A, f64)>)>,
}

impl<G: Game> Blueprint<G> {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut entries: Vec<_> = self
            .table
            .iter()
            .map(|(o, d)| (o.clone(), d.to_vec()))
            .collect();
        // `sort_by_cached_key` encodes each observation once, not once per comparison.
        entries.sort_by_cached_key(|(o, _)| postcard::to_allocvec(o).unwrap());
        let file = BlueprintFile {
            game_id: self.game_id.clone(),
            game_version: self.game_version,
            iterations: self.iterations,
            entries,
        };
        postcard::to_allocvec(&file).expect("blueprints always serialize")
    }

    /// Load, checking it was trained for this game and rules version.
    pub fn from_bytes(game: &G, bytes: &[u8]) -> Result<Blueprint<G>, String> {
        let file: BlueprintFile<G::Observation, G::Action> =
            postcard::from_bytes(bytes).map_err(|e| format!("corrupt blueprint: {e}"))?;
        let meta = game.meta();
        if file.game_id != meta.id || file.game_version != meta.version {
            return Err(format!(
                "blueprint is for {} v{}, not {} v{}",
                file.game_id, file.game_version, meta.id, meta.version
            ));
        }
        let mut bp = Blueprint::new(&file.game_id, file.game_version, file.iterations);
        for (o, d) in file.entries {
            bp.insert(o, d.into_iter().collect());
        }
        Ok(bp)
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
