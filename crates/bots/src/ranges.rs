//! Hold'em hand ranges and a range-weighted hidden-card sampler (PRD FR-EV-2, `bots.md` §5).
//!
//! Notation (comma separated, each item optionally `:weight`, default 1):
//! - `AA`, `AKs`, `AKo`, `AK` (= suited + offsuit), `AhKh` (one exact combo)
//! - `TT+` (TT, JJ, …, AA), `A2s+` (A2s … AKs), `KTo+` (KTo, KJo, KQo)
//! - `TT-77`, `A5s-A2s` (inclusive spans)
//!
//! A range is a weight for each of the 1,326 two-card combos. Sampling picks a combo in proportion
//! to its weight, among combos whose cards are both still available (card removal).

use std::collections::BTreeMap;

use engine::game::sample_weighted;
use engine::{Card, CardSet, CardVec, DetRng, HiddenSampler, Rank, Seat, Suit, UniformSampler};

/// A two-card combo, stored with the lower card first so each combo has one key.
pub type Combo = (Card, Card);

fn combo(a: Card, b: Card) -> Combo {
    if a < b { (a, b) } else { (b, a) }
}

/// Weights over two-card combos. `BTreeMap` (sorted) rather than `HashMap` so iteration order,
/// and therefore sampling, is deterministic.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Range {
    weights: BTreeMap<Combo, f64>,
}

impl Range {
    /// Every combo with weight 1.
    pub fn any() -> Range {
        let cards: Vec<Card> = CardSet::FULL_DECK.iter().collect();
        let mut r = Range::default();
        for i in 0..cards.len() {
            for j in i + 1..cards.len() {
                r.weights.insert((cards[i], cards[j]), 1.0);
            }
        }
        r
    }

    pub fn parse(text: &str) -> Result<Range, String> {
        let mut r = Range::default();
        for item in text.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let (body, weight) = match item.split_once(':') {
                Some((b, w)) => (
                    b.trim(),
                    w.trim()
                        .parse::<f64>()
                        .map_err(|_| format!("bad weight in {item:?}"))?,
                ),
                None => (item, 1.0),
            };
            if !(0.0..=1.0).contains(&weight) {
                return Err(format!("weight must be 0..=1 in {item:?}"));
            }
            for c in expand(body)? {
                r.weights.insert(c, weight);
            }
        }
        if r.weights.is_empty() {
            return Err("empty range".into());
        }
        Ok(r)
    }

    /// Number of combos with nonzero weight.
    pub fn len(&self) -> usize {
        self.weights.values().filter(|&&w| w > 0.0).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn weight(&self, a: Card, b: Card) -> f64 {
        self.weights.get(&combo(a, b)).copied().unwrap_or(0.0)
    }

    /// Weighted combos whose cards are both in `available`.
    pub fn available(&self, available: CardSet) -> Vec<(Combo, f64)> {
        self.weights
            .iter()
            .filter(|((a, b), w)| **w > 0.0 && available.contains(*a) && available.contains(*b))
            .map(|(&c, &w)| (c, w))
            .collect()
    }
}

