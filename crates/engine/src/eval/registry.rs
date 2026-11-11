//! Evaluators behind one trait, looked up by name (catalog §2, G0), so games share them and
//! every evaluator is oracle-tested in one place (the test below runs over the whole registry).
//!
//! An evaluator sees a hand as private cards + board cards + the game's wild set. High and A-6
//! low use the union; board-constrained rankings (Omaha 2-from-hand, 3-from-board) use the split.
//! Add a new evaluator by implementing [`Evaluator`] and listing it in [`EVALUATORS`]; the
//! registry test then checks it against its own `oracle`.

use crate::card::{Card, CardSet};

use super::HandValue;
use super::extra::{eval_badugi, eval_low27, eval_short_deck};
use super::high::eval_high_wild;
use super::low_a6::eval_low_a6_wild;
use super::oracle::{oracle_high, oracle_high_wild, oracle_low_a6, oracle_low_a6_wild};

/// The cards an evaluator scores.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct HandCards {
    /// The seat's own cards (hole cards, or all of a stud hand).
    pub private: CardSet,
    /// Shared cards (empty in stud and draw games).
    pub board: CardSet,
    /// Cards that are wild in this game right now (e.g. jokers, queens in FTQ). A joker not in
    /// this set is dead (see [`CardSet::split_wild`]).
    pub wild: CardSet,
}

impl HandCards {
    /// A hand with no board.
    pub fn new(private: CardSet, wild: CardSet) -> HandCards {
        HandCards {
            private,
            board: CardSet::EMPTY,
            wild,
        }
    }
}

pub trait Evaluator: Send + Sync {
    /// Stable name games use in their config, e.g. `"high"`.
    fn id(&self) -> &'static str;
    fn description(&self) -> &'static str;
    /// The best hand under this ranking (bigger is better), or `None` if these cards can't
    /// form one (wrong number of cards).
    fn eval(&self, hand: &HandCards) -> Option<HandValue>;
    /// Slow, obviously-correct reference. Tests compare `eval` with it.
    fn oracle(&self, hand: &HandCards) -> Option<HandValue>;
}

/// Naturals and wild count for union-based evaluators; `None` outside 5–7 cards.
fn union_split(hand: &HandCards) -> Option<(CardSet, usize)> {
    let (nat, w) = hand.private.union(hand.board).split_wild(hand.wild);
    (5..=7).contains(&(nat.len() + w)).then_some((nat, w))
}

fn cards(set: CardSet) -> Vec<Card> {
    set.iter().collect()
}

/// High poker, best 5 of 5–7, wilds allowed (house ranking: Royal > Five of a kind > …).
pub struct High;

impl Evaluator for High {
    fn id(&self) -> &'static str {
        "high"
    }
    fn description(&self) -> &'static str {
        "High poker, best five of five to seven cards, wild cards allowed"
    }
    fn eval(&self, hand: &HandCards) -> Option<HandValue> {
        union_split(hand).map(|(nat, w)| eval_high_wild(nat, w))
    }
    fn oracle(&self, hand: &HandCards) -> Option<HandValue> {
        union_split(hand).map(|(nat, w)| {
            if w == 0 {
                oracle_high(&cards(nat))
            } else {
                oracle_high_wild(nat, w)
            }
        })
    }
}

/// Ace-to-six lowball (house low, no qualifier), best 5 of 5–7, wilds allowed.
pub struct LowA6;

impl Evaluator for LowA6 {
    fn id(&self) -> &'static str {
        "low_a6"
    }
    fn description(&self) -> &'static str {
        "Ace-to-six low, best five of five to seven cards, wild cards allowed"
    }
    fn eval(&self, hand: &HandCards) -> Option<HandValue> {
        union_split(hand).map(|(nat, w)| eval_low_a6_wild(nat, w))
    }
    fn oracle(&self, hand: &HandCards) -> Option<HandValue> {
        union_split(hand).map(|(nat, w)| {
            if w == 0 {
                oracle_low_a6(&cards(nat))
            } else {
                oracle_low_a6_wild(nat, w)
            }
        })
    }
}

/// Naturals for rankings that never use wilds: `None` if any card is wild or the count is off.
fn no_wild(hand: &HandCards, counts: std::ops::RangeInclusive<usize>) -> Option<CardSet> {
    let (nat, w) = hand.private.union(hand.board).split_wild(hand.wild);
    (w == 0 && counts.contains(&nat.len())).then_some(nat)
}

/// Brute force: the best value of `f` over every 5-card subset (independent of the fast
/// functions' own subset loops).
fn brute5(set: CardSet, f: impl Fn(CardSet) -> HandValue) -> HandValue {
    let list = cards(set);
    let mut best = HandValue(0);
    super::oracle::for_each_subset(&list, 5, |five| {
        best = best.max(f(five.iter().copied().collect()));
    });
    best
}

/// Deuce-to-seven low (2-7 draw games). No wilds.
pub struct Low27;

impl Evaluator for Low27 {
    fn id(&self) -> &'static str {
        "low_27"
    }
    fn description(&self) -> &'static str {
        "Deuce-to-seven low: aces high, straights and flushes count against, no wilds"
    }
    fn eval(&self, hand: &HandCards) -> Option<HandValue> {
        no_wild(hand, 5..=7).map(eval_low27)
    }
    fn oracle(&self, hand: &HandCards) -> Option<HandValue> {
        no_wild(hand, 5..=7).map(|c| brute5(c, eval_low27))
    }
}

