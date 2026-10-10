//! Generic conformance tests for game drivers: checks C1–C8 from `docs/api/game-driver.md` §6.
//!
//! Every driver's test module calls [`check_conformance`]. It plays random hands (random legal
//! actions, sampled chance) and asserts the API invariants at every step. A failure message
//! names the check, the hand, and the step, so you can replay it.
//!
//! This is public (not `#[cfg(test)]`) so other crates' tests can use it.

use std::collections::HashSet;

use rand::seq::IndexedRandom;

use crate::game::{Chips, Game, HandSetup, Seat, ToAct, UniformSampler};
use crate::replay::replay;
use crate::rng::{Stream, derive_rng};

/// Knobs for [`check_conformance`].
#[derive(Clone, Debug)]
pub struct Conformance {
    /// Random hands per seat count.
    pub hands: usize,
    /// Seat counts to test, e.g. `vec![2, 3, 6]`.
    pub seat_counts: Vec<u8>,
    /// Starting stack for every seat.
    pub stack: Chips,
    /// C8: a hand longer than this is assumed to loop forever.
    pub max_steps: usize,
    /// Determinizations per seat per decision point (C4, C6).
    pub determinizations: usize,
    pub seed: u64,
}

impl Default for Conformance {
    fn default() -> Conformance {
        Conformance {
            hands: 200,
            seat_counts: vec![2],
            stack: Chips(10_000),
            max_steps: 1_000,
            determinizations: 4,
            seed: 0xC0FFEE,
        }
    }
}

/// Run the checks. Panics with a descriptive message on the first violation.
pub fn check_conformance<G: Game>(game: &G, cfg: &Conformance) {
    for &n in &cfg.seat_counts {
        assert!(
            game.meta().seats.contains(&n),
            "seat count {n} outside the game's declared range {:?}",
            game.meta().seats
        );
        // varied[q][r]: while determinizing from seat q's view, did seat r's private view ever
        // change? If never, r's private info is probably leaking into q's observation (C4).
        let mut varied = vec![vec![false; n as usize]; n as usize];
        for hand in 0..cfg.hands {
            play_one(game, cfg, n, hand, &mut varied);
        }
        for (q, row) in varied.iter().enumerate() {
            for (r, &did_vary) in row.iter().enumerate() {
                assert!(
                    q == r || did_vary,
                    "C4 (no leaks), {n} seats: determinizing from s{q}'s observation never changed \
                     s{r}'s observation. s{r}'s hidden cards are probably visible to s{q}, or \
                     `determinize` ignores the sampler."
                );
            }
        }
    }
}

