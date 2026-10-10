//! Betting rules for every game: who may act, what they may bet, when a street closes.
//! PRD FR-EC-3/4, NFR-5; `docs/tech/engine.md` §4.
//!
//! Drivers never do chip arithmetic themselves (conformance C9). They hold one [`Betting`] for
//! the hand, call [`Betting::start_street`] at each betting round, and pass player actions to
//! [`Betting::apply`]. That puts the rules people most often get wrong (minimum raises, short
//! all-ins that don't reopen the betting, raise caps, pot-limit maximums) in one tested place.
//!
//! Seat sets are `u16` bitmasks (bit i = seat i): at most 16 seats, cheap to copy and compare.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::game::{Chips, IllegalAction, Seat};

/// How much may be bet (PRD FR-EC-4).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Structure {
    /// Any amount from the minimum raise up to all-in.
    NoLimit,
    /// Like no-limit, but at most the size of the pot (after calling).
    PotLimit,
    /// Every bet/raise is exactly `small` (streets before `big_from_street`) or `big`.
    FixedLimit {
        small: Chips,
        big: Chips,
        big_from_street: u8,
        max_raises: u8,
    },
    /// Each bet/raise increases the bet by `min_inc..=max_inc` in multiples of `step`.
    /// FTQ: 50¢..=$2 in 50¢ steps, 4 bets/raises per street.
    SpreadLimit {
        min_inc: Chips,
        max_inc: Chips,
        step: Chips,
        max_raises: u8,
    },
}

/// A betting decision. Games wrap this in their own `Action` enum.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum BetAction {
    Fold,
    /// Check if nothing is owed; otherwise call (or call all-in for less).
    CheckCall,
    /// Bet or raise so this seat's total for the street becomes the amount.
    RaiseTo(Chips),
}

/// Abstract bet sizes for bot menus (FR-BT-7). Converted to real amounts by [`Betting::menu`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BetSize {
    MinRaise,
    /// The largest legal raise (the cap in limit/spread games; all-in in no-limit).
    MaxRaise,
    /// Raise by `num/den` of the pot after calling: 1/1 is a pot-sized raise.
    PotFraction(u32, u32),
    AllIn,
}

/// What a seat may do right now.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct BetOptions {
    /// Amount needed to call (zero = check). Already capped at the seat's stack.
    pub call: Chips,
    /// Fold is offered only when there's something to call.
    pub can_fold: bool,
    /// Inclusive raise-to range, or `None` if raising isn't allowed.
    pub raise: Option<(Chips, Chips)>,
    /// Raise amounts must be `min + k·step`, except an all-in which is always allowed.
    pub step: Chips,
}

/// All betting state for one hand.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Betting {
    structure: Structure,
    num_seats: u8,
    /// Minimum opening bet and minimum raise increment (the big blind / small bet).
    min_bet: Chips,
    /// Chips each seat still has behind.
    stacks: Vec<Chips>,
    /// Chips each seat has put in this hand, antes and blinds included.
    contributed: Vec<Chips>,
    /// The part of `contributed` that was dead money (antes, dealer ante). Dead money goes to
    /// the main pot for everyone; it must not create side-pot layers of its own.
    dead: Vec<Chips>,
    /// Chips each seat has put in on the current street (live money only).
    street: Vec<Chips>,
    folded: u16,
    all_in: u16,
    street_index: u8,
    /// Highest street total so far.
    current_bet: Chips,
    /// Size of the last full bet/raise; the next raise must be at least this big.
    last_full_raise: Chips,
    /// Full bets/raises this street (for caps).
    raises: u8,
    /// Seats that must still act before the street closes.
    needs: u16,
    /// Seats that acted since the last full raise. They can't re-raise a short all-in.
    acted: u16,
    /// Where to start looking for the next actor.
    cursor: Seat,
}

const fn bit(seat: Seat) -> u16 {
    1 << seat.0
}

impl Betting {
    /// A fresh hand. `min_bet` is the big blind (NL/PL) or the small bet (limit games).
    pub fn new(structure: Structure, stacks: &[Chips], min_bet: Chips) -> Betting {
        let n = stacks.len();
        assert!((2..=16).contains(&n), "betting supports 2..=16 seats");
        Betting {
            structure,
            num_seats: n as u8,
            min_bet,
            stacks: stacks.to_vec(),
            contributed: vec![Chips::ZERO; n],
            dead: vec![Chips::ZERO; n],
            street: vec![Chips::ZERO; n],
            folded: 0,
            all_in: stacks
                .iter()
                .enumerate()
                .filter(|(_, s)| s.0 <= 0)
                .fold(0, |m, (i, _)| m | 1 << i),
            street_index: 0,
            current_bet: Chips::ZERO,
            last_full_raise: min_bet,
            raises: 0,
            needs: 0,
            acted: 0,
            cursor: Seat(0),
        }
    }

