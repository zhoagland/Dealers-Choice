//! High poker evaluation: best 5 of 5–7 cards, natural or with wild cards.
//!
//! These are the fast paths used during play and search, so they work directly on bit masks
//! instead of looping over the 21 five-card subsets of a 7-card hand. Their correctness is
//! established by comparing against the brute-force [`super::oracle`] in the tests.

use smallvec::SmallVec;

use super::mask::{self, straight_top, top, top_n};
use super::{Category, HandValue};
use crate::card::{CardSet, Suit};

/// Best high hand from 5–7 natural cards.
pub fn eval_high(cards: CardSet) -> HandValue {
    debug_assert!(
        (5..=7).contains(&cards.len()),
        "eval_high needs 5-7 cards, got {}",
        cards.len()
    );
    let suits = suit_masks(cards);
    let all = suits[0] | suits[1] | suits[2] | suits[3];

    // "At least k of this rank" masks, built by stacking the four suit masks. A rank's bit
    // reaches `ge2` when it appears in a second suit, `ge3` in a third, and so on, like a
    // little counter per rank that runs in parallel across all 13 ranks.
    let (mut ge1, mut ge2, mut ge3, mut ge4) = (0u16, 0u16, 0u16, 0u16);
    for m in suits {
        ge4 |= ge3 & m;
        ge3 |= ge2 & m;
        ge2 |= ge1 & m;
        ge1 |= m;
    }

    // At most one suit can have 5+ cards out of 7.
    let flush = suits.iter().copied().find(|m| m.count_ones() >= 5);
    if let Some(f) = flush
        && let Some(t) = straight_top(f)
    {
        return if t == 12 {
            HandValue::pack(Category::RoyalFlush, &[12])
        } else {
            HandValue::pack(Category::StraightFlush, &[t])
        };
    }
    if ge4 != 0 {
        let q = top(ge4);
        return HandValue::pack(Category::Quads, &[q, top(all & !(1 << q))]);
    }
    if ge3 != 0 {
        let t = top(ge3);
        let pairs = ge2 & !(1 << t); // a second set of trips also counts as the pair
        if pairs != 0 {
            return HandValue::pack(Category::FullHouse, &[t, top(pairs)]);
        }
    }
    if let Some(f) = flush {
        return HandValue::pack(Category::Flush, &top_n(f, 5));
    }
    if let Some(t) = straight_top(all) {
        return HandValue::pack(Category::Straight, &[t]);
    }
    if ge3 != 0 {
        let t = top(ge3);
        let mut ranks: SmallVec<[u8; 5]> = SmallVec::from_slice(&[t]);
        ranks.extend(top_n(all & !(1 << t), 2));
        return HandValue::pack(Category::Trips, &ranks);
    }
    if ge2.count_ones() >= 2 {
        let p1 = top(ge2);
        let p2 = top(ge2 & !(1 << p1));
        let kicker = top(all & !(1 << p1) & !(1 << p2));
        return HandValue::pack(Category::TwoPair, &[p1, p2, kicker]);
    }
    if ge2 != 0 {
        let p = top(ge2);
        let mut ranks: SmallVec<[u8; 5]> = SmallVec::from_slice(&[p]);
        ranks.extend(top_n(all & !(1 << p), 3));
        return HandValue::pack(Category::Pair, &ranks);
    }
    HandValue::pack(Category::HighCard, &top_n(all, 5))
}