fn play_one<G: Game>(game: &G, cfg: &Conformance, n: u8, hand: usize, varied: &mut [Vec<bool>]) {
    let ctx = |step: usize| format!("[{n} seats, hand {hand}, step {step}]");
    let setup = HandSetup {
        num_seats: n,
        button: Seat((hand % n as usize) as u8),
        chooser: Seat((hand % n as usize) as u8),
        first_hand_of_game: hand.is_multiple_of(2),
        stacks: vec![cfg.stack; n as usize],
    };
    let mut deal_rng = derive_rng(cfg.seed, hand as u64, Stream::Deal);
    let mut play_rng = derive_rng(cfg.seed, hand as u64, Stream::Seat(0));
    let mut det_rng = derive_rng(cfg.seed, hand as u64, Stream::Custom(0));

    let mut state = game.new_hand(&setup);
    let mut log = Vec::new();
    let mut legal = Vec::new();
    let mut outcomes = Vec::new();
    // C5: each seat's observations at its own decisions along one path must all differ.
    let mut seen: Vec<HashSet<G::Observation>> = vec![HashSet::new(); n as usize];

    for step in 0.. {
        assert!(
            step <= cfg.max_steps,
            "C8 (termination) {}: exceeded max_steps",
            ctx(step)
        );
        let action = match game.to_act(&state) {
            ToAct::Terminal => break,
            ToAct::Chance => {
                game.chance_outcomes(&state, &mut outcomes);
                assert!(
                    !outcomes.is_empty(),
                    "C1 {}: chance node with no outcomes",
                    ctx(step)
                );
                assert!(
                    outcomes.iter().all(|&(_, p)| p >= 0.0),
                    "C1 {}: negative probability",
                    ctx(step)
                );
                let total: f64 = outcomes.iter().map(|&(_, p)| p).sum();
                assert!(
                    (total - 1.0).abs() < 1e-9,
                    "C1 {}: probabilities sum to {total}",
                    ctx(step)
                );
                let a = game.sample_chance(&state, &mut deal_rng);
                assert!(
                    outcomes.iter().any(|&(o, p)| o == a && p > 0.0),
                    "C1 {}: sample_chance returned {a:?}, which isn't a positive-probability outcome",
                    ctx(step)
                );
                a
            }
            ToAct::Seat(p) => {
                game.legal_actions(&state, &mut legal);
                assert!(
                    !legal.is_empty(),
                    "C2 {}: {p} to act with no legal actions",
                    ctx(step)
                );
                for &a in &legal {
                    if let Err(e) = game.validate(&state, a) {
                        panic!(
                            "C2 {}: legal_actions offered {a:?} but validate says {e}",
                            ctx(step)
                        );
                    }
                }
                let obs = game.observe(&state, p);
                assert_eq!(game.observer(&obs), p, "observer() disagrees {}", ctx(step));
                assert!(
                    seen[p.index()].insert(obs),
                    "C5 (perfect recall) {}: {p} got an identical observation at two of its \
                     decisions in one hand; the observation is missing history",
                    ctx(step)
                );
                check_determinize(
                    game,
                    cfg,
                    &state,
                    &legal,
                    n,
                    &mut det_rng,
                    varied,
                    &ctx(step),
                );
                *legal.choose(&mut play_rng).unwrap()
            }
        };
        game.apply(&mut state, action);
        log.push(action);
    }

    // C3: zero-sum, and nobody loses more than they brought.
    let payoffs = game.payoffs(&state);
    assert_eq!(
        payoffs.len(),
        n as usize,
        "C3 {}: payoffs length",
        ctx(log.len())
    );
    let sum: Chips = payoffs.iter().copied().sum();
    assert_eq!(
        sum,
        Chips::ZERO,
        "C3 {}: payoffs {payoffs:?} don't sum to zero",
        ctx(log.len())
    );
    for (i, &p) in payoffs.iter().enumerate() {
        assert!(
            p >= -cfg.stack,
            "C3 {}: s{i} lost {p}, more than its stack",
            ctx(log.len())
        );
    }

    // C7: replaying the log reproduces the hand exactly.
    let replayed =
        replay(game, &setup, &log).unwrap_or_else(|e| panic!("C7 {}: {e}", ctx(log.len())));
    assert_eq!(
        game.to_act(&replayed),
        ToAct::Terminal,
        "C7 {}: replay not terminal",
        ctx(log.len())
    );
    assert_eq!(
        game.payoffs(&replayed),
        payoffs,
        "C7 {}: replay payoffs differ",
        ctx(log.len())
    );
    for seat in setup.seats() {
        assert_eq!(
            game.observe(&replayed, seat),
            game.observe(&state, seat),
            "C7 {}: replayed observation differs for {seat}",
            ctx(log.len())
        );
    }
}

