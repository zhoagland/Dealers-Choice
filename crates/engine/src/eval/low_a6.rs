//! Ace-to-six lowball ("A-6 low"), the Bible's low ranking.
//!
//! Rules: the ace is the lowest card, and straights and flushes **count against** you. The
//! best hand is 6-4-3-2-A (5-4-3-2-A is a straight, so it doesn't qualify). Hands compare by their
//! highest card, then the next, and so on; any pair is worse than any unpaired hand.
//!
//! Key identity: an A-6 low hand ranks exactly like the **worst** high hand when the ace counts
//! as low. So this module reuses the reference 5-card scorer with `ace_low = true`, takes the
//! *minimum* over 5-card subsets, then inverts the number so that bigger = better, like every
//! other `HandValue`.
//!
//! This isn't a bitmask fast path; it loops over the ≤ 21 five-card subsets. Lows are only
//! evaluated at showdown in hi/lo games, so that's acceptable until profiling says otherwise.

use smallvec::SmallVec;

use super::oracle::for_each_subset;
use super::{HandValue, score5};
use crate::card::{Card, CardSet, Rank};

/// Flips a packed high value so bigger means better low. Any value below 2^24 maps into range.
const LOW_FLIP: u32 = 0x00FF_FFFF;

/// Wilds in a low hand are searched over the lowest this-many ranks (A through 9). See
/// [`eval_low_a6_wild`] for why that's enough.
const WILD_RANKS: u32 = 9;

/// Ace-low rank index: Ace = 0, Two = 1, …, King = 12.
pub(crate) fn ace_low_index(r: Rank) -> u8 {
    (r as u8 + 1) % 13
}

pub(crate) fn invert(v: HandValue) -> HandValue {
    HandValue(LOW_FLIP - v.0)
}

/// Best A-6 low from 5–7 natural cards. Bigger = better low.
pub fn eval_low_a6(cards: CardSet) -> HandValue {
    debug_assert!((5..=7).contains(&cards.len()));
    let list: SmallVec<[Card; 7]> = cards.iter().collect();
    let mut worst_high = HandValue(u32::MAX);
    for_each_subset(&list, 5, |five| {
        let ranks = five.map(|c| ace_low_index(c.rank()));
        let flush = five.iter().all(|c| c.suit() == five[0].suit());
        worst_high = worst_high.min(score5(ranks, flush, true));
    });
    invert(worst_high)
}