/// Badugi (four cards, distinct ranks and suits, ace low). No wilds.
pub struct Badugi;

impl Evaluator for Badugi {
    fn id(&self) -> &'static str {
        "badugi"
    }
    fn description(&self) -> &'static str {
        "Badugi: best set of distinct ranks and suits, ace low"
    }
    fn eval(&self, hand: &HandCards) -> Option<HandValue> {
        no_wild(hand, 1..=7).map(eval_badugi)
    }
    fn oracle(&self, hand: &HandCards) -> Option<HandValue> {
        // Best over every 4-card (or smaller) subset, each scored on its own.
        no_wild(hand, 1..=7).map(|c| {
            let list = cards(c);
            let mut best = HandValue(0);
            for k in 1..=list.len().min(4) {
                crate::eval::registry::each_subset(&list, k, &mut |s| {
                    best = best.max(eval_badugi(s));
                });
            }
            best
        })
    }
}

/// Every `k`-subset of `list` (small k, tests only).
pub(crate) fn each_subset(list: &[Card], k: usize, f: &mut dyn FnMut(CardSet)) {
    fn go(list: &[Card], k: usize, i: usize, acc: CardSet, f: &mut dyn FnMut(CardSet)) {
        if acc.len() == k {
            f(acc);
        } else if i < list.len() {
            let mut with = acc;
            with.insert(list[i]);
            go(list, k, i + 1, with, f);
            go(list, k, i + 1, acc, f);
        }
    }
    go(list, k, 0, CardSet::EMPTY, f);
}

/// 6+ Hold'em high (flush beats full house). No wilds.
pub struct ShortDeck;

impl Evaluator for ShortDeck {
    fn id(&self) -> &'static str {
        "short_deck"
    }
    fn description(&self) -> &'static str {
        "6+ (short deck) high: flush beats full house, A-6-7-8-9 is the lowest straight"
    }
    fn eval(&self, hand: &HandCards) -> Option<HandValue> {
        no_wild(hand, 5..=7).map(eval_short_deck)
    }
    fn oracle(&self, hand: &HandCards) -> Option<HandValue> {
        no_wild(hand, 5..=7).map(|c| brute5(c, eval_short_deck))
    }
}

/// Every registered evaluator.
pub static EVALUATORS: &[&dyn Evaluator] = &[&High, &LowA6, &Low27, &Badugi, &ShortDeck];

/// Look an evaluator up by its [`Evaluator::id`].
pub fn evaluator(id: &str) -> Option<&'static dyn Evaluator> {
    EVALUATORS.iter().copied().find(|e| e.id() == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::Rank;
    use crate::game::sample_cards;
    use crate::rng::rng_from_seed;
    use rand::Rng;

    #[test]
    fn ids_are_unique_and_found() {
        for (i, e) in EVALUATORS.iter().enumerate() {
            assert!(
                EVALUATORS[..i].iter().all(|o| o.id() != e.id()),
                "duplicate evaluator id {}",
                e.id()
            );
            assert_eq!(evaluator(e.id()).unwrap().id(), e.id());
        }
        assert!(evaluator("nope").is_none());
    }

    /// Every registered evaluator agrees with its oracle on random hands: 5–7 cards from the
    /// 54-card deck, split between private and board, with jokers and sometimes a wild rank.
    #[test]
    fn every_evaluator_matches_its_oracle() {
        let mut rng = rng_from_seed(0xE7A1);
        for e in EVALUATORS {
            for t in 0..250 {
                let n = rng.random_range(5..=7);
                let dealt = sample_cards(CardSet::FULL_DECK_WITH_JOKERS, n, &mut rng);
                let split = rng.random_range(0..=n);
                let private: CardSet = dealt[..split].iter().copied().collect();
                let board: CardSet = dealt[split..].iter().copied().collect();
                let mut wild = if rng.random_bool(0.7) {
                    CardSet::JOKERS
                } else {
                    CardSet::EMPTY
                };
                if t % 5 == 0 {
                    let r = Rank::ALL[rng.random_range(0..13)];
                    for s in crate::card::Suit::ALL {
                        wild.insert(Card::new(r, s));
                    }
                }
                let hand = HandCards {
                    private,
                    board,
                    wild,
                };
                // Keep the oracle's 52^w substitutions affordable.
                let (_, w) = private.union(board).split_wild(wild);
                if w > 2 {
                    continue;
                }
                assert_eq!(
                    e.eval(&hand),
                    e.oracle(&hand),
                    "{}: {:?} + board {:?}, wild {:?}",
                    e.id(),
                    private,
                    board,
                    wild
                );
            }
        }
    }

    #[test]
    fn wrong_card_counts_make_no_hand() {
        let four: CardSet = ["As", "Ks", "Qs", "Js"]
            .iter()
            .map(|c| c.parse::<Card>().unwrap())
            .collect();
        // Badugi is a four-card game; every other ranking needs five.
        for e in EVALUATORS.iter().filter(|e| e.id() != "badugi") {
            assert_eq!(
                e.eval(&HandCards::new(four, CardSet::EMPTY)),
                None,
                "{}",
                e.id()
            );
        }
        // A dead joker doesn't count toward the five.
        let mut five = four;
        five.insert(Card::RED_JOKER);
        assert_eq!(High.eval(&HandCards::new(five, CardSet::EMPTY)), None);
        assert!(High.eval(&HandCards::new(five, CardSet::JOKERS)).is_some());
    }
}
