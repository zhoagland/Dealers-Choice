//! Balance statistics for a run (FR-SP-4, `docs/tech/sim-program.md` §4.1): how swingy a game is,
//! how big its pots get, how often it reaches showdown, how position matters, and (multi-hand
//! games) how many hands a game lasts. Collected from every hand, mergeable across chunks.

use serde::{Deserialize, Serialize};

use crate::stats::Welford;

/// Pot-size histogram buckets, in eval units: [0,1), [1,2), … the last bucket is open-ended.
pub const POT_BUCKETS: usize = 200;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BalanceStats {
    pub hands: u64,
    /// Games played (each multi-hand game counts once; single-hand games equal `hands`).
    pub games: u64,
    /// Every seat's result per hand (eval units): its standard deviation is the swinginess.
    pub result: Welford,
    /// Chips won by the winners of a hand (eval units).
    pub pot: Welford,
    pub pot_hist: Vec<u64>,
    pub showdowns: u64,
    /// Result per hand by position: 0 = first seat left of the button, last = the button.
    pub by_position: Vec<Welford>,
    pub hands_per_game: Welford,
}

impl BalanceStats {
    /// Record one finished hand: each seat's result (eval units) by position from the button,
    /// the pot, and whether two or more hands were shown.
    pub fn add_hand(&mut self, results_by_position: &[f64], showdown: bool) {
        self.hands += 1;
        if self.by_position.len() < results_by_position.len() {
            self.by_position
                .resize(results_by_position.len(), Welford::default());
        }
        let mut pot = 0.0;
        for (i, &r) in results_by_position.iter().enumerate() {
            self.result.add(r);
            self.by_position[i].add(r);
            if r > 0.0 {
                pot += r;
            }
        }
        self.pot.add(pot);
        if self.pot_hist.len() < POT_BUCKETS {
            self.pot_hist.resize(POT_BUCKETS, 0);
        }
        let b = (pot.max(0.0) as usize).min(POT_BUCKETS - 1);
        self.pot_hist[b] += 1;
        if showdown {
            self.showdowns += 1;
        }
    }

    /// Record a finished game of `hands` hands.
    pub fn add_game(&mut self, hands: u64) {
        self.games += 1;
        self.hands_per_game.add(hands as f64);
    }

    pub fn merge(&mut self, o: &BalanceStats) {
        self.hands += o.hands;
        self.games += o.games;
        self.result.merge(&o.result);
        self.pot.merge(&o.pot);
        if self.pot_hist.len() < o.pot_hist.len() {
            self.pot_hist.resize(o.pot_hist.len(), 0);
        }
        for (a, b) in self.pot_hist.iter_mut().zip(&o.pot_hist) {
            *a += b;
        }
        self.showdowns += o.showdowns;
        if self.by_position.len() < o.by_position.len() {
            self.by_position
                .resize(o.by_position.len(), Welford::default());
        }
        for (a, b) in self.by_position.iter_mut().zip(&o.by_position) {
            a.merge(b);
        }
        self.hands_per_game.merge(&o.hands_per_game);
    }

    /// The pot size (eval units) below which `q` of hands fall, from the histogram.
    pub fn pot_quantile(&self, q: f64) -> f64 {
        let total: u64 = self.pot_hist.iter().sum();
        if total == 0 {
            return 0.0;
        }
        let target = (q * total as f64).ceil() as u64;
        let mut seen = 0;
        for (i, &c) in self.pot_hist.iter().enumerate() {
            seen += c;
            if seen >= target {
                return (i + 1) as f64;
            }
        }
        POT_BUCKETS as f64
    }

    /// Share of hands that reached a showdown, with its 95 % Wilson interval.
    pub fn showdown_rate(&self) -> (f64, Option<(f64, f64)>) {
        let r = crate::analytics::Ratio {
            hits: self.showdowns,
            chances: self.hands,
        };
        (self.showdowns as f64 / self.hands.max(1) as f64, r.ci95())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_and_quantiles() {
        let mut a = BalanceStats::default();
        a.add_hand(&[2.0, -1.0, -1.0], true);
        a.add_hand(&[-0.5, 0.5, 0.0], false);
        a.add_game(2);
        let mut b = BalanceStats::default();
        b.add_hand(&[10.0, -10.0, 0.0], true);
        b.add_game(1);
        a.merge(&b);
        assert_eq!(a.hands, 3);
        assert_eq!(a.games, 2);
        assert_eq!(a.showdowns, 2);
        assert_eq!(a.by_position.len(), 3);
        assert!((a.pot.mean - (2.0 + 0.5 + 10.0) / 3.0).abs() < 1e-9);
        assert_eq!(a.pot_quantile(0.9), 11.0);
        assert!((a.hands_per_game.mean - 1.5).abs() < 1e-9);
    }
}