/// Best A-6 low from `naturals` plus `wilds` wild cards (5–7 cards total).
///
/// A wild can be any card, so using it is never worse than using a natural card: it could
/// copy that card exactly. So the best hand uses `min(wilds, 5)` wilds plus the best choice of
/// naturals for the remaining slots. Then each wild takes a rank; the search tries every set of
/// distinct ranks (two wilds of the same rank would be a pair, never better than choosing a
/// different rank). A wild's suit can always be chosen to break a flush, so with any wild in
/// the hand there's no flush.
///
/// Search bound: wilds only ever need the lowest few ranks. The best wild ranks are the
/// lowest ones the chosen naturals don't use, with at most one bumped up a rank to break a
/// straight, so no wild needs a rank above index `need + used + 1 = 6` (a 7). The search allows the
/// lowest [`WILD_RANKS`] = 9 ranks, for margin. The oracle tests check this bound.
///
/// Cost: ≤ C(9, used) rank sets × C(naturals, need) natural choices, a few hundred 5-card
/// scorings at most.
pub fn eval_low_a6_wild(naturals: CardSet, wilds: usize) -> HandValue {
    let total = naturals.len() + wilds;
    debug_assert!((5..=7).contains(&total));
    if wilds == 0 {
        return eval_low_a6(naturals);
    }
    let used = wilds.min(5);
    let need = 5 - used;
    let nat: SmallVec<[Card; 7]> = naturals.iter().collect();

    let mut worst_high = HandValue(u32::MAX);
    // Each `pick` is a bitmask choosing `need` naturals; each `wild_ranks` a 13-bit mask of
    // `used` distinct ace-low ranks among the lowest WILD_RANKS.
    for pick in 0u32..(1 << nat.len()) {
        if pick.count_ones() as usize != need {
            continue;
        }
        let mut base: SmallVec<[u8; 5]> = SmallVec::new();
        for (i, c) in nat.iter().enumerate() {
            if pick & (1 << i) != 0 {
                base.push(ace_low_index(c.rank()));
            }
        }
        for wild_ranks in 0u16..(1 << WILD_RANKS) {
            if wild_ranks.count_ones() as usize != used {
                continue;
            }
            let mut ranks = [0u8; 5];
            ranks[..need].copy_from_slice(&base);
            let mut j = need;
            for r in 0..WILD_RANKS as u8 {
                if wild_ranks & (1 << r) != 0 {
                    ranks[j] = r;
                    j += 1;
                }
            }
            worst_high = worst_high.min(score5(ranks, false, true));
        }
    }
    invert(worst_high)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::oracle::{oracle_low_a6, oracle_low_a6_wild};
    use crate::rng::rng_from_seed;
    use rand::seq::SliceRandom;

    fn set(s: &str) -> CardSet {
        s.split_whitespace()
            .map(|c| c.parse::<Card>().unwrap())
            .collect()
    }

    fn low(s: &str) -> HandValue {
        eval_low_a6(set(s))
    }

    #[test]
    fn bible_low_order() {
        // Poker Bible §Low Poker: 64321, 65321, 65421, 65431, 74321 (65432 skipped: straight).
        let order = [
            low("6d 4s 3s 2s As"),
            low("6d 5s 3s 2s As"),
            low("6d 5s 4s 2s As"),
            low("6d 5s 4s 3s As"),
            low("7d 4s 3s 2s As"),
        ];
        for w in order.windows(2) {
            assert!(w[0] > w[1], "{:?} should beat {:?}", w[0], w[1]);
        }
    }

    #[test]
    fn straights_and_flushes_count_against() {
        let seven_low = low("7d 5s 4s 2s As");
        assert!(seven_low > low("5d 4s 3c 2s Ah"), "A-2-3-4-5 is a straight");
        assert!(seven_low > low("6d 5s 4c 3s 2h"), "6-5-4-3-2 is a straight");
        assert!(seven_low > low("6s 4s 3s 2s As"), "a flush is bad");
        assert!(
            low("7d 4s 3c 2s Ah") > low("7d 5s 4c 2s Ah"),
            "second card decides"
        );
    }

    #[test]
    fn any_pair_is_worse_than_no_pair() {
        assert!(low("Kd Qs Jc 9s 8h") > low("Ad As 2c 3s 4h"));
    }

    #[test]
    fn seven_cards_pick_the_best_five() {
        assert_eq!(low("6d 4s 3c 2s Ah Kd Kc"), low("6d 4s 3c 2s Ah"));
    }

    #[test]
    fn matches_oracle_natural() {
        let mut rng = rng_from_seed(21);
        let mut d: Vec<Card> = CardSet::FULL_DECK.iter().collect();
        for i in 0..5_000 {
            d.shuffle(&mut rng);
            let hand = &d[..5 + i % 3];
            assert_eq!(
                eval_low_a6(hand.iter().copied().collect()),
                oracle_low_a6(hand),
                "{hand:?}"
            );
        }
    }

    #[test]
    fn wild_matches_oracle_for_one_and_two_wilds() {
        let mut rng = rng_from_seed(22);
        let mut d: Vec<Card> = CardSet::FULL_DECK.iter().collect();
        for i in 0..200 {
            d.shuffle(&mut rng);
            let w = 1 + i % 2;
            let total = 5 + (i / 2) % 3;
            let naturals: CardSet = d[..total - w].iter().copied().collect();
            assert_eq!(
                eval_low_a6_wild(naturals, w),
                oracle_low_a6_wild(naturals, w),
                "{naturals:?} + {w}"
            );
        }
    }

    /// Hands full of low cards are where the WILD_RANKS bound could bite (wilds pushed upward
    /// to avoid pairs and straights), so check those against the oracle specifically.
    #[test]
    fn wild_bound_holds_on_crowded_low_hands() {
        for (naturals, w) in [
            ("As 2d 3h 4c 5s", 1),
            ("As 2d 3h 4c 5s 6d", 1),
            ("As 2d 3h 4c", 2),
            ("Ac Ad 2h 2c 3s", 2),
            ("2c 3d 4h 5s 6c", 2),
            ("As 2s 3s 4s 6s", 2),
            ("Ac Ad As 2c 2d", 2),
        ] {
            assert_eq!(
                eval_low_a6_wild(set(naturals), w),
                oracle_low_a6_wild(set(naturals), w),
                "{naturals} + {w}"
            );
        }
    }

    #[test]
    fn wild_hand_picked() {
        // 3 wilds must play with 2 naturals: the lowest two are T and J; wilds become A, 2, 3.
        assert_eq!(
            eval_low_a6_wild(set("Kc Qd Jh Ts"), 3),
            low("Js Ts 3c 2d Ah")
        );
        // 4 wilds + one natural (the 9): wilds become A, 2, 3, 4.
        assert_eq!(eval_low_a6_wild(set("Kc Qd 9h"), 4), low("9h 4c 3d 2s Ah"));
        // A wild avoids pairing and straights: with 2-3-4-5, an ace makes the wheel and a 6 makes
        // 6-5-4-3-2, both straights, so the wild becomes a 7.
        assert_eq!(
            eval_low_a6_wild(set("2c 3d 4h 5s"), 1),
            low("7d 5s 4h 3d 2c")
        );
        // A wild breaks a flush: four low spades + wild beats the natural flush.
        assert_eq!(
            eval_low_a6_wild(set("As 2s 3s 4s"), 1),
            low("6d 4s 3s 2s As")
        );
    }
}

#[cfg(test)]
mod bench {
    use super::*;
    use crate::rng::rng_from_seed;
    use rand::seq::SliceRandom;

    /// `cargo test --release -p engine -- --ignored --nocapture bench_low`
    #[test]
    #[ignore]
    fn bench_low_a6() {
        let mut rng = rng_from_seed(5);
        let mut d: Vec<Card> = CardSet::FULL_DECK.iter().collect();
        for wilds in [0usize, 1, 2, 4] {
            let hands: Vec<CardSet> = (0..20_000)
                .map(|_| {
                    d.shuffle(&mut rng);
                    d[..7 - wilds].iter().copied().collect()
                })
                .collect();
            let start = std::time::Instant::now();
            let mut acc = 0u64;
            for &h in &hands {
                acc = acc.wrapping_add(std::hint::black_box(eval_low_a6_wild(h, wilds)).0 as u64);
            }
            let per = start.elapsed().as_secs_f64() / hands.len() as f64;
            println!(
                "eval_low_a6_wild 7 cards, {wilds} wilds: {:.2} µs/eval (checksum {acc})",
                per * 1e6
            );
        }
    }
}
