//! Multiple-observer Information Set MCTS: the default bot (PRD FR-BT-2, `docs/tech/bots.md` §2).
//!
//! **How one decision is made.** Repeat until the budget runs out:
//! 1. *Determinize*: sample a full deal consistent with what this bot can see.
//! 2. *Select*: walk down the game. At each decision, the acting seat looks up **its own**
//!    information set in **its own** table and picks an action by UCB1 (good average result,
//!    plus a bonus for rarely-tried actions).
//! 3. *Expand*: the first unseen information set on the path gets a table entry.
//! 4. *Roll out*: finish the hand with a cheap fixed policy (check/call down).
//! 5. *Backpropagate*: each seat's table entries along the path get that seat's own result.
//!
//! Then play the most-tried root action (or, for coaching, report every action's average).
//!
//! **Why multiple observers.** With a single shared tree (SO-ISMCTS), simulated opponents would
//! choose without looking at their own sampled cards, so they'd never fold junk or bet the
//! nuts. Here, each seat's choices are keyed by that seat's observation, which includes its
//! own (sampled) cards.
//!
//! **Why hash tables instead of a node tree.** The key is `G::Observation`, which by the API
//! contract *is* the information set. Tables avoid `Rc<RefCell<Node>>` pointer trees, the part of
//! Rust that's hardest to read and get right.

pub mod rollout;

use std::collections::HashMap;
use std::time::Instant;

use smallvec::SmallVec;

use engine::{DetRng, Game, HiddenSampler, Seat, ToAct, UniformSampler};

use crate::policy::{ActionValue, Budget, Evaluator};
pub use rollout::Rollout;

/// Search settings.
#[derive(Clone, Debug)]
pub struct IsmctsConfig {
    /// Exploration constant, in units of the observed reward standard deviation (§2: rewards
    /// aren't in [0, 1], so a fixed constant would need re-tuning per game).
    pub exploration: f64,
    pub rollout: Rollout,
    /// Evaluation mode: every root action gets at least this many visits before UCB takes over,
    /// so even bad actions get a usable value estimate (§2.2). 0 = play mode.
    pub root_min_visits: u32,
}

impl Default for IsmctsConfig {
    fn default() -> IsmctsConfig {
        IsmctsConfig {
            exploration: 0.7,
            rollout: Rollout::CheckCallDown,
            root_min_visits: 0,
        }
    }
}

#[derive(Copy, Clone, Debug, Default)]
struct Stats {
    visits: u32,
    sum: f64,
    sum_sq: f64,
}

impl Stats {
    fn mean(&self) -> f64 {
        if self.visits == 0 {
            0.0
        } else {
            self.sum / self.visits as f64
        }
    }

    /// Standard error of the mean.
    fn se(&self) -> f64 {
        if self.visits < 2 {
            return f64::INFINITY;
        }
        let n = self.visits as f64;
        let var = (self.sum_sq / n - self.mean().powi(2)).max(0.0) * n / (n - 1.0);
        (var / n).sqrt()
    }

    fn add(&mut self, r: f64) {
        self.visits += 1;
        self.sum += r;
        self.sum_sq += r * r;
    }
}

/// One information set's statistics: one entry per legal action, in `legal_actions` order.
struct Node<A> {
    actions: SmallVec<[(A, Stats); 8]>,
    visits: u32,
}

/// The searcher. Reusable across decisions; tables are cleared at the start of each search.
pub struct Ismcts<G: Game> {
    pub config: IsmctsConfig,
    /// How opponents' hidden cards are sampled: uniform by default, ranges in spot analysis.
    pub sampler: Box<dyn HiddenSampler + Send>,
    tables: Vec<HashMap<G::Observation, Node<G::Action>>>,
    /// Running reward statistics, for scaling the exploration bonus.
    reward_stats: Stats,
    /// Playouts done in the last search (for reporting and benchmarks).
    pub last_playouts: u32,
}

impl<G: Game> Ismcts<G> {
    pub fn new(config: IsmctsConfig) -> Ismcts<G> {
        Ismcts {
            config,
            sampler: Box::new(UniformSampler),
            tables: Vec::new(),
            reward_stats: Stats::default(),
            last_playouts: 0,
        }
    }

    /// Run a search from `root_obs` and return the root statistics.
    fn search(&mut self, game: &G, root_obs: &G::Observation, budget: Budget, rng: &mut DetRng) {
        let me = game.observer(root_obs);
        self.tables.clear();
        self.reward_stats = Stats::default();

        let start = Instant::now();
        let mut playouts = 0u32;
        loop {
            match budget {
                Budget::Playouts(n) if playouts >= n => break,
                // Checking the clock every iteration is wasteful; every 64 is plenty.
                Budget::Time(t) if playouts.is_multiple_of(64) && start.elapsed() >= t => break,
                _ => {}
            }
            self.iterate(game, root_obs, me, rng);
            playouts += 1;
        }
        self.last_playouts = playouts;
    }

