//! Driving the hands of one game, carrying [`Game::Carry`] from each hand to the next (API v2,
//! `docs/tech/game-catalog.md` §2). The CLI, the sim runner and the conformance tests all use
//! [`GameCarry`], so "how an `UntilDone` game is driven" is defined once.
//!
//! For `SingleHand`/`Orbit` games the carry is `()` and every method is a no-op wrapper, so
//! callers can use it for every game without branching.
//!
//! A game ends when, after a hand:
//! - [`Game::game_over`] is true (the normal end; any carried pot has been awarded), or
//! - the `max_hands` cap is reached, or fewer than two seats can play. Any chips still carried
//!   are then paid out by [`Game::settle_carry`] ([`GameCarry::close`]).

use crate::game::{Chips, Game, GameLength, HandSetup};

/// What one finished hand did to the game.
#[derive(Clone, Debug)]
pub struct HandCarry<C> {
    /// The carry the hand started from: with the setup and the action log, replays the hand.
    pub carry_in: C,
    /// True when the game ended normally after this hand.
    pub game_over: bool,
}

/// The carry between the hands of one game.
#[derive(Clone, Debug)]
pub struct GameCarry<G: Game> {
    carry: G::Carry,
    hands: u32,
    until_done: bool,
    over: bool,
}

impl<G: Game> GameCarry<G> {
    pub fn new(game: &G) -> Self {
        GameCarry {
            carry: G::Carry::default(),
            hands: 0,
            until_done: matches!(game.meta().length, GameLength::UntilDone { .. }),
            over: false,
        }
    }

    pub fn carry(&self) -> &G::Carry {
        &self.carry
    }

    /// Hands finished so far.
    pub fn hands(&self) -> u32 {
        self.hands
    }

    /// True once `game_over` has been seen. Other game lengths never set it (the session counts
    /// their hands).
    pub fn is_over(&self) -> bool {
        self.over
    }

    /// Seats the game has removed (`Game::seats_out`), for `Table::next_hand_excluding`.
    pub fn seats_out(&self, game: &G) -> u16 {
        if self.until_done {
            game.seats_out(&self.carry)
        } else {
            0
        }
    }

    /// Start the next hand from the current carry.
    pub fn new_hand(&self, game: &G, setup: &HandSetup) -> G::State {
        debug_assert!(!self.over, "game already over");
        game.new_hand_with(setup, &self.carry)
    }

    /// Record a finished hand. `end` must be terminal and `payoffs` its payoffs. Panics if the
    /// payoffs don't account for the change in carried chips (a driver bug).
    pub fn finish_hand(
        &mut self,
        game: &G,
        end: &G::State,
        payoffs: &[Chips],
    ) -> HandCarry<G::Carry> {
        let carry_out = game.carry_out(end);
        let sum: Chips = payoffs.iter().copied().sum();
        let expect = game.carried_chips(&self.carry) - game.carried_chips(&carry_out);
        assert_eq!(
            sum,
            expect,
            "{}: hand payoffs sum to {sum}, but carried chips went {} -> {}",
            game.meta().id,
            game.carried_chips(&self.carry),
            game.carried_chips(&carry_out)
        );
        let carry_in = std::mem::replace(&mut self.carry, carry_out);
        self.hands += 1;
        if self.until_done && game.game_over(&self.carry) {
            self.over = true;
            assert_eq!(
                game.carried_chips(&self.carry),
                Chips::ZERO,
                "{}: game_over with chips still carried",
                game.meta().id
            );
        }
        HandCarry {
            carry_in,
            game_over: self.over,
        }
    }

