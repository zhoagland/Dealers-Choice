//! The `Game` trait: the contract between a game's rules and everything else.
//!
//! **Read `docs/api/game-driver.md` first.** It explains the model (one actor per step, chance
//! as a player, observations as information sets) and the invariants every driver must keep.
//! This file is the Rust form of that document.
//!
//! Implements PRD FR-GD-1…6, FR-EC-5, FR-EC-6.

use std::fmt;
use std::hash::Hash;
use std::ops::{Add, AddAssign, Neg, RangeInclusive, Sub, SubAssign};

use rand::Rng;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use smallvec::SmallVec;

use crate::card::{Card, CardSet};
use crate::rng::DetRng;

/// Bumped on any breaking change to this trait. Stored in every hand record.
pub const API_VERSION: u32 = 1;

// ---------------------------------------------------------------------------------------------
// Small value types
// ---------------------------------------------------------------------------------------------

/// A seat at the table, `0..num_seats`. Seat numbers are fixed for a hand; who is the dealer
/// is described separately by [`HandSetup::button`].
///
/// A *newtype* (a struct wrapping one value) instead of a bare `u8`, so the compiler stops you
/// from passing a chip count where a seat was expected.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Seat(pub u8);

impl Seat {
    /// For indexing per-seat arrays: `stacks[seat.index()]`.
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    /// The next seat clockwise (to the left), wrapping around.
    pub const fn next(self, num_seats: u8) -> Seat {
        Seat((self.0 + 1) % num_seats)
    }
}

impl fmt::Display for Seat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "s{}", self.0)
    }
}

/// An amount of money in **cents**. `i64` because payoffs are negative for losers, and integer
/// because floating point can't represent $0.10 exactly; summing floats would slowly create or
/// destroy chips.
#[derive(
    Copy, Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct Chips(pub i64);

impl Chips {
    pub const ZERO: Chips = Chips(0);

    /// `Chips::dollars(1.5)` = 150 cents. For configs and tests; rounds to the nearest cent.
    pub fn dollars(d: f64) -> Chips {
        Chips((d * 100.0).round() as i64)
    }

    pub const fn cents(self) -> i64 {
        self.0
    }

    pub fn min(self, other: Chips) -> Chips {
        Chips(self.0.min(other.0))
    }

    pub fn max(self, other: Chips) -> Chips {
        Chips(self.0.max(other.0))
    }
}

// Operator overloading: implementing `Add` is what makes `a + b` compile for `Chips`.
impl Add for Chips {
    type Output = Chips;
    fn add(self, rhs: Chips) -> Chips {
        Chips(self.0 + rhs.0)
    }
}

impl Sub for Chips {
    type Output = Chips;
    fn sub(self, rhs: Chips) -> Chips {
        Chips(self.0 - rhs.0)
    }
}

impl AddAssign for Chips {
    fn add_assign(&mut self, rhs: Chips) {
        self.0 += rhs.0;
    }
}

impl SubAssign for Chips {
    fn sub_assign(&mut self, rhs: Chips) {
        self.0 -= rhs.0;
    }
}

impl Neg for Chips {
    type Output = Chips;
    fn neg(self) -> Chips {
        Chips(-self.0)
    }
}

/// Lets you write `payoffs.iter().copied().sum::<Chips>()`.
impl std::iter::Sum for Chips {
    fn sum<I: Iterator<Item = Chips>>(iter: I) -> Chips {
        Chips(iter.map(|c| c.0).sum())
    }
}

/// Prints as `$1.50` / `-$0.50`.
impl fmt::Display for Chips {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let abs = self.0.unsigned_abs();
        write!(f, "{sign}${}.{:02}", abs / 100, abs % 100)
    }
}

// ---------------------------------------------------------------------------------------------
// Hand and game description
// ---------------------------------------------------------------------------------------------

/// Who acts next. See the API doc §1: there is never more than one actor at a time.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ToAct {
    Seat(Seat),
    Chance,
    Terminal,
}

/// Where a player's betting money comes from (PRD FR-SS-2).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StakeModel {
    /// Bet straight from the bankroll.
    Bankroll,
    /// Move this much to the table at game start; only it can be bet. Leftovers return at game end.
    TableStake(Chips),
}

/// How long one "game" (one dealer's choice) lasts. PRD §4.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GameLength {
    /// The default: one hand.
    SingleHand,
    /// One trip of the button, starting with the chooser dealing.
    Orbit,
}

/// Static facts about a game. Returned by [`Game::meta`].
#[derive(Clone, Debug)]
pub struct GameMeta {
    /// Stable identifier used in configs, records and the registry: `"kuhn"`, `"holdem_nl"`.
    pub id: &'static str,
    /// Bump on any rules change. Saved blueprints and records are keyed by it.
    pub version: u32,
    pub display_name: &'static str,
    pub seats: RangeInclusive<u8>,
    pub stake_model: StakeModel,
    pub length: GameLength,
    /// Where the rules are written down, e.g. `"Poker Bible §Follow the Queen"`.
    pub rules_ref: &'static str,
}

