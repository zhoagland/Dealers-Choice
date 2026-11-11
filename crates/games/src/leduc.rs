//! Leduc hold'em: a test fixture for MCCFR and best response (not a user-facing game).
//!
//! Rules (Southey et al., 2005): 6 cards (J, Q, K in two suits), 2 players, ante 1 each, one
//! private card each. Round 1: fixed bets of 2. Then one public board card. Round 2: fixed bets
//! of 4. Each round allows at most 2 bets/raises. Showdown: pairing the board wins; otherwise
//! higher rank; equal ranks split.
//!
//! Why it's here: it has multiple rounds and a public card (which Kuhn doesn't), but it's still
//! small enough (on the order of 10³ information sets) to compute exact best responses.

use engine::game::sample_hidden_hands;
use engine::replay::replay_interleaved;
use engine::{
    ActionClass, Card, CardSet, Chips, DetRng, Game, GameLength, GameMeta, HandSetup,
    HiddenSampler, IllegalAction, Rank, Seat, StakeModel, Suit, ToAct,
};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

pub const DECK: [Card; 6] = [
    Card::new(Rank::Jack, Suit::Spades),
    Card::new(Rank::Jack, Suit::Hearts),
    Card::new(Rank::Queen, Suit::Spades),
    Card::new(Rank::Queen, Suit::Hearts),
    Card::new(Rank::King, Suit::Spades),
    Card::new(Rank::King, Suit::Hearts),
];

