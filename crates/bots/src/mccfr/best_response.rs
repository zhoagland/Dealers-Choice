//! Exact best response and exploitability for small two-player games (Kuhn, Leduc).
//! `docs/tech/bots.md` §3. This is how "how close to equilibrium is this bot?" gets measured.
//!
//! **Best response (BR)** to a strategy σ: the strategy that maximizes a seat's expected value
//! when everyone else plays σ. **Exploitability** of σ: how much a best-responding opponent gains,
//! averaged over seats and positions. It's 0 exactly at a Nash equilibrium.
//!
//! **Why this is harder than ordinary expectimax.** The best responder doesn't see the opponent's
//! cards, so it must pick *one* action per information set, not one per game state. The value of
//! an action at an information set I is Σ over the states h in I of P(reach h | others) · value(h, a).
//!
//! Algorithm:
//! 1. Walk the whole tree once, grouping the BR seat's decision states by information set, and
//!    recording each state's reach probability from chance and the opponent.
//! 2. Decide information sets **deepest first**. With perfect recall, every BR decision below I
//!    is deeper than I, so it's already decided when I is evaluated.
//! 3. Evaluate the root with all decisions fixed.
//!
//! Cost is roughly (tree size × depth): instant for Kuhn, about a second for Leduc. Far too big
//! for Hold'em, which is why it's only used on the test games.

use std::collections::HashMap;

use engine::rng::{Stream, derive_rng};
use engine::{Chips, Game, HandSetup, Seat, ToAct};

use crate::policy::Policy;
use crate::strategy::{Probs, Strategy};

/// The BR seat's decision states for one information set.
struct Group<S> {
    depth: usize,
    /// (state, probability that chance and the opponent reach it)
    states: Vec<(S, f64)>,
}

/// Value (in eval units) that `br_seat` gets by best-responding to `others`, from `setup`.
pub fn best_response_value<G: Game>(
    game: &G,
    setup: &HandSetup,
    br_seat: Seat,
    others: &dyn Strategy<G>,
) -> f64 {
    let root = game.new_hand(setup);

    // Step 1: group the BR seat's states by information set.
    let mut groups: HashMap<G::Observation, Group<G::State>> = HashMap::new();
    collect(game, &root, br_seat, others, 1.0, 0, &mut groups);

    // Step 2: decide deepest-first.
    let mut order: Vec<(&G::Observation, &Group<G::State>)> = groups.iter().collect();
    order.sort_by(|a, b| b.1.depth.cmp(&a.1.depth));
    let mut choice: HashMap<G::Observation, G::Action> = HashMap::new();
    let mut legal = Vec::new();
    for (obs, group) in order {
        game.legal_actions(&group.states[0].0, &mut legal);
        let mut best = (f64::NEG_INFINITY, legal[0]);
        for &a in &legal {
            let value: f64 = group
                .states
                .iter()
                .map(|(s, reach)| {
                    let mut next = s.clone();
                    game.apply(&mut next, a);
                    reach * evaluate(game, &next, br_seat, others, &choice)
                })
                .sum();
            // Strict `>` keeps the first action on ties, so results are deterministic.
            if value > best.0 {
                best = (value, a);
            }
        }
        choice.insert(obs.clone(), best.1);
    }

    // Step 3.
    evaluate(game, &root, br_seat, others, &choice)
}

fn collect<G: Game>(
    game: &G,
    s: &G::State,
    br_seat: Seat,
    others: &dyn Strategy<G>,
    reach: f64,
    depth: usize,
    groups: &mut HashMap<G::Observation, Group<G::State>>,
) {
    let mut legal = Vec::new();
    match game.to_act(s) {
        ToAct::Terminal => {}
        ToAct::Chance => {
            let mut outcomes = Vec::new();
            game.chance_outcomes(s, &mut outcomes);
            for (a, p) in outcomes {
                let mut next = s.clone();
                game.apply(&mut next, a);
                collect(game, &next, br_seat, others, reach * p, depth, groups);
            }
        }
        ToAct::Seat(p) if p == br_seat => {
            let group = groups.entry(game.observe(s, p)).or_insert_with(|| Group {
                depth,
                states: Vec::new(),
            });
            group.states.push((s.clone(), reach));
            game.legal_actions(s, &mut legal);
            for &a in &legal {
                let mut next = s.clone();
                game.apply(&mut next, a);
                collect(game, &next, br_seat, others, reach, depth + 1, groups);
            }
        }
        ToAct::Seat(p) => {
            game.legal_actions(s, &mut legal);
            let probs = others.probs(game, &game.observe(s, p), &legal);
            for (&a, &prob) in legal.iter().zip(&probs) {
                if prob > 0.0 {
                    let mut next = s.clone();
                    game.apply(&mut next, a);
                    collect(game, &next, br_seat, others, reach * prob, depth, groups);
                }
            }
        }
    }
}

