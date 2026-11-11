//! Abstracted heads-up No-Limit Hold'em for MCCFR blueprints (PRD FR-SP-7, backlog #7,
//! `docs/tech/sim-program.md` §4.4). Training only: not offered in menus.
//!
//! The game plays exactly like `holdem_nl` heads-up (same rules, same bot bet menu, FR-BT-7); only
//! the **observation** is coarser, so MCCFR's table stays small:
//! - cards are replaced by a strength **bucket** per street (0 = weakest … 7): preflop from the
//!   Chen score, postflop from a Monte Carlo hand-strength estimate against one random hand
//!   (deterministic per card combination, so equal cards always share a bucket);
//! - the board's cards are dropped (their effect is in the bucket); the betting history is kept;
//! - actions are abstracted too: bets are pot-size or all-in only, and at most
//!   [`MAX_RAISES`] bets/raises per street (without this the betting histories alone run to
//!   millions of information sets within a few thousand iterations).
//!
//! `determinize` samples deals whose buckets match the observer's (rejection sampling), which is
//! enough for evaluating blueprints against search bots; it isn't an exact inverse of `observe`.

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use engine::betting::BetAction;
use engine::eval::eval_high;
use engine::game::sample_cards;
use engine::pot::order_from_button;
use engine::replay::replay_interleaved;
use engine::rng::rng_from_seed;
use engine::{
    ActionClass, Card, CardSet, Chips, DetRng, Game, GameLength, GameMeta, HandSetup,
    HiddenSampler, IllegalAction, Seat, StakeModel, TableView, ToAct,
};

use crate::analysis_util::chen_score;
use crate::holdem::{self, Holdem, PublicAction};

/// Buckets per street.
pub const BUCKETS: u8 = 8;
/// Monte Carlo samples per postflop bucket.
const SAMPLES: usize = 32;
/// Bets/raises allowed per street in the abstraction.
pub const MAX_RAISES: usize = 2;

pub struct AbstractHoldem {
    inner: Holdem,
    meta: GameMeta,
}

#[derive(Clone, Debug)]
pub struct State {
    pub inner: holdem::State,
    /// `buckets[seat][street]`, 255 = not yet known.
    buckets: [[u8; 4]; 2],
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Observation {
    pub seat: Seat,
    pub button: Seat,
    /// This seat's bucket on each street reached so far.
    pub buckets: SmallVec<[u8; 4]>,
    /// (street, seat, bet) for every betting action so far.
    pub bets: SmallVec<[(u8, Seat, BetAction); 24]>,
}

/// Strength bucket of `hole` on `board` (0..BUCKETS).
pub fn bucket(hole: [Card; 2], board: &[Card]) -> u8 {
    if board.is_empty() {
        // Chen score runs from about -1 (72o) to 20 (AA).
        let c = chen_score(hole[0], hole[1]);
        return match c {
            i32::MIN..=2 => 0,
            3..=4 => 1,
            5..=6 => 2,
            7..=8 => 3,
            9..=10 => 4,
            11..=12 => 5,
            13..=15 => 6,
            _ => 7,
        };
    }
    let mut used: CardSet = board.iter().copied().collect();
    used.insert(hole[0]);
    used.insert(hole[1]);
    let left = CardSet::FULL_DECK.difference(used);
    // Seed from the cards themselves, so the same cards always get the same bucket.
    let seed = hole.iter().chain(board).fold(0x9E37_79B9u64, |h, c| {
        h.wrapping_mul(1_000_003) ^ c.index() as u64
    });
    let mut rng = rng_from_seed(seed);
    let need = 5 - board.len();
    let (mut score, mut n) = (0.0, 0.0);
    for _ in 0..SAMPLES {
        let draw = sample_cards(left, 2 + need, &mut rng);
        let runout: CardSet = board
            .iter()
            .copied()
            .chain(draw[2..].iter().copied())
            .collect();
        let mine = eval_high(runout.union([hole[0], hole[1]].into_iter().collect()));
        let theirs = eval_high(runout.union([draw[0], draw[1]].into_iter().collect()));
        score += if mine > theirs {
            1.0
        } else if mine == theirs {
            0.5
        } else {
            0.0
        };
        n += 1.0;
    }
    ((score / n * BUCKETS as f64) as u8).min(BUCKETS - 1)
}

fn street_of(board_len: usize) -> usize {
    match board_len {
        0 => 0,
        3 => 1,
        4 => 2,
        _ => 3,
    }
}

impl AbstractHoldem {
    /// Bets and raises made on the current street.
    fn raises_this_street(&self, s: &State) -> usize {
        let o = self.inner.observe(&s.inner, Seat(0));
        let mut n = 0;
        for e in &o.history {
            match e {
                PublicAction::Board(_) => n = 0,
                PublicAction::Bet(_, BetAction::RaiseTo(_)) => n += 1,
                _ => {}
            }
        }
        n
    }