/// C6 (round-trip) and the data for C4 (leaks), at one decision point.
#[allow(clippy::too_many_arguments)]
fn check_determinize<G: Game>(
    game: &G,
    cfg: &Conformance,
    state: &G::State,
    legal: &[G::Action],
    n: u8,
    rng: &mut crate::rng::DetRng,
    varied: &mut [Vec<bool>],
    ctx: &str,
) {
    let mut legal2 = Vec::new();
    for q in (0..n).map(Seat) {
        let obs_q = game.observe(state, q);
        for _ in 0..cfg.determinizations {
            let s2 = game.determinize(&obs_q, &mut UniformSampler, rng);
            assert_eq!(
                game.observe(&s2, q),
                obs_q,
                "C6 (determinize round-trip) {ctx}: observe(determinize(obs), {q}) != obs"
            );
            // Who acts and what they may do is public: a determinization can't change it.
            assert_eq!(
                game.to_act(&s2),
                game.to_act(state),
                "C6 {ctx}: to_act changed"
            );
            game.legal_actions(&s2, &mut legal2);
            assert_eq!(
                legal2, legal,
                "C6 {ctx}: legal actions depend on hidden information"
            );
            for r in (0..n).map(Seat) {
                if r != q && game.observe(&s2, r) != game.observe(state, r) {
                    varied[q.index()][r.index()] = true;
                }
            }
        }
    }
}

