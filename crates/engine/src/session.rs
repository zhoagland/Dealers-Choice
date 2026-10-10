//! A Dealer's Choice table: who's sitting, their money, whose turn it is to choose the game.
//! PRD FR-SS-1…5, §4; `docs/tech/engine.md` §6. Pure logic, no I/O: the CLI drives it and
//! `storage` persists its [`SessionEvent`]s.
//!
//! Money model:
//! - **bankroll**: the player's money overall (`None` = a bot's infinite bankroll).
//! - **stack**: what's on the table right now. For `TableStake` games, buying in moves the
//!   stake from bankroll to stack and cashing out moves the remainder back. For `Bankroll`
//!   games the stack *is* the bankroll (bots get `bot_stack`).
//!
//! A game is one hand by default, or one orbit (`GameLength`). After each game, the seat to the
//! chooser's left chooses next.

use serde::{Deserialize, Serialize};

use crate::game::{Chips, GameLength, GameMeta, HandSetup, Seat, StakeModel};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SeatKind {
    Human,
    Bot,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeatInfo {
    pub name: String,
    pub kind: SeatKind,
    /// `None` = infinite (bots).
    pub bankroll: Option<Chips>,
    /// Chips on the table now.
    pub stack: Chips,
    /// Busted and chose not to rebuy: dealt out until the game ends.
    pub sitting_out: bool,
}

/// Things that happened, for the ledger (storage) and for display.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionEvent {
    GameChosen {
        chooser: usize,
        game_id: String,
    },
    BuyIn {
        seat: usize,
        amount: Chips,
    },
    CashOut {
        seat: usize,
        amount: Chips,
    },
    Rebuy {
        seat: usize,
        amount: Chips,
    },
    SitOut {
        seat: usize,
    },
    /// Net result of one hand for one table seat.
    HandResult {
        seat: usize,
        delta: Chips,
    },
}

/// Why a rebuy wasn't possible.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RebuyError {
    NotBusted,
    InsufficientBankroll { need: Chips, have: Chips },
}

/// The table across games.
#[derive(Clone, Debug)]
pub struct Table {
    pub seats: Vec<SeatInfo>,
    /// Table index of who chooses the current/next game.
    pub chooser: usize,
    /// Stack bots get in `Bankroll`-model games (they have no real bankroll).
    pub bot_stack: Chips,
    /// The game in progress.
    current: Option<GameInProgress>,
    events: Vec<SessionEvent>,
}

#[derive(Clone, Debug)]
struct GameInProgress {
    stake: StakeModel,
    hands_total: usize,
    hands_played: usize,
}

impl Table {
    pub fn new(seats: Vec<SeatInfo>, bot_stack: Chips) -> Table {
        assert!(seats.len() >= 2, "a table needs at least two seats");
        Table {
            seats,
            chooser: 0,
            bot_stack,
            current: None,
            events: Vec::new(),
        }
    }

    /// Take the events recorded since the last call.
    pub fn drain_events(&mut self) -> Vec<SessionEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn in_game(&self) -> bool {
        self.current.is_some()
    }

    /// Start a game of `meta`: buy everyone in. Returns how many hands the game lasts.
    pub fn begin_game(&mut self, meta: &GameMeta) -> usize {
        assert!(self.current.is_none(), "previous game not ended");
        self.events.push(SessionEvent::GameChosen {
            chooser: self.chooser,
            game_id: meta.id.to_string(),
        });
        for i in 0..self.seats.len() {
            self.seats[i].sitting_out = false;
            self.buy_in(i, meta.stake_model);
        }
        let hands_total = match meta.length {
            GameLength::SingleHand => 1,
            GameLength::Orbit => self.seats.len(),
        };
        self.current = Some(GameInProgress {
            stake: meta.stake_model,
            hands_total,
            hands_played: 0,
        });
        hands_total
    }

    fn buy_in(&mut self, i: usize, stake: StakeModel) {
        let seat = &mut self.seats[i];
        let amount = match (stake, seat.bankroll) {
            (StakeModel::TableStake(x), None) => x,
            (StakeModel::TableStake(x), Some(b)) => x.min(b),
            (StakeModel::Bankroll, None) => self.bot_stack,
            (StakeModel::Bankroll, Some(b)) => b,
        };
        if let (StakeModel::TableStake(_), Some(b)) = (stake, seat.bankroll.as_mut()) {
            *b -= amount;
        }
        seat.stack = amount;
        if amount == Chips::ZERO {
            seat.sitting_out = true;
            self.events.push(SessionEvent::SitOut { seat: i });
        } else {
            self.events.push(SessionEvent::BuyIn { seat: i, amount });
        }
    }

    /// Table indices dealt into the next hand.
    pub fn active(&self) -> Vec<usize> {
        (0..self.seats.len())
            .filter(|&i| !self.seats[i].sitting_out && self.seats[i].stack > Chips::ZERO)
            .collect()
    }