/// Best high hand from `naturals` plus `wilds` wild cards (5–7 cards in total).
///
/// A wild can become any card, including a copy of one already held, except that a flush needs
/// five different ranks (see the module docs in `eval`). Works top-down: for each category from
/// Royal Flush downward, "can the wilds make this?", and the first yes wins, with the best
/// kickers for that category.
pub fn eval_high_wild(naturals: CardSet, wilds: usize) -> HandValue {
    let total = naturals.len() + wilds;
    debug_assert!(
        (5..=7).contains(&total),
        "eval_high_wild needs 5-7 cards, got {total}"
    );
    if wilds == 0 {
        return eval_high(naturals);
    }
    let w = wilds as u32;
    let suits = suit_masks(naturals);
    let all = suits[0] | suits[1] | suits[2] | suits[3];
    let mut counts = [0u32; 13];
    for c in naturals {
        counts[c.rank() as usize] += 1;
    }
    let max_count = counts.iter().copied().max().unwrap_or(0);

    // Royal: some suit already holds (5 - w) of T, J, Q, K, A.
    const ROYAL: u16 = 0b1_1111_0000_0000;
    if suits
        .iter()
        .any(|&m| 5 - (m & ROYAL).count_ones().min(5) <= w)
    {
        return HandValue::pack(Category::RoyalFlush, &[12]);
    }
    // Five of a kind: the highest rank the wilds can bring to five.
    if max_count + w >= 5 {
        let r = (0..13u8)
            .rev()
            .find(|&r| counts[r as usize] + w >= 5)
            .unwrap();
        return HandValue::pack(Category::FiveOfAKind, &[r]);
    }
    // Straight flush (K-high or lower; A-high would have been a royal).
    let mut best_sf: Option<u8> = None;
    for &m in &suits {
        if let Some(t) = best_straight_with_wilds(m, w)
            && best_sf.is_none_or(|b| t > b)
        {
            best_sf = Some(t);
        }
    }
    if let Some(t) = best_sf {
        return HandValue::pack(Category::StraightFlush, &[t]);
    }
    // Four of a kind. The quad rank matters most, so take the highest reachable one.
    if max_count + w >= 4 {
        let q = (0..13u8)
            .rev()
            .find(|&r| counts[r as usize] + w >= 4)
            .unwrap();
        // Every wild goes into the quads: a wild left over would have made five of a kind above.
        // So the kicker is the best natural card of another rank (there is one, since the hand
        // has at least 5 cards).
        debug_assert_eq!(counts[q as usize] + w, 4);
        return HandValue::pack(Category::Quads, &[q, top(all & !(1 << q))]);
    }
    // Full house: try (trips rank, pair rank) from best to worst; the first one the wilds can
    // afford wins. 13 × 12 pairs is small, and this is obviously correct.
    for t in (0..13u8).rev() {
        let cost_t = 3u32.saturating_sub(counts[t as usize]);
        if cost_t > w {
            continue;
        }
        for p in (0..13u8).rev().filter(|&p| p != t) {
            if cost_t + 2u32.saturating_sub(counts[p as usize]) <= w {
                return HandValue::pack(Category::FullHouse, &[t, p]);
            }
        }
    }
    // Flush: wilds fill the highest ranks missing from the suit.
    let mut best_flush: Option<HandValue> = None;
    for &m in &suits {
        if m.count_ones() + w >= 5 {
            let mut filled = m;
            let mut left = w;
            for r in (0..13u8).rev() {
                if left == 0 {
                    break;
                }
                if filled & (1 << r) == 0 {
                    filled |= 1 << r;
                    left -= 1;
                }
            }
            let v = HandValue::pack(Category::Flush, &top_n(filled, 5));
            best_flush = Some(best_flush.map_or(v, |b| b.max(v)));
        }
    }
    if let Some(v) = best_flush {
        return v;
    }
    if let Some(t) = best_straight_with_wilds(all, w) {
        return HandValue::pack(Category::Straight, &[t]);
    }
    // From here on, wilds are scarce: if anything were left over, a better category above would
    // have been reachable. The debug_asserts document (and check) that reasoning.
    if max_count + w >= 3 {
        let t = (0..13u8)
            .rev()
            .find(|&r| counts[r as usize] + w >= 3)
            .unwrap();
        debug_assert_eq!(
            counts[t as usize] + w,
            3,
            "a leftover wild would have made quads"
        );
        let mut ranks: SmallVec<[u8; 5]> = SmallVec::from_slice(&[t]);
        ranks.extend(top_n(all & !(1 << t), 2));
        return HandValue::pack(Category::Trips, &ranks);
    }
    // Two pair can't use a wild (wild + pair = trips), so with one wild the best is a pair of
    // the highest natural rank.
    debug_assert_eq!(w, 1, "two or more wilds always make at least trips");
    let p = top(all);
    let mut ranks: SmallVec<[u8; 5]> = SmallVec::from_slice(&[p]);
    ranks.extend(top_n(all & !(1 << p), 3));
    HandValue::pack(Category::Pair, &ranks)
}

/// Top rank of the best straight that `m` (a rank mask) plus `w` wild ranks can make.
fn best_straight_with_wilds(m: u16, w: u32) -> Option<u8> {
    // Windows from A-high (top = 12) down to 6-high (top = 4), then the wheel (top = 3).
    for t in (4..=12u8).rev() {
        let window = 0x1Fu16 << (t - 4);
        if 5 - (m & window).count_ones() <= w {
            return Some(t);
        }
    }
    if 5 - (m & mask::WHEEL).count_ones() <= w {
        Some(3)
    } else {
        None
    }
}