/// Expected value for `br_seat` from `s`, with the BR seat following `choice` and everyone else
/// following `others`.
fn evaluate<G: Game>(
    game: &G,
    s: &G::State,
    br_seat: Seat,
    others: &dyn Strategy<G>,
    choice: &HashMap<G::Observation, G::Action>,
) -> f64 {
    let mut legal = Vec::new();
    match game.to_act(s) {
        ToAct::Terminal => to_units(game, game.payoffs(s)[br_seat.index()]),
        ToAct::Chance => {
            let mut outcomes = Vec::new();
            game.chance_outcomes(s, &mut outcomes);
            outcomes
                .into_iter()
                .map(|(a, p)| {
                    let mut next = s.clone();
                    game.apply(&mut next, a);
                    p * evaluate(game, &next, br_seat, others, choice)
                })
                .sum()
        }
        ToAct::Seat(p) if p == br_seat => {
            let a = *choice
                .get(&game.observe(s, p))
                .expect("deeper info set not decided yet; perfect recall violated?");
            let mut next = s.clone();
            game.apply(&mut next, a);
            evaluate(game, &next, br_seat, others, choice)
        }
        ToAct::Seat(p) => {
            game.legal_actions(s, &mut legal);
            let probs = others.probs(game, &game.observe(s, p), &legal);
            legal
                .iter()
                .zip(&probs)
                .filter(|(_, prob)| **prob > 0.0)
                .map(|(&a, &prob)| {
                    let mut next = s.clone();
                    game.apply(&mut next, a);
                    prob * evaluate(game, &next, br_seat, others, choice)
                })
                .sum()
        }
    }
}

/// Expected value per seat (eval units) when every seat plays `strategy`.
pub fn expected_values<G: Game>(
    game: &G,
    setup: &HandSetup,
    strategy: &dyn Strategy<G>,
) -> Vec<f64> {
    fn walk<G: Game>(
        game: &G,
        s: &G::State,
        strategy: &dyn Strategy<G>,
        out: &mut [f64],
        weight: f64,
    ) {
        let mut legal = Vec::new();
        match game.to_act(s) {
            ToAct::Terminal => {
                for (o, pay) in out.iter_mut().zip(game.payoffs(s)) {
                    *o += weight * to_units(game, pay);
                }
            }
            ToAct::Chance => {
                let mut outcomes = Vec::new();
                game.chance_outcomes(s, &mut outcomes);
                for (a, p) in outcomes {
                    let mut next = s.clone();
                    game.apply(&mut next, a);
                    walk(game, &next, strategy, out, weight * p);
                }
            }
            ToAct::Seat(p) => {
                game.legal_actions(s, &mut legal);
                let probs = strategy.probs(game, &game.observe(s, p), &legal);
                for (&a, &prob) in legal.iter().zip(&probs) {
                    if prob > 0.0 {
                        let mut next = s.clone();
                        game.apply(&mut next, a);
                        walk(game, &next, strategy, out, weight * prob);
                    }
                }
            }
        }
    }
    let mut out = vec![0.0; setup.num_seats as usize];
    walk(game, &game.new_hand(setup), strategy, &mut out, 1.0);
    out
}

/// Exploitability of `strategy` (eval units per hand) in a two-player game, averaged over both
/// button positions: mean over buttons of (BR₀ + BR₁) / 2. Zero exactly at a Nash equilibrium.
pub fn exploitability<G: Game>(game: &G, strategy: &dyn Strategy<G>) -> f64 {
    let mut total = 0.0;
    for button in [Seat(0), Seat(1)] {
        let setup = HandSetup {
            button,
            chooser: button,
            ..HandSetup::simple(2, Chips(10_000))
        };
        let br0 = best_response_value(game, &setup, Seat(0), strategy);
        let br1 = best_response_value(game, &setup, Seat(1), strategy);
        total += (br0 + br1) / 2.0;
    }
    total / 2.0
}

/// Every information set reachable from `setup`, with its legal actions. Small games only (it
/// walks the whole tree). Used to turn a sampling bot into a strategy table.
pub fn information_sets<G: Game>(
    game: &G,
    setup: &HandSetup,
) -> HashMap<G::Observation, Vec<G::Action>> {
    fn walk<G: Game>(game: &G, s: &G::State, out: &mut HashMap<G::Observation, Vec<G::Action>>) {
        let mut legal = Vec::new();
        game.legal_actions(s, &mut legal);
        match game.to_act(s) {
            ToAct::Terminal => return,
            ToAct::Chance => {}
            ToAct::Seat(p) => {
                out.entry(game.observe(s, p))
                    .or_insert_with(|| legal.clone());
            }
        }
        for &a in &legal {
            let mut next = s.clone();
            game.apply(&mut next, a);
            walk(game, &next, out);
        }
    }
    let mut out = HashMap::new();
    walk(game, &game.new_hand(setup), &mut out);
    out
}