    // ----- forced bets -------------------------------------------------------------------

    /// Dead money straight into the pot (antes, dealer ante): doesn't count toward calling.
    /// Takes what the seat has if it's short.
    pub fn post_dead(&mut self, seat: Seat, amount: Chips) {
        let paid = self.take(seat, amount);
        self.contributed[seat.index()] += paid;
        self.dead[seat.index()] += paid;
    }

    /// A live blind: counts as this seat's bet on the current street. Call after `start_street`.
    pub fn post_blind(&mut self, seat: Seat, amount: Chips) {
        let paid = self.take(seat, amount);
        self.contributed[seat.index()] += paid;
        self.street[seat.index()] += paid;
        self.current_bet = self.current_bet.max(self.street[seat.index()]);
        self.normalize_needs();
    }

    /// Remove up to `amount` from a stack; marks all-in if it empties.
    fn take(&mut self, seat: Seat, amount: Chips) -> Chips {
        let i = seat.index();
        let paid = amount.min(self.stacks[i]);
        self.stacks[i] -= paid;
        if self.stacks[i] == Chips::ZERO {
            self.all_in |= bit(seat);
        }
        paid
    }

    // ----- streets -----------------------------------------------------------------------

    /// Begin a betting round; `first` acts first (or the next seat after it that can act).
    pub fn start_street(&mut self, street_index: u8, first: Seat) {
        self.street_index = street_index;
        self.street.iter_mut().for_each(|c| *c = Chips::ZERO);
        self.current_bet = Chips::ZERO;
        self.last_full_raise = self.min_bet;
        self.raises = 0;
        self.acted = 0;
        self.needs = self.can_act_mask();
        self.normalize_needs();
        self.cursor = first;
    }

    /// Rules that remove pointless decisions, applied after every change:
    /// - all-in and folded seats never act;
    /// - once everyone else has folded, nobody acts;
    /// - if only one seat can still put in chips, it acts only if it owes something (it must
    ///   decide whether to call an all-in), never just to check against nobody.
    fn normalize_needs(&mut self) {
        let can_act = self.can_act_mask();
        self.needs &= can_act;
        if self.live_count() == 1 {
            self.needs = 0;
        } else if can_act.count_ones() == 1 {
            let s = Seat(can_act.trailing_zeros() as u8);
            let owes = self.street[s.index()] < self.current_bet;
            self.needs = if owes { can_act } else { 0 };
        }
    }

    /// Seats neither folded nor all-in.
    fn can_act_mask(&self) -> u16 {
        let all = (1u16 << self.num_seats) - 1;
        all & !self.folded & !self.all_in
    }

    /// The seat to act, or `None` when the street is closed.
    pub fn to_act(&self) -> Option<Seat> {
        (0..self.num_seats)
            .map(|k| Seat((self.cursor.0 + k) % self.num_seats))
            .find(|&s| self.needs & bit(s) != 0)
    }

    // ----- decisions ---------------------------------------------------------------------

    pub fn options(&self, seat: Seat) -> BetOptions {
        let i = seat.index();
        let owed = self.current_bet - self.street[i];
        let call = owed.min(self.stacks[i]);
        let max_total = self.street[i] + self.stacks[i]; // street total if all-in
        let others_can_act = (self.can_act_mask() & !bit(seat)) != 0;
        let may_raise =
            self.acted & bit(seat) == 0 && max_total > self.current_bet && others_can_act;

        let (raise, step) = match self.structure {
            Structure::NoLimit | Structure::PotLimit => {
                let min = self.current_bet + self.last_full_raise;
                let cap = if self.structure == Structure::PotLimit {
                    // Pot after calling, then raise by that much (engine.md §4). A minimum
                    // raise is always allowed, even when the pot is smaller than it (e.g. a
                    // short all-in blind), so the cap never drops below `min`.
                    (self.current_bet + self.pot() + owed).max(min)
                } else {
                    max_total
                };
                (Some((min.min(max_total), cap.min(max_total))), Chips(1))
            }
            Structure::FixedLimit {
                small,
                big,
                big_from_street,
                max_raises,
            } => {
                let size = if self.street_index >= big_from_street {
                    big
                } else {
                    small
                };
                let to = (self.current_bet + size).min(max_total);
                let allowed = self.raises < max_raises;
                (allowed.then_some((to, to)), size)
            }
            Structure::SpreadLimit {
                min_inc,
                max_inc,
                step,
                max_raises,
            } => {
                let lo = (self.current_bet + min_inc).min(max_total);
                let hi = (self.current_bet + max_inc).min(max_total);
                let allowed = self.raises < max_raises;
                (allowed.then_some((lo, hi)), step)
            }
        };
        BetOptions {
            call,
            can_fold: owed > Chips::ZERO,
            raise: if may_raise { raise } else { None },
            step,
        }
    }

