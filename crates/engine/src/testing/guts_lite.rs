//! Guts-lite: the smallest `UntilDone` game, a fixture for the API v2 carry machinery
//! (`docs/tech/game-catalog.md` §2, exit criterion of G0). Not a Bible game.
//!
//! - First hand only: every seat antes 1.
//! - Each seat dealt in gets one card, then in seat order says In or Out (public, in turn).
//! - Nobody In: the pot carries over unchanged.
//! - One In: that seat takes the pot and the game is over.
//! - Two or more In: the highest card (rank, then suit) takes the pot; every other In seat
//!   matches the pot, capped at [`MATCH_CAP`], into a new pot that carries to the next hand.
//! - `OUT_IS_OUT`: when somebody went In, the seats that said Out are out for the rest of the game.
//! - `BUG`: forgets the matched chips when carrying the pot (conformance must catch it).

use serde::{Deserialize, Serialize};

use crate::card::{Card, CardSet};
use crate::game::{
    ActionClass, CardVec, Chips, Game, GameLength, GameMeta, HandSetup, HiddenSampler,
    IllegalAction, Seat, StakeModel, ToAct, sample_hidden_hands,
};
use crate::rng::DetRng;
use crate::view::TableView;

/// The most an In seat that loses pays into the next pot.
pub const MATCH_CAP: i64 = 4;
/// Safety cap (D-G0-1): at this many hands the session splits the pot.
pub const MAX_HANDS: u32 = 12;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Act {
    Deal(Card),
    In,
    Out,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Carry {
    pub pot: i64,
    /// Seats out for the rest of the game (bitmask).
    pub out: u16,
    /// Hands played; 0 = the first hand (antes).
    pub hands: u32,
    pub over: bool,
}

#[derive(Clone, Debug)]
pub struct State {
    setup: HandSetup,
    carry: Carry,
    /// Seats dealt in, in deal and decision order.
    order: Vec<Seat>,
    /// `cards[k]` belongs to `order[k]`.
    cards: Vec<Card>,
    decisions: Vec<(Seat, Act)>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Obs {
    seat: Seat,
    setup: HandSetup,
    carry: Carry,
    own: Option<Card>,
    dealt: usize,
    decisions: Vec<(Seat, Act)>,
}

pub struct GutsLite<const OUT_IS_OUT: bool, const BUG: bool = false> {
    meta: GameMeta,
}

impl<const O: bool, const B: bool> GutsLite<O, B> {
    fn stayers(s: &State) -> Vec<usize> {
        s.decisions
            .iter()
            .filter(|(_, a)| *a == Act::In)
            .map(|(p, _)| s.order.iter().position(|q| q == p).unwrap())
            .collect()
    }

    /// (payoffs, carry_out). Only meaningful at Terminal.
    fn settle(&self, s: &State) -> (Vec<Chips>, Carry) {
        let n = s.setup.num_seats as usize;
        let mut pay = vec![0i64; n];
        let mut pot = s.carry.pot;
        if s.carry.hands == 0 {
            for p in &s.order {
                pay[p.index()] -= 1;
                pot += 1;
            }
        }
        let stayers = Self::stayers(s);
        let mut out = s.carry.out;
        let mut over = false;
        match stayers.len() {
            0 => {}
            1 => {
                pay[s.order[stayers[0]].index()] += pot;
                pot = 0;
                over = true;
            }
            _ => {
                let w = *stayers
                    .iter()
                    .max_by_key(|&&k| (s.cards[k].rank(), s.cards[k].suit()))
                    .unwrap();
                let won = pot;
                pay[s.order[w].index()] += won;
                pot = 0;
                for &k in stayers.iter().filter(|&&k| k != w) {
                    let m = won.min(MATCH_CAP);
                    pay[s.order[k].index()] -= m;
                    if !B {
                        pot += m; // BUG: the matched chips vanish instead
                    }
                }
            }
        }
        if O && !stayers.is_empty() {
            for (p, a) in &s.decisions {
                if *a == Act::Out {
                    out |= 1 << p.0;
                }
            }
        }
        let carry = Carry {
            pot,
            out,
            hands: s.carry.hands + 1,
            over,
        };
        (pay.into_iter().map(Chips).collect(), carry)
    }
}

impl<const O: bool, const B: bool> Game for GutsLite<O, B> {
    type State = State;
    type Action = Act;
    type Observation = Obs;
    type Config = ();
    type Carry = Carry;

    fn new(_: ()) -> Self {
        GutsLite {
            meta: GameMeta {
                id: "toy_guts_lite",
                version: 1,
                display_name: "Toy Guts-lite",
                seats: 2..=6,
                stake_model: StakeModel::Bankroll,
                length: GameLength::UntilDone {
                    max_hands: MAX_HANDS,
                },
                rules_ref: "engine::testing::guts_lite",
            },
        }
    }
    fn meta(&self) -> &GameMeta {
        &self.meta
    }
    fn new_hand_with(&self, setup: &HandSetup, carry: &Carry) -> State {
        State {
            setup: setup.clone(),
            carry: carry.clone(),
            order: setup.seats().filter(|&p| setup.dealt_in(p)).collect(),
            cards: Vec::new(),
            decisions: Vec::new(),
        }
    }
    fn carry_out(&self, s: &State) -> Carry {
        self.settle(s).1
    }
    fn game_over(&self, c: &Carry) -> bool {
        c.over
    }
    fn carried_chips(&self, c: &Carry) -> Chips {
        Chips(c.pot)
    }
    fn seats_out(&self, c: &Carry) -> u16 {
        c.out
    }
    fn to_act(&self, s: &State) -> ToAct {
        if s.cards.len() < s.order.len() {
            ToAct::Chance
        } else if s.decisions.len() < s.order.len() {
            ToAct::Seat(s.order[s.decisions.len()])
        } else {
            ToAct::Terminal
        }
    }
    fn legal_actions(&self, s: &State, out: &mut Vec<Act>) {
        out.clear();
        match self.to_act(s) {
            ToAct::Chance => {
                let mut o = Vec::new();
                self.chance_outcomes(s, &mut o);
                out.extend(o.into_iter().map(|(a, _)| a));
            }
            ToAct::Seat(_) => out.extend([Act::In, Act::Out]),
            ToAct::Terminal => {}
        }
    }
    fn validate(&self, s: &State, a: Act) -> Result<(), IllegalAction> {
        let ok = match (self.to_act(s), a) {
            (ToAct::Chance, Act::Deal(c)) => !s.cards.contains(&c),
            (ToAct::Seat(_), Act::In | Act::Out) => true,
            _ => false,
        };
        if ok {
            Ok(())
        } else {
            Err(IllegalAction(format!("{a:?} not allowed now")))
        }
    }
    fn apply(&self, s: &mut State, a: Act) {
        match a {
            Act::Deal(c) => s.cards.push(c),
            _ => {
                let p = s.order[s.decisions.len()];
                s.decisions.push((p, a));
            }
        }
    }
    fn chance_outcomes(&self, s: &State, out: &mut Vec<(Act, f64)>) {
        out.clear();
        let dealt: CardSet = s.cards.iter().copied().collect();
        let left = CardSet::FULL_DECK.difference(dealt);
        let p = 1.0 / left.len() as f64;
        out.extend(left.iter().map(|c| (Act::Deal(c), p)));
    }
    fn payoffs(&self, s: &State) -> Vec<Chips> {
        self.settle(s).0
    }
    fn observe(&self, s: &State, seat: Seat) -> Obs {
        Obs {
            seat,
            setup: s.setup.clone(),
            carry: s.carry.clone(),
            own: s
                .order
                .iter()
                .position(|&p| p == seat)
                .and_then(|k| s.cards.get(k).copied()),
            dealt: s.cards.len(),
            decisions: s.decisions.clone(),
        }
    }
    fn observer(&self, obs: &Obs) -> Seat {
        obs.seat
    }
    fn determinize(&self, obs: &Obs, sampler: &mut dyn HiddenSampler, rng: &mut DetRng) -> State {
        let mut s = self.new_hand_with(&obs.setup, &obs.carry);
        s.decisions = obs.decisions.clone();
        let mut known = CardSet::EMPTY;
        if let Some(c) = obs.own {
            known.insert(c);
        }
        let others: Vec<(Seat, usize)> = s.order[..obs.dealt]
            .iter()
            .filter(|&&p| p != obs.seat)
            .map(|&p| (p, 1))
            .collect();
        let hands: Vec<CardVec> =
            sample_hidden_hands(sampler, &others, CardSet::FULL_DECK.difference(known), rng);
        let mut sampled = hands.into_iter();
        s.cards = s.order[..obs.dealt]
            .iter()
            .map(|&p| {
                if p == obs.seat {
                    obs.own.unwrap()
                } else {
                    sampled.next().unwrap()[0]
                }
            })
            .collect();
        s
    }
    fn classify(&self, _: &State, a: Act) -> ActionClass {
        match a {
            Act::Deal(_) => ActionClass::Chance,
            Act::Out => ActionClass::Fold,
            Act::In => ActionClass::CheckOrCall { owed: Chips::ZERO },
        }
    }
    fn eval_unit(&self) -> Chips {
        Chips(1)
    }
    fn view(&self, obs: &Obs) -> TableView {
        TableView {
            game: self.meta.display_name.into(),
            phase: String::new(),
            viewer: obs.seat,
            button: obs.setup.button,
            to_act: None,
            seats: Vec::new(),
            boards: Vec::new(),
            pots: Vec::new(),
            wilds: Vec::new(),
            notes: vec![format!("pot {}", obs.carry.pot)],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Conformance, check_conformance};

    fn cfg() -> Conformance {
        Conformance {
            hands: 300,
            seat_counts: vec![2, 3, 5],
            ..Conformance::default()
        }
    }

    #[test]
    fn guts_lite_passes_conformance_with_carry() {
        check_conformance(&GutsLite::<false>::new(()), &cfg());
    }

    #[test]
    fn guts_lite_out_is_out_passes_conformance() {
        check_conformance(&GutsLite::<true>::new(()), &cfg());
    }

    #[test]
    #[should_panic(expected = "C3")]
    fn lost_carried_chips_are_caught() {
        check_conformance(&GutsLite::<false, true>::new(()), &cfg());
    }
}
