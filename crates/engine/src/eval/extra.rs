//! Rankings added for the game families (catalog K4): deuce-to-seven low, Badugi, and 6+
//! (short-deck) high. None of them use wild cards in any Bible game.
//!
//! All loop over 5-card (or 4-card) subsets: they're only evaluated at showdown.

use super::low_a6::invert;
use super::{Category, HandValue, score5};
use crate::card::{Card, CardSet, Rank, Suit};

fn five_ranks(cards: &[Card; 5]) -> ([u8; 5], bool) {
    let ranks = cards.map(|c| c.rank() as u8);
    let suited = cards.iter().all(|c| c.suit() == cards[0].suit());
    (ranks, suited)
}

fn for_each_five(cards: CardSet, mut f: impl FnMut([Card; 5])) {
    let list: Vec<Card> = cards.iter().collect();
    let n = list.len();
    if n < 5 {
        return;
    }
    let mut idx = [0, 1, 2, 3, 4];
    loop {
        f(idx.map(|i| list[i]));
        // Next combination in lexicographic order.
        let mut i = 4;
        loop {
            if idx[i] < n - 5 + i {
                break;
            }
            if i == 0 {
                return;
            }
            i -= 1;
        }
        idx[i] += 1;
        for j in i + 1..5 {
            idx[j] = idx[j - 1] + 1;
        }
    }
}

/// Deuce-to-seven low, best 5 of 5–7 natural cards: aces are always high, A-2-3-4-5 is not a
/// straight, and straights and flushes count against you. Best hand 7-5-4-3-2 unsuited.
/// Bigger = better low.
pub fn eval_low27(cards: CardSet) -> HandValue {
    let mut best: Option<HandValue> = None;
    for_each_five(cards, |five| {
        let (ranks, suited) = five_ranks(&five);
        // `ace_low = true` disables the wheel and the royal; ranks stay ace-high, which is
        // exactly 2-7's rule.
        let v = invert(score5(ranks, suited, true));
        best = best.max(Some(v));
    });
    best.expect("eval_low27 needs at least 5 cards")
}

/// Badugi: the best set of cards with all-different ranks **and** suits (ace low). More cards
/// beat fewer; among equal counts, the lower top card wins, then the next. Takes up to the best
/// 4 of any number of cards. Bigger = better.
pub fn eval_badugi(cards: CardSet) -> HandValue {
    let list: Vec<Card> = cards.iter().collect();
    let n = list.len().min(16);
    let mut best = HandValue(0);
    for mask in 1u32..(1 << n) {
        if mask.count_ones() > 4 {
            continue;
        }
        let pick: Vec<Card> = (0..n)
            .filter(|&i| mask & (1 << i) != 0)
            .map(|i| list[i])
            .collect();
        let (mut ranks, mut suits) = (0u16, 0u8);
        let ok = pick.iter().all(|c| {
            let r = 1u16 << ace_low(c.rank());
            let s = 1u8 << c.suit() as u8;
            let fresh = ranks & r == 0 && suits & s == 0;
            ranks |= r;
            suits |= s;
            fresh
        });
        if !ok {
            continue;
        }
        let mut lows: Vec<u8> = pick.iter().map(|c| ace_low(c.rank())).collect();
        lows.sort_unstable_by(|a, b| b.cmp(a)); // highest first
        let mut v = (pick.len() as u32) << 16;
        for (i, r) in lows.iter().enumerate() {
            v |= ((12 - *r) as u32) << (4 * (3 - i));
        }
        best = best.max(HandValue(v));
    }
    best
}

fn ace_low(r: Rank) -> u8 {
    (r as u8 + 1) % 13
}

