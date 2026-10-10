//! Hand evaluation. Implements PRD FR-EC-2; design in `docs/tech/engine.md` §3.
//!
//! Every evaluator returns a [`HandValue`] where **bigger is better**, including the low
//! evaluators (they invert their encoding). So pot code just compares with `>`, and never needs
//! to know whether it's splitting a high or a low pot.
//!
//! Modules:
//! - [`high`]: high poker, natural and with wild cards (fast paths used in play and search).
//! - [`low_a6`]: ace-to-six lowball, natural and with wild cards.
//! - [`partial_board`]: 1–4 visible stud upcards, for "best board acts first".
//! - [`oracle`]: slow, obviously-correct brute force. Tests compare the fast paths against it.
//!
//! House rules that matter here (PRD §4):
//! - Ranking: Royal Flush > Five of a Kind > Straight Flush > Four of a Kind > … > High Card.
//!   Note the Royal is above Five of a Kind; it is its own [`Category`], not just the top
//!   straight flush.
//! - A wild card can become **any** card, even one already in the hand (needed for five of a
//!   kind). One exception, resolved here because the Bible doesn't say: a flush must have five
//!   **different** ranks, so a wild can't make a "double-ace flush". It takes the highest rank
//!   missing from the flush instead.

pub mod high;
pub mod low_a6;
pub mod oracle;
pub mod partial_board;

use std::fmt;

use serde::{Deserialize, Serialize};

pub use high::{eval_high, eval_high_wild};
pub use low_a6::{eval_low_a6, eval_low_a6_wild};
pub use partial_board::eval_partial_board;

/// Hand categories in **house order**, worst to best. The discriminant (`= 0`, `= 1`, …) is the
/// value stored in the top 4 bits of a [`HandValue`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(u8)]
pub enum Category {
    HighCard = 0,
    Pair = 1,
    TwoPair = 2,
    Trips = 3,
    Straight = 4,
    Flush = 5,
    FullHouse = 6,
    Quads = 7,
    StraightFlush = 8,
    FiveOfAKind = 9,
    RoyalFlush = 10,
}

impl Category {
    pub const ALL: [Category; 11] = [
        Category::HighCard,
        Category::Pair,
        Category::TwoPair,
        Category::Trips,
        Category::Straight,
        Category::Flush,
        Category::FullHouse,
        Category::Quads,
        Category::StraightFlush,
        Category::FiveOfAKind,
        Category::RoyalFlush,
    ];
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Category::HighCard => "High card",
            Category::Pair => "Pair",
            Category::TwoPair => "Two pair",
            Category::Trips => "Three of a kind",
            Category::Straight => "Straight",
            Category::Flush => "Flush",
            Category::FullHouse => "Full house",
            Category::Quads => "Four of a kind",
            Category::StraightFlush => "Straight flush",
            Category::FiveOfAKind => "Five of a kind",
            Category::RoyalFlush => "Royal flush",
        };
        f.write_str(name)
    }
}

/// A hand's strength as one integer. Compare two hands with `>`, `<`, `==`.
///
/// Layout (24 of the 32 bits used): `[category:4][r1:4][r2:4][r3:4][r4:4][r5:4]`, where
/// `r1..r5` are rank indices (0 = Two … 12 = Ace) in order of significance. For example a full
/// house K-K-K-7-7 stores `FullHouse, K, 7, 0, 0, 0`. Because the most significant field is
/// highest, comparing the integers compares category first, then each kicker in turn, which is
/// exactly how poker hands compare.
///
/// For **low** evaluators the whole number is inverted, so bigger is still better; there,
/// [`HandValue::category`] doesn't mean anything.
#[derive(
    Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default,
)]
pub struct HandValue(pub u32);

impl HandValue {
    /// Pack a category and up to 5 rank indices, most significant first. Missing ranks are 0.
    pub(crate) fn pack(category: Category, ranks: &[u8]) -> HandValue {
        debug_assert!(ranks.len() <= 5);
        let mut v = category as u32;
        for i in 0..5 {
            let r = ranks.get(i).copied().unwrap_or(0) as u32;
            debug_assert!(r < 13);
            v = (v << 4) | r;
        }
        HandValue(v)
    }

    /// The category of a **high** hand value.
    pub fn category(self) -> Category {
        Category::ALL[(self.0 >> 20) as usize]
    }
}

/// Helpers for 13-bit rank masks (bit r set = rank r present; 0 = Two … 12 = Ace).
pub(crate) mod mask {
    /// All 13 ranks.
    pub const ALL: u16 = 0x1FFF;
    /// A-2-3-4-5: the wheel, a 5-high straight (bits Ace, Two, Three, Four, Five).
    pub const WHEEL: u16 = (1 << 12) | 0b1111;

    /// Highest rank in a non-empty mask. `leading_zeros` counts zero bits from the top of the
    /// u16, so the highest set bit is at position `15 - leading_zeros`.
    pub fn top(m: u16) -> u8 {
        debug_assert!(m != 0);
        (15 - m.leading_zeros()) as u8
    }

