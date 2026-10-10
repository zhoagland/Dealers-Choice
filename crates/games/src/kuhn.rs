//! Kuhn poker: the smallest interesting poker game, with a known equilibrium.
//!
//! Rules: 3 cards (J < Q < K), 2 players, each antes 1 and gets one card. The first player
//! checks or bets 1. After a check, the second player checks (showdown) or bets. Facing a bet,
//! a player folds or calls (showdown). The higher card wins the pot.
//!
//! Why it's here (PRD §7): its Nash equilibrium is known in closed form, so it's the test that
//! can fail for MCCFR and best response. The first player's game value is −1/18. Equilibria form a
//! one-parameter family: bet J with probability α ∈ [0, ⅓], bet K with 3α, always check Q.
//!
//! "First player" here is the seat left of the button. Kuhn ignores stacks (max loss is 2).

use engine::game::sample_hidden_hands;
use engine::replay::replay_interleaved;
use engine::{
    ActionClass, Card, CardSet, Chips, DetRng, Game, GameLength, GameMeta, HandSetup,
    HiddenSampler, IllegalAction, Rank, Seat, StakeModel, Suit, ToAct,
};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

/// The three cards. Suit is arbitrary; only rank matters.
pub const DECK: [Card; 3] = [
    Card::new(Rank::Jack, Suit::Spades),
    Card::new(Rank::Queen, Suit::Spades),
    Card::new(Rank::King, Suit::Spades),
];

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Action {
    Deal(Card),
    Check,
    Bet,
    Call,
    Fold,
}

/// Kuhn has nothing to configure. An empty struct (rather than `()`) so it can be read from an
/// empty TOML table like every other game's config.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Config {}

#[derive(Clone, Debug)]
pub struct State {
    setup: HandSetup,
    /// Cards in deal order: `[first player, second player]`.
    cards: SmallVec<[Card; 2]>,
    /// Player actions only (deals are in `cards`). At most 3.
    history: SmallVec<[Action; 3]>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct Observation {
    pub seat: Seat,
    pub button: Seat,
    /// How many cards have been dealt (0–2). Needed so `determinize` knows where "now" is.
    pub dealt: u8,
    pub own: Option<Card>,
    pub history: SmallVec<[Action; 3]>,
    /// The opponent's card, visible only after a showdown.
    pub revealed: Option<Card>,
}

pub struct Kuhn {
    meta: GameMeta,
}

impl State {
    fn first(&self) -> Seat {
        self.setup.button.next(2)
    }

    /// The seat dealt the i-th card (and acting i-th in the betting).
    fn seat_of(&self, i: usize) -> Seat {
        if i == 0 {
            self.first()
        } else {
            self.first().next(2)
        }
    }

    /// Index into `cards` for a seat.
    fn order_of(&self, seat: Seat) -> usize {
        if seat == self.first() { 0 } else { 1 }
    }

    fn is_showdown(&self) -> bool {
        use Action::*;
        matches!(
            self.history.as_slice(),
            [Check, Check] | [Bet, Call] | [Check, Bet, Call]
        )
    }

    fn is_over(&self) -> bool {
        use Action::*;
        self.is_showdown() || matches!(self.history.as_slice(), [Bet, Fold] | [Check, Bet, Fold])
    }

    /// The player actions legal now, as a fixed slice: Kuhn's betting is so small it's easiest
    /// to just spell it out.
    fn player_actions(&self) -> &'static [Action] {
        use Action::*;
        match self.history.as_slice() {
            [] | [Check] => &[Check, Bet],
            [Bet] | [Check, Bet] => &[Fold, Call],
            _ => &[],
        }
    }
}

impl Game for Kuhn {
    type State = State;
    type Action = Action;
    type Observation = Observation;
    type Config = Config;

    fn new(_config: Config) -> Kuhn {
        Kuhn {
            meta: GameMeta {
                id: "kuhn",
                version: 1,
                display_name: "Kuhn poker",
                seats: 2..=2,
                stake_model: StakeModel::Bankroll,
                length: GameLength::SingleHand,
                rules_ref: "Kuhn (1950); docs/tech/games.md §Kuhn",
            },
        }
    }

    fn meta(&self) -> &GameMeta {
        &self.meta
    }

