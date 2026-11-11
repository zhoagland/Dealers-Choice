//! Partial stud boards: who's showing the best hand on 1–4 upcards (FTQ action order, PRD §7,
//! assumption A8).
//!
//! Only pairs, two pair, trips, quads, and five of a kind count. Straights and flushes don't
//! exist on partial boards. Visible wild cards count: each joins the biggest group of natural
//! cards (ties go to the higher rank), which is the best thing a wild can do on a board this
//! small. A board of only wilds shows as that many aces.

use smallvec::SmallVec;

use super::{Category, HandValue};
use crate::card::{Card, Rank};

/// Strength of the visible upcards. Bigger = better board.
pub fn eval_partial_board(upcards: &[Card], wild: Option<Rank>) -> HandValue {
    debug_assert!(
        (1..=4).contains(&upcards.len()),
        "partial boards have 1-4 upcards"
    );
    let mut counts = [0u8; 13];
    let mut wilds = 0u8;
    for c in upcards {
        if Some(c.rank()) == wild {
            wilds += 1;
        } else {
            counts[c.rank() as usize] += 1;
        }
    }
    // Natural ranks as (count, rank), biggest group first, then highest rank.
    let mut groups: SmallVec<[(u8, u8); 4]> = (0..13u8)
        .filter(|&r| counts[r as usize] > 0)
        .map(|r| (counts[r as usize], r))
        .collect();
    groups.sort_by(|a, b| b.cmp(a));

    let (lead_rank, lead_count) = match groups.first() {
        Some(&(c, r)) => (r, c + wilds),
        None => (Rank::Ace as u8, wilds), // all wilds: play as aces
    };
    // Kickers: the remaining natural ranks, best group first.
    let rest: SmallVec<[(u8, u8); 4]> = groups.iter().skip(1).copied().collect();

    let category = match lead_count {
        5.. => Category::FiveOfAKind,
        4 => Category::Quads,
        3 => Category::Trips,
        2 if rest.first().is_some_and(|&(c, _)| c >= 2) => Category::TwoPair,
        2 => Category::Pair,
        _ => Category::HighCard,
    };
    let mut ranks: SmallVec<[u8; 5]> = SmallVec::from_slice(&[lead_rank]);
    ranks.extend(rest.iter().map(|&(_, r)| r));
    HandValue::pack(category, &ranks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board(s: &str, wild: Option<Rank>) -> HandValue {
        let cards: Vec<Card> = s.split_whitespace().map(|c| c.parse().unwrap()).collect();
        eval_partial_board(&cards, wild)
    }

    #[test]
    fn higher_card_and_pairs() {
        assert!(board("Ah", None) > board("Kd", None));
        assert!(board("2c 2d", None) > board("Ah Kd", None));
        assert!(
            board("2c 2d 3h 3s", None) > board("Ac Ad Kh Qs", None),
            "two pair > pair"
        );
        assert!(
            board("2c 2d 2h", None) > board("Ac Ad Kh Ks", None),
            "trips > two pair"
        );
        assert!(board("Ac Ad 9h", None) > board("Ac Ad 8h", None), "kicker");
    }

    #[test]
    fn no_straights_or_flushes() {
        assert_eq!(board("9h Th Jh Qh", None).category(), Category::HighCard);
    }

    #[test]
    fn wilds_join_the_best_group() {
        let fives = Some(Rank::Five);
        assert_eq!(
            board("Kc 5h", fives),
            board("Kc Kd", None),
            "K + wild = pair of kings"
        );
        assert_eq!(
            board("5h", fives),
            board("Ad", None),
            "lone wild plays as an ace"
        );
        assert_eq!(board("9c 9d 5h", fives).category(), Category::Trips);
        assert_eq!(board("5c 5d 5h 5s", fives).category(), Category::Quads);
    }
}