/// Everything needed to start a hand. Produced by the session, consumed by [`Game::new_hand`].
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HandSetup {
    pub num_seats: u8,
    /// The dealer for this hand.
    pub button: Seat,
    /// Who picked the game. Pays the dealer ante when `first_hand_of_game`.
    pub chooser: Seat,
    pub first_hand_of_game: bool,
    /// Chips each seat can bet this hand.
    pub stacks: Vec<Chips>,
}

impl HandSetup {
    /// A simple setup for tests and sims: button and chooser at seat 0, equal stacks.
    pub fn simple(num_seats: u8, stack: Chips) -> HandSetup {
        HandSetup {
            num_seats,
            button: Seat(0),
            chooser: Seat(0),
            first_hand_of_game: true,
            stacks: vec![stack; num_seats as usize],
        }
    }

    /// All seats in order, as `Seat` values.
    pub fn seats(&self) -> impl Iterator<Item = Seat> + use<> {
        (0..self.num_seats).map(Seat)
    }
}

/// Game-agnostic meaning of an action. The only semantic hint bots get (API doc §9).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ActionClass {
    Fold,
    /// `owed` = what calling costs; zero means this is a check. Stats need the difference
    /// (a check isn't "voluntarily putting money in"; a call is).
    CheckOrCall {
        owed: Chips,
    },
    BetOrRaise {
        to: Chips,
    },
    /// A hidden commitment such as a hi/lo declaration.
    Declare,
    /// Discard / keep / pass decisions (future draw games).
    CardChoice,
    Chance,
}

/// Why an action was rejected. Shown to humans, so the message should say what *is* allowed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IllegalAction(pub String);

impl fmt::Display for IllegalAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "illegal action: {}", self.0)
    }
}

impl std::error::Error for IllegalAction {}

// ---------------------------------------------------------------------------------------------
// The trait
// ---------------------------------------------------------------------------------------------

/// A card game's rules as a state machine. See `docs/api/game-driver.md` for the full contract.
///
/// The `type …;` lines are **associated types**: each game picks its own concrete `State`,
/// `Action`, etc. Code that works with any game is written as `fn run<G: Game>(game: &G)`
/// and refers to them as `G::State`, `G::Action`. The compiler then generates a specialized
/// copy for each game (monomorphization), so there's no virtual-call overhead in hot loops.
pub trait Game: Send + Sync + Sized + 'static {
    /// Everything about the hand, including hidden cards. Never given to a bot.
    type State: Clone + fmt::Debug + Send + Sync;
    /// Player actions *and* chance outcomes (deals). Small and `Copy`.
    type Action: Copy + fmt::Debug + Eq + Hash + Ord + Serialize + DeserializeOwned + Send + Sync;
    /// One seat's view of the hand. Two states with equal observations for a seat must be
    /// indistinguishable to that seat; equality defines an information set.
    /// `DeserializeOwned` because saved blueprints are keyed by observations.
    type Observation: Clone + fmt::Debug + Eq + Hash + Serialize + DeserializeOwned + Send + Sync;
    /// Per-game tunables (blinds, structure, bot bet menu).
    type Config: Clone + fmt::Debug + Default + Serialize + DeserializeOwned + Send + Sync;

    fn new(config: Self::Config) -> Self;
    fn meta(&self) -> &GameMeta;

    fn new_hand(&self, setup: &HandSetup) -> Self::State;
    fn to_act(&self, s: &Self::State) -> ToAct;

    /// Seat to act: the abstract action menu bots search over. Chance: every outcome.
    /// Clears `out` first. The `&mut Vec` out-parameter lets hot loops reuse one buffer
    /// instead of allocating a new `Vec` on every call.
    fn legal_actions(&self, s: &Self::State, out: &mut Vec<Self::Action>);

    /// Full legality check, including off-menu bets a human might type.
    fn validate(&self, s: &Self::State, a: Self::Action) -> Result<(), IllegalAction>;

    /// Apply an action that passed `validate`. May panic on illegal input.
    fn apply(&self, s: &mut Self::State, a: Self::Action);

    /// Only valid at Chance. Clears `out`, then fills it; probabilities sum to 1.
    fn chance_outcomes(&self, s: &Self::State, out: &mut Vec<(Self::Action, f64)>);

    /// Draw one chance outcome. The default enumerates `chance_outcomes`; drivers may override
    /// it with something faster (e.g. picking a random remaining card directly), as long as the
    /// distribution is the same.
    fn sample_chance(&self, s: &Self::State, rng: &mut DetRng) -> Self::Action {
        let mut outcomes = Vec::new();
        self.chance_outcomes(s, &mut outcomes);
        sample_weighted(&outcomes, rng)
    }

    /// Only valid at Terminal. Net chips per seat for this hand; sums to zero.
    fn payoffs(&self, s: &Self::State) -> Vec<Chips>;

    fn observe(&self, s: &Self::State, seat: Seat) -> Self::Observation;

    /// The seat this observation belongs to.
    fn observer(&self, obs: &Self::Observation) -> Seat;

    /// A full state consistent with `obs`: everything the observer can't see is sampled.
    /// Must satisfy `observe(&determinize(obs, …), observer) == *obs`.
    fn determinize(
        &self,
        obs: &Self::Observation,
        sampler: &mut dyn HiddenSampler,
        rng: &mut DetRng,
    ) -> Self::State;

    fn classify(&self, s: &Self::State, a: Self::Action) -> ActionClass;

    /// Cards only `seat` holds (hole cards, stud downcards). Used by player analytics for range
    /// charts. Default: none (games where this doesn't apply).
    fn private_cards(&self, _s: &Self::State, _seat: Seat) -> CardVec {
        CardVec::new()
    }

    /// Value unit for EV display and bot temperature: the big blind or the ante.
    fn eval_unit(&self) -> Chips;

    // ----- Text interface (optional; used by the CLI) ----------------------------------------
    //
    // These live on the trait (with defaults) rather than in the CLI because the registry only
    // hands out `G: Game`; a separate rendering trait would need a second per-game registry.

    /// Human-readable view of what a seat sees. Default: `Debug`.
    fn describe_observation(&self, obs: &Self::Observation) -> String {
        format!("{obs:#?}")
    }

    /// Human-readable action. Default: `Debug`.
    fn describe_action(&self, a: Self::Action) -> String {
        format!("{a:?}")
    }

    /// Parse free-form human input (e.g. `"r 6.50"`) into an action; `legal` is the bot menu
    /// for context. Return `None` if the text isn't understood. Default: understands nothing,
    /// so humans pick from the numbered menu.
    fn parse_action(&self, _input: &str, _legal: &[Self::Action]) -> Option<Self::Action> {
        None
    }
}