    fn new_hand(&self, setup: &HandSetup) -> State {
        assert_eq!(setup.num_seats, 2, "Kuhn is heads-up only");
        State {
            setup: setup.clone(),
            cards: SmallVec::new(),
            history: SmallVec::new(),
        }
    }

    fn to_act(&self, s: &State) -> ToAct {
        if s.cards.len() < 2 {
            ToAct::Chance
        } else if s.is_over() {
            ToAct::Terminal
        } else {
            // Players alternate, so the actor is decided by how many actions so far.
            ToAct::Seat(s.seat_of(s.history.len() % 2))
        }
    }

    fn legal_actions(&self, s: &State, out: &mut Vec<Action>) {
        out.clear();
        match self.to_act(s) {
            ToAct::Chance => out.extend(
                DECK.iter()
                    .filter(|c| !s.cards.contains(c))
                    .map(|&c| Action::Deal(c)),
            ),
            ToAct::Seat(_) => out.extend_from_slice(s.player_actions()),
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
            Err(IllegalAction(format!(
                "{a:?} not allowed; legal: {:?}",
                s.player_actions()
            )))
        }
    }

    fn apply(&self, s: &mut State, a: Action) {
        match a {
            Action::Deal(c) => s.cards.push(c),
            other => s.history.push(other),
        }
    }

    fn chance_outcomes(&self, s: &State, out: &mut Vec<(Action, f64)>) {
        out.clear();
        let left: SmallVec<[Card; 3]> = DECK
            .iter()
            .copied()
            .filter(|c| !s.cards.contains(c))
            .collect();
        let p = 1.0 / left.len() as f64;
        out.extend(left.iter().map(|&c| (Action::Deal(c), p)));
    }

    fn payoffs(&self, s: &State) -> Vec<Chips> {
        use Action::*;
        // Amount the loser loses: ante 1, plus 1 more if a bet was called.
        let (winner_order, amount) = match s.history.as_slice() {
            [Bet, Fold] => (0, 1),
            [Check, Bet, Fold] => (1, 1),
            [Check, Check] => (showdown_winner(s), 1),
            [Bet, Call] | [Check, Bet, Call] => (showdown_winner(s), 2),
            h => panic!("payoffs called on non-terminal history {h:?}"),
        };
        let winner = s.seat_of(winner_order);
        let mut pay = vec![Chips(-amount); 2];
        pay[winner.index()] = Chips(amount);
        pay
    }

