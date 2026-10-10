//! Cards, ranks, suits, card sets, and decks. Implements PRD FR-EC-1.
//!
//! Representation (see `docs/tech/engine.md` §3):
//! - [`Card`] is one byte: `suit * 16 + rank`, with rank 0 = Two … 12 = Ace.
//! - [`CardSet`] is a `u64` bitmask: bit `i` is set when `Card(i)` is in the set.
//!
//! Each suit occupies its own 16-bit "lane" of the `u64` (only 13 of the 16 bits are used).
//! This makes "which ranks do I hold in hearts?" a single shift-and-mask
//! ([`CardSet::rank_mask`]), which is what the hand evaluators will build on.

use std::fmt;
use std::str::FromStr;

use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};

use crate::rng::DetRng;

// ---------------------------------------------------------------------------------------------
// Suit
// ---------------------------------------------------------------------------------------------

/// A card suit. No suit outranks another in any Bible game; the order here is only for storage.
///
/// `#[repr(u8)]` pins each variant to the number written after `=`, so `suit as u8` is that
/// number. We rely on it when packing a `Card`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(u8)]
pub enum Suit {
    Clubs = 0,
    Diamonds = 1,
    Hearts = 2,
    Spades = 3,
}

impl Suit {
    /// Every suit, in index order. `Suit::ALL[i]` is the suit with index `i`.
    pub const ALL: [Suit; 4] = [Suit::Clubs, Suit::Diamonds, Suit::Hearts, Suit::Spades];

    /// The one-letter form used in card strings like `"Ah"`.
    pub const fn to_char(self) -> char {
        match self {
            Suit::Clubs => 'c',
            Suit::Diamonds => 'd',
            Suit::Hearts => 'h',
            Suit::Spades => 's',
        }
    }

    /// Accepts upper or lower case.
    pub fn from_char(c: char) -> Option<Suit> {
        match c.to_ascii_lowercase() {
            'c' => Some(Suit::Clubs),
            'd' => Some(Suit::Diamonds),
            'h' => Some(Suit::Hearts),
            's' => Some(Suit::Spades),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Rank
// ---------------------------------------------------------------------------------------------

/// A card rank, ordered **ace-high** (`Two < … < King < Ace`).
///
/// Games that treat the ace as low (A-6 lowball, the wheel straight A-2-3-4-5) handle that in
/// their evaluator. Storing one canonical order keeps `Card` simple and makes `Ord` on `Rank`
/// mean "higher in high poker", which is the common case.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(u8)]
pub enum Rank {
    Two = 0,
    Three = 1,
    Four = 2,
    Five = 3,
    Six = 4,
    Seven = 5,
    Eight = 6,
    Nine = 7,
    Ten = 8,
    Jack = 9,
    Queen = 10,
    King = 11,
    Ace = 12,
}

impl Rank {
    /// Every rank, low to high. `Rank::ALL[i]` is the rank with index `i`.
    pub const ALL: [Rank; 13] = [
        Rank::Two,
        Rank::Three,
        Rank::Four,
        Rank::Five,
        Rank::Six,
        Rank::Seven,
        Rank::Eight,
        Rank::Nine,
        Rank::Ten,
        Rank::Jack,
        Rank::Queen,
        Rank::King,
        Rank::Ace,
    ];

    /// The one-character form used in card strings: `2`–`9`, `T`, `J`, `Q`, `K`, `A`.
    pub const fn to_char(self) -> char {
        match self {
            Rank::Two => '2',
            Rank::Three => '3',
            Rank::Four => '4',
            Rank::Five => '5',
            Rank::Six => '6',
            Rank::Seven => '7',
            Rank::Eight => '8',
            Rank::Nine => '9',
            Rank::Ten => 'T',
            Rank::Jack => 'J',
            Rank::Queen => 'Q',
            Rank::King => 'K',
            Rank::Ace => 'A',
        }
    }

    /// Accepts upper or lower case. Ten is `T` (not `10`) so every card is exactly two characters.
    pub fn from_char(c: char) -> Option<Rank> {
        let rank = match c.to_ascii_uppercase() {
            '2' => Rank::Two,
            '3' => Rank::Three,
            '4' => Rank::Four,
            '5' => Rank::Five,
            '6' => Rank::Six,
            '7' => Rank::Seven,
            '8' => Rank::Eight,
            '9' => Rank::Nine,
            'T' => Rank::Ten,
            'J' => Rank::Jack,
            'Q' => Rank::Queen,
            'K' => Rank::King,
            'A' => Rank::Ace,
            _ => return None,
        };
        Some(rank)
    }
}

// ---------------------------------------------------------------------------------------------
// Card
// ---------------------------------------------------------------------------------------------

/// One playing card, packed into a byte as `suit * 16 + rank`.
///
/// `Copy` means a `Card` is duplicated on assignment instead of moved, like an integer, so
/// you never fight the borrow checker over cards.
///
/// The field is private (`Card(u8)`, not `pub u8`), so the only ways to make a `Card` are
/// [`Card::new`], [`Card::from_index`], parsing, or deserializing. All of them guarantee the
/// byte is a valid card.
///
/// Serde goes through `u8` with validation (`try_from`), so a corrupt file can't create an
/// invalid card.
///
/// Note: the derived `Ord` sorts by the packed byte (suit first, then rank). That's a
/// stable storage order, **not** poker strength. Compare `card.rank()` for strength.
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct Card(u8);

impl Card {
    /// `const fn` can run at compile time, so cards can be constants:
    /// `const ACE_SPADES: Card = Card::new(Rank::Ace, Suit::Spades);`
    pub const fn new(rank: Rank, suit: Suit) -> Card {
        Card((suit as u8) * 16 + rank as u8)
    }

    pub const fn rank(self) -> Rank {
        Rank::ALL[(self.0 & 0x0F) as usize]
    }

    pub const fn suit(self) -> Suit {
        Suit::ALL[(self.0 >> 4) as usize]
    }

    /// The packed byte, 0..=60. Not contiguous: indices 13–15, 29–31, 45–47 are unused.
    pub const fn index(self) -> u8 {
        self.0
    }

    /// Inverse of [`Card::index`]. `None` for bytes that aren't a card.
    pub const fn from_index(i: u8) -> Option<Card> {
        if (i & 0x0F) < 13 && (i >> 4) < 4 {
            Some(Card(i))
        } else {
            None
        }
    }

    /// This card's bit within a [`CardSet`].
    const fn bit(self) -> u64 {
        1u64 << self.0
    }
}

impl From<Card> for u8 {
    fn from(card: Card) -> u8 {
        card.0
    }
}

impl TryFrom<u8> for Card {
    type Error = InvalidCardIndex;

    fn try_from(i: u8) -> Result<Card, InvalidCardIndex> {
        Card::from_index(i).ok_or(InvalidCardIndex(i))
    }
}

/// Prints as `"As"`, `"Td"`, `"2c"`.
impl fmt::Display for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.rank().to_char(), self.suit().to_char())
    }
}