/// Pick from `(item, probability)` pairs. Probabilities need not be normalized.
///
/// Panics if `outcomes` is empty or all weights are zero. That would be a driver bug
/// (conformance check C1).
pub fn sample_weighted<A: Copy>(outcomes: &[(A, f64)], rng: &mut DetRng) -> A {
    let total: f64 = outcomes.iter().map(|&(_, p)| p).sum();
    assert!(
        total > 0.0,
        "sample_weighted: no outcome has positive probability"
    );
    let mut target = rng.random::<f64>() * total;
    for &(a, p) in outcomes {
        if target < p {
            return a;
        }
        target -= p;
    }
    // Floating-point rounding can leave `target` a hair above the last weight.
    // Return the last outcome that has nonzero weight.
    outcomes.iter().rev().find(|&&(_, p)| p > 0.0).unwrap().0
}

// ---------------------------------------------------------------------------------------------
// Hidden-card sampling (API doc §5)
// ---------------------------------------------------------------------------------------------

/// Up to 7 cards without heap allocation: enough for any private hand in the Bible.
pub type CardVec = SmallVec<[Card; 7]>;

/// Supplies the hidden private cards of opponents during [`Game::determinize`].
///
/// `&mut dyn HiddenSampler` in the trait is a *trait object*: one function works with any
/// sampler type at runtime (uniform, range-weighted…). Here a virtual call is fine; it happens
/// once per determinization, not per card.
pub trait HiddenSampler {
    /// Return `n` distinct cards from `available` for `seat`, or `None` to request a retry
    /// (e.g. the range has no combo left given the cards already assigned).
    fn sample_private(
        &mut self,
        seat: Seat,
        n: usize,
        available: CardSet,
        rng: &mut DetRng,
    ) -> Option<CardVec>;
}

/// Every unseen card equally likely.
#[derive(Copy, Clone, Debug, Default)]
pub struct UniformSampler;

impl HiddenSampler for UniformSampler {
    fn sample_private(
        &mut self,
        _seat: Seat,
        n: usize,
        available: CardSet,
        rng: &mut DetRng,
    ) -> Option<CardVec> {
        if available.len() < n {
            return None;
        }
        Some(sample_cards(available, n, rng))
    }
}