    pub fn validate(&self, seat: Seat, a: BetAction) -> Result<(), IllegalAction> {
        if self.to_act() != Some(seat) {
            return Err(IllegalAction(format!("it isn't {seat}'s turn to bet")));
        }
        let o = self.options(seat);
        match a {
            BetAction::CheckCall => Ok(()),
            BetAction::Fold if o.can_fold => Ok(()),
            BetAction::Fold => Err(IllegalAction(
                "nothing to call; check instead of folding".into(),
            )),
            BetAction::RaiseTo(x) => {
                let Some((lo, hi)) = o.raise else {
                    return Err(IllegalAction("raising isn't allowed now".into()));
                };
                let all_in = self.street[seat.index()] + self.stacks[seat.index()];
                let on_step = (x - lo).0 % o.step.0 == 0;
                if x == all_in && x > self.current_bet && x <= hi {
                    Ok(())
                } else if x < lo || x > hi {
                    Err(IllegalAction(format!(
                        "raise to must be between {lo} and {hi}"
                    )))
                } else if !on_step {
                    Err(IllegalAction(format!(
                        "raise to {lo} plus a multiple of {}",
                        o.step
                    )))
                } else {
                    Ok(())
                }
            }
        }
    }

    /// Apply a validated action. Panics (debug) on invalid input.
    pub fn apply(&mut self, seat: Seat, a: BetAction) {
        debug_assert!(
            self.validate(seat, a).is_ok(),
            "{seat} {a:?}: {:?}",
            self.validate(seat, a)
        );
        let i = seat.index();
        self.needs &= !bit(seat);
        match a {
            BetAction::Fold => self.folded |= bit(seat),
            BetAction::CheckCall => {
                let owed = self.current_bet - self.street[i];
                let paid = self.take(seat, owed);
                self.street[i] += paid;
                self.contributed[i] += paid;
                self.acted |= bit(seat);
            }
            BetAction::RaiseTo(to) => {
                let paid = self.take(seat, to - self.street[i]);
                self.street[i] += paid;
                self.contributed[i] += paid;
                let size = to - self.current_bet;
                self.current_bet = to;
                if size >= self.last_full_raise {
                    // Full raise: everyone else must act again and may re-raise.
                    self.last_full_raise = size;
                    self.raises += 1;
                    self.acted = bit(seat);
                    self.needs = self.can_act_mask() & !bit(seat);
                } else {
                    // Short all-in: others must respond, but those who already acted
                    // may only call or fold (betting is not reopened for them).
                    self.acted |= bit(seat);
                    let behind: u16 = (0..self.num_seats)
                        .map(Seat)
                        .filter(|&s| self.street[s.index()] < to)
                        .fold(0, |m, s| m | bit(s));
                    self.needs = self.can_act_mask() & behind & !bit(seat);
                }
            }
        }
        self.normalize_needs();
        self.cursor = seat.next(self.num_seats);
    }

    /// Concrete actions for a bot menu (FR-BT-7): fold (if facing a bet), check/call, and each
    /// distinct legal raise amount from `sizes`. Sizes are snapped to the step and clamped to
    /// the legal range; duplicates collapse.
    pub fn menu(&self, seat: Seat, sizes: &[BetSize]) -> Vec<BetAction> {
        let o = self.options(seat);
        let mut out = Vec::new();
        if o.can_fold {
            out.push(BetAction::Fold);
        }
        out.push(BetAction::CheckCall);
        if let Some((lo, hi)) = o.raise {
            let i = seat.index();
            let owed = self.current_bet - self.street[i];
            let all_in = self.street[i] + self.stacks[i];
            for &size in sizes {
                let raw = match size {
                    BetSize::MinRaise => lo,
                    BetSize::MaxRaise => hi,
                    BetSize::AllIn => all_in.min(hi),
                    BetSize::PotFraction(num, den) => {
                        let pot_after_call = self.pot() + owed;
                        self.current_bet + Chips(pot_after_call.0 * num as i64 / den as i64)
                    }
                };
                let mut to = raw.max(lo).min(hi);
                // Snap down to lo + k·step, unless it's the all-in amount.
                if to != all_in {
                    to = lo + Chips((to - lo).0 / o.step.0 * o.step.0);
                }
                let a = BetAction::RaiseTo(to);
                if !out.contains(&a) {
                    out.push(a);
                }
            }
        }
        out
    }

