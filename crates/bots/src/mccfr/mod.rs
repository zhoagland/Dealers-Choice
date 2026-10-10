//! External-sampling Monte Carlo CFR (Lanctot et al., 2009). PRD FR-BT-3, `docs/tech/bots.md` §3.
//!
//! **Idea in one paragraph.** CFR repeatedly plays the game against itself. At each information
//! set it tracks *regret*: how much better each action would have done than the current mix.
//! The next mix plays actions in proportion to positive regret (regret matching). In two-player
//! zero-sum games the **average** of all the mixes converges to a Nash equilibrium, with
//! exploitability shrinking like O(1/√T).
//!
//! **External sampling.** Walking the full tree each iteration is too expensive, so each
//! iteration picks one *traverser*. At the traverser's nodes every action is explored (to measure
//! regret); at opponent and chance nodes one action is sampled. That gives unbiased regret
//! estimates at a tiny fraction of the cost.
//!
//! Tables are keyed by `G::Observation`, which is exactly "an information set" by the API contract.

pub mod best_response;
pub mod blueprint;

use std::collections::HashMap;

use smallvec::SmallVec;

use engine::game::sample_weighted;
use engine::{Chips, DetRng, Game, HandSetup, Seat, ToAct};

use crate::strategy::{Probs, regret_matching};
pub use blueprint::{Blueprint, BlueprintPolicy};

/// Training options.
#[derive(Clone, Debug)]
pub struct MccfrConfig {
    pub num_seats: u8,
    pub stack: Chips,
    /// Regret matching⁺: clamp cumulative regrets at zero. Default on: on Kuhn, 200k iterations
    /// reach exploitability < 0.005 with it, ~0.0054 without.
    pub regret_plus: bool,
    /// Weight iteration t's strategy by t in the average, so early (bad) iterations fade.
    pub linear_averaging: bool,
}

impl Default for MccfrConfig {
    fn default() -> MccfrConfig {
        MccfrConfig {
            num_seats: 2,
            stack: Chips(10_000),
            regret_plus: true,
            linear_averaging: true,
        }
    }
}

/// Per-information-set data.
#[derive(Clone, Debug)]
struct InfoSet<A> {
    actions: SmallVec<[A; 8]>,
    regret: Probs,
    strategy_sum: Probs,
}

/// An MCCFR trainer for game `G`.
pub struct Mccfr<G: Game> {
    config: MccfrConfig,
    table: HashMap<G::Observation, InfoSet<G::Action>>,
    iterations: u64,
}

impl<G: Game> Mccfr<G> {
    pub fn new(config: MccfrConfig) -> Mccfr<G> {
        Mccfr {
            config,
            table: HashMap::new(),
            iterations: 0,
        }
    }

    pub fn iterations(&self) -> u64 {
        self.iterations
    }

    pub fn info_sets(&self) -> usize {
        self.table.len()
    }

    /// Run `iterations` more iterations. Each iteration rotates the button (so every position
    /// is trained) and does one traversal per seat.
    pub fn train(&mut self, game: &G, iterations: u64, rng: &mut DetRng) {
        let n = self.config.num_seats;
        let mut legal = Vec::new();
        for _ in 0..iterations {
            self.iterations += 1;
            let mut setup = HandSetup::simple(n, self.config.stack);
            setup.button = Seat((self.iterations % n as u64) as u8);
            setup.chooser = setup.button;
            let root = game.new_hand(&setup);
            for traverser in setup.seats() {
                self.traverse(game, &root, traverser, rng, &mut legal);
            }
        }
    }

    /// Returns the traverser's sampled utility (in eval units) from `state` onward.
    fn traverse(
        &mut self,
        game: &G,
        state: &G::State,
        traverser: Seat,
        rng: &mut DetRng,
        legal: &mut Vec<G::Action>,
    ) -> f64 {
        let seat = match game.to_act(state) {
            ToAct::Terminal => {
                let pay = game.payoffs(state)[traverser.index()];
                return pay.0 as f64 / game.eval_unit().0 as f64;
            }
            ToAct::Chance => {
                let a = game.sample_chance(state, rng);
                let mut next = state.clone();
                game.apply(&mut next, a);
                return self.traverse(game, &next, traverser, rng, legal);
            }
            ToAct::Seat(p) => p,
        };

        game.legal_actions(state, legal);
        let actions: SmallVec<[G::Action; 8]> = legal.iter().copied().collect();
        let obs = game.observe(state, seat);

        // Look up (or create) the info set and copy out its current strategy.
        //
        // Borrowing note: `entry(...)` returns a mutable borrow of `self.table`. We can't keep it
        // while recursing (the recursion needs `&mut self` too), so we copy what we need into
        // locals, let the borrow end, and look the entry up again afterwards. Two hash lookups
        // per node is the price of not fighting the borrow checker.
        let strategy = {
            let node = self.table.entry(obs.clone()).or_insert_with(|| InfoSet {
                actions: actions.clone(),
                regret: actions.iter().map(|_| 0.0).collect(),
                strategy_sum: actions.iter().map(|_| 0.0).collect(),
            });
            debug_assert_eq!(
                node.actions, actions,
                "legal actions changed for one info set"
            );
            regret_matching(&node.regret)
        };

        if seat == traverser {
            // Explore every action to measure regret.
            let mut values: Probs = SmallVec::new();
            for &a in &actions {
                let mut next = state.clone();
                game.apply(&mut next, a);
                values.push(self.traverse(game, &next, traverser, rng, legal));
            }
            let node_value: f64 = strategy.iter().zip(&values).map(|(p, v)| p * v).sum();
            let node = self.table.get_mut(&obs).unwrap();
            for (r, v) in node.regret.iter_mut().zip(&values) {
                *r += v - node_value;
                if self.config.regret_plus {
                    *r = r.max(0.0);
                }
            }
            node_value
        } else {
            // Opponent: accumulate the average strategy here, then sample one action.
            let weight = if self.config.linear_averaging {
                self.iterations as f64
            } else {
                1.0
            };
            let node = self.table.get_mut(&obs).unwrap();
            for (s, p) in node.strategy_sum.iter_mut().zip(&strategy) {
                *s += weight * p;
            }
            let weighted: SmallVec<[(usize, f64); 8]> =
                strategy.iter().copied().enumerate().collect();
            let i = sample_weighted(&weighted, rng);
            let mut next = state.clone();
            game.apply(&mut next, actions[i]);
            self.traverse(game, &next, traverser, rng, legal)
        }
    }