    fn iterate(&mut self, game: &G, root_obs: &G::Observation, me: Seat, rng: &mut DetRng) {
        let mut state = game.determinize(root_obs, self.sampler.as_mut(), rng);
        let mut path: SmallVec<[(Seat, G::Observation, usize); 32]> = SmallVec::new();
        let mut legal = Vec::new();
        let mut expanded = false;
        let mut is_root = true;

        loop {
            let seat = match game.to_act(&state) {
                ToAct::Terminal => break,
                ToAct::Chance => {
                    let a = game.sample_chance(&state, rng);
                    game.apply(&mut state, a);
                    continue;
                }
                ToAct::Seat(p) => p,
            };
            game.legal_actions(&state, &mut legal);
            if expanded {
                // Below the tree: cheap rollout policy.
                let a = self.config.rollout.choose(game, &state, &legal, rng);
                game.apply(&mut state, a);
                continue;
            }
            let obs = game.observe(&state, seat);
            let scale = self.exploration_scale();
            let min_visits = if is_root && seat == me {
                self.config.root_min_visits
            } else {
                0
            };
            // One table per seat, created the first time that seat acts.
            if self.tables.len() <= seat.index() {
                self.tables.resize_with(seat.index() + 1, HashMap::new);
            }
            let table = &mut self.tables[seat.index()];
            let idx = match table.get_mut(&obs) {
                Some(node) => select(node, self.config.exploration * scale, min_visits),
                None => {
                    // Expand: create the node, take its first action, then roll out.
                    let node = Node {
                        actions: legal.iter().map(|&a| (a, Stats::default())).collect(),
                        visits: 0,
                    };
                    table.insert(obs.clone(), node);
                    expanded = true;
                    0
                }
            };
            is_root = false;
            let a = legal[idx];
            path.push((seat, obs, idx));
            game.apply(&mut state, a);
        }

        let unit = game.eval_unit().0 as f64;
        let rewards: SmallVec<[f64; 9]> = game
            .payoffs(&state)
            .iter()
            .map(|c| c.0 as f64 / unit)
            .collect();
        for &r in &rewards {
            self.reward_stats.add(r);
        }
        for (seat, obs, idx) in path {
            let node = self.tables[seat.index()]
                .get_mut(&obs)
                .expect("path node missing");
            node.visits += 1;
            node.actions[idx].1.add(rewards[seat.index()]);
        }
    }

    /// Standard deviation of rewards seen so far (1.0 until there's data).
    fn exploration_scale(&self) -> f64 {
        let s = &self.reward_stats;
        if s.visits < 10 {
            return 1.0;
        }
        let n = s.visits as f64;
        (s.sum_sq / n - (s.sum / n).powi(2)).max(1e-9).sqrt()
    }

    /// Root statistics after a search, in `legal` order.
    fn root_values(
        &self,
        game: &G,
        root_obs: &G::Observation,
        legal: &[G::Action],
    ) -> Vec<ActionValue<G::Action>> {
        let me = game.observer(root_obs);
        let node = self.tables.get(me.index()).and_then(|t| t.get(root_obs));
        legal
            .iter()
            .enumerate()
            .map(|(i, &a)| {
                let st = node.map(|n| n.actions[i].1).unwrap_or_default();
                ActionValue {
                    action: a,
                    q: st.mean(),
                    se: st.se(),
                    visits: st.visits,
                }
            })
            .collect()
    }

    /// Search, then return the most-visited root action (play mode). Ties → earlier in `legal`.
    pub fn best_action(
        &mut self,
        game: &G,
        obs: &G::Observation,
        legal: &[G::Action],
        budget: Budget,
        rng: &mut DetRng,
    ) -> G::Action {
        self.search(game, obs, budget, rng);
        let values = self.root_values(game, obs, legal);
        let mut best = 0;
        for (i, v) in values.iter().enumerate() {
            if v.visits > values[best].visits {
                best = i;
            }
        }
        legal[best]
    }
}

/// UCB1 over a node's actions. Untried actions first; `min_visits` forces round-robin at the root
/// in evaluation mode.
fn select<A>(node: &Node<A>, c: f64, min_visits: u32) -> usize {
    if let Some(i) = node.actions.iter().position(|(_, s)| s.visits == 0) {
        return i;
    }
    if min_visits > 0 {
        let (i, s) = node
            .actions
            .iter()
            .enumerate()
            .min_by_key(|(_, (_, s))| s.visits)
            .unwrap();
        if s.1.visits < min_visits {
            return i;
        }
    }
    let ln_n = (node.visits.max(1) as f64).ln();
    let mut best = (f64::NEG_INFINITY, 0);
    for (i, (_, s)) in node.actions.iter().enumerate() {
        let score = s.mean() + c * (ln_n / s.visits as f64).sqrt();
        if score > best.0 {
            best = (score, i);
        }
    }
    best.1
}

impl<G: Game> Evaluator<G> for Ismcts<G> {
    /// Values for every legal action, best first. Uses evaluation mode at the root (at least
    /// `root_min_visits`, default 32 if unset, per action) so every estimate has a usable SE.
    fn evaluate(
        &mut self,
        game: &G,
        obs: &G::Observation,
        legal: &[G::Action],
        budget: Budget,
        rng: &mut DetRng,
    ) -> Vec<ActionValue<G::Action>> {
        let saved = self.config.root_min_visits;
        if saved == 0 {
            self.config.root_min_visits = 32;
        }
        self.search(game, obs, budget, rng);
        self.config.root_min_visits = saved;
        let mut values = self.root_values(game, obs, legal);
        // Stable sort keeps `legal` order among equal values: deterministic.
        values.sort_by(|a, b| b.q.partial_cmp(&a.q).unwrap_or(std::cmp::Ordering::Equal));
        values
    }
}

#[cfg(test)]
mod tests;