    fn observe(&self, s: &State, seat: Seat) -> Observation {
        let me = s.order_of(seat);
        Observation {
            seat,
            button: s.setup.button,
            dealt: s.cards.len() as u8,
            own: s.cards.get(me).copied(),
            history: s.history.clone(),
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
        let first = obs.button.next(2);
        let me = if obs.seat == first { 0 } else { 1 };
        let opp = obs.seat.next(2);

        // The opponent's card: revealed if there was a showdown, otherwise sampled.
        let opp_card = if obs.dealt as usize > 1 - me {
            Some(match obs.revealed {
                Some(c) => c,
                None => {
                    let available = DECK
                        .iter()
                        .copied()
                        .filter(|&c| Some(c) != obs.own)
                        .collect::<CardSet>();
                    sample_hidden_hands(sampler, &[(opp, 1)], available, rng)[0][0]
                }
            })
        } else {
            None
        };

        let mut deals = SmallVec::<[Action; 2]>::new();
        for i in 0..obs.dealt as usize {
            let card = if i == me { obs.own } else { opp_card };
            deals.push(Action::Deal(card.expect("dealt card missing")));
        }
        replay_interleaved(self, &setup, deals, obs.history.iter().copied())
    }

    fn classify(&self, _s: &State, a: Action) -> ActionClass {
        match a {
            Action::Deal(_) => ActionClass::Chance,
            Action::Check | Action::Call => ActionClass::CheckOrCall,
            Action::Bet => ActionClass::BetOrRaise { to: Chips(2) },
            Action::Fold => ActionClass::Fold,
        }
    }

    fn eval_unit(&self) -> Chips {
        Chips(1)
    }

    fn describe_observation(&self, obs: &Observation) -> String {
        let card = obs
            .own
            .map(|c| c.rank().to_char().to_string())
            .unwrap_or("?".into());
        let hist: Vec<String> = obs
            .history
            .iter()
            .map(|a| self.describe_action(*a))
            .collect();
        let mut s = format!(
            "You ({}): {card}   history: [{}]",
            obs.seat,
            hist.join(", ")
        );
        if let Some(c) = obs.revealed {
            s += &format!("   opponent shows {}", c.rank().to_char());
        }
        s
    }

    fn describe_action(&self, a: Action) -> String {
        match a {
            Action::Deal(c) => format!("deal {}", c.rank().to_char()),
            other => format!("{other:?}").to_lowercase(),
        }
    }

    fn parse_action(&self, input: &str, legal: &[Action]) -> Option<Action> {
        let want = input.trim().to_lowercase();
        legal
            .iter()
            .copied()
            .find(|&a| self.describe_action(a).starts_with(&want) && !want.is_empty())
    }
}

/// 0 if the first player's card is higher, else 1.
fn showdown_winner(s: &State) -> usize {
    if s.cards[0].rank() > s.cards[1].rank() {
        0
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::replay::replay;
    use engine::testing::{Conformance, check_conformance};

    fn card(s: &str) -> Card {
        s.parse().unwrap()
    }

    fn play(first: &str, second: &str, actions: &[Action]) -> Vec<Chips> {
        let game = Kuhn::new(Config {});
        // Button = seat 1, so seat 0 acts first and gets the first card.
        let setup = HandSetup {
            button: Seat(1),
            ..HandSetup::simple(2, Chips(100))
        };
        let mut log = vec![Action::Deal(card(first)), Action::Deal(card(second))];
        log.extend_from_slice(actions);
        let s = replay(&game, &setup, &log).unwrap();
        assert_eq!(game.to_act(&s), ToAct::Terminal);
        game.payoffs(&s)
    }

    #[test]
    fn conformance() {
        check_conformance(
            &Kuhn::new(Config {}),
            &Conformance {
                hands: 300,
                ..Conformance::default()
            },
        );
    }

    #[test]
    fn payoffs_for_every_terminal_history() {
        use Action::*;
        assert_eq!(play("Ks", "Js", &[Check, Check]), vec![Chips(1), Chips(-1)]);
        assert_eq!(play("Js", "Ks", &[Check, Check]), vec![Chips(-1), Chips(1)]);
        assert_eq!(play("Js", "Ks", &[Bet, Fold]), vec![Chips(1), Chips(-1)]);
        assert_eq!(play("Js", "Ks", &[Bet, Call]), vec![Chips(-2), Chips(2)]);
        assert_eq!(
            play("Ks", "Qs", &[Check, Bet, Fold]),
            vec![Chips(-1), Chips(1)]
        );
        assert_eq!(
            play("Ks", "Qs", &[Check, Bet, Call]),
            vec![Chips(2), Chips(-2)]
        );
    }

    #[test]
    fn opponent_card_hidden_until_showdown() {
        use Action::*;
        let game = Kuhn::new(Config {});
        let setup = HandSetup {
            button: Seat(1),
            ..HandSetup::simple(2, Chips(100))
        };
        let s = replay(&game, &setup, &[Deal(card("Ks")), Deal(card("Js")), Bet]).unwrap();
        assert_eq!(game.observe(&s, Seat(1)).revealed, None);
        let s = replay(
            &game,
            &setup,
            &[Deal(card("Ks")), Deal(card("Js")), Bet, Fold],
        )
        .unwrap();
        assert_eq!(
            game.observe(&s, Seat(1)).revealed,
            None,
            "no showdown on a fold"
        );
        let s = replay(
            &game,
            &setup,
            &[Deal(card("Ks")), Deal(card("Js")), Bet, Call],
        )
        .unwrap();
        assert_eq!(game.observe(&s, Seat(1)).revealed, Some(card("Ks")));
    }

    #[test]
    fn illegal_actions_rejected() {
        use Action::*;
        let game = Kuhn::new(Config {});
        let setup = HandSetup::simple(2, Chips(100));
        assert!(
            replay(&game, &setup, &[Deal(card("Ks")), Deal(card("Ks"))]).is_err(),
            "same card twice"
        );
        assert!(
            replay(&game, &setup, &[Deal(card("As"))]).is_err(),
            "not a Kuhn card"
        );
        assert!(replay(&game, &setup, &[Deal(card("Ks")), Deal(card("Js")), Call]).is_err());
    }
}
