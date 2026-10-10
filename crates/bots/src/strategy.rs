//! Strategies as explicit probability tables, for algorithms that need probabilities instead of
//! one sampled action: best response, exploitability, expected values.

use smallvec::SmallVec;

use engine::Game;

/// Up to 8 probabilities without a heap allocation (no game here has more legal actions per menu).
pub type Probs = SmallVec<[f64; 8]>;

/// A (possibly mixed) strategy for **every** seat: given a seat's observation and the legal
/// actions, the probability of each action. Seats are told apart by their observation, which
/// always includes the seat.
pub trait Strategy<G: Game> {
    /// One probability per entry of `legal`, same order, summing to 1.
    fn probs(&self, game: &G, obs: &G::Observation, legal: &[G::Action]) -> Probs;
}

/// Every legal action equally likely.
#[derive(Copy, Clone, Debug, Default)]
pub struct UniformStrategy;

impl<G: Game> Strategy<G> for UniformStrategy {
    fn probs(&self, _: &G, _: &G::Observation, legal: &[G::Action]) -> Probs {
        let p = 1.0 / legal.len() as f64;
        legal.iter().map(|_| p).collect()
    }
}

/// Regret matching: play each action in proportion to its positive regret; uniform if none is
/// positive. The core update of CFR.
pub fn regret_matching(regrets: &[f64]) -> Probs {
    let positive: f64 = regrets.iter().map(|&r| r.max(0.0)).sum();
    if positive > 0.0 {
        regrets.iter().map(|&r| r.max(0.0) / positive).collect()
    } else {
        let p = 1.0 / regrets.len() as f64;
        regrets.iter().map(|_| p).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regret_matching_cases() {
        assert_eq!(regret_matching(&[1.0, 3.0]).as_slice(), &[0.25, 0.75]);
        assert_eq!(regret_matching(&[-1.0, 2.0]).as_slice(), &[0.0, 1.0]);
        assert_eq!(regret_matching(&[-1.0, -2.0]).as_slice(), &[0.5, 0.5]);
    }
}
