//! How a scripted game builds and ranks a hand for one pot share.
//!
//! A share says which cards may be used ([`Build`]) and how hands compare ([`Ranking`]). Values
//! are `HandValue`s where bigger is better, so `engine::pot::award_shares` compares them directly.

use serde::{Deserialize, Serialize};

use engine::eval::{
    eval_badugi, eval_high_wild, eval_low_a6_wild, eval_low27, eval_short_deck, short_deck_category,
};
use engine::{Card, CardSet, HandValue, Rank, Suit};

/// How hands compare.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ranking {
    /// High poker (house order, wilds allowed).
    High,
    /// Ace-to-six low (house low, no qualifier), wilds allowed.
    LowA6,
    /// Deuce-to-seven low: aces high, straights and flushes count against. No wilds.
    Low27,
    /// Badugi: best set of distinct ranks and suits, ace low (4-card hands).
    Badugi,
    /// 6+ Hold'em: 36-card deck, flush beats full house, A-6-7-8-9 is the lowest straight.
    ShortDeck,
    /// Chicago High: the highest spade among the seat's own cards wins (D-CHI-1).
    SpadeHigh,
    /// Chicago Low: the lowest spade (ace high, so 2♠ is lowest; D-CHI-2).
    SpadeLow,
    /// Indian Poker: the single highest card (ace high; same rank ties).
    HighCard,
}

/// Which cards make a hand.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Build {
    /// Best five of the seat's cards plus the board, any mix (Hold'em, stud).
    Any,
    /// Exactly `hole` of the seat's cards and exactly `board` of the board's (Omaha: 2 and 3).
    Exactly { hole: u8, board: u8 },
    /// The seat's own cards only (draw games, Badugi, spade halves).
    Own,
    /// Any of these (own, board) splits of five cards (Industrial Elevator: 2+3 or 3+2).
    Combos(&'static [(u8, u8)]),
}

/// One share of the pot.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ShareRule {
    pub name: &'static str,
    pub ranking: Ranking,
    pub build: Build,
    /// Which board this share plays on (multi-board games); ignored when there's no board.
    pub board: u8,
}

/// Every `k`-subset of `cards`, as `CardSet`s.
pub fn subsets(cards: &[Card], k: usize, mut f: impl FnMut(CardSet)) {
    fn go(cards: &[Card], k: usize, start: usize, acc: CardSet, f: &mut dyn FnMut(CardSet)) {
        if acc.len() == k {
            f(acc);
            return;
        }
        let need = k - acc.len();
        for i in start..cards.len() {
            if cards.len() - i < need {
                break;
            }
            let mut next = acc;
            next.insert(cards[i]);
            go(cards, k, i + 1, next, f);
        }
    }
    if k <= cards.len() {
        go(cards, k, 0, CardSet::EMPTY, &mut f);
    }
}

/// Value of exactly five cards (wilds per `wild`).
fn five(r: Ranking, cards: CardSet, wild: CardSet) -> Option<HandValue> {
    let (nat, w) = cards.split_wild(wild);
    if nat.len() + w != 5 {
        return None; // a dead joker
    }
    Some(match r {
        Ranking::High => eval_high_wild(nat, w),
        Ranking::LowA6 => eval_low_a6_wild(nat, w),
        Ranking::Low27 if w == 0 => eval_low27(nat),
        Ranking::ShortDeck if w == 0 => eval_short_deck(nat),
        Ranking::Low27 | Ranking::ShortDeck => return None, // never wild in any Bible game
        Ranking::Badugi | Ranking::SpadeHigh | Ranking::SpadeLow | Ranking::HighCard => {
            unreachable!()
        }
    })
}

/// Best value from any 5 of `cards`.
fn best_of(r: Ranking, cards: &[Card], wild: CardSet) -> Option<HandValue> {
    let set: CardSet = cards.iter().copied().collect();
    let (nat, w) = set.split_wild(wild);
    let n = nat.len() + w;
    // The fast evaluators take 5–7 cards; beyond that, try every 5-card subset.
    if (5..=7).contains(&n) && matches!(r, Ranking::High | Ranking::LowA6) {
        return Some(match r {
            Ranking::High => eval_high_wild(nat, w),
            _ => eval_low_a6_wild(nat, w),
        });
    }
    let mut best = None;
    subsets(cards, 5, |s| {
        if let Some(v) = five(r, s, wild) {
            best = best.max(Some(v));
        }
    });
    best
}

/// Best value using exactly `hole` own cards and `nb` board cards.
fn exactly(
    r: Ranking,
    own: &[Card],
    board: &[Card],
    hole: u8,
    nb: u8,
    wild: CardSet,
) -> Option<HandValue> {
    let mut best = None;
    subsets(own, hole as usize, |h| {
        subsets(board, nb as usize, |b| {
            if let Some(v) = five(r, h.union(b), wild) {
                best = best.max(Some(v));
            }
        });
    });
    best
}

