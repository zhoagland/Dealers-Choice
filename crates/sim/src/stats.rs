//! Online statistics (`docs/tech/sim.md` §2–3).
//!
//! Welford's algorithm keeps a running mean and sum of squared deviations without storing samples
//! and without the catastrophic cancellation of the naive `Σx² − (Σx)²/n`. Chan's formula merges two
//! partial results, which lets per-batch statistics be combined exactly.

use serde::Serialize;

#[derive(Copy, Clone, Debug, Default, PartialEq, Serialize)]
pub struct Welford {
    pub n: u64,
    pub mean: f64,
    m2: f64,
}

impl Welford {
    pub fn add(&mut self, x: f64) {
        self.n += 1;
        let d = x - self.mean;
        self.mean += d / self.n as f64;
        self.m2 += d * (x - self.mean);
    }

    /// Combine with another accumulator (Chan et al.).
    pub fn merge(&mut self, o: &Welford) {
        if o.n == 0 {
            return;
        }
        if self.n == 0 {
            *self = *o;
            return;
        }
        let n = (self.n + o.n) as f64;
        let d = o.mean - self.mean;
        self.mean += d * o.n as f64 / n;
        self.m2 += o.m2 + d * d * self.n as f64 * o.n as f64 / n;
        self.n += o.n;
    }

    /// Sample variance (n − 1 denominator).
    pub fn variance(&self) -> f64 {
        if self.n < 2 {
            0.0
        } else {
            self.m2 / (self.n - 1) as f64
        }
    }

    /// Standard error of the mean.
    pub fn se(&self) -> f64 {
        if self.n < 2 {
            f64::INFINITY
        } else {
            (self.variance() / self.n as f64).sqrt()
        }
    }

    /// Half-width of the 95% confidence interval (normal approximation; fine for n ≳ 30).
    pub fn ci95(&self) -> f64 {
        1.96 * self.se()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_two_pass_formula() {
        let xs = [2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
        let mut w = Welford::default();
        xs.iter().for_each(|&x| w.add(x));
        assert!((w.mean - 5.0).abs() < 1e-12);
        assert!((w.variance() - 32.0 / 7.0).abs() < 1e-12);
    }

    #[test]
    fn merge_equals_single_pass() {
        let xs: Vec<f64> = (0..1000)
            .map(|i| ((i * 7919) % 101) as f64 - 50.0)
            .collect();
        let mut all = Welford::default();
        xs.iter().for_each(|&x| all.add(x));
        let (mut a, mut b) = (Welford::default(), Welford::default());
        xs[..337].iter().for_each(|&x| a.add(x));
        xs[337..].iter().for_each(|&x| b.add(x));
        a.merge(&b);
        assert_eq!(a.n, all.n);
        assert!((a.mean - all.mean).abs() < 1e-9);
        assert!((a.variance() - all.variance()).abs() < 1e-6);
    }
}