/// Tests for the harness itself, using a toy game small enough to read in one sitting:
/// every seat antes 1, gets one card, then (in seat order) folds or stays. The highest card among
/// stayers takes the pot (ranks compared, then suits, so no ties); if everyone folds, antes return.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::{Card, CardSet};
    use crate::game::{ActionClass, CardVec, GameLength, GameMeta, IllegalAction, StakeModel};
    use crate::game::{HiddenSampler, sample_hidden_hands};
    use crate::rng::DetRng;
    use serde::{Deserialize, Serialize};

    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
    enum Act {
        Deal(Card),
        Fold,
        Stay,
    }

    #[derive(Clone, Debug)]
    struct St {
        n: u8,
        cards: Vec<Card>,
        decisions: Vec<(Seat, Act)>,
    }

    #[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
    struct Obs {
        seat: Seat,
        n: u8,
        own: Option<Card>,
        dealt: usize,
        decisions: Vec<(Seat, Act)>,
        /// Only the leaky variant fills this in.
        leaked: Vec<Card>,
    }

    /// `LEAK = true` puts every card in every observation: a bug C4 must catch.
    struct HighCard<const LEAK: bool> {
        meta: GameMeta,
    }

    impl<const LEAK: bool> Game for HighCard<LEAK> {
        type State = St;
        type Action = Act;
        type Observation = Obs;
        type Config = ();

        fn new(_: ()) -> Self {
            HighCard {
                meta: GameMeta {
                    id: "toy_high_card",
                    version: 1,
                    display_name: "Toy high card",
                    seats: 2..=4,
                    stake_model: StakeModel::Bankroll,
                    length: GameLength::SingleHand,
                    rules_ref: "engine::testing tests",
                },
            }
        }
        fn meta(&self) -> &GameMeta {
            &self.meta
        }
        fn new_hand(&self, setup: &HandSetup) -> St {
            St {
                n: setup.num_seats,
                cards: Vec::new(),
                decisions: Vec::new(),
            }
        }
        fn to_act(&self, s: &St) -> ToAct {
            if s.cards.len() < s.n as usize {
                ToAct::Chance
            } else if s.decisions.len() < s.n as usize {
                ToAct::Seat(Seat(s.decisions.len() as u8))
            } else {
                ToAct::Terminal
            }
        }
        fn legal_actions(&self, s: &St, out: &mut Vec<Act>) {
            out.clear();
            match self.to_act(s) {
                ToAct::Chance => {
                    let mut o = Vec::new();
                    self.chance_outcomes(s, &mut o);
                    out.extend(o.into_iter().map(|(a, _)| a));
                }
                ToAct::Seat(_) => out.extend([Act::Fold, Act::Stay]),
                ToAct::Terminal => {}
            }
        }
        fn validate(&self, s: &St, a: Act) -> Result<(), IllegalAction> {
            let ok = match (self.to_act(s), a) {
                (ToAct::Chance, Act::Deal(c)) => !s.cards.contains(&c),
                (ToAct::Seat(_), Act::Fold | Act::Stay) => true,
                _ => false,
            };
            if ok {
                Ok(())
            } else {
                Err(IllegalAction(format!("{a:?} not allowed now")))
            }
        }
        fn apply(&self, s: &mut St, a: Act) {
            match a {
                Act::Deal(c) => s.cards.push(c),
                _ => {
                    let seat = Seat(s.decisions.len() as u8);
                    s.decisions.push((seat, a));
                }
            }
        }
        fn chance_outcomes(&self, s: &St, out: &mut Vec<(Act, f64)>) {
            out.clear();
            let dealt: CardSet = s.cards.iter().copied().collect();
            let left = CardSet::FULL_DECK.difference(dealt);
            let p = 1.0 / left.len() as f64;
            out.extend(left.iter().map(|c| (Act::Deal(c), p)));
        }
        fn payoffs(&self, s: &St) -> Vec<Chips> {
            let n = s.n as usize;
            let stayers: Vec<usize> = s
                .decisions
                .iter()
                .filter(|(_, a)| *a == Act::Stay)
                .map(|(p, _)| p.index())
                .collect();
            let mut pay = vec![Chips::ZERO; n];
            let Some(&winner) = stayers
                .iter()
                .max_by_key(|&&i| (s.cards[i].rank(), s.cards[i].suit()))
            else {
                return pay; // everyone folded: antes returned
            };
            for (i, p) in pay.iter_mut().enumerate() {
                *p = if i == winner {
                    Chips(n as i64 - 1)
                } else {
                    Chips(-1)
                };
            }
            pay
        }
        fn observe(&self, s: &St, seat: Seat) -> Obs {
            Obs {
                seat,
                n: s.n,
                own: s.cards.get(seat.index()).copied(),
                dealt: s.cards.len(),
                decisions: s.decisions.clone(),
                leaked: if LEAK { s.cards.clone() } else { Vec::new() },
            }
        }
        fn observer(&self, obs: &Obs) -> Seat {
            obs.seat
        }
        fn determinize(&self, obs: &Obs, sampler: &mut dyn HiddenSampler, rng: &mut DetRng) -> St {
            let cards: Vec<Card> = if LEAK {
                obs.leaked.clone() // a leaky driver "knows" the answer
            } else {
                let mut known = CardSet::EMPTY;
                if let Some(c) = obs.own {
                    known.insert(c);
                }
                let others: Vec<(Seat, usize)> = (0..obs.dealt)
                    .map(|i| Seat(i as u8))
                    .filter(|&p| p != obs.seat)
                    .map(|p| (p, 1))
                    .collect();
                let hands: Vec<CardVec> = sample_hidden_hands(
                    sampler,
                    &others,
                    CardSet::FULL_DECK.difference(known),
                    rng,
                );
                let mut sampled = hands.into_iter();
                (0..obs.dealt)
                    .map(|i| {
                        if i == obs.seat.index() {
                            obs.own.unwrap()
                        } else {
                            sampled.next().unwrap()[0]
                        }
                    })
                    .collect()
            };
            St {
                n: obs.n,
                cards,
                decisions: obs.decisions.clone(),
            }
        }
        fn classify(&self, _: &St, a: Act) -> ActionClass {
            match a {
                Act::Deal(_) => ActionClass::Chance,
                Act::Fold => ActionClass::Fold,
                Act::Stay => ActionClass::CheckOrCall,
            }
        }
        fn eval_unit(&self) -> Chips {
            Chips(1)
        }
    }

    fn cfg() -> Conformance {
        Conformance {
            hands: 50,
            seat_counts: vec![2, 3, 4],
            ..Conformance::default()
        }
    }

    #[test]
    fn correct_toy_game_passes() {
        check_conformance(&HighCard::<false>::new(()), &cfg());
    }

    #[test]
    #[should_panic(expected = "C4")]
    fn leaky_toy_game_is_caught() {
        check_conformance(&HighCard::<true>::new(()), &cfg());
    }
}