/// Debug uses the same short form, so test failures read `[As, Kd]` instead of `[Card(60), …]`.
impl fmt::Debug for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// Parses `"As"`, `"td"`, `"2C"`. Implementing `FromStr` is what makes `"As".parse::<Card>()` work.
impl FromStr for Card {
    type Err = ParseCardError;

    fn from_str(s: &str) -> Result<Card, ParseCardError> {
        let mut chars = s.chars();
        // Pattern-match exactly two characters. Anything else falls to the error arm.
        match (chars.next(), chars.next(), chars.next()) {
            (Some(r), Some(su), None) => match (Rank::from_char(r), Suit::from_char(su)) {
                (Some(rank), Some(suit)) => Ok(Card::new(rank, suit)),
                _ => Err(ParseCardError(s.to_string())),
            },
            _ => Err(ParseCardError(s.to_string())),
        }
    }
}

/// Returned when a string isn't a card like `"As"`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseCardError(pub String);

impl fmt::Display for ParseCardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid card {:?}: expected rank (2-9,T,J,Q,K,A) then suit (c,d,h,s)",
            self.0
        )
    }
}

impl std::error::Error for ParseCardError {}

/// Returned when a byte isn't a valid packed card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidCardIndex(pub u8);

impl fmt::Display for InvalidCardIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} is not a valid card index", self.0)
    }
}

impl std::error::Error for InvalidCardIndex {}

// ---------------------------------------------------------------------------------------------
// CardSet
// ---------------------------------------------------------------------------------------------

/// The 13 low bits of one suit lane.
const RANK_BITS: u64 = 0x1FFF;

/// A set of distinct cards, as a 64-bit mask. `Copy`, 8 bytes, and every operation is a few
/// instructions, so it's cheap enough to use inside search loops.
///
/// Use it for "which cards are known / dealt / remaining". Use a `Vec<Card>` or array when order
/// matters, e.g. a stud hand's down-up-up order.
#[derive(Copy, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct CardSet(u64);