    /// Setup for the next hand, plus the map from hand seat → table index. `None` if the game is
    /// over or fewer than two players can play.
    pub fn next_hand(&self) -> Option<(HandSetup, Vec<usize>)> {
        let g = self.current.as_ref()?;
        if g.hands_played >= g.hands_total {
            return None;
        }
        let active = self.active();
        if active.len() < 2 {
            return None;
        }
        let n = self.seats.len();
        // The button moves one seat per hand starting at the chooser; skip seats not dealt in.
        let want = (self.chooser + g.hands_played) % n;
        let pos = |table_idx: usize| {
            (0..n)
                .map(|k| (table_idx + k) % n)
                .find(|i| active.contains(i))
                .unwrap()
        };
        let button_table = pos(want);
        let chooser_table = pos(self.chooser);
        let to_hand = |t: usize| Seat(active.iter().position(|&i| i == t).unwrap() as u8);
        let setup = HandSetup {
            num_seats: active.len() as u8,
            button: to_hand(button_table),
            chooser: to_hand(chooser_table),
            first_hand_of_game: g.hands_played == 0,
            stacks: active.iter().map(|&i| self.seats[i].stack).collect(),
        };
        Some((setup, active))
    }

    /// Record a finished hand: apply payoffs (hand seats mapped back through `map`).
    pub fn settle_hand(&mut self, map: &[usize], payoffs: &[Chips]) {
        let stake = self.current.as_ref().expect("no game in progress").stake;
        for (&i, &delta) in map.iter().zip(payoffs) {
            let seat = &mut self.seats[i];
            seat.stack += delta;
            assert!(
                seat.stack >= Chips::ZERO,
                "negative stack: chip accounting bug"
            );
            if let (StakeModel::Bankroll, Some(b)) = (stake, seat.bankroll.as_mut()) {
                *b = seat.stack;
            }
            self.events
                .push(SessionEvent::HandResult { seat: i, delta });
        }
        self.current.as_mut().unwrap().hands_played += 1;
    }

    /// Seats (table indices) with an empty stack, in a game that isn't over.
    pub fn busted(&self) -> Vec<usize> {
        if self.next_hand_pending() {
            (0..self.seats.len())
                .filter(|&i| !self.seats[i].sitting_out && self.seats[i].stack == Chips::ZERO)
                .collect()
        } else {
            Vec::new()
        }
    }

    fn next_hand_pending(&self) -> bool {
        self.current
            .as_ref()
            .is_some_and(|g| g.hands_played < g.hands_total)
    }

    /// Rebuy after a bust (FR-SS-3). TableStake: another stake from bankroll. Bankroll model:
    /// bots reset to `bot_stack`; a human needs a deposit first (outside the session).
    pub fn rebuy(&mut self, i: usize) -> Result<Chips, RebuyError> {
        let stake = self.current.as_ref().expect("no game in progress").stake;
        if self.seats[i].stack > Chips::ZERO {
            return Err(RebuyError::NotBusted);
        }
        let seat = &mut self.seats[i];
        let amount = match (stake, seat.bankroll.as_mut()) {
            (StakeModel::TableStake(x), None) => x,
            (StakeModel::TableStake(x), Some(b)) => {
                if *b < x {
                    return Err(RebuyError::InsufficientBankroll { need: x, have: *b });
                }
                *b -= x;
                x
            }
            (StakeModel::Bankroll, None) => self.bot_stack,
            (StakeModel::Bankroll, Some(b)) => {
                return Err(RebuyError::InsufficientBankroll {
                    need: Chips(1),
                    have: *b,
                });
            }
        };
        seat.stack = amount;
        self.events.push(SessionEvent::Rebuy { seat: i, amount });
        Ok(amount)
    }

    /// A busted seat declines to rebuy: it's dealt out for the rest of this game.
    pub fn sit_out(&mut self, i: usize) {
        self.seats[i].sitting_out = true;
        self.events.push(SessionEvent::SitOut { seat: i });
    }

    /// Finish the game: cash out table stakes, pass the choice to the left.
    pub fn end_game(&mut self) {
        let g = self.current.take().expect("no game in progress");
        for i in 0..self.seats.len() {
            let seat = &mut self.seats[i];
            if let StakeModel::TableStake(_) = g.stake {
                let amount = seat.stack;
                if let Some(b) = seat.bankroll.as_mut() {
                    *b += amount;
                }
                if amount > Chips::ZERO {
                    self.events.push(SessionEvent::CashOut { seat: i, amount });
                }
            }
            seat.stack = Chips::ZERO;
            seat.sitting_out = false;
        }
        self.chooser = (self.chooser + 1) % self.seats.len();
    }

