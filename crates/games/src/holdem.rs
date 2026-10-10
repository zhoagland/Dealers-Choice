//! No-Limit Texas Hold'em, house rules (Poker Bible §Texas Hold'em). PRD §7; `docs/tech/games.md`.
//!
//! - Blinds 50¢/$1 (configurable). The chooser posts the $2 dealer ante (dead money) on the
//!   first hand of the game. Stakes come from a $20 table stake for the whole orbit.
//! - Hole cards are dealt one at a time, starting left of the button. Burn cards are omitted
//!   (they don't change the distribution of the dealt cards, API doc §1).
//! - Preflop action starts left of the big blind; postflop, with the first live seat left of the
//!   button. Heads-up: the button posts the small blind and acts first preflop, last postflop.
//! - Showdown: the last river aggressor shows first (otherwise the first live seat left of the
//!   button); every hand that wins chips is shown; the others are mucked and stay hidden.
//!
//! Chip arithmetic is delegated entirely to `engine::betting` and `engine::pot` (NFR-5).

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use engine::betting::{BetAction, BetSize, Betting, Structure};
use engine::eval::high::eval_high;
use engine::game::sample_hidden_hands;
use engine::pot::{award_high, order_from_button, side_pots};
use engine::replay::replay_interleaved;
use engine::{
    ActionClass, Card, CardSet, Chips, DetRng, Game, GameLength, GameMeta, HandSetup, HandValue,
    HiddenSampler, IllegalAction, Seat, StakeModel, ToAct,
};
use rand::Rng;

// ---------------------------------------------------------------------------------------------
// Config, actions
// ---------------------------------------------------------------------------------------------

/// Per-game settings, loaded from TOML. `#[serde(default)]` fills any field the file omits
/// from `Default`, so an empty table means "Bible defaults".
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub small_blind: Chips,
    pub big_blind: Chips,
    /// Paid by the chooser on the first hand of each game.
    pub dealer_ante: Chips,
    pub table_stake: Chips,
    /// The bet sizes bots consider (humans may bet any legal amount).
    pub bet_menu: Vec<BetSize>,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            small_blind: Chips(50),
            big_blind: Chips(100),
            dealer_ante: Chips(200),
            table_stake: Chips(2_000),
            bet_menu: vec![
                BetSize::MinRaise,
                BetSize::PotFraction(1, 3),
                BetSize::PotFraction(2, 3),
                BetSize::PotFraction(1, 1),
                BetSize::AllIn,
            ],
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Action {
    Deal(Card),
    Fold,
    CheckCall,
    RaiseTo(Chips),
}

impl Action {
    fn as_bet(self) -> Option<BetAction> {
        match self {
            Action::Fold => Some(BetAction::Fold),
            Action::CheckCall => Some(BetAction::CheckCall),
            Action::RaiseTo(x) => Some(BetAction::RaiseTo(x)),
            Action::Deal(_) => None,
        }
    }

