//! Evaluators behind one trait, looked up by name (catalog §2, G0), so games share them and
//! every evaluator is oracle-tested in one place (the test below runs over the whole registry).
//!
//! An evaluator sees a hand as private cards + board cards + the game's wild set. High and A-6
//! low use the union; board-constrained rankings (Omaha 2-from-hand, 3-from-board) use the split.
//! Add a new evaluator by implementing [`Evaluator`] and listing it in [`EVALUATORS`]; the
//! registry test then checks it against its own `oracle`.

use crate::card::{Card, CardSet};

use super::HandValue;
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

/// Every registered evaluator.
pub static EVALUATORS: &[&dyn Evaluator] = &[&High, &LowA6];

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
        for e in EVALUATORS {
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
