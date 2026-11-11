//! Bot strength and style (PRD FR-BT-5/6, `docs/tech/bots.md` §4).
//!
//! **Strength** = how close to the best action the bot plays:
//! `P(a) ∝ exp((Q(a) + bias(a)) / τ)`. Q is in eval units (big blinds or antes), so τ means the same
//! thing in every game: an action τ worse than the best is 1/e as likely. τ = 0 means
//! always play the best (most-searched) action. A smaller search budget also weakens play, because
//! the Q estimates get noisier.
//!
//! **Style** shifts preferences independently of strength: `aggression` is added to the value of
//! bets and raises, so a strong aggressive bot and a weak passive bot are both expressible.

use std::time::Duration;

use smallvec::SmallVec;

use engine::game::sample_weighted;
use engine::{ActionClass, DetRng, Game, UniformSampler};

use crate::ismcts::{Ismcts, IsmctsConfig};
use crate::policy::{Budget, Evaluator, Policy};

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Strength {
    /// Softmax temperature in eval units. 0 = always best.
    pub tau: f64,
    pub budget: Budget,
}

impl Strength {
    /// Presets 1..=10 (bots.md §4 table). `interactive` picks the wall-clock budget for play;
    /// otherwise a deterministic playout budget for sims, tests, and the coach.
    pub fn preset(level: u8, interactive: bool) -> Strength {
        // (τ, playouts, milliseconds)
        const TABLE: [(f64, u32, u64); 10] = [
            (8.0, 500, 50),
            (4.0, 700, 75),
            (2.0, 1_000, 100),
            (1.0, 2_000, 175),
            (0.5, 3_000, 250),
            (0.25, 6_000, 375),
            (0.15, 10_000, 500),
            (0.08, 15_000, 800),
            (0.04, 20_000, 1_100),
            (0.0, 30_000, 1_500),
        ];
        let (tau, playouts, ms) = TABLE[(level.clamp(1, 10) - 1) as usize];
        let budget = if interactive {
            Budget::Time(Duration::from_millis(ms))
        } else {
            Budget::Playouts(playouts)
        };
        Strength { tau, budget }
    }
}

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Style {
    /// Added to Q (eval units) for bets and raises. Positive = more aggressive.
    pub aggression: f64,
}

/// An ISMCTS bot with a strength and a style.
pub struct IsmctsBot<G: Game> {
    search: Ismcts<G>,
    pub strength: Strength,
    pub style: Style,
    label: String,
}

impl<G: Game> IsmctsBot<G> {
    pub fn new(strength: Strength, style: Style, label: impl Into<String>) -> IsmctsBot<G> {
        IsmctsBot {
            search: Ismcts::new(IsmctsConfig::default()),
            strength,
            style,
            label: label.into(),
        }
    }

    /// Preset `level` with neutral style, named `"ismcts:<level>"`.
    pub fn preset(level: u8, interactive: bool) -> IsmctsBot<G> {
        IsmctsBot::new(
            Strength::preset(level, interactive),
            Style::default(),
            format!("ismcts:{level}"),
        )
    }

    /// Playouts in the most recent decision.
    pub fn last_playouts(&self) -> u32 {
        self.search.last_playouts
    }
}

impl<G: Game> Policy<G> for IsmctsBot<G> {
    fn act(
        &mut self,
        game: &G,
        obs: &G::Observation,
        legal: &[G::Action],
        rng: &mut DetRng,
    ) -> G::Action {
        if legal.len() == 1 {
            return legal[0];
        }
        if self.strength.tau == 0.0 && self.style.aggression == 0.0 {
            return self
                .search
                .best_action(game, obs, legal, self.strength.budget, rng);
        }
        let values = self
            .search
            .evaluate(game, obs, legal, self.strength.budget, rng);
        // Action classes are public, so any determinization classifies them the same way.
        let sample_state = game.determinize(obs, &mut UniformSampler, rng);
        let scored: SmallVec<[(G::Action, f64); 8]> = values
            .iter()
            .map(|v| {
                let bias = match game.classify(&sample_state, v.action) {
                    ActionClass::BetOrRaise { .. } => self.style.aggression,
                    _ => 0.0,
                };
                (v.action, v.q + bias)
            })
            .collect();
        softmax_sample(&scored, self.strength.tau, rng)
    }

    fn name(&self) -> String {
        self.label.clone()
    }
}

/// Sample from softmax(score / τ). τ = 0 → the max (first on ties). Subtracting the max before
/// `exp` keeps the numbers in range: exp of a large positive number would overflow to infinity.
pub fn softmax_sample<A: Copy>(scored: &[(A, f64)], tau: f64, rng: &mut DetRng) -> A {
    let max = scored
        .iter()
        .map(|&(_, s)| s)
        .fold(f64::NEG_INFINITY, f64::max);
    if tau <= 0.0 {
        return scored.iter().find(|&&(_, s)| s == max).unwrap().0;
    }
    let weights: SmallVec<[(A, f64); 8]> = scored
        .iter()
        .map(|&(a, s)| (a, ((s - max) / tau).exp()))
        .collect();
    sample_weighted(&weights, rng)
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::rng::rng_from_seed;

    #[test]
    fn softmax_extremes() {
        let mut rng = rng_from_seed(1);
        let scored = [('a', 1.0), ('b', 3.0), ('c', 2.0)];
        assert_eq!(softmax_sample(&scored, 0.0, &mut rng), 'b');
        // Very high τ ≈ uniform: every action shows up.
        let mut seen = std::collections::HashSet::new();
        for _ in 0..300 {
            seen.insert(softmax_sample(&scored, 1e6, &mut rng));
        }
        assert_eq!(seen.len(), 3);
    }

    #[test]
    fn softmax_ratio_matches_temperature() {
        // Two actions 1 unit apart at τ = 1: P(best)/P(other) = e.
        let mut rng = rng_from_seed(2);
        let scored = [(0, 0.0), (1, 1.0)];
        let n = 40_000;
        let best = (0..n)
            .filter(|_| softmax_sample(&scored, 1.0, &mut rng) == 1)
            .count() as f64;
        let ratio = best / (n as f64 - best);
        assert!((ratio - std::f64::consts::E).abs() < 0.15, "ratio {ratio}");
    }

    #[test]
    fn presets_are_monotone() {
        for l in 1..10 {
            let (a, b) = (Strength::preset(l, false), Strength::preset(l + 1, false));
            assert!(b.tau <= a.tau);
            let (Budget::Playouts(pa), Budget::Playouts(pb)) = (a.budget, b.budget) else {
                panic!()
            };
            assert!(pb >= pa);
        }
    }
}