    /// Total chips: all finite bankrolls plus every stack. For conservation checks.
    pub fn total_human_money(&self) -> Chips {
        self.seats
            .iter()
            .filter(|s| s.bankroll.is_some())
            .map(|s| {
                s.bankroll.unwrap()
                    + if self
                        .current
                        .as_ref()
                        .is_some_and(|g| matches!(g.stake, StakeModel::TableStake(_)))
                    {
                        s.stack
                    } else {
                        Chips::ZERO
                    }
            })
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(stake: StakeModel, length: GameLength) -> GameMeta {
        GameMeta {
            id: "test",
            version: 1,
            display_name: "test",
            seats: 2..=9,
            stake_model: stake,
            length,
            rules_ref: "",
        }
    }

    fn human(bankroll: i64) -> SeatInfo {
        SeatInfo {
            name: "you".into(),
            kind: SeatKind::Human,
            bankroll: Some(Chips(bankroll)),
            stack: Chips::ZERO,
            sitting_out: false,
        }
    }

    fn bot(name: &str) -> SeatInfo {
        SeatInfo {
            name: name.into(),
            kind: SeatKind::Bot,
            bankroll: None,
            stack: Chips::ZERO,
            sitting_out: false,
        }
    }

    #[test]
    fn single_hand_game_then_choice_passes_left() {
        let mut t = Table::new(vec![human(10_000), bot("a"), bot("b")], Chips(5_000));
        assert_eq!(
            t.begin_game(&meta(StakeModel::Bankroll, GameLength::SingleHand)),
            1
        );
        let (setup, map) = t.next_hand().unwrap();
        assert_eq!(setup.button, Seat(0));
        assert!(setup.first_hand_of_game);
        assert_eq!(
            setup.stacks,
            vec![Chips(10_000), Chips(5_000), Chips(5_000)]
        );
        t.settle_hand(&map, &[Chips(-300), Chips(300), Chips(0)]);
        assert!(t.next_hand().is_none(), "one hand per game");
        t.end_game();
        assert_eq!(t.seats[0].bankroll, Some(Chips(9_700)));
        assert_eq!(t.chooser, 1);
    }

    #[test]
    fn orbit_game_with_table_stake() {
        let mut t = Table::new(vec![human(10_000), bot("a"), bot("b")], Chips(5_000));
        assert_eq!(
            t.begin_game(&meta(
                StakeModel::TableStake(Chips(2_000)),
                GameLength::Orbit
            )),
            3
        );
        assert_eq!(
            t.seats[0].bankroll,
            Some(Chips(8_000)),
            "stake moved to the table"
        );
        let mut buttons = Vec::new();
        while let Some((setup, map)) = t.next_hand() {
            buttons.push(setup.button);
            assert_eq!(setup.first_hand_of_game, buttons.len() == 1);
            t.settle_hand(&map, &[Chips(100), Chips(-50), Chips(-50)]);
        }
        assert_eq!(
            buttons,
            vec![Seat(0), Seat(1), Seat(2)],
            "button moves around the orbit"
        );
        t.end_game();
        assert_eq!(
            t.seats[0].bankroll,
            Some(Chips(10_300)),
            "stake + winnings return"
        );
    }

    #[test]
    fn bust_rebuy_and_sit_out() {
        let mut t = Table::new(vec![human(3_000), bot("a"), bot("b")], Chips(5_000));
        t.begin_game(&meta(
            StakeModel::TableStake(Chips(2_000)),
            GameLength::Orbit,
        ));
        let (_, map) = t.next_hand().unwrap();
        t.settle_hand(&map, &[Chips(-2_000), Chips(2_000), Chips(0)]);
        assert_eq!(t.busted(), vec![0]);
        // Bankroll has $10 left, a stake is $20: can't rebuy.
        assert_eq!(
            t.rebuy(0),
            Err(RebuyError::InsufficientBankroll {
                need: Chips(2_000),
                have: Chips(1_000)
            })
        );
        t.sit_out(0);
        let (setup, map) = t.next_hand().unwrap();
        assert_eq!(setup.num_seats, 2, "sitting-out seat not dealt");
        assert_eq!(map, vec![1, 2]);
    }

    #[test]
    fn bots_rebuy_from_infinite_bankroll() {
        let mut t = Table::new(vec![human(10_000), bot("a")], Chips(5_000));
        t.begin_game(&meta(
            StakeModel::TableStake(Chips(2_000)),
            GameLength::Orbit,
        ));
        let (_, map) = t.next_hand().unwrap();
        t.settle_hand(&map, &[Chips(2_000), Chips(-2_000)]);
        assert_eq!(t.rebuy(1), Ok(Chips(2_000)));
    }

    #[test]
    fn human_money_is_conserved_through_a_game() {
        let mut t = Table::new(vec![human(10_000), human(10_000), bot("a")], Chips(5_000));
        let before = t.total_human_money();
        t.begin_game(&meta(
            StakeModel::TableStake(Chips(2_000)),
            GameLength::Orbit,
        ));
        assert_eq!(t.total_human_money(), before);
        while let Some((_, map)) = t.next_hand() {
            t.settle_hand(&map, &[Chips(100), Chips(-100), Chips(0)]);
        }
        t.end_game();
        assert_eq!(t.total_human_money(), before);
    }
}