/// Expand one notation item (without weight) into combos.
fn expand(item: &str) -> Result<Vec<Combo>, String> {
    let bad = || format!("can't read range item {item:?}");
    let chars: Vec<char> = item.chars().collect();
    // Exact combo: "AhKh".
    if chars.len() == 4 && Suit::from_char(chars[1]).is_some() {
        let a: Card = item[0..2].parse().map_err(|_| bad())?;
        let b: Card = item[2..4].parse().map_err(|_| bad())?;
        if a == b {
            return Err(bad());
        }
        return Ok(vec![combo(a, b)]);
    }
    // Spans: "TT-77", "A5s-A2s".
    if let Some((hi, lo)) = item.split_once('-') {
        let (h1, h2, hs) = shape(hi).ok_or_else(bad)?;
        let (l1, l2, ls) = shape(lo).ok_or_else(bad)?;
        if hs != ls {
            return Err(bad());
        }
        let mut out = Vec::new();
        if h1 == h2 && l1 == l2 {
            for r in l1 as u8..=h1 as u8 {
                out.extend(class(Rank::ALL[r as usize], Rank::ALL[r as usize], hs));
            }
        } else if h1 == l1 {
            for k in l2 as u8..=h2 as u8 {
                out.extend(class(h1, Rank::ALL[k as usize], hs));
            }
        } else {
            return Err(bad());
        }
        return Ok(out);
    }
    // "+" forms.
    if let Some(base) = item.strip_suffix('+') {
        let (r1, r2, s) = shape(base).ok_or_else(bad)?;
        let mut out = Vec::new();
        if r1 == r2 {
            for r in r1 as u8..=Rank::Ace as u8 {
                out.extend(class(Rank::ALL[r as usize], Rank::ALL[r as usize], s));
            }
        } else {
            // Kicker rises up to one below the top card: A2s+ = A2s..AKs.
            for k in r2 as u8..r1 as u8 {
                out.extend(class(r1, Rank::ALL[k as usize], s));
            }
        }
        return Ok(out);
    }
    let (r1, r2, s) = shape(item).ok_or_else(bad)?;
    Ok(class(r1, r2, s))
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum Suitedness {
    Suited,
    Offsuit,
    Any,
}

/// Parse "AK", "AKs", "AKo", "TT" into (high rank, low rank, suitedness).
fn shape(s: &str) -> Option<(Rank, Rank, Suitedness)> {
    let c: Vec<char> = s.chars().collect();
    let (a, b) = (Rank::from_char(*c.first()?)?, Rank::from_char(*c.get(1)?)?);
    let suit = match c.get(2) {
        None => Suitedness::Any,
        Some('s') => Suitedness::Suited,
        Some('o') => Suitedness::Offsuit,
        _ => return None,
    };
    if c.len() > 3 || (a == b && suit != Suitedness::Any) {
        return None;
    }
    Some((a.max(b), a.min(b), suit))
}

/// All combos of a hand class: a pair has 6, suited 4, offsuit 12.
fn class(r1: Rank, r2: Rank, s: Suitedness) -> Vec<Combo> {
    let mut out = Vec::new();
    for &s1 in &Suit::ALL {
        for &s2 in &Suit::ALL {
            let suited = s1 == s2;
            let ok = match s {
                _ if r1 == r2 => s1 < s2,
                Suitedness::Suited => suited,
                Suitedness::Offsuit => !suited,
                Suitedness::Any => true,
            };
            if ok {
                out.push(combo(Card::new(r1, s1), Card::new(r2, s2)));
            }
        }
    }
    out
}

/// A [`HiddenSampler`] that draws two-card hands from per-seat ranges; seats without a range,
/// or requests for other than two cards, fall back to uniform.
#[derive(Clone, Debug, Default)]
pub struct RangeSampler {
    pub ranges: BTreeMap<Seat, Range>,
}

impl HiddenSampler for RangeSampler {
    fn sample_private(
        &mut self,
        seat: Seat,
        n: usize,
        available: CardSet,
        rng: &mut DetRng,
    ) -> Option<CardVec> {
        match (self.ranges.get(&seat), n) {
            (Some(range), 2) => {
                let options = range.available(available);
                if options.is_empty() {
                    return None; // caller retries, then falls back to uniform
                }
                let (a, b) = sample_weighted(&options, rng);
                Some(CardVec::from_slice(&[a, b]))
            }
            _ => UniformSampler.sample_private(seat, n, available, rng),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::rng::rng_from_seed;

    fn n(text: &str) -> usize {
        Range::parse(text).unwrap().len()
    }

    #[test]
    fn combo_counts() {
        assert_eq!(n("AA"), 6);
        assert_eq!(n("AKs"), 4);
        assert_eq!(n("AKo"), 12);
        assert_eq!(n("AK"), 16);
        assert_eq!(n("TT+"), 5 * 6);
        assert_eq!(n("A2s+"), 12 * 4);
        assert_eq!(n("KTo+"), 3 * 12);
        assert_eq!(n("TT-77"), 4 * 6);
        assert_eq!(n("A5s-A2s"), 4 * 4);
        assert_eq!(n("AhKh"), 1);
        assert_eq!(n("AA, AA"), 6, "duplicates collapse");
        assert_eq!(Range::any().len(), 1326);
    }

    #[test]
    fn weights_and_errors() {
        let r = Range::parse("AA, KK:0.5").unwrap();
        assert_eq!(r.weight("As".parse().unwrap(), "Ah".parse().unwrap()), 1.0);
        assert_eq!(r.weight("Ks".parse().unwrap(), "Kh".parse().unwrap()), 0.5);
        assert_eq!(r.weight("Qs".parse().unwrap(), "Qh".parse().unwrap()), 0.0);
        for bad in ["", "AAs", "XY", "AK:2", "TT-AKs", "AsAs"] {
            assert!(Range::parse(bad).is_err(), "{bad:?} should fail");
        }
    }

    #[test]
    fn sampler_respects_range_and_card_removal() {
        let mut s = RangeSampler::default();
        s.ranges.insert(Seat(1), Range::parse("AA").unwrap());
        let mut rng = rng_from_seed(1);
        // Two aces are already known: only one AA combo remains.
        let mut available = CardSet::FULL_DECK;
        available.remove("As".parse().unwrap());
        available.remove("Ah".parse().unwrap());
        for _ in 0..50 {
            let hand = s.sample_private(Seat(1), 2, available, &mut rng).unwrap();
            let set: CardSet = hand.iter().copied().collect();
            assert_eq!(
                set,
                ["Ad", "Ac"]
                    .iter()
                    .map(|c| c.parse::<Card>().unwrap())
                    .collect()
            );
        }
        // No combo left at all → None (caller falls back).
        available.remove("Ad".parse().unwrap());
        assert!(s.sample_private(Seat(1), 2, available, &mut rng).is_none());
    }
}