/// `n` distinct cards drawn uniformly from `from`. Panics if `from` has fewer than `n` cards.
pub fn sample_cards(from: CardSet, n: usize, rng: &mut DetRng) -> CardVec {
    let mut pool: SmallVec<[Card; 52]> = from.iter().collect();
    assert!(
        pool.len() >= n,
        "sample_cards: need {n} cards, only {} available",
        pool.len()
    );
    // Partial Fisher–Yates: only the first n positions need shuffling.
    for i in 0..n {
        let j = rng.random_range(i..pool.len());
        pool.swap(i, j);
    }
    pool[..n].iter().copied().collect()
}

/// Ask `sampler` for each `(seat, n)` in order, removing assigned cards as it goes, so no card is
/// handed out twice (card removal). Retries the whole assignment up to `max_tries` times when the
/// sampler returns `None`, then falls back to uniform so callers always get an answer.
pub fn sample_hidden_hands(
    sampler: &mut dyn HiddenSampler,
    requests: &[(Seat, usize)],
    available: CardSet,
    rng: &mut DetRng,
) -> Vec<CardVec> {
    const MAX_TRIES: usize = 1_000;
    'attempt: for _ in 0..MAX_TRIES {
        let mut remaining = available;
        let mut hands = Vec::with_capacity(requests.len());
        for &(seat, n) in requests {
            match sampler.sample_private(seat, n, remaining, rng) {
                Some(cards) => {
                    for &c in &cards {
                        debug_assert!(remaining.contains(c), "sampler returned unavailable {c}");
                        remaining.remove(c);
                    }
                    hands.push(cards);
                }
                None => continue 'attempt,
            }
        }
        return hands;
    }
    // Ranges incompatible with the known cards: ignore them rather than loop forever.
    sample_hidden_hands(&mut UniformSampler, requests, available, rng)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::rng_from_seed;

    #[test]
    fn chips_display() {
        assert_eq!(Chips(150).to_string(), "$1.50");
        assert_eq!(Chips(-5).to_string(), "-$0.05");
        assert_eq!(Chips::dollars(20.0), Chips(2000));
    }

    #[test]
    fn chips_arithmetic() {
        let mut c = Chips(100) + Chips(50) - Chips(25);
        c += Chips(5);
        c -= Chips(30);
        assert_eq!(c, Chips(100));
        assert_eq!(-c, Chips(-100));
        assert_eq!(
            [Chips(1), Chips(2)].iter().copied().sum::<Chips>(),
            Chips(3)
        );
    }

    #[test]
    fn seat_next_wraps() {
        assert_eq!(Seat(2).next(3), Seat(0));
        assert_eq!(Seat(0).next(3), Seat(1));
    }

    #[test]
    fn sample_weighted_matches_probabilities() {
        let mut rng = rng_from_seed(5);
        let outcomes = [('a', 0.2), ('b', 0.0), ('c', 0.8)];
        let n = 20_000;
        let mut counts = [0usize; 3];
        for _ in 0..n {
            match sample_weighted(&outcomes, &mut rng) {
                'a' => counts[0] += 1,
                'b' => counts[1] += 1,
                _ => counts[2] += 1,
            }
        }
        assert_eq!(counts[1], 0, "zero-probability outcome must never be drawn");
        // 0.2 * 20000 = 4000, std dev ≈ 57; ±300 is >5σ.
        assert!((counts[0] as i64 - 4000).abs() < 300, "{counts:?}");
    }

    #[test]
    fn sample_cards_distinct_and_from_pool() {
        let mut rng = rng_from_seed(1);
        for _ in 0..100 {
            let cards = sample_cards(CardSet::FULL_DECK, 7, &mut rng);
            let set: CardSet = cards.iter().copied().collect();
            assert_eq!(set.len(), 7);
        }
    }

    #[test]
    fn hidden_hands_never_share_cards() {
        let mut rng = rng_from_seed(2);
        let reqs = [(Seat(1), 2), (Seat(2), 2), (Seat(3), 3)];
        let available: CardSet = CardSet::FULL_DECK.iter().take(10).collect();
        for _ in 0..100 {
            let hands = sample_hidden_hands(&mut UniformSampler, &reqs, available, &mut rng);
            let all: CardSet = hands.iter().flatten().copied().collect();
            assert_eq!(all.len(), 7);
            assert!(all.difference(available).is_empty());
        }
    }

    /// A sampler that always refuses, to prove the uniform fallback prevents an infinite loop.
    struct Refuse;
    impl HiddenSampler for Refuse {
        fn sample_private(
            &mut self,
            _: Seat,
            _: usize,
            _: CardSet,
            _: &mut DetRng,
        ) -> Option<CardVec> {
            None
        }
    }

    #[test]
    fn impossible_sampler_falls_back_to_uniform() {
        let mut rng = rng_from_seed(3);
        let hands = sample_hidden_hands(&mut Refuse, &[(Seat(1), 2)], CardSet::FULL_DECK, &mut rng);
        assert_eq!(hands[0].len(), 2);
    }
}