const ANTE: i64 = 1;
const BET_SIZE: [i64; 2] = [2, 4];
const MAX_RAISES: usize = 2;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Action {
    Deal(Card),
    Fold,
    /// Check when nothing is owed, call otherwise.
    CheckCall,
    /// Bet when nothing is owed, raise otherwise. Size fixed by the round.
    Raise,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Config {}

/// One round's player actions. At most 4: check, raise, raise, call.
type RoundLog = SmallVec<[Action; 4]>;

#[derive(Clone, Debug)]
pub struct State {
    setup: HandSetup,
    /// Deal order: first player's card, second player's card, board.
    cards: SmallVec<[Card; 3]>,
    rounds: [RoundLog; 2],
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Observation {
    pub seat: Seat,
    pub button: Seat,
    pub dealt: u8,
    pub own: Option<Card>,
    pub board: Option<Card>,
    pub rounds: [RoundLog; 2],
    /// Opponent's card after a showdown.
    pub revealed: Option<Card>,
}

pub struct Leduc {
    meta: GameMeta,
}

/// A round is over once a check or call happens that isn't the round's first action.
/// ([CheckCall, CheckCall] = check-check; [.., Raise, CheckCall] = call.)
fn round_closed(r: &[Action]) -> bool {
    r.len() >= 2 && r.last() == Some(&Action::CheckCall)
}

impl State {
    fn first(&self) -> Seat {
        self.setup.button.next(2)
    }

    fn seat_of(&self, order: usize) -> Seat {
        if order == 0 {
            self.first()
        } else {
            self.first().next(2)
        }
    }

    fn order_of(&self, seat: Seat) -> usize {
        if seat == self.first() { 0 } else { 1 }
    }

    fn folded(&self) -> bool {
        self.rounds.iter().any(|r| r.last() == Some(&Action::Fold))
    }

    /// The betting round in progress (or most recent).
    fn round(&self) -> usize {
        if self.cards.len() == 3 { 1 } else { 0 }
    }

    fn is_showdown(&self) -> bool {
        !self.folded() && round_closed(&self.rounds[1])
    }

    /// Chips each player (in deal order) has put in so far.
    fn contributions(&self) -> [i64; 2] {
        let mut total = [ANTE; 2];
        for (round, log) in self.rounds.iter().enumerate() {
            let mut committed = [0i64; 2];
            let mut level = 0;
            for (i, &a) in log.iter().enumerate() {
                let who = i % 2;
                match a {
                    Action::Raise => {
                        level += BET_SIZE[round];
                        committed[who] = level;
                    }
                    Action::CheckCall => committed[who] = level,
                    _ => {}
                }
            }
            total[0] += committed[0];
            total[1] += committed[1];
        }
        total
    }

    /// Legal player actions now (empty if a player isn't to act).
    fn player_actions(&self) -> SmallVec<[Action; 3]> {
        let log = &self.rounds[self.round()];
        let facing = log.last() == Some(&Action::Raise);
        let raises = log.iter().filter(|&&a| a == Action::Raise).count();
        let mut v = SmallVec::new();
        // Folding when you could check for free is never better, so it isn't offered.
        if facing {
            v.push(Action::Fold);
        }
        v.push(Action::CheckCall);
        if raises < MAX_RAISES {
            v.push(Action::Raise);
        }
        v
    }
}

impl Game for Leduc {
    type State = State;
    type Action = Action;
    type Observation = Observation;
    type Config = Config;

    fn new(_config: Config) -> Leduc {
        Leduc {
            meta: GameMeta {
                id: "leduc",
                version: 1,
                display_name: "Leduc hold'em",
                seats: 2..=2,
                stake_model: StakeModel::Bankroll,
                length: GameLength::SingleHand,
                rules_ref: "Southey et al. (2005); docs/tech/games.md §Leduc",
            },
        }
    }

    fn meta(&self) -> &GameMeta {
        &self.meta
    }

    fn new_hand(&self, setup: &HandSetup) -> State {
        assert_eq!(setup.num_seats, 2, "Leduc is heads-up only");
        State {
            setup: setup.clone(),
            cards: SmallVec::new(),
            rounds: Default::default(),
        }
    }

    fn to_act(&self, s: &State) -> ToAct {
        if s.cards.len() < 2 {
            return ToAct::Chance;
        }
        if s.folded() || round_closed(&s.rounds[1]) {
            return ToAct::Terminal;
        }
        if s.cards.len() == 2 && round_closed(&s.rounds[0]) {
            return ToAct::Chance; // deal the board
        }
        ToAct::Seat(s.seat_of(s.rounds[s.round()].len() % 2))
    }

    fn legal_actions(&self, s: &State, out: &mut Vec<Action>) {
        out.clear();
        match self.to_act(s) {
            ToAct::Chance => out.extend(
                DECK.iter()
                    .filter(|c| !s.cards.contains(c))
                    .map(|&c| Action::Deal(c)),
            ),
            ToAct::Seat(_) => out.extend(s.player_actions()),
            ToAct::Terminal => {}
        }
    }

    fn validate(&self, s: &State, a: Action) -> Result<(), IllegalAction> {
        let ok = match (self.to_act(s), a) {
            (ToAct::Chance, Action::Deal(c)) => DECK.contains(&c) && !s.cards.contains(&c),
            (ToAct::Seat(_), a) => s.player_actions().contains(&a),
            _ => false,
        };
        if ok {
            Ok(())
        } else {
            Err(IllegalAction(format!("{a:?} not allowed now")))
        }
    }

    fn apply(&self, s: &mut State, a: Action) {
        match a {
            Action::Deal(c) => s.cards.push(c),
            other => {
                let r = s.round();
                s.rounds[r].push(other);
            }
        }
    }

    fn chance_outcomes(&self, s: &State, out: &mut Vec<(Action, f64)>) {
        out.clear();
        let n = DECK.len() - s.cards.len();
        let p = 1.0 / n as f64;
        out.extend(
            DECK.iter()
                .filter(|c| !s.cards.contains(c))
                .map(|&c| (Action::Deal(c), p)),
        );
    }

    fn payoffs(&self, s: &State) -> Vec<Chips> {
        let contrib = s.contributions();
        // Winner in deal order, or None for a split.
        let winner = if s.folded() {
            // The folder is whoever made the last action.
            let log = if s.rounds[1].is_empty() {
                &s.rounds[0]
            } else {
                &s.rounds[1]
            };
            let folder = (log.len() - 1) % 2;
            Some(1 - folder)
        } else {
            let board = s.cards[2].rank();
            let (a, b) = (s.cards[0].rank(), s.cards[1].rank());
            match (a == board, b == board) {
                (true, _) => Some(0),
                (_, true) => Some(1),
                _ if a > b => Some(0),
                _ if b > a => Some(1),
                _ => None,
            }
        };
        let mut pay = vec![Chips::ZERO; 2];
        if let Some(w) = winner {
            // The winner gains what the loser put in; the loser loses it.
            let loser = 1 - w;
            pay[s.seat_of(w).index()] = Chips(contrib[loser]);
            pay[s.seat_of(loser).index()] = Chips(-contrib[loser]);
        }
        pay
    }

    fn observe(&self, s: &State, seat: Seat) -> Observation {
        let me = s.order_of(seat);
        Observation {
            seat,
            button: s.setup.button,
            dealt: s.cards.len() as u8,
            own: s.cards.get(me).copied(),
            board: s.cards.get(2).copied(),
            rounds: s.rounds.clone(),
            revealed: if s.is_showdown() {
                Some(s.cards[1 - me])
            } else {
                None
            },
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
        let setup = HandSetup {
            button: obs.button,
            ..HandSetup::simple(2, Chips(100))
        };
        let me = if obs.seat == obs.button.next(2) { 0 } else { 1 };
        let opp_dealt = obs.dealt as usize > 1 - me;

        let opp_card = match (opp_dealt, obs.revealed) {
            (false, _) => None,
            (true, Some(c)) => Some(c),
            (true, None) => {
                let known: CardSet = obs.own.iter().chain(obs.board.iter()).copied().collect();
                let available: CardSet = DECK
                    .iter()
                    .copied()
                    .filter(|c| !known.contains(*c))
                    .collect();
                Some(sample_hidden_hands(sampler, &[(obs.seat.next(2), 1)], available, rng)[0][0])
            }
        };

        let mut deals = SmallVec::<[Action; 3]>::new();
        for i in 0..obs.dealt as usize {
            let card = match i {
                2 => obs.board,
                i if i == me => obs.own,
                _ => opp_card,
            };
            deals.push(Action::Deal(card.expect("dealt card missing")));
        }
        let actions = obs.rounds[0].iter().chain(obs.rounds[1].iter()).copied();
        replay_interleaved(self, &setup, deals, actions)
    }

    fn classify(&self, s: &State, a: Action) -> ActionClass {
        match a {
            Action::Deal(_) => ActionClass::Chance,
            Action::Fold => ActionClass::Fold,
            Action::CheckCall => {
                let facing = s.rounds[s.round()].last() == Some(&Action::Raise);
                ActionClass::CheckOrCall {
                    owed: Chips(if facing { BET_SIZE[s.round()] } else { 0 }),
                }
            }
            Action::Raise => {
                // Total this player will have in after the raise.
                let order = s.rounds[s.round()].len() % 2;
                let after = s.contributions()[1 - order] + BET_SIZE[s.round()];
                ActionClass::BetOrRaise { to: Chips(after) }
            }
        }
    }

    fn eval_unit(&self) -> Chips {
        Chips(1)
    }

    fn private_cards(&self, s: &State, seat: Seat) -> engine::CardVec {
        s.cards.get(s.order_of(seat)).copied().into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Action::*;
    use engine::replay::replay;
    use engine::testing::{Conformance, check_conformance};

    fn d(s: &str) -> Action {
        Deal(s.parse().unwrap())
    }

    /// Seat 0 acts first (button = seat 1).
    fn play(log: &[Action]) -> (Leduc, State) {
        let game = Leduc::new(Config {});
        let setup = HandSetup {
            button: Seat(1),
            ..HandSetup::simple(2, Chips(100))
        };
        let s = replay(&game, &setup, log).unwrap();
        (game, s)
    }

    #[test]
    fn conformance() {
        check_conformance(
            &Leduc::new(Config {}),
            &Conformance {
                hands: 300,
                ..Conformance::default()
            },
        );
    }

    #[test]
    fn check_check_check_check_high_card_wins_antes() {
        let (g, s) = play(&[
            d("Ks"),
            d("Js"),
            CheckCall,
            CheckCall,
            d("Qh"),
            CheckCall,
            CheckCall,
        ]);
        assert_eq!(g.to_act(&s), ToAct::Terminal);
        assert_eq!(g.payoffs(&s), vec![Chips(1), Chips(-1)]);
    }

    #[test]
    fn pair_with_board_beats_higher_card() {
        let (g, s) = play(&[
            d("Ks"),
            d("Js"),
            Raise,
            CheckCall,
            d("Jh"),
            Raise,
            CheckCall,
        ]);
        // Each put in 1 + 2 + 4 = 7. Seat 1 paired the board.
        assert_eq!(g.payoffs(&s), vec![Chips(-7), Chips(7)]);
    }

    #[test]
    fn same_rank_splits() {
        let (g, s) = play(&[
            d("Ks"),
            d("Kh"),
            Raise,
            Raise,
            CheckCall,
            d("Js"),
            CheckCall,
            CheckCall,
        ]);
        assert_eq!(g.payoffs(&s), vec![Chips(0), Chips(0)]);
    }

    #[test]
    fn raise_cap_and_fold() {
        let (g, s) = play(&[d("Ks"), d("Js"), Raise, Raise]);
        let mut legal = Vec::new();
        g.legal_actions(&s, &mut legal);
        assert_eq!(legal, vec![Fold, CheckCall], "cap of 2 raises reached");
        let (g, s) = play(&[d("Ks"), d("Js"), Raise, Raise, Fold]);
        // Seat 0 folds after putting in 1 + 2 = 3.
        assert_eq!(g.payoffs(&s), vec![Chips(-3), Chips(3)]);
    }

    #[test]
    fn fold_not_offered_when_checking_is_free() {
        let (g, s) = play(&[d("Ks"), d("Js")]);
        let mut legal = Vec::new();
        g.legal_actions(&s, &mut legal);
        assert_eq!(legal, vec![CheckCall, Raise]);
    }
}