    fn from_bet(b: BetAction) -> Action {
        match b {
            BetAction::Fold => Action::Fold,
            BetAction::CheckCall => Action::CheckCall,
            BetAction::RaiseTo(x) => Action::RaiseTo(x),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Street {
    Preflop,
    Flop,
    Turn,
    River,
    /// The hand is over (showdown, or everyone else folded).
    Done,
}

/// Where the next chance card goes.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
enum Slot {
    Hole(Seat),
    Board,
}

// ---------------------------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct State {
    setup: HandSetup,
    hole: Vec<SmallVec<[Card; 2]>>,
    board: SmallVec<[Card; 5]>,
    dealt: CardSet,
    street: Street,
    betting: Betting,
    /// Pending chance cards, next first.
    queue: SmallVec<[Slot; 24]>,
    /// Everything that happened, chance included.
    history: Vec<(Option<Seat>, Action)>,
    /// Last seat to bet or raise on the river (shows first).
    river_aggressor: Option<Seat>,
}

impl State {
    fn n(&self) -> u8 {
        self.setup.num_seats
    }

    /// The first seat at or after `from` (clockwise) that hasn't folded.
    fn first_live_from(&self, from: Seat) -> Seat {
        (0..self.n())
            .map(|k| Seat((from.0 + k) % self.n()))
            .find(|&s| !self.betting.is_folded(s))
            .unwrap()
    }

    /// After every action: deal streets / move on once betting closes.
    fn advance(&mut self) {
        while self.queue.is_empty()
            && self.street != Street::Done
            && self.betting.to_act().is_none()
        {
            if self.betting.live_count() <= 1 {
                self.street = Street::Done;
                break;
            }
            let (next, cards) = match self.street {
                Street::Preflop => (Street::Flop, 3),
                Street::Flop => (Street::Turn, 1),
                Street::Turn => (Street::River, 1),
                Street::River | Street::Done => (Street::Done, 0),
            };
            self.street = next;
            if next == Street::Done {
                break;
            }
            self.queue.extend(std::iter::repeat_n(Slot::Board, cards));
            let first = self.first_live_from(self.setup.button.next(self.n()));
            self.betting.start_street(next as u8, first);
        }
    }

    /// High hand value per seat for live seats at showdown. Without a showdown (everyone else
    /// folded) nothing is evaluated: the lone seat wins regardless, and may have no board yet.
    fn showdown_values(&self) -> Vec<Option<HandValue>> {
        if !self.is_showdown() {
            return vec![None; self.n() as usize];
        }
        let board: CardSet = self.board.iter().copied().collect();
        (0..self.n())
            .map(Seat)
            .map(|s| {
                if self.betting.is_folded(s) {
                    None
                } else {
                    Some(eval_high(
                        board.union(self.hole[s.index()].iter().copied().collect()),
                    ))
                }
            })
            .collect()
    }

    /// Chips won per seat (before subtracting contributions).
    fn winnings(&self) -> Vec<Chips> {
        let n = self.n() as usize;
        let folded = (0..n)
            .filter(|&i| self.betting.is_folded(Seat(i as u8)))
            .fold(0u16, |m, i| m | 1 << i);
        let pots = side_pots(self.betting.contributed(), folded);
        award_high(
            &pots,
            &self.showdown_values(),
            &order_from_button(self.setup.button, self.n()),
        )
    }

    fn is_showdown(&self) -> bool {
        self.street == Street::Done && self.betting.live_count() > 1
    }

    /// Seats whose cards are shown at showdown: the first shower plus everyone who wins chips.
    fn shown(&self) -> u16 {
        if !self.is_showdown() {
            return 0;
        }
        let first = self
            .river_aggressor
            .unwrap_or_else(|| self.first_live_from(self.setup.button.next(self.n())));
        let won = self.winnings();
        let mut mask = 1u16 << first.0;
        for (i, w) in won.iter().enumerate() {
            if w.0 > 0 && !self.betting.is_folded(Seat(i as u8)) {
                mask |= 1 << i;
            }
        }
        mask
    }
}

// ---------------------------------------------------------------------------------------------
// Observation
// ---------------------------------------------------------------------------------------------

/// What one seat sees in the history: other seats' hole cards become `HoleDealt`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PublicAction {
    HoleDealt,
    Board(Card),
    Bet(Seat, BetAction),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct Observation {
    pub seat: Seat,
    pub setup: HandSetup,
    /// This seat's hole cards dealt so far.
    pub hole: SmallVec<[Card; 2]>,
    pub history: SmallVec<[PublicAction; 48]>,
    /// Hands shown at showdown, per seat.
    pub shown: Vec<Option<SmallVec<[Card; 2]>>>,
}

impl Observation {
    pub fn board(&self) -> SmallVec<[Card; 5]> {
        self.history
            .iter()
            .filter_map(|a| {
                if let PublicAction::Board(c) = a {
                    Some(*c)
                } else {
                    None
                }
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------------------------
// Game
// ---------------------------------------------------------------------------------------------

pub struct Holdem {
    config: Config,
    meta: GameMeta,
}

impl Holdem {
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Small blind and big blind seats for a hand.
    fn blind_seats(setup: &HandSetup) -> (Seat, Seat) {
        let n = setup.num_seats;
        if n == 2 {
            (setup.button, setup.button.next(n))
        } else {
            (setup.button.next(n), setup.button.next(n).next(n))
        }
    }

    /// The seat dealt the k-th hole card (one at a time, starting left of the button).
    fn hole_seat(setup: &HandSetup, k: usize) -> Seat {
        let n = setup.num_seats as usize;
        Seat(((setup.button.index() + 1 + k % n) % n) as u8)
    }
}

impl Game for Holdem {
    type State = State;
    type Action = Action;
    type Observation = Observation;
    type Config = Config;

    fn new(config: Config) -> Holdem {
        let stake = config.table_stake;
        Holdem {
            config,
            meta: GameMeta {
                id: "holdem_nl",
                version: 1,
                display_name: "No-Limit Texas Hold'em",
                seats: 2..=9,
                stake_model: StakeModel::TableStake(stake),
                length: GameLength::Orbit,
                rules_ref: "Poker Bible §Texas Hold'em",
            },
        }
    }

    fn meta(&self) -> &GameMeta {
        &self.meta
    }

    fn new_hand(&self, setup: &HandSetup) -> State {
        let n = setup.num_seats;
        let mut betting = Betting::new(Structure::NoLimit, &setup.stacks, self.config.big_blind);
        if setup.first_hand_of_game && self.config.dealer_ante > Chips::ZERO {
            betting.post_dead(setup.chooser, self.config.dealer_ante);
        }
        let (sb, bb) = Holdem::blind_seats(setup);
        betting.start_street(0, bb.next(n));
        betting.post_blind(sb, self.config.small_blind);
        betting.post_blind(bb, self.config.big_blind);
        let queue = (0..2 * n as usize)
            .map(|k| Slot::Hole(Holdem::hole_seat(setup, k)))
            .collect();
        State {
            setup: setup.clone(),
            hole: vec![SmallVec::new(); n as usize],
            board: SmallVec::new(),
            dealt: CardSet::EMPTY,
            street: Street::Preflop,
            betting,
            queue,
            history: Vec::new(),
            river_aggressor: None,
        }
    }

    fn to_act(&self, s: &State) -> ToAct {
        if !s.queue.is_empty() {
            ToAct::Chance
        } else if s.street == Street::Done {
            ToAct::Terminal
        } else {
            match s.betting.to_act() {
                Some(seat) => ToAct::Seat(seat),
                None => ToAct::Terminal, // unreachable: `advance` never leaves this state
            }
        }
    }

    fn legal_actions(&self, s: &State, out: &mut Vec<Action>) {
        out.clear();
        match self.to_act(s) {
            ToAct::Chance => out.extend(
                CardSet::FULL_DECK
                    .difference(s.dealt)
                    .iter()
                    .map(Action::Deal),
            ),
            ToAct::Seat(p) => out.extend(
                s.betting
                    .menu(p, &self.config.bet_menu)
                    .into_iter()
                    .map(Action::from_bet),
            ),
            ToAct::Terminal => {}
        }
    }

    fn validate(&self, s: &State, a: Action) -> Result<(), IllegalAction> {
        match (self.to_act(s), a) {
            (ToAct::Chance, Action::Deal(c)) if !s.dealt.contains(c) => Ok(()),
            (ToAct::Chance, _) => Err(IllegalAction(format!(
                "expected a deal of an undealt card, got {a:?}"
            ))),
            (ToAct::Seat(p), a) => match a.as_bet() {
                Some(b) => s.betting.validate(p, b),
                None => Err(IllegalAction("a player can't deal".into())),
            },
            (ToAct::Terminal, _) => Err(IllegalAction("the hand is over".into())),
        }
    }

    fn apply(&self, s: &mut State, a: Action) {
        match a {
            Action::Deal(c) => {
                s.dealt.insert(c);
                match s.queue.remove(0) {
                    Slot::Hole(seat) => s.hole[seat.index()].push(c),
                    Slot::Board => s.board.push(c),
                }
                s.history.push((None, a));
            }
            other => {
                let seat = s.betting.to_act().expect("no seat to act");
                let bet = other.as_bet().unwrap();
                s.betting.apply(seat, bet);
                if s.street == Street::River && matches!(bet, BetAction::RaiseTo(_)) {
                    s.river_aggressor = Some(seat);
                }
                s.history.push((Some(seat), other));
            }
        }
        s.advance();
    }

    fn chance_outcomes(&self, s: &State, out: &mut Vec<(Action, f64)>) {
        out.clear();
        let left = CardSet::FULL_DECK.difference(s.dealt);
        let p = 1.0 / left.len() as f64;
        out.extend(left.iter().map(|c| (Action::Deal(c), p)));
    }

    /// Faster than enumerating 52 outcomes: pick the k-th remaining card directly.
    fn sample_chance(&self, s: &State, rng: &mut DetRng) -> Action {
        let left = CardSet::FULL_DECK.difference(s.dealt);
        let k = rng.random_range(0..left.len());
        Action::Deal(left.iter().nth(k).unwrap())
    }

    fn payoffs(&self, s: &State) -> Vec<Chips> {
        s.winnings()
            .iter()
            .zip(s.betting.contributed())
            .map(|(&w, &c)| w - c)
            .collect()
    }

    fn observe(&self, s: &State, seat: Seat) -> Observation {
        let history = s
            .history
            .iter()
            .scan(0usize, |hole_k, &(who, a)| {
                Some(match (who, a) {
                    (None, Action::Deal(c)) => {
                        let k = *hole_k;
                        if k < 2 * s.n() as usize {
                            *hole_k += 1;
                            PublicAction::HoleDealt
                        } else {
                            PublicAction::Board(c)
                        }
                    }
                    (Some(p), other) => PublicAction::Bet(p, other.as_bet().unwrap()),
                    (None, _) => unreachable!("only chance has no seat"),
                })
            })
            .collect();
        let shown = s.shown();
        Observation {
            seat,
            setup: s.setup.clone(),
            hole: s.hole[seat.index()].clone(),
            history,
            shown: (0..s.n() as usize)
                .map(|i| {
                    if shown & (1 << i) != 0 && i != seat.index() {
                        Some(s.hole[i].clone())
                    } else {
                        None
                    }
                })
                .collect(),
        }
    }

    fn observer(&self, obs: &Observation) -> Seat {
        obs.seat
    }

    fn determinize(
        &self,
        obs: &Observation,
        sampler: &mut dyn HiddenSampler,
        rng: &mut DetRng,
    ) -> State {
        let setup = &obs.setup;
        let n = setup.num_seats as usize;
        let hole_dealt = obs
            .history
            .iter()
            .filter(|a| **a == PublicAction::HoleDealt)
            .count();
        // Hole cards dealt per seat so far.
        let mut count = vec![0usize; n];
        for k in 0..hole_dealt {
            count[Holdem::hole_seat(setup, k).index()] += 1;
        }

        let mut known: CardSet = obs.hole.iter().copied().chain(obs.board()).collect();
        for cards in obs.shown.iter().flatten() {
            known = known.union(cards.iter().copied().collect());
        }
        let requests: Vec<(Seat, usize)> = (0..n)
            .map(|i| Seat(i as u8))
            .filter(|&p| p != obs.seat && obs.shown[p.index()].is_none() && count[p.index()] > 0)
            .map(|p| (p, count[p.index()]))
            .collect();
        let sampled = sample_hidden_hands(
            sampler,
            &requests,
            CardSet::FULL_DECK.difference(known),
            rng,
        );

        let mut holes: Vec<SmallVec<[Card; 2]>> = vec![SmallVec::new(); n];
        holes[obs.seat.index()] = obs.hole.clone();
        for (i, cards) in obs.shown.iter().enumerate() {
            if let Some(c) = cards {
                holes[i] = c.clone();
            }
        }
        for (&(p, _), cards) in requests.iter().zip(sampled) {
            holes[p.index()] = cards.into_iter().collect();
        }

        // Rebuild the deal sequence in the order the cards were dealt.
        let mut next_hole = vec![0usize; n];
        let mut k = 0;
        let deals: Vec<Action> = obs
            .history
            .iter()
            .filter_map(|a| match *a {
                PublicAction::HoleDealt => {
                    let p = Holdem::hole_seat(setup, k).index();
                    k += 1;
                    next_hole[p] += 1;
                    Some(Action::Deal(holes[p][next_hole[p] - 1]))
                }
                PublicAction::Board(c) => Some(Action::Deal(c)),
                PublicAction::Bet(..) => None,
            })
            .collect();
        let bets = obs.history.iter().filter_map(|a| match *a {
            PublicAction::Bet(_, b) => Some(Action::from_bet(b)),
            _ => None,
        });
        replay_interleaved(self, setup, deals, bets)
    }

    fn classify(&self, _s: &State, a: Action) -> ActionClass {
        match a {
            Action::Deal(_) => ActionClass::Chance,
            Action::Fold => ActionClass::Fold,
            Action::CheckCall => ActionClass::CheckOrCall,
            Action::RaiseTo(to) => ActionClass::BetOrRaise { to },
        }
    }

    fn eval_unit(&self) -> Chips {
        self.config.big_blind
    }
}

/// Read-only accessors for renderers and tests.
impl State {
    pub fn board(&self) -> &[Card] {
        &self.board
    }

    pub fn street(&self) -> Street {
        self.street
    }

    pub fn betting(&self) -> &Betting {
        &self.betting
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::replay::replay;
    use engine::testing::{Conformance, check_conformance};

    fn deal(cards: &str) -> Vec<Action> {
        cards
            .split_whitespace()
            .map(|c| Action::Deal(c.parse().unwrap()))
            .collect()
    }

    fn game() -> Holdem {
        Holdem::new(Config::default())
    }

    #[test]
    fn conformance_2_3_6_9_seats() {
        check_conformance(
            &game(),
            &Conformance {
                hands: 60,
                seat_counts: vec![2, 3, 6, 9],
                stack: Chips(2_000),
                ..Conformance::default()
            },
        );
    }

    #[test]
    fn conformance_short_stacks_exercise_side_pots() {
        // Stacks of 2-4 big blinds: lots of all-ins and side pots.
        check_conformance(
            &game(),
            &Conformance {
                hands: 150,
                seat_counts: vec![3, 5],
                stack: Chips(300),
                ..Conformance::default()
            },
        );
    }

    /// Button seat 0, 3 seats: SB = seat 1, BB = seat 2, seat 0 acts first preflop.
    fn setup3() -> HandSetup {
        HandSetup {
            first_hand_of_game: false,
            ..HandSetup::simple(3, Chips(2_000))
        }
    }

    #[test]
    fn blinds_order_and_dealer_ante() {
        let g = game();
        let mut setup = setup3();
        setup.first_hand_of_game = true;
        let s = g.new_hand(&setup);
        assert_eq!(
            s.betting.contributed(),
            &[Chips(200), Chips(50), Chips(100)]
        );
        let s = replay(&g, &setup, &deal("As Ks Qs Js Ts 9s")).unwrap();
        assert_eq!(g.to_act(&s), ToAct::Seat(Seat(0)), "left of BB acts first");
    }

    #[test]
    fn heads_up_button_is_small_blind_and_acts_first_preflop_last_after() {
        let g = game();
        let setup = HandSetup {
            first_hand_of_game: false,
            ..HandSetup::simple(2, Chips(2_000))
        };
        let s = g.new_hand(&setup);
        assert_eq!(s.betting.contributed(), &[Chips(50), Chips(100)]);
        let mut log = deal("As Ks Qs Js");
        let s2 = replay(&g, &setup, &log).unwrap();
        assert_eq!(g.to_act(&s2), ToAct::Seat(Seat(0)));
        log.extend([Action::CheckCall, Action::CheckCall]);
        log.extend(deal("2c 3c 4c"));
        let s3 = replay(&g, &setup, &log).unwrap();
        assert_eq!(
            g.to_act(&s3),
            ToAct::Seat(Seat(1)),
            "non-button acts first postflop"
        );
    }

    #[test]
    fn showdown_best_hand_wins_and_loser_mucks() {
        let g = game();
        let setup = setup3();
        // Hole order: s1, s2, s0, s1, s2, s0. s0 = AA, s1 = KK, s2 = 72.
        let mut log = deal("Kh 7c Ah Kd 2d Ad");
        log.extend([Action::CheckCall, Action::CheckCall, Action::CheckCall]); // limp, complete, BB checks
        for street_cards in ["3s 8s 9h", "Jc", "4d"] {
            log.extend(deal(street_cards));
            log.extend([Action::CheckCall, Action::CheckCall, Action::CheckCall]);
        }
        let s = replay(&g, &setup, &log).unwrap();
        assert_eq!(g.to_act(&s), ToAct::Terminal);
        assert_eq!(g.payoffs(&s), vec![Chips(200), Chips(-100), Chips(-100)]);
        let obs = g.observe(&s, Seat(1));
        assert!(obs.shown[0].is_some(), "winner shows");
        assert!(obs.shown[2].is_none(), "loser who didn't show first mucks");
    }

    #[test]
    fn everyone_folds_to_big_blind() {
        let g = game();
        let mut log = deal("As Ks Qs Js Ts 9s");
        log.extend([Action::Fold, Action::Fold]);
        let s = replay(&g, &setup3(), &log).unwrap();
        assert_eq!(g.to_act(&s), ToAct::Terminal);
        assert_eq!(g.payoffs(&s), vec![Chips(0), Chips(-50), Chips(50)]);
        assert!(
            g.observe(&s, Seat(0)).shown.iter().all(Option::is_none),
            "no showdown, no reveal"
        );
    }

    #[test]
    fn all_in_preflop_runs_out_the_board() {
        let g = game();
        let setup = HandSetup {
            first_hand_of_game: false,
            ..HandSetup::simple(2, Chips(500))
        };
        // Dealing starts left of the button: seat 1 gets cards 1 and 3 (KK), seat 0 gets 2 and 4 (AA).
        let mut log = deal("Kd As Kh Ah");
        log.extend([Action::RaiseTo(Chips(500)), Action::CheckCall]);
        let s = replay(&g, &setup, &log).unwrap();
        assert_eq!(
            g.to_act(&s),
            ToAct::Chance,
            "no more betting: deal the board"
        );
        log.extend(deal("2c 3d 4h 9s Tc"));
        let s = replay(&g, &setup, &log).unwrap();
        assert_eq!(g.to_act(&s), ToAct::Terminal);
        assert_eq!(g.payoffs(&s), vec![Chips(500), Chips(-500)], "AA beats KK");
    }

    #[test]
    fn split_pot_on_the_board() {
        let g = game();
        let setup = HandSetup {
            first_hand_of_game: false,
            ..HandSetup::simple(2, Chips(1_000))
        };
        let mut log = deal("2c 3d 2h 3h");
        log.extend([Action::CheckCall, Action::CheckCall]);
        for c in ["As Ks Qs", "Js", "Ts"] {
            log.extend(deal(c));
            log.extend([Action::CheckCall, Action::CheckCall]);
        }
        let s = replay(&g, &setup, &log).unwrap();
        assert_eq!(
            g.payoffs(&s),
            vec![Chips(0), Chips(0)],
            "royal on board: chop"
        );
    }

    #[test]
    fn off_menu_human_bet_is_legal() {
        let g = game();
        let s = replay(&g, &setup3(), &deal("As Ks Qs Js Ts 9s")).unwrap();
        assert!(g.validate(&s, Action::RaiseTo(Chips(237))).is_ok());
        assert!(
            g.validate(&s, Action::RaiseTo(Chips(150))).is_err(),
            "below min raise"
        );
    }
}
