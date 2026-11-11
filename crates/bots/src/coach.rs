//! Coach mode: score a player's decisions by EV lost (PRD FR-EV-3/4/5, `docs/tech/bots.md` §7).
//!
//! For each of the player's decisions in a finished hand: rebuild what they saw, evaluate every
//! legal action with an [`Evaluator`] in evaluation mode, and report
//! `EV_loss = Q(best) − Q(chosen)` with a standard error. A decision is **flagged** only when the
//! loss exceeds two standard errors, so search noise isn't reported as a mistake.
//!
//! Caveat (OQ-7): Q is the value *against the evaluator's own model of the opponents*. For
//! ISMCTS that means opponents who play like ISMCTS. It is not a GTO verdict.

use engine::{DetRng, Game, HandSetup, Seat, ToAct};

use crate::policy::{ActionValue, Budget, Evaluator};

/// One reviewed decision.
#[derive(Clone, Debug)]
pub struct DecisionReview<A> {
    /// Index of the decision in the hand's action log.
    pub step: usize,
    pub chosen: A,
    /// Every menu action, best first.
    pub values: Vec<ActionValue<A>>,
    /// `None` when the chosen action wasn't on the bot menu (an off-menu bet size), so it has
    /// no estimate of its own.
    pub ev_loss: Option<f64>,
    pub se: f64,
    pub flagged: bool,
}

/// Review `seat`'s decisions in a recorded hand.
pub fn review_hand<G: Game>(
    game: &G,
    setup: &HandSetup,
    actions: &[G::Action],
    seat: Seat,
    evaluator: &mut dyn Evaluator<G>,
    budget: Budget,
    rng: &mut DetRng,
) -> Vec<DecisionReview<G::Action>> {
    let mut state = game.new_hand(setup);
    let mut legal = Vec::new();
    let mut out = Vec::new();
    for (step, &a) in actions.iter().enumerate() {
        if game.to_act(&state) == ToAct::Seat(seat) {
            game.legal_actions(&state, &mut legal);
            let obs = game.observe(&state, seat);
            let values = evaluator.evaluate(game, &obs, &legal, budget, rng);
            out.push(score(step, a, values));
        }
        game.apply(&mut state, a);
    }
    out
}

/// Score a single decision from its evaluated values (used for live hints too).
pub fn score<A: Copy + PartialEq>(
    step: usize,
    chosen: A,
    values: Vec<ActionValue<A>>,
) -> DecisionReview<A> {
    let best = values[0].clone();
    let (ev_loss, se) = match values.iter().find(|v| v.action == chosen) {
        Some(c) => (Some(best.q - c.q), (best.se.powi(2) + c.se.powi(2)).sqrt()),
        None => (None, best.se),
    };
    let flagged = ev_loss.is_some_and(|l| l > 2.0 * se);
    DecisionReview {
        step,
        chosen,
        values,
        ev_loss,
        se,
        flagged,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ismcts::{Ismcts, IsmctsConfig};
    use engine::rng::rng_from_seed;
    use engine::{Chips, Game, HandSetup};
    use games::kuhn::{Action, Config, Kuhn};

    #[test]
    fn calling_with_a_jack_is_flagged_folding_is_not() {
        use Action::*;
        let game = Kuhn::new(Config {});
        let setup = HandSetup {
            button: Seat(1),
            ..HandSetup::simple(2, Chips(100))
        };
        let j = Deal("Js".parse().unwrap());
        let k = Deal("Ks".parse().unwrap());
        let mut search = Ismcts::<Kuhn>::new(IsmctsConfig::default());
        let mut rng = rng_from_seed(1);

        // Seat 0: J; checks, faces a bet, CALLS (always loses 2 instead of 1).
        let bad = review_hand(
            &game,
            &setup,
            &[j, k, Check, Bet, Call],
            Seat(0),
            &mut search,
            Budget::Playouts(2_000),
            &mut rng,
        );
        assert_eq!(bad.len(), 2);
        let call = &bad[1];
        assert_eq!(call.chosen, Call);
        assert!((call.ev_loss.unwrap() - 1.0).abs() < 1e-9, "{call:?}");
        assert!(call.flagged);

        // Same spot, folds: no loss.
        let good = review_hand(
            &game,
            &setup,
            &[j, k, Check, Bet, Fold],
            Seat(0),
            &mut search,
            Budget::Playouts(2_000),
            &mut rng,
        );
        assert_eq!(good[1].ev_loss, Some(0.0));
        assert!(!good[1].flagged);
    }
}