    /// The average strategy so far: the part that converges to equilibrium.
    pub fn blueprint(&self, game: &G) -> Blueprint<G> {
        let mut bp = Blueprint::new(game.meta().id, game.meta().version, self.iterations);
        for (obs, node) in &self.table {
            let total: f64 = node.strategy_sum.iter().sum();
            let probs: Probs = if total > 0.0 {
                node.strategy_sum.iter().map(|s| s / total).collect()
            } else {
                regret_matching(&node.regret)
            };
            bp.insert(
                obs.clone(),
                node.actions.iter().copied().zip(probs).collect(),
            );
        }
        bp
    }
}

#[cfg(test)]
mod tests {
    use super::best_response::{expected_values, exploitability};
    use super::*;
    use crate::strategy::UniformStrategy;
    use engine::rng::rng_from_seed;
    use games::kuhn::{self, Kuhn};
    use games::leduc::{self, Leduc};

    /// NFR-2: MCCFR on Kuhn converges to (near) equilibrium.
    #[test]
    fn kuhn_converges_to_equilibrium() {
        let game = Kuhn::new(kuhn::Config {});
        let mut trainer = Mccfr::new(MccfrConfig {
            regret_plus: true,
            ..MccfrConfig::default()
        });
        trainer.train(&game, 200_000, &mut rng_from_seed(1));
        let bp = trainer.blueprint(&game);
        assert_eq!(
            trainer.info_sets(),
            24,
            "6 info sets per seat per button position"
        );

        let expl = exploitability(&game, &bp);
        assert!(expl < 0.005, "exploitability {expl}");

        // First player's value is -1/18 at equilibrium. Button = seat 1 → seat 0 acts first.
        let setup = HandSetup {
            button: Seat(1),
            ..HandSetup::simple(2, Chips(100))
        };
        let v = expected_values(&game, &setup, &bp)[0];
        assert!((v - (-1.0 / 18.0)).abs() < 0.01, "first-player value {v}");

        // Equilibrium family: P(bet | K) = 3·P(bet | J), and Q always checks.
        let bet = |rank| first_player_bet_prob(&bp, rank);
        let (j, q, k) = (
            bet(engine::Rank::Jack),
            bet(engine::Rank::Queen),
            bet(engine::Rank::King),
        );
        assert!(j <= 1.0 / 3.0 + 0.05, "P(bet|J) = {j}");
        assert!(
            (k - 3.0 * j).abs() < 0.1,
            "P(bet|K) = {k}, 3·P(bet|J) = {}",
            3.0 * j
        );
        assert!(q < 0.05, "P(bet|Q) = {q}");
    }

    /// Opening bet probability for the first player holding `rank`, averaged over both buttons.
    fn first_player_bet_prob(bp: &Blueprint<Kuhn>, rank: engine::Rank) -> f64 {
        let mut total = 0.0;
        let mut n = 0.0;
        for (obs, dist) in bp.iter() {
            let first = obs.button.next(2) == obs.seat;
            if first && obs.history.is_empty() && obs.own.map(|c| c.rank()) == Some(rank) {
                total += dist
                    .iter()
                    .find(|(a, _)| *a == kuhn::Action::Bet)
                    .unwrap()
                    .1;
                n += 1.0;
            }
        }
        assert!(n > 0.0);
        total / n
    }

    #[test]
    fn blueprint_round_trips_and_checks_version() {
        let game = Kuhn::new(kuhn::Config {});
        let mut trainer = Mccfr::new(MccfrConfig::default());
        trainer.train(&game, 2_000, &mut rng_from_seed(1));
        let bp = trainer.blueprint(&game);
        let bytes = bp.to_bytes();
        assert_eq!(
            bytes,
            trainer.blueprint(&game).to_bytes(),
            "deterministic encoding"
        );
        let back = Blueprint::from_bytes(&game, &bytes).unwrap();
        assert_eq!(back.len(), bp.len());
        assert!((exploitability(&game, &back) - exploitability(&game, &bp)).abs() < 1e-12);
        // A Leduc blueprint must not load as Kuhn.
        let leduc = Leduc::new(leduc::Config {});
        let mut t2 = Mccfr::new(MccfrConfig::default());
        t2.train(&leduc, 10, &mut rng_from_seed(1));
        assert!(Blueprint::from_bytes(&game, &t2.blueprint(&leduc).to_bytes()).is_err());
    }

    /// Leduc is bigger and has two rounds: just check that training clearly reduces exploitability.
    #[test]
    fn leduc_exploitability_drops() {
        let game = Leduc::new(leduc::Config {});
        let uniform = exploitability(&game, &UniformStrategy);
        let mut trainer = Mccfr::new(MccfrConfig::default());
        trainer.train(&game, 20_000, &mut rng_from_seed(2));
        let trained = exploitability(&game, &trainer.blueprint(&game));
        assert!(
            trained < uniform / 5.0,
            "uniform {uniform}, trained {trained}"
        );
    }
}
