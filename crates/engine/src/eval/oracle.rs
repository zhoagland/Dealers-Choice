//! Brute-force reference evaluators: slow on purpose, and simple enough to check by reading.
//!
//! The fast evaluators are tested by comparing against these on many hands. When the two
//! disagree, the oracle is presumed right: it just tries every possibility.
//!
//! - High: best [`score5`](super::score5) over all 5-card subsets.
//! - With wilds: replace each wild with each of the 52 cards (duplicates allowed) and take the
//!   best. That's 52^w substitutions, fine for tests and far too slow for search.

use super::{HandValue, score5};
use crate::card::{Card, CardSet};

/// Call `f` with every `k`-element subset of `cards` (by index bitmask, so ≤ 32 cards).
pub(crate) fn for_each_subset(cards: &[Card], k: usize, mut f: impl FnMut([Card; 5])) {
    debug_assert_eq!(k, 5);
    let n = cards.len();
    for bits in 0u32..(1 << n) {
        if bits.count_ones() as usize != k {
            continue;
        }
        let mut pick = [cards[0]; 5];
        let mut j = 0;
        for (i, &c) in cards.iter().enumerate() {
            if bits & (1 << i) != 0 {
                pick[j] = c;
                j += 1;
            }
        }
        f(pick);
    }
}

fn same_suit(cards: &[Card; 5]) -> bool {
    cards.iter().all(|c| c.suit() == cards[0].suit())
}

/// Best high hand among 5–7 cards. Accepts repeated cards (wild substitutions).
pub fn oracle_high(cards: &[Card]) -> HandValue {
    let mut best = HandValue(0);
    for_each_subset(cards, 5, |five| {
        let ranks = five.map(|c| c.rank() as u8);
        best = best.max(score5(ranks, same_suit(&five), false));
    });
    best
}

/// Best A-6 low among 5–7 cards (bigger = better low). Accepts repeated cards.
pub fn oracle_low_a6(cards: &[Card]) -> HandValue {
    let mut best: Option<HandValue> = None;
    for_each_subset(cards, 5, |five| {
        let ranks = five.map(|c| super::low_a6::ace_low_index(c.rank()));
        let v = super::low_a6::invert(score5(ranks, same_suit(&five), true));
        best = Some(best.map_or(v, |b| b.max(v)));
    });
    best.expect("need at least 5 cards")
}

/// Best high hand from `naturals` + `wilds`, by trying every substitution.
pub fn oracle_high_wild(naturals: CardSet, wilds: usize) -> HandValue {
    best_substitution(naturals, wilds, &oracle_high)
}

/// Best A-6 low from `naturals` + `wilds`, by trying every substitution.
pub fn oracle_low_a6_wild(naturals: CardSet, wilds: usize) -> HandValue {
    best_substitution(naturals, wilds, &oracle_low_a6)
}

fn best_substitution(
    naturals: CardSet,
    wilds: usize,
    eval: &dyn Fn(&[Card]) -> HandValue,
) -> HandValue {
    let mut hand: Vec<Card> = naturals.iter().collect();
    let mut best = None;
    substitute(&mut hand, wilds, eval, &mut best);
    best.unwrap()
}

/// Recursively append one substituted card per remaining wild.
fn substitute(
    hand: &mut Vec<Card>,
    wilds_left: usize,
    eval: &dyn Fn(&[Card]) -> HandValue,
    best: &mut Option<HandValue>,
) {
    if wilds_left == 0 {
        let v = eval(hand);
        *best = Some(best.map_or(v, |b: HandValue| b.max(v)));
        return;
    }
    for c in CardSet::FULL_DECK {
        hand.push(c);
        substitute(hand, wilds_left - 1, eval, best);
        hand.pop();
    }
}