/// The seat's value for one share. `own` = the seat's cards (down and up), `board` = the
/// share's board. `None` when no hand can be made.
pub fn share_value(
    rule: &ShareRule,
    own: &[Card],
    board: &[Card],
    wild: CardSet,
) -> Option<HandValue> {
    match rule.ranking {
        Ranking::SpadeHigh => own
            .iter()
            .filter(|c| !c.is_joker() && c.suit() == Suit::Spades)
            .map(|c| c.rank() as u32 + 1)
            .max()
            .map(HandValue),
        Ranking::SpadeLow => own
            .iter()
            .filter(|c| !c.is_joker() && c.suit() == Suit::Spades)
            .map(|c| 13 - c.rank() as u32)
            .max()
            .map(HandValue),
        Ranking::HighCard => own
            .iter()
            .filter(|c| !c.is_joker())
            .map(|c| c.rank() as u32 + 1)
            .max()
            .map(HandValue),
        Ranking::Badugi => {
            let set: CardSet = own.iter().copied().filter(|c| !c.is_joker()).collect();
            Some(eval_badugi(set))
        }
        r => match rule.build {
            Build::Own => best_of(r, own, wild),
            Build::Any => {
                let all: Vec<Card> = own.iter().chain(board).copied().collect();
                best_of(r, &all, wild)
            }
            Build::Exactly { hole, board: nb } => exactly(r, own, board, hole, nb, wild),
            Build::Combos(splits) => splits
                .iter()
                .filter_map(|&(h, b)| exactly(r, own, board, h, b, wild))
                .max(),
        },
    }
}

/// Short label for a hand value under a ranking (showdown text).
pub fn describe(r: Ranking, v: HandValue) -> String {
    match r {
        Ranking::High => v.category().to_string(),
        Ranking::ShortDeck => short_deck_category(v).to_string(),
        Ranking::SpadeHigh => format!("{}♠ high spade", Rank::ALL[(v.0 - 1) as usize].to_char()),
        Ranking::SpadeLow => format!("{}♠ low spade", Rank::ALL[(13 - v.0) as usize].to_char()),
        Ranking::HighCard => format!("{} high", Rank::ALL[(v.0 - 1) as usize].to_char()),
        _ => format!("{r:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cards(s: &str) -> Vec<Card> {
        s.split_whitespace().map(|c| c.parse().unwrap()).collect()
    }

    const OMAHA_HIGH: ShareRule = ShareRule {
        name: "High",
        ranking: Ranking::High,
        build: Build::Exactly { hole: 2, board: 3 },
        board: 0,
    };

    #[test]
    fn omaha_uses_exactly_two_hole_cards() {
        // Four spades on board + one spade in hand is NOT a flush in Omaha.
        let own = cards("As Kd Qc Jh");
        let board = cards("2s 5s 8s 9s 3d");
        let v = share_value(&OMAHA_HIGH, &own, &board, CardSet::EMPTY).unwrap();
        assert_eq!(v.category(), engine::Category::HighCard);
        // The same cards in Hold'em-style "any five" make a flush.
        let any = ShareRule {
            build: Build::Any,
            ..OMAHA_HIGH
        };
        let v = share_value(&any, &own, &board, CardSet::EMPTY).unwrap();
        assert_eq!(v.category(), engine::Category::Flush);
    }

    #[test]
    fn omaha_matches_brute_force() {
        use engine::eval::oracle::oracle_high;
        use engine::game::sample_cards;
        use engine::rng::rng_from_seed;
        let mut rng = rng_from_seed(4);
        for _ in 0..200 {
            let dealt = sample_cards(CardSet::FULL_DECK, 9, &mut rng);
            let (own, board) = dealt.split_at(4);
            let mut want = None;
            for i in 0..4 {
                for j in i + 1..4 {
                    subsets(board, 3, |b| {
                        let mut five: Vec<Card> = b.iter().collect();
                        five.extend([own[i], own[j]]);
                        want = want.max(Some(oracle_high(&five)));
                    });
                }
            }
            assert_eq!(share_value(&OMAHA_HIGH, own, board, CardSet::EMPTY), want);
        }
    }

    #[test]
    fn spades_for_chicago() {
        let hi = ShareRule {
            name: "Spade",
            ranking: Ranking::SpadeHigh,
            build: Build::Own,
            board: 0,
        };
        let lo = ShareRule {
            ranking: Ranking::SpadeLow,
            ..hi
        };
        let a = cards("As 2s Kd");
        let b = cards("Ks 3s Qd");
        let none = cards("Kd Qh Jc");
        let v = |r: &ShareRule, c: &[Card]| share_value(r, c, &[], CardSet::EMPTY);
        assert!(v(&hi, &a) > v(&hi, &b), "A♠ beats K♠ for high spade");
        assert!(
            v(&lo, &a) > v(&lo, &b),
            "2♠ beats 3♠ for low spade (ace high)"
        );
        assert_eq!(v(&hi, &none), None);
    }

    #[test]
    fn more_than_seven_cards_takes_the_best_five() {
        // 3 Blind Mice: best 5 of 8.
        let any = ShareRule {
            name: "High",
            ranking: Ranking::High,
            build: Build::Any,
            board: 0,
        };
        let own = cards("2c 7d 9h Ks Kd");
        let board = cards("Kh Kc 4s");
        let v = share_value(&any, &own, &board, CardSet::EMPTY).unwrap();
        assert_eq!(v.category(), engine::Category::Quads);
    }
}