impl CardSet {
    pub const EMPTY: CardSet = CardSet(0);

    /// All 52 cards.
    pub const FULL_DECK: CardSet =
        CardSet(RANK_BITS | (RANK_BITS << 16) | (RANK_BITS << 32) | (RANK_BITS << 48));

    pub const fn contains(self, card: Card) -> bool {
        self.0 & card.bit() != 0
    }

    /// Returns `true` if the card was newly added (like `HashSet::insert`).
    pub fn insert(&mut self, card: Card) -> bool {
        let was_absent = !self.contains(card);
        self.0 |= card.bit();
        was_absent
    }

    /// Returns `true` if the card was present.
    pub fn remove(&mut self, card: Card) -> bool {
        let was_present = self.contains(card);
        self.0 &= !card.bit();
        was_present
    }

    /// Number of cards. `count_ones` compiles to a single POPCNT instruction on modern CPUs.
    pub const fn len(self) -> usize {
        self.0.count_ones() as usize
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub const fn union(self, other: CardSet) -> CardSet {
        CardSet(self.0 | other.0)
    }

    pub const fn intersection(self, other: CardSet) -> CardSet {
        CardSet(self.0 & other.0)
    }

    /// Cards in `self` that are not in `other`.
    pub const fn difference(self, other: CardSet) -> CardSet {
        CardSet(self.0 & !other.0)
    }

    pub const fn is_disjoint(self, other: CardSet) -> bool {
        self.0 & other.0 == 0
    }

    /// The ranks held in one suit as a 13-bit mask: bit 0 = Two … bit 12 = Ace.
    /// Example: {Ah, Kh, 2c}.rank_mask(Hearts) == 0b1_1000_0000_0000.
    pub const fn rank_mask(self, suit: Suit) -> u16 {
        ((self.0 >> (16 * suit as u32)) & RANK_BITS) as u16
    }

    /// Cards in ascending index order (all clubs, then diamonds, …).
    pub fn iter(self) -> CardSetIter {
        CardSetIter(self.0)
    }
}

impl From<CardSet> for u64 {
    fn from(set: CardSet) -> u64 {
        set.0
    }
}

impl TryFrom<u64> for CardSet {
    type Error = InvalidCardSetBits;

    /// Rejects masks with bits outside the 52 valid card positions.
    fn try_from(bits: u64) -> Result<CardSet, InvalidCardSetBits> {
        if bits & !CardSet::FULL_DECK.0 == 0 {
            Ok(CardSet(bits))
        } else {
            Err(InvalidCardSetBits(bits))
        }
    }
}

/// Returned when a `u64` has bits that aren't cards.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidCardSetBits(pub u64);

impl fmt::Display for InvalidCardSetBits {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#018x} has bits outside the 52 card positions", self.0)
    }
}

impl std::error::Error for InvalidCardSetBits {}

/// Iterates a [`CardSet`] by repeatedly taking its lowest set bit.
pub struct CardSetIter(u64);

impl Iterator for CardSetIter {
    type Item = Card;

    fn next(&mut self) -> Option<Card> {
        if self.0 == 0 {
            return None;
        }
        let index = self.0.trailing_zeros() as u8;
        // `x & (x - 1)` clears the lowest set bit: subtracting 1 flips that bit and every
        // zero below it, so the AND keeps everything above it unchanged.
        self.0 &= self.0 - 1;
        Some(Card(index))
    }

    // Telling Rust the exact length lets `collect::<Vec<_>>()` allocate once.
    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.0.count_ones() as usize;
        (n, Some(n))
    }
}

impl ExactSizeIterator for CardSetIter {}

/// Lets you write `for card in set { … }`.
impl IntoIterator for CardSet {
    type Item = Card;
    type IntoIter = CardSetIter;

    fn into_iter(self) -> CardSetIter {
        self.iter()
    }
}

/// Lets you write `cards.iter().copied().collect::<CardSet>()`. Duplicates collapse.
impl FromIterator<Card> for CardSet {
    fn from_iter<I: IntoIterator<Item = Card>>(iter: I) -> CardSet {
        let mut set = CardSet::EMPTY;
        for card in iter {
            set.insert(card);
        }
        set
    }
}

/// Prints as `{2c As}`.
impl fmt::Debug for CardSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{{")?;
        for (i, card) in self.iter().enumerate() {
            if i > 0 {
                write!(f, " ")?;
            }
            write!(f, "{card}")?;
        }
        write!(f, "}}")
    }
}

// ---------------------------------------------------------------------------------------------
// Deck
// ---------------------------------------------------------------------------------------------