    /// The `n` highest ranks in `m`, highest first. Fewer if `m` has fewer bits.
    pub fn top_n(mut m: u16, n: usize) -> smallvec::SmallVec<[u8; 5]> {
        let mut out = smallvec::SmallVec::new();
        while out.len() < n && m != 0 {
            let r = top(m);
            out.push(r);
            m &= !(1 << r);
        }
        out
    }

    /// Top rank of the best straight in `m`, if any (3 = the 5-high wheel).
    ///
    /// The trick: `m & m<<1 & m<<2 & m<<3 & m<<4` has bit `i` set exactly when bits
    /// `i, i-1, …, i-4` are all set in `m`, i.e. when there's a straight topped by rank `i`.
    pub fn straight_top(m: u16) -> Option<u8> {
        let runs = m & (m << 1) & (m << 2) & (m << 3) & (m << 4) & ALL;
        if runs != 0 {
            Some(top(runs))
        } else if m & WHEEL == WHEEL {
            Some(3)
        } else {
            None
        }
    }
}

/// The reference 5-card scorer shared by [`oracle`] and [`low_a6`]: simple, slow, obviously
/// correct. Takes rank indices (possibly repeated, e.g. after wild substitution) and whether all
/// five cards share a suit.
///
/// `ace_low`: if true, ranks are already in ace-low order (0 = Ace … 12 = King), the only
/// straights are 5 consecutive indices, and there is no royal flush. Used for A-6 lowball.
pub(crate) fn score5(ranks: [u8; 5], same_suit: bool, ace_low: bool) -> HandValue {
    let mut counts = [0u8; 13];
    for r in ranks {
        counts[r as usize] += 1;
    }
    // Groups as (count, rank), sorted so bigger groups come first, then higher ranks.
    let mut groups: smallvec::SmallVec<[(u8, u8); 5]> = (0..13u8)
        .rev()
        .filter(|&r| counts[r as usize] > 0)
        .map(|r| (counts[r as usize], r))
        .collect();
    groups.sort_by(|a, b| b.cmp(a));
    let order: smallvec::SmallVec<[u8; 5]> = groups.iter().map(|&(_, r)| r).collect();

    if groups.len() == 5 {
        // Five distinct ranks: possibly a straight and/or flush.
        let (hi, lo) = (order[0], order[4]);
        let straight_top = if hi - lo == 4 {
            Some(hi)
        } else if !ace_low && order.as_slice() == [12, 3, 2, 1, 0] {
            Some(3) // the wheel, A-2-3-4-5, plays 5-high
        } else {
            None
        };
        return match (straight_top, same_suit) {
            (Some(12), true) if !ace_low => HandValue::pack(Category::RoyalFlush, &[12]),
            (Some(t), true) => HandValue::pack(Category::StraightFlush, &[t]),
            (None, true) => HandValue::pack(Category::Flush, &order),
            (Some(t), false) => HandValue::pack(Category::Straight, &[t]),
            (None, false) => HandValue::pack(Category::HighCard, &order),
        };
    }
    let shape: smallvec::SmallVec<[u8; 5]> = groups.iter().map(|&(c, _)| c).collect();
    let category = match shape.as_slice() {
        [5] => Category::FiveOfAKind,
        [4, 1] => Category::Quads,
        [3, 2] => Category::FullHouse,
        [3, 1, 1] => Category::Trips,
        [2, 2, 1] => Category::TwoPair,
        [2, 1, 1, 1] => Category::Pair,
        other => unreachable!("impossible 5-card shape {other:?}"),
    };
    HandValue::pack(category, &order)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_compare_in_house_order() {
        let royal = HandValue::pack(Category::RoyalFlush, &[12]);
        let five = HandValue::pack(Category::FiveOfAKind, &[12]);
        let sf = HandValue::pack(Category::StraightFlush, &[11]);
        let quads = HandValue::pack(Category::Quads, &[12, 11]);
        assert!(royal > five && five > sf && sf > quads);
        assert_eq!(royal.category(), Category::RoyalFlush);
    }

    #[test]
    fn kickers_break_ties() {
        let aak = HandValue::pack(Category::Pair, &[12, 11, 5, 3]);
        let aaq = HandValue::pack(Category::Pair, &[12, 10, 9, 8]);
        assert!(aak > aaq);
    }

    #[test]
    fn straight_top_finds_best_and_wheel() {
        assert_eq!(mask::straight_top(0b1_1111_0000_0000), Some(12)); // T-J-Q-K-A
        assert_eq!(mask::straight_top(mask::WHEEL), Some(3));
        assert_eq!(
            mask::straight_top(mask::WHEEL | (1 << 4)),
            Some(4),
            "6-high beats the wheel"
        );
        assert_eq!(mask::straight_top(0b1_0111_0000_0000), None);
    }
}