/// 6+ Hold'em (36-card deck, 6 through A): **flush beats full house**, three of a kind stays
/// below a straight (D-6P-1), and A-6-7-8-9 is the lowest straight. Best 5 of 5–7 naturals.
///
/// The value is packed with the category slots of Flush and Full house swapped, so comparisons
/// are right; [`short_deck_category`] gives the real name.
pub fn eval_short_deck(cards: CardSet) -> HandValue {
    let mut best: Option<HandValue> = None;
    for_each_five(cards, |five| {
        let (ranks, suited) = five_ranks(&five);
        let mut sorted = ranks;
        sorted.sort_unstable_by(|a, b| b.cmp(a));
        // A-6-7-8-9 (ranks 12,7,6,5,4): a 9-high straight.
        let low_wheel = sorted == [12, 7, 6, 5, 4];
        let mut v = if low_wheel {
            let c = if suited {
                Category::StraightFlush
            } else {
                Category::Straight
            };
            HandValue::pack(c, &[7])
        } else {
            score5(ranks, suited, false)
        };
        v = match v.category() {
            Category::Flush => HandValue::pack(Category::FullHouse, &unpack_ranks(v)),
            Category::FullHouse => HandValue::pack(Category::Flush, &unpack_ranks(v)),
            _ => v,
        };
        best = best.max(Some(v));
    });
    best.expect("eval_short_deck needs at least 5 cards")
}

/// The real category of a [`eval_short_deck`] value.
pub fn short_deck_category(v: HandValue) -> Category {
    match v.category() {
        Category::Flush => Category::FullHouse,
        Category::FullHouse => Category::Flush,
        c => c,
    }
}

fn unpack_ranks(v: HandValue) -> Vec<u8> {
    (0..5)
        .map(|i| ((v.0 >> (16 - 4 * i)) & 0xF) as u8)
        .collect()
}

/// The 36-card deck (6 through A).
pub fn short_deck() -> CardSet {
    let mut s = CardSet::EMPTY;
    for suit in Suit::ALL {
        for r in &Rank::ALL[4..] {
            s.insert(Card::new(*r, suit));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(s: &str) -> CardSet {
        s.split_whitespace()
            .map(|c| c.parse::<Card>().unwrap())
            .collect()
    }

    #[test]
    fn deuce_to_seven() {
        let best = eval_low27(set("7c 5d 4h 3s 2c"));
        assert!(best > eval_low27(set("7c 6d 4h 3s 2c")));
        assert!(best > eval_low27(set("8c 5d 4h 3s 2c")));
        // A-2-3-4-5 is ace-high, not a straight: worse than any 7- or K-high.
        assert!(eval_low27(set("Kc 5d 4h 3s 2c")) > eval_low27(set("Ac 5d 4h 3s 2c")));
        // Straights and flushes count against.
        assert!(eval_low27(set("8c 7d 5h 3s 2c")) > eval_low27(set("6c 5d 4h 3s 2c")));
        assert!(eval_low27(set("8d 7d 5h 3s 2c")) > eval_low27(set("8c 7c 5c 3c 2c")));
        // Pairs are bad.
        assert!(eval_low27(set("Kc Qd Jh 9s 8c")) > eval_low27(set("2c 2d 3h 4s 5c")));
    }

    #[test]
    fn badugi() {
        let four = eval_badugi(set("Ac 2d 3h 4s"));
        assert!(
            four > eval_badugi(set("Ac 2d 3h 5s")),
            "lower top card wins"
        );
        assert!(
            eval_badugi(set("Kc Qd Jh Ts")) > eval_badugi(set("Ac 2c 3h 4s")),
            "4 cards beat 3"
        );
        // A pair or a suit repeat drops a card.
        assert_eq!(eval_badugi(set("Ac Ad 3h 4s")).0 >> 16, 3);
        assert_eq!(eval_badugi(set("Ac 2c 3c 4c")).0 >> 16, 1);
    }

    #[test]
    fn short_deck_order() {
        let flush = eval_short_deck(set("Ac 9c 8c 7c Jc"));
        let boat = eval_short_deck(set("Ac Ad Ah Kc Kd"));
        assert!(flush > boat, "flush beats full house");
        assert_eq!(short_deck_category(flush), Category::Flush);
        assert_eq!(short_deck_category(boat), Category::FullHouse);
        let low_straight = eval_short_deck(set("Ac 6d 7h 8s 9c"));
        assert_eq!(short_deck_category(low_straight), Category::Straight);
        assert!(eval_short_deck(set("6c 7d 8h 9s Tc")) > low_straight);
        let trips = eval_short_deck(set("Ac Ad Ah Kc Qd"));
        assert!(low_straight > trips, "D-6P-1: trips stay below a straight");
        assert_eq!(short_deck().len(), 36);
    }
}