/// An ordered stack of cards to deal from.
///
/// Game drivers mostly deal through chance nodes (one card per step, see the API doc), and they
/// track "remaining cards" with a [`CardSet`]. `Deck` is for code that wants a physically shuffled
/// order: tests, simple simulations, the CLI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Deck {
    /// The top of the deck is the **end** of the Vec, so dealing is an O(1) `pop`.
    cards: Vec<Card>,
}

impl Deck {
    /// All 52 cards in a fixed order (2c is dealt last, As first).
    pub fn new_ordered() -> Deck {
        Deck {
            cards: CardSet::FULL_DECK.iter().collect(),
        }
    }

    /// A fresh 52-card deck, shuffled.
    pub fn shuffled(rng: &mut DetRng) -> Deck {
        let mut deck = Deck::new_ordered();
        deck.shuffle(rng);
        deck
    }

    /// A deck of exactly these cards, e.g. Kuhn's {J, Q, K} or a stub with known cards removed.
    pub fn from_cards(cards: CardSet) -> Deck {
        Deck {
            cards: cards.iter().collect(),
        }
    }

    /// Uniform (Fisher–Yates) shuffle. Determinism: the same RNG state gives the same order.
    /// `Cargo.lock` pins the `rand` version; upgrading `rand` could change which cards a given
    /// seed deals, so treat that like a game-version bump for saved seeds.
    pub fn shuffle(&mut self, rng: &mut DetRng) {
        self.cards.shuffle(rng);
    }

    /// Takes the top card, or `None` if the deck is empty.
    pub fn deal(&mut self) -> Option<Card> {
        self.cards.pop()
    }

    /// Takes a specific card out of the deck (e.g. a spot's known board card).
    /// Returns `false` if it wasn't there. O(n), fine for 52 cards.
    pub fn remove(&mut self, card: Card) -> bool {
        match self.cards.iter().position(|&c| c == card) {
            Some(i) => {
                self.cards.remove(i);
                true
            }
            None => false,
        }
    }

    pub fn len(&self) -> usize {
        self.cards.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }

    /// The cards still in the deck, as a set.
    pub fn remaining(&self) -> CardSet {
        self.cards.iter().copied().collect()
    }
}