/// A strategy table built by asking a stochastic policy what it would do, `samples` times per
/// information set, for both button positions. This is how exploitability gets measured for
/// bots like ISMCTS that have no explicit strategy. Sampling noise is of order 1/√samples.
pub struct SampledStrategy<G: Game> {
    table: HashMap<G::Observation, Probs>,
}

impl<G: Game> SampledStrategy<G> {
    pub fn from_policy(
        game: &G,
        policy: &mut dyn Policy<G>,
        samples: u32,
        seed: u64,
    ) -> SampledStrategy<G> {
        let mut table = HashMap::new();
        for button in [Seat(0), Seat(1)] {
            let setup = HandSetup {
                button,
                chooser: button,
                ..HandSetup::simple(2, Chips(10_000))
            };
            // HashMap iteration order differs between runs; sort so seeds map to the same info sets.
            let mut sets: Vec<_> = information_sets(game, &setup).into_iter().collect();
            sets.sort_by_key(|(obs, _)| format!("{obs:?}"));
            for (i, (obs, legal)) in sets.into_iter().enumerate() {
                let mut rng = derive_rng(seed, i as u64, Stream::Custom(button.0 as u32));
                let mut counts = vec![0u32; legal.len()];
                for _ in 0..samples {
                    let a = policy.act(game, &obs, &legal, &mut rng);
                    counts[legal.iter().position(|&l| l == a).unwrap()] += 1;
                }
                table.insert(
                    obs,
                    counts.iter().map(|&c| c as f64 / samples as f64).collect(),
                );
            }
        }
        SampledStrategy { table }
    }
}

impl<G: Game> Strategy<G> for SampledStrategy<G> {
    fn probs(&self, _: &G, obs: &G::Observation, legal: &[G::Action]) -> Probs {
        match self.table.get(obs) {
            Some(p) => p.clone(),
            None => legal.iter().map(|_| 1.0 / legal.len() as f64).collect(),
        }
    }
}

fn to_units<G: Game>(game: &G, chips: Chips) -> f64 {
    chips.0 as f64 / game.eval_unit().0 as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategy::Probs;
    use engine::Rank;
    use games::kuhn::{Action, Config, Kuhn, Observation};

    /// A known Kuhn Nash equilibrium (α = 0), written out by hand from the literature.
    struct KuhnNash;

    impl Strategy<Kuhn> for KuhnNash {
        fn probs(&self, _: &Kuhn, obs: &Observation, legal: &[Action]) -> Probs {
            use Action::*;
            let rank = obs.own.unwrap().rank();
            // Probability of the aggressive option (Bet, or Call when facing a bet).
            let aggressive = match (obs.history.as_slice(), rank) {
                // First player, opening: with α = 0, never bet (not even K).
                ([], _) => 0.0,
                // Second player after a check: bet K, bluff J 1/3, check Q.
                ([Check], Rank::King) => 1.0,
                ([Check], Rank::Jack) => 1.0 / 3.0,
                ([Check], _) => 0.0,
                // Facing a bet: call K, call Q with 1/3 (α + 1/3), fold J.
                (_, Rank::King) => 1.0,
                (_, Rank::Queen) => 1.0 / 3.0,
                (_, _) => 0.0,
            };
            legal
                .iter()
                .map(|a| {
                    if matches!(a, Bet | Call) {
                        aggressive
                    } else {
                        1.0 - aggressive
                    }
                })
                .collect()
        }
    }

    #[test]
    fn known_equilibrium_has_zero_exploitability() {
        let game = Kuhn::new(Config {});
        let e = exploitability(&game, &KuhnNash);
        assert!(e.abs() < 1e-12, "exploitability {e}");
    }

    #[test]
    fn known_equilibrium_has_value_minus_one_eighteenth() {
        let game = Kuhn::new(Config {});
        let setup = HandSetup {
            button: Seat(1),
            ..HandSetup::simple(2, Chips(100))
        };
        let v = expected_values(&game, &setup, &KuhnNash);
        assert!(
            (v[0] + 1.0 / 18.0).abs() < 1e-12,
            "first player value {}",
            v[0]
        );
        assert!((v[0] + v[1]).abs() < 1e-12, "zero-sum");
    }

    #[test]
    fn uniform_is_exploitable() {
        let game = Kuhn::new(Config {});
        assert!(exploitability(&game, &crate::strategy::UniformStrategy) > 0.1);
    }
}