    fn update_buckets(&self, s: &mut State) {
        for p in 0..2u8 {
            let obs = self.inner.observe(&s.inner, Seat(p));
            if obs.hole.len() < 2 {
                continue;
            }
            let board = obs.board();
            let st = street_of(board.len());
            if board.len() != 1 && board.len() != 2 && s.buckets[p as usize][st] == 255 {
                s.buckets[p as usize][st] = bucket([obs.hole[0], obs.hole[1]], &board);
            }
        }
    }
}

impl Game for AbstractHoldem {
    type State = State;
    type Action = holdem::Action;
    type Observation = Observation;
    type Config = holdem::Config;
    type Carry = ();

    fn new(mut config: holdem::Config) -> AbstractHoldem {
        // Action abstraction: pot-size bets and all-in only.
        config.bet_menu = vec![
            engine::betting::BetSize::PotFraction(1, 1),
            engine::betting::BetSize::AllIn,
        ];
        AbstractHoldem {
            inner: Holdem::new(config),
            meta: GameMeta {
                id: "holdem_hu_abstract",
                version: 1,
                display_name: "Heads-up Hold'em (abstracted, training only)",
                seats: 2..=2,
                stake_model: StakeModel::Bankroll,
                length: GameLength::SingleHand,
                rules_ref: "games/src/holdem_abstract.rs",
            },
        }
    }

    fn meta(&self) -> &GameMeta {
        &self.meta
    }

    fn new_hand_with(&self, setup: &HandSetup, _carry: &()) -> State {
        let mut s = State {
            inner: self.inner.new_hand(setup),
            buckets: [[255; 4]; 2],
        };
        self.update_buckets(&mut s);
        s
    }

    fn to_act(&self, s: &State) -> ToAct {
        self.inner.to_act(&s.inner)
    }

    fn legal_actions(&self, s: &State, out: &mut Vec<holdem::Action>) {
        self.inner.legal_actions(&s.inner, out);
        if self.raises_this_street(s) >= MAX_RAISES {
            out.retain(|a| !matches!(a, holdem::Action::RaiseTo(_)));
        }
    }

    fn validate(&self, s: &State, a: holdem::Action) -> Result<(), IllegalAction> {
        if matches!(a, holdem::Action::RaiseTo(_)) && self.raises_this_street(s) >= MAX_RAISES {
            return Err(IllegalAction(format!(
                "at most {MAX_RAISES} bets per street in the abstraction"
            )));
        }
        self.inner.validate(&s.inner, a)
    }

    fn apply(&self, s: &mut State, a: holdem::Action) {
        self.inner.apply(&mut s.inner, a);
        self.update_buckets(s);
    }

    fn chance_outcomes(&self, s: &State, out: &mut Vec<(holdem::Action, f64)>) {
        self.inner.chance_outcomes(&s.inner, out)
    }

    fn sample_chance(&self, s: &State, rng: &mut DetRng) -> holdem::Action {
        self.inner.sample_chance(&s.inner, rng)
    }

    fn payoffs(&self, s: &State) -> Vec<Chips> {
        self.inner.payoffs(&s.inner)
    }

    fn observe(&self, s: &State, seat: Seat) -> Observation {
        let o = self.inner.observe(&s.inner, seat);
        let mut street = 0u8;
        let mut board = 0usize;
        let mut bets = SmallVec::new();
        for e in &o.history {
            match *e {
                PublicAction::Board(_) => {
                    board += 1;
                    street = street_of(board) as u8;
                }
                PublicAction::Bet(p, b) => bets.push((street, p, b)),
                PublicAction::HoleDealt => {}
            }
        }
        let reached = street_of(o.board().len());
        Observation {
            seat,
            button: o.setup.button,
            buckets: (0..=reached)
                .map(|k| s.buckets[seat.index()][k])
                .filter(|&b| b != 255)
                .collect(),
            bets,
        }
    }