// ---------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::rng_from_seed;

    /// All 52 cards, built from the enums so the test doesn't share code with what it's testing.
    fn all_cards() -> Vec<Card> {
        let mut v = Vec::new();
        for suit in Suit::ALL {
            for rank in Rank::ALL {
                v.push(Card::new(rank, suit));
            }
        }
        v
    }

    #[test]
    fn rank_and_suit_round_trip_for_every_card() {
        for card in all_cards() {
            assert_eq!(Card::new(card.rank(), card.suit()), card);
            assert_eq!(Card::from_index(card.index()), Some(card));
        }
    }

    #[test]
    fn every_valid_index_is_a_distinct_card_and_others_are_rejected() {
        let valid: Vec<Card> = (0..=u8::MAX).filter_map(Card::from_index).collect();
        assert_eq!(valid.len(), 52);
        assert_eq!(Card::from_index(13), None); // first unused slot in the clubs lane
        assert_eq!(Card::from_index(64), None); // past the last lane
    }

    #[test]
    fn display_and_parse_round_trip() {
        for card in all_cards() {
            let s = card.to_string();
            assert_eq!(s.len(), 2);
            assert_eq!(s.parse::<Card>(), Ok(card));
        }
    }

    #[test]
    fn parse_accepts_any_case_and_rejects_garbage() {
        assert_eq!("as".parse::<Card>(), Ok(Card::new(Rank::Ace, Suit::Spades)));
        assert_eq!(
            "TD".parse::<Card>(),
            Ok(Card::new(Rank::Ten, Suit::Diamonds))
        );
        for bad in ["", "A", "10h", "Ax", "1s", "Ass"] {
            assert!(bad.parse::<Card>().is_err(), "{bad:?} should not parse");
        }
    }

    #[test]
    fn rank_order_is_ace_high() {
        assert!(Rank::Ace > Rank::King);
        assert!(Rank::Two < Rank::Three);
        assert_eq!(Rank::ALL.iter().max(), Some(&Rank::Ace));
    }

    #[test]
    fn full_deck_has_52_cards_and_iterates_all_of_them() {
        assert_eq!(CardSet::FULL_DECK.len(), 52);
        let from_iter: Vec<Card> = CardSet::FULL_DECK.iter().collect();
        let mut expected = all_cards();
        expected.sort();
        assert_eq!(from_iter, expected);
    }

    #[test]
    fn insert_remove_contains() {
        let ah: Card = "Ah".parse().unwrap();
        let mut set = CardSet::EMPTY;
        assert!(set.insert(ah));
        assert!(!set.insert(ah), "second insert reports already present");
        assert!(set.contains(ah));
        assert_eq!(set.len(), 1);
        assert!(set.remove(ah));
        assert!(!set.remove(ah));
        assert!(set.is_empty());
    }

    #[test]
    fn set_algebra() {
        let a: CardSet = ["As", "Kd"]
            .iter()
            .map(|s| s.parse::<Card>().unwrap())
            .collect();
        let b: CardSet = ["Kd", "2c"]
            .iter()
            .map(|s| s.parse::<Card>().unwrap())
            .collect();
        assert_eq!(a.union(b).len(), 3);
        assert_eq!(a.intersection(b).len(), 1);
        assert_eq!(
            a.difference(b).iter().collect::<Vec<_>>(),
            vec!["As".parse().unwrap()]
        );
        assert!(!a.is_disjoint(b));
        assert!(CardSet::FULL_DECK.difference(a).is_disjoint(a));
    }

    #[test]
    fn rank_mask_selects_one_suit() {
        let set: CardSet = ["Ah", "Kh", "2c", "2h"]
            .iter()
            .map(|s| s.parse::<Card>().unwrap())
            .collect();
        let ace = 1 << Rank::Ace as u16;
        let king = 1 << Rank::King as u16;
        let two = 1 << Rank::Two as u16;
        assert_eq!(set.rank_mask(Suit::Hearts), ace | king | two);
        assert_eq!(set.rank_mask(Suit::Clubs), two);
        assert_eq!(set.rank_mask(Suit::Spades), 0);
        assert_eq!(CardSet::FULL_DECK.rank_mask(Suit::Diamonds), 0x1FFF);
    }

    #[test]
    fn debug_format_is_readable() {
        let set: CardSet = ["As", "2c"]
            .iter()
            .map(|s| s.parse::<Card>().unwrap())
            .collect();
        assert_eq!(format!("{set:?}"), "{2c As}");
    }

    #[test]
    fn serde_rejects_invalid_values() {
        assert!(Card::try_from(13u8).is_err());
        assert!(CardSet::try_from(1u64 << 13).is_err());
        assert!(CardSet::try_from(u64::from(CardSet::FULL_DECK)).is_ok());
    }

    #[test]
    fn deck_deals_every_card_exactly_once() {
        let mut deck = Deck::shuffled(&mut rng_from_seed(7));
        let mut seen = CardSet::EMPTY;
        while let Some(card) = deck.deal() {
            assert!(seen.insert(card), "{card} dealt twice");
        }
        assert_eq!(seen, CardSet::FULL_DECK);
    }

    #[test]
    fn same_seed_same_order_different_seed_different_order() {
        let a = Deck::shuffled(&mut rng_from_seed(1));
        let b = Deck::shuffled(&mut rng_from_seed(1));
        let c = Deck::shuffled(&mut rng_from_seed(2));
        assert_eq!(a, b);
        // Two independent shuffles colliding has probability 1/52! ≈ 1e-68.
        assert_ne!(a, c);
    }

    #[test]
    fn deck_remove_and_remaining() {
        let mut deck = Deck::new_ordered();
        let as_: Card = "As".parse().unwrap();
        assert!(deck.remove(as_));
        assert!(!deck.remove(as_));
        assert_eq!(deck.len(), 51);
        assert!(!deck.remaining().contains(as_));
    }

    #[test]
    fn shuffle_is_roughly_uniform() {
        // Statistical sanity check, not a proof: over N shuffles, the top card should be
        // uniform over 52 cards. Expected count per card = N/52 ≈ 385 for N = 20 000;
        // the binomial std dev is ≈ 19.4, so a ±30% band is >5σ and won't flake.
        const N: usize = 20_000;
        let mut rng = rng_from_seed(123);
        let mut counts = [0usize; 64];
        for _ in 0..N {
            let mut deck = Deck::shuffled(&mut rng);
            counts[deck.deal().unwrap().index() as usize] += 1;
        }
        let expected = N as f64 / 52.0;
        for card in CardSet::FULL_DECK {
            let c = counts[card.index() as usize] as f64;
            assert!(
                (c - expected).abs() < 0.3 * expected,
                "{card}: {c} vs {expected}"
            );
        }
    }
}