    // ----- queries -----------------------------------------------------------------------

    /// Everything put in this hand.
    pub fn pot(&self) -> Chips {
        self.contributed.iter().copied().sum()
    }

    /// Live money per seat (bets, calls, blinds): what side pots are layered from.
    pub fn live_contributed(&self) -> Vec<Chips> {
        self.contributed
            .iter()
            .zip(&self.dead)
            .map(|(&c, &d)| c - d)
            .collect()
    }

    /// Total dead money: added to the main pot.
    pub fn dead_total(&self) -> Chips {
        self.dead.iter().copied().sum()
    }

    /// Everything each seat put in, live and dead (what a seat loses if it wins nothing).
    pub fn contributed(&self) -> &[Chips] {
        &self.contributed
    }

    pub fn stacks(&self) -> &[Chips] {
        &self.stacks
    }

    pub fn street_committed(&self, seat: Seat) -> Chips {
        self.street[seat.index()]
    }

    pub fn current_bet(&self) -> Chips {
        self.current_bet
    }

    pub fn is_folded(&self, seat: Seat) -> bool {
        self.folded & bit(seat) != 0
    }

    pub fn is_all_in(&self, seat: Seat) -> bool {
        self.all_in & bit(seat) != 0
    }

    /// Bitmask of seats still in the hand.
    pub fn live_mask(&self) -> u16 {
        ((1u16 << self.num_seats) - 1) & !self.folded
    }

    /// Seats still in the hand (not folded).
    pub fn live_count(&self) -> u32 {
        self.live_mask().count_ones()
    }

    /// True when no further betting can happen this hand: at most one seat can still put in
    /// chips. The driver then deals out the remaining cards ("runs it out").
    pub fn betting_finished(&self) -> bool {
        self.live_count() <= 1 || (self.can_act_mask().count_ones() <= 1 && self.to_act().is_none())
    }

    /// Amount `seat` must put in to call `to` from its current street total, for display.
    pub fn owed(&self, seat: Seat) -> Chips {
        self.current_bet - self.street[seat.index()]
    }
}