    /// The game stopped. If chips are still carried (cap reached, or too few seats left), the
    /// payoffs that award them: `eligible` = game seats still in the game; if that is empty,
    /// `fallback` (e.g. every seat that started). `None` when nothing is carried.
    pub fn close(
        &self,
        game: &G,
        num_seats: u8,
        eligible: u16,
        fallback: u16,
    ) -> Option<Vec<Chips>> {
        let amount = game.carried_chips(&self.carry);
        if amount == Chips::ZERO {
            return None;
        }
        let all = (1u16 << num_seats) - 1;
        let eligible = if eligible & all != 0 {
            eligible & all
        } else {
            fallback & all
        };
        assert!(eligible != 0, "no seat to award the carried pot to");
        let pay = game.settle_carry(&self.carry, eligible, num_seats);
        assert_eq!(
            pay.len(),
            num_seats as usize,
            "settle_carry: payoffs length"
        );
        assert_eq!(
            pay.iter().copied().sum::<Chips>(),
            amount,
            "{}: settle_carry must pay out exactly the carried chips",
            game.meta().id
        );
        for (i, &p) in pay.iter().enumerate() {
            assert!(
                p >= Chips::ZERO && (p == Chips::ZERO || eligible & (1 << i) != 0),
                "{}: settle_carry paid {p} to s{i}, outside the eligible seats",
                game.meta().id
            );
        }
        Some(pay)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::{Card, Rank, Suit};
    use crate::game::{Seat, ToAct};
    use crate::session::{SeatInfo, SeatKind, Table};
    use crate::testing::guts_lite::{Act, GutsLite, MAX_HANDS};

    fn bot(name: &str) -> SeatInfo {
        SeatInfo {
            name: name.into(),
            kind: SeatKind::Bot,
            bankroll: None,
            stack: Chips::ZERO,
            sitting_out: false,
        }
    }

    /// Play one hand: deal fresh cards, then each seat's choice from `choose(seat)`.
    fn play<G: Game<Action = Act>>(
        game: &G,
        gc: &GameCarry<G>,
        setup: &HandSetup,
        choose: impl Fn(Seat) -> Act,
    ) -> G::State {
        let mut s = gc.new_hand(game, setup);
        let mut next = 0u8;
        loop {
            let a = match game.to_act(&s) {
                ToAct::Terminal => return s,
                ToAct::Chance => {
                    next += 1;
                    Act::Deal(Card::new(Rank::ALL[next as usize], Suit::Spades))
                }
                ToAct::Seat(p) => choose(p),
            };
            game.apply(&mut s, a);
        }
    }

    #[test]
    fn cap_splits_the_carried_pot_evenly_among_seats_still_in() {
        let game = GutsLite::<false>::new(());
        let mut table = Table::new(vec![bot("a"), bot("b"), bot("c")], Chips(1_000));
        table.begin_game(game.meta());
        let mut gc = GameCarry::new(&game);
        let mut hands = 0;
        while let Some((setup, map)) = table.next_hand_excluding(gc.seats_out(&game)) {
            // Nobody ever goes In: the 3 antes ride until the cap.
            let end = play(&game, &gc, &setup, |_| Act::Out);
            let pay = game.payoffs(&end);
            let done = gc.finish_hand(&game, &end, &pay);
            assert!(!done.game_over);
            table.settle_hand(&map, &pay);
            hands += 1;
        }
        assert_eq!(hands, MAX_HANDS);
        let eligible = table.game_eligible(gc.seats_out(&game));
        let pay = gc.close(&game, 3, eligible, 0b111).unwrap();
        assert_eq!(
            pay,
            vec![Chips(1), Chips(1), Chips(1)],
            "D-G0-1: even split"
        );
        table.settle_carry(table.game_seats().unwrap().to_vec().as_slice(), &pay);
        assert!(
            table.seats.iter().all(|s| s.stack == Chips(1_000)),
            "antes returned"
        );
        table.end_game();
    }

    #[test]
    fn out_is_out_deals_seats_out_and_the_last_seat_in_takes_the_pot() {
        let game = GutsLite::<true>::new(());
        let mut table = Table::new(vec![bot("a"), bot("b"), bot("c")], Chips(1_000));
        table.begin_game(game.meta());
        let mut gc = GameCarry::new(&game);

        // Hand 1: s0 and s1 go In (s1 has the higher card), s2 goes Out and is out of the game.
        let (setup, map) = table.next_hand_excluding(gc.seats_out(&game)).unwrap();
        let end = play(
            &game,
            &gc,
            &setup,
            |p| if p.0 < 2 { Act::In } else { Act::Out },
        );
        let pay = game.payoffs(&end);
        // Pot 3 (antes) to s1; s0 matches 3 into the next pot.
        assert_eq!(pay, vec![Chips(-4), Chips(2), Chips(-1)]);
        assert!(!gc.finish_hand(&game, &end, &pay).game_over);
        table.settle_hand(&map, &pay);

        // Hand 2: s2 is dealt out; s0 alone goes In and wins the game.
        let (setup, map) = table.next_hand_excluding(gc.seats_out(&game)).unwrap();
        assert_eq!(setup.dealt_out, 0b100);
        let end = play(&game, &gc, &setup, |p| {
            if p.0 == 0 { Act::In } else { Act::Out }
        });
        let pay = game.payoffs(&end);
        assert_eq!(pay, vec![Chips(3), Chips(0), Chips(0)]);
        assert!(gc.finish_hand(&game, &end, &pay).game_over);
        table.settle_hand(&map, &pay);
        table.finish_game_early();
        assert!(table.next_hand().is_none());
        assert_eq!(
            gc.close(&game, 3, 0b011, 0b111),
            None,
            "nothing left carried"
        );
        let stacks: Vec<i64> = table.seats.iter().map(|s| s.stack.0).collect();
        assert_eq!(stacks, vec![999, 1_002, 999]);
    }
}