/// The four 13-bit rank masks, in `Suit::ALL` order.
fn suit_masks(cards: CardSet) -> [u16; 4] {
    Suit::ALL.map(|s| cards.rank_mask(s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::Card;
    use crate::eval::oracle::{oracle_high, oracle_high_wild};
    use crate::rng::rng_from_seed;
    use rand::seq::SliceRandom;

    fn set(s: &str) -> CardSet {
        s.split_whitespace()
            .map(|c| c.parse::<Card>().unwrap())
            .collect()
    }

    fn cat(s: &str) -> Category {
        eval_high(set(s)).category()
    }

    fn deck() -> Vec<Card> {
        CardSet::FULL_DECK.iter().collect()
    }

    #[test]
    fn hand_picked_categories() {
        assert_eq!(cat("As Ks Qs Js Ts 2c 3d"), Category::RoyalFlush);
        assert_eq!(cat("9s Ks Qs Js Ts 2c 3d"), Category::StraightFlush);
        assert_eq!(cat("As 2s 3s 4s 5s Kc Kd"), Category::StraightFlush);
        assert_eq!(cat("Ah Ad Ac As 2c 3d 4h"), Category::Quads);
        assert_eq!(cat("Ah Ad Ac Ks Kc 3d 4h"), Category::FullHouse);
        assert_eq!(cat("Ah Ad Ac Ks Kc Kd 4h"), Category::FullHouse);
        assert_eq!(cat("2h 7h 9h Jh Kh Ac Ad"), Category::Flush);
        assert_eq!(cat("Ah 2d 3c 4s 5h 9c Kd"), Category::Straight);
        assert_eq!(cat("2h 2d 2c 9s Th Jc Kd"), Category::Trips);
        assert_eq!(cat("2h 2d 9c 9s Th Tc Kd"), Category::TwoPair);
        assert_eq!(cat("2h 2d 9c 8s Th Jc Kd"), Category::Pair);
        assert_eq!(cat("2h 4d 9c 8s Th Jc Kd"), Category::HighCard);
    }

    #[test]
    fn wheel_is_the_lowest_straight() {
        let wheel = eval_high(set("Ah 2d 3c 4s 5h"));
        let six = eval_high(set("6h 2d 3c 4s 5h"));
        assert!(six > wheel);
    }

    #[test]
    fn kickers_matter() {
        assert!(eval_high(set("Ah Ad Kc 7s 4h 3c 2d")) > eval_high(set("Ah Ad Qc 7s 4h 3c 2d")));
        // Third pair can't count; the best kicker among the rest plays.
        assert_eq!(
            eval_high(set("Ah Ad Kc Ks 3h 3c Qd")),
            eval_high(set("Ah Ad Kc Ks 2h 4c Qd"))
        );
    }

    /// NFR-2: exhaustive over all C(52,5) = 2,598,960 five-card hands.
    #[test]
    fn five_card_category_counts_match_published_table() {
        let d = deck();
        let mut counts = [0u64; 11];
        for a in 0..52 {
            for b in a + 1..52 {
                for c in b + 1..52 {
                    for e in c + 1..52 {
                        for f in e + 1..52 {
                            let hand: CardSet =
                                [d[a], d[b], d[c], d[e], d[f]].into_iter().collect();
                            counts[eval_high(hand).category() as usize] += 1;
                        }
                    }
                }
            }
        }
        // HighCard, Pair, TwoPair, Trips, Straight, Flush, FullHouse, Quads, SF, 5oak, Royal
        assert_eq!(
            counts,
            [
                1_302_540, 1_098_240, 123_552, 54_912, 10_200, 5_108, 3_744, 624, 36, 0, 4
            ]
        );
    }

    #[test]
    fn matches_oracle_on_random_hands() {
        let mut rng = rng_from_seed(11);
        let mut d = deck();
        for i in 0..20_000 {
            d.shuffle(&mut rng);
            let n = 5 + i % 3;
            let hand = &d[..n];
            let fast = eval_high(hand.iter().copied().collect());
            assert_eq!(fast, oracle_high(hand), "{hand:?}");
        }
    }

    #[test]
    fn wild_matches_oracle_for_one_and_two_wilds() {
        let mut rng = rng_from_seed(12);
        let mut d = deck();
        for i in 0..400 {
            d.shuffle(&mut rng);
            let w = 1 + i % 2;
            let total = 5 + (i / 2) % 3;
            let naturals: CardSet = d[..total - w].iter().copied().collect();
            assert_eq!(
                eval_high_wild(naturals, w),
                oracle_high_wild(naturals, w),
                "{naturals:?} + {w} wild"
            );
        }
    }

    #[test]
    fn wild_hand_picked() {
        let w = |s: &str, n| eval_high_wild(set(s), n).category();
        // Three wilds + a pair = five of a kind.
        assert_eq!(w("9c 9d 2h 5s", 3), Category::FiveOfAKind);
        // Four wilds + anything: at least five of a kind (here a royal is possible: As + 4 wilds).
        assert_eq!(w("As 2d 7h", 4), Category::RoyalFlush);
        assert_eq!(w("2c 7d 9h", 4), Category::FiveOfAKind);
        // Royal with wilds outranks natural five of a kind possibilities.
        assert_eq!(w("Ah Kh Qh 2c 2d", 2), Category::RoyalFlush);
        // One wild + trips = quads, wild + two pair = full house.
        assert_eq!(w("8c 8d 8h 2s 5d 9c", 1), Category::Quads);
        assert_eq!(w("8c 8d 3h 3s Kd Jc", 1), Category::FullHouse);
        // A wild doesn't duplicate a flush rank: A-K-Q-9-x flush + wild = A-K-Q-J-9.
        assert_eq!(
            eval_high_wild(set("Ah Kh Qh 9h 2c 3d"), 1),
            HandValue::pack(Category::Flush, &[12, 11, 10, 9, 7])
        );
        assert_eq!(
            eval_high_wild(set("Kc Kd Kh 2s 3d"), 2),
            HandValue::pack(Category::FiveOfAKind, &[11]),
            "two wilds + trips = five of a kind"
        );
        // Both wilds go into the quads; the best natural (7) is the kicker.
        assert_eq!(
            eval_high_wild(set("Kc Kd 2s 3d 7h"), 2),
            HandValue::pack(Category::Quads, &[11, 5])
        );
        assert_eq!(
            eval_high_wild(set("Kc Kd Kh 2s"), 1),
            HandValue::pack(Category::Quads, &[11, 0])
        );
    }

    /// Release-only: exhaustive over all 133,784,560 seven-card hands.
    /// Run with `cargo test --release -p engine -- --ignored seven_card`.
    #[test]
    #[ignore]
    fn seven_card_category_counts_match_published_table() {
        let d = deck();
        let mut counts = [0u64; 11];
        let mut idx = [0usize, 1, 2, 3, 4, 5, 6];
        loop {
            let hand: CardSet = idx.iter().map(|&i| d[i]).collect();
            counts[eval_high(hand).category() as usize] += 1;
            // Advance to the next 7-combination in lexicographic order.
            let mut i = 6;
            loop {
                if idx[i] < 52 - 7 + i {
                    break;
                }
                if i == 0 {
                    assert_eq!(
                        counts,
                        [
                            23_294_460, 58_627_800, 31_433_400, 6_461_620, 6_180_020, 4_047_644,
                            3_473_184, 224_848, 37_260, 0, 4_324
                        ]
                    );
                    return;
                }
                i -= 1;
            }
            idx[i] += 1;
            for j in i + 1..7 {
                idx[j] = idx[j - 1] + 1;
            }
        }
    }

    /// Release-only throughput check for NFR-3 (target ≥ 10 M evals/s).
    /// `cargo test --release -p engine -- --ignored --nocapture bench_eval_high`
    #[test]
    #[ignore]
    fn bench_eval_high_7_cards() {
        let mut rng = rng_from_seed(99);
        let mut d = deck();
        let hands: Vec<CardSet> = (0..1_000_000)
            .map(|_| {
                d.shuffle(&mut rng);
                d[..7].iter().copied().collect()
            })
            .collect();
        let start = std::time::Instant::now();
        let mut acc = 0u64;
        for _ in 0..5 {
            for &h in &hands {
                acc = acc.wrapping_add(std::hint::black_box(eval_high(h)).0 as u64);
            }
        }
        let secs = start.elapsed().as_secs_f64();
        println!(
            "eval_high 7 cards: {:.1} M evals/s (checksum {acc})",
            5.0 * hands.len() as f64 / secs / 1e6
        );
    }
}