impl fmt::Display for BetAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BetAction::Fold => write!(f, "fold"),
            BetAction::CheckCall => write!(f, "check/call"),
            BetAction::RaiseTo(x) => write!(f, "raise to {x}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::rng_from_seed;
    use rand::Rng;

    const NL: Structure = Structure::NoLimit;

    /// 3 seats, $1 BB; seat 1 posts SB 50¢, seat 2 posts BB $1, seat 0 acts first.
    fn preflop(stacks: [i64; 3]) -> Betting {
        let stacks: Vec<Chips> = stacks.iter().map(|&c| Chips(c)).collect();
        let mut b = Betting::new(NL, &stacks, Chips(100));
        b.start_street(0, Seat(0));
        b.post_blind(Seat(1), Chips(50));
        b.post_blind(Seat(2), Chips(100));
        b
    }

    #[test]
    fn min_raise_is_last_full_raise() {
        let mut b = preflop([10_000; 3]);
        assert_eq!(b.options(Seat(0)).raise, Some((Chips(200), Chips(10_000))));
        b.apply(Seat(0), BetAction::RaiseTo(Chips(500))); // raise by 400
        assert_eq!(
            b.options(Seat(1)).raise.unwrap().0,
            Chips(900),
            "next raise ≥ 400 more"
        );
    }

    #[test]
    fn big_blind_has_the_option() {
        let mut b = preflop([10_000; 3]);
        b.apply(Seat(0), BetAction::CheckCall);
        b.apply(Seat(1), BetAction::CheckCall);
        assert_eq!(b.to_act(), Some(Seat(2)), "BB may still raise after limps");
        assert!(b.options(Seat(2)).raise.is_some());
        b.apply(Seat(2), BetAction::CheckCall);
        assert_eq!(b.to_act(), None);
        assert_eq!(b.pot(), Chips(300));
    }

    #[test]
    fn short_all_in_does_not_reopen_betting() {
        // Seat 0 raises to $3; seat 1 calls; seat 2 (BB) shoves for $4 total (raise of only $1,
        // less than the $2 full raise). Seat 0 and seat 1 already acted: they may call or fold only.
        let mut b = preflop([10_000, 10_000, 400]);
        b.apply(Seat(0), BetAction::RaiseTo(Chips(300)));
        b.apply(Seat(1), BetAction::CheckCall);
        b.apply(Seat(2), BetAction::RaiseTo(Chips(400)));
        assert_eq!(b.to_act(), Some(Seat(0)));
        assert_eq!(
            b.options(Seat(0)).raise,
            None,
            "short all-in doesn't reopen"
        );
        assert_eq!(b.options(Seat(0)).call, Chips(100));
        b.apply(Seat(0), BetAction::CheckCall);
        assert_eq!(b.options(Seat(1)).raise, None);
        b.apply(Seat(1), BetAction::CheckCall);
        assert_eq!(b.to_act(), None);
    }

    #[test]
    fn full_all_in_raise_does_reopen() {
        let mut b = preflop([10_000, 10_000, 600]);
        b.apply(Seat(0), BetAction::RaiseTo(Chips(300)));
        b.apply(Seat(1), BetAction::CheckCall);
        b.apply(Seat(2), BetAction::RaiseTo(Chips(600))); // raise of 300 ≥ 200: full
        assert!(b.options(Seat(0)).raise.is_some());
    }

    #[test]
    fn fold_only_when_facing_a_bet() {
        let mut b = Betting::new(NL, &[Chips(1000), Chips(1000)], Chips(100));
        b.start_street(1, Seat(0));
        assert!(b.validate(Seat(0), BetAction::Fold).is_err());
        b.apply(Seat(0), BetAction::RaiseTo(Chips(100)));
        assert!(b.validate(Seat(1), BetAction::Fold).is_ok());
    }

    #[test]
    fn everyone_folds_ends_betting() {
        let mut b = preflop([10_000; 3]);
        b.apply(Seat(0), BetAction::Fold);
        b.apply(Seat(1), BetAction::Fold);
        assert_eq!(b.to_act(), None);
        assert!(b.betting_finished());
        assert_eq!(b.live_count(), 1);
    }

    #[test]
    fn lone_actor_must_answer_an_all_in_blind() {
        // Heads-up: seat 0 (SB) has chips; seat 1's big blind is all-in for 80 > SB 50.
        let mut b = Betting::new(NL, &[Chips(1_000), Chips(80)], Chips(100));
        b.start_street(0, Seat(0));
        b.post_blind(Seat(0), Chips(50));
        b.post_blind(Seat(1), Chips(100));
        assert_eq!(
            b.to_act(),
            Some(Seat(0)),
            "SB must decide whether to call the all-in"
        );
        assert_eq!(b.options(Seat(0)).call, Chips(30));
        assert_eq!(
            b.options(Seat(0)).raise,
            None,
            "nobody left to raise against"
        );
        b.apply(Seat(0), BetAction::CheckCall);
        assert_eq!(b.to_act(), None);
    }

    #[test]
    fn lone_actor_owing_nothing_does_not_act() {
        // Seat 1 is all-in for less than the SB: seat 0 owes nothing, so no decision.
        let mut b = Betting::new(NL, &[Chips(1_000), Chips(30)], Chips(100));
        b.start_street(0, Seat(0));
        b.post_blind(Seat(0), Chips(50));
        b.post_blind(Seat(1), Chips(100));
        assert_eq!(b.to_act(), None);
    }

    #[test]
    fn pot_limit_maximum() {
        // Blinds 50/100 heads-up-ish: pot = 150, seat 0 owes 100. Max raise-to =
        // current bet 100 + pot 150 + owed 100 = 350.
        let stacks = [Chips(10_000), Chips(10_000), Chips(10_000)];
        let mut b = Betting::new(Structure::PotLimit, &stacks, Chips(100));
        b.start_street(0, Seat(0));
        b.post_blind(Seat(1), Chips(50));
        b.post_blind(Seat(2), Chips(100));
        assert_eq!(b.options(Seat(0)).raise, Some((Chips(200), Chips(350))));
    }

    #[test]
    fn spread_limit_ftq() {
        let s = Structure::SpreadLimit {
            min_inc: Chips(50),
            max_inc: Chips(200),
            step: Chips(50),
            max_raises: 4,
        };
        let mut b = Betting::new(s, &[Chips(10_000); 3], Chips(50));
        b.start_street(0, Seat(0));
        assert_eq!(b.options(Seat(0)).raise, Some((Chips(50), Chips(200))));
        assert!(
            b.validate(Seat(0), BetAction::RaiseTo(Chips(75))).is_err(),
            "off step"
        );
        b.apply(Seat(0), BetAction::RaiseTo(Chips(200)));
        b.apply(Seat(1), BetAction::RaiseTo(Chips(400)));
        b.apply(Seat(2), BetAction::RaiseTo(Chips(600)));
        b.apply(Seat(0), BetAction::RaiseTo(Chips(800)));
        // 4 bets/raises made: capped at $8 for the street.
        assert_eq!(b.options(Seat(1)).raise, None);
        assert_eq!(b.options(Seat(1)).call, Chips(400));
    }

    #[test]
    fn fixed_limit_sizes_and_cap() {
        let s = Structure::FixedLimit {
            small: Chips(100),
            big: Chips(200),
            big_from_street: 2,
            max_raises: 4,
        };
        let mut b = Betting::new(s, &[Chips(10_000); 2], Chips(100));
        b.start_street(2, Seat(0));
        assert_eq!(b.options(Seat(0)).raise, Some((Chips(200), Chips(200))));
    }

    #[test]
    fn menu_snaps_and_dedupes() {
        let b = preflop([10_000; 3]);
        let sizes = [
            BetSize::MinRaise,
            BetSize::PotFraction(1, 3),
            BetSize::PotFraction(1, 1),
            BetSize::AllIn,
        ];
        let m = b.menu(Seat(0), &sizes);
        // pot 150, owed 100 → pot after call 250. 1/3 → 100 + 83 = 183 → clamped up to min 200 (dup).
        // Pot → 100 + 250 = 350. All-in 10_000.
        assert_eq!(
            m,
            vec![
                BetAction::Fold,
                BetAction::CheckCall,
                BetAction::RaiseTo(Chips(200)),
                BetAction::RaiseTo(Chips(350)),
                BetAction::RaiseTo(Chips(10_000)),
            ]
        );
    }

    /// Property test: random legal actions on random stacks always conserve chips and always
    /// close the street.
    #[test]
    fn random_streets_conserve_chips_and_terminate() {
        let mut rng = rng_from_seed(9);
        for _ in 0..2_000 {
            let n = rng.random_range(2..=9);
            let stacks: Vec<Chips> = (0..n).map(|_| Chips(rng.random_range(1..=5_000))).collect();
            let total: Chips = stacks.iter().copied().sum();
            let structure = match rng.random_range(0..3) {
                0 => Structure::NoLimit,
                1 => Structure::PotLimit,
                _ => Structure::SpreadLimit {
                    min_inc: Chips(50),
                    max_inc: Chips(200),
                    step: Chips(50),
                    max_raises: 4,
                },
            };
            let mut b = Betting::new(structure, &stacks, Chips(100));
            for street in 0..4u8 {
                b.start_street(street, Seat(0));
                if street == 0 {
                    b.post_blind(Seat(0), Chips(50));
                    b.post_blind(Seat(1), Chips(100));
                }
                let mut steps = 0;
                while let Some(seat) = b.to_act() {
                    steps += 1;
                    assert!(steps < 500, "street didn't terminate");
                    let menu = b.menu(
                        seat,
                        &[
                            BetSize::MinRaise,
                            BetSize::PotFraction(1, 2),
                            BetSize::AllIn,
                        ],
                    );
                    for &a in &menu {
                        if let Err(e) = b.validate(seat, a) {
                            panic!(
                                "menu offered illegal {a:?}: {e}; options {:?}; {b:?}",
                                b.options(seat)
                            );
                        }
                    }
                    let a = menu[rng.random_range(0..menu.len())];
                    b.apply(seat, a);
                    let now: Chips = b.stacks().iter().copied().sum::<Chips>() + b.pot();
                    assert_eq!(now, total, "chips not conserved");
                    assert!(b.stacks().iter().all(|s| s.0 >= 0));
                }
                // Street closed: every live seat that isn't all-in has matched the bet.
                for s in (0..n).map(|i| Seat(i as u8)) {
                    if !b.is_folded(s) && !b.is_all_in(s) && b.live_count() > 1 {
                        assert_eq!(b.street_committed(s), b.current_bet(), "{s} didn't match");
                    }
                }
                if b.live_count() == 1 {
                    break;
                }
            }
        }
    }
}