    fn observer(&self, obs: &Observation) -> Seat {
        obs.seat
    }

    fn determinize(
        &self,
        obs: &Observation,
        _sampler: &mut dyn HiddenSampler,
        rng: &mut DetRng,
    ) -> State {
        let setup = HandSetup {
            button: obs.button,
            chooser: obs.button,
            ..HandSetup::simple(2, Chips(10_000))
        };
        let boards_needed = match obs
            .bets
            .last()
            .map(|b| b.0)
            .unwrap_or(0)
            .max(obs.buckets.len().saturating_sub(1) as u8)
        {
            0 => 0,
            1 => 3,
            2 => 4,
            _ => 5,
        };
        // Rejection-sample a deal whose buckets match the observer's.
        let mut best = None;
        for _ in 0..300 {
            let cards = sample_cards(CardSet::FULL_DECK, 4 + boards_needed, rng);
            let me = obs.seat.index();
            let hole = [[cards[0], cards[1]], [cards[2], cards[3]]];
            let board = &cards[4..];
            let ok = obs.buckets.iter().enumerate().all(|(st, &b)| {
                let len = [0, 3, 4, 5][st].min(board.len());
                bucket(hole[me], &board[..len]) == b
            });
            best = Some(cards.clone());
            if ok {
                break;
            }
        }
        let cards = best.unwrap();
        // Hole cards go one at a time from the button's left, two rounds; then the board.
        let order = order_from_button(setup.button, 2);
        let mut deals = Vec::new();
        for round in 0..2 {
            for p in &order {
                deals.push(holdem::Action::Deal(cards[2 * p.index() + round]));
            }
        }
        deals.extend(cards[4..].iter().map(|&c| holdem::Action::Deal(c)));
        let actions: Vec<holdem::Action> = obs
            .bets
            .iter()
            .map(|&(_, _, b)| match b {
                BetAction::Fold => holdem::Action::Fold,
                BetAction::CheckCall => holdem::Action::CheckCall,
                BetAction::RaiseTo(x) => holdem::Action::RaiseTo(x),
            })
            .collect();
        replay_interleaved(self, &setup, deals, actions)
    }

    fn classify(&self, s: &State, a: holdem::Action) -> ActionClass {
        self.inner.classify(&s.inner, a)
    }

    fn eval_unit(&self) -> Chips {
        self.inner.eval_unit()
    }

    fn view(&self, obs: &Observation) -> TableView {
        let s = self.determinize(obs, &mut engine::UniformSampler, &mut rng_from_seed(0));
        self.inner.view(&self.inner.observe(&s.inner, obs.seat))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(s: &str) -> Card {
        s.parse().unwrap()
    }

    #[test]
    fn buckets_order_hands_sensibly() {
        assert_eq!(bucket([card("As"), card("Ad")], &[]), 7);
        assert!(bucket([card("7c"), card("2d")], &[]) <= 1);
        let board = [card("Ah"), card("Kd"), card("7c")];
        assert!(
            bucket([card("As"), card("Ac")], &board) > bucket([card("3s"), card("2c")], &board)
        );
        // Deterministic.
        assert_eq!(
            bucket([card("Qs"), card("Jc")], &board),
            bucket([card("Qs"), card("Jc")], &board)
        );
    }

    #[test]
    fn observations_hide_cards_but_keep_betting() {
        let g = AbstractHoldem::new(holdem::Config::default());
        let setup = HandSetup::simple(2, Chips(10_000));
        let mut s = g.new_hand(&setup);
        let mut rng = rng_from_seed(1);
        while g.to_act(&s) == ToAct::Chance {
            let a = g.sample_chance(&s, &mut rng);
            g.apply(&mut s, a);
        }
        let o = g.observe(&s, Seat(0));
        assert_eq!(o.buckets.len(), 1, "preflop bucket only");
        assert!(o.bets.is_empty());
        assert!(matches!(g.to_act(&s), ToAct::Seat(_)));
        g.apply(&mut s, holdem::Action::CheckCall);
        assert_eq!(g.observe(&s, Seat(0)).bets.len(), 1);
    }
}
