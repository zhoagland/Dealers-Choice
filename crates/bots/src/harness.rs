//! Play one hand with a policy in every seat. The sim, the CLI and the tests all share this loop,
//! so "how a hand is driven" is defined exactly once.

use engine::{Chips, DetRng, Game, HandSetup, ToAct};

use crate::policy::Policy;

/// What happened in one hand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandResult<A> {
    /// The full log, chance included: with the setup, enough to replay exactly.
    pub actions: Vec<A>,
    pub payoffs: Vec<Chips>,
    /// Player decisions (not counting chance).
    pub decisions: usize,
}

/// Play `setup` to the end. `seats[i]` decides for seat `i`, using `seat_rngs[i]`. Chance uses
/// `deal_rng`. Keeping them separate means a bot's randomness can't change the cards.
///
/// `&mut dyn Policy<G>` lets each seat be a different bot type.
///
/// Panics if a policy returns an action that fails `validate`: that's a bot bug, and silently
/// continuing would corrupt results.
pub fn play_hand<G: Game>(
    game: &G,
    setup: &HandSetup,
    seats: &mut [&mut dyn Policy<G>],
    deal_rng: &mut DetRng,
    seat_rngs: &mut [DetRng],
) -> HandResult<G::Action> {
    assert_eq!(seats.len(), setup.num_seats as usize, "one policy per seat");
    assert_eq!(seat_rngs.len(), seats.len(), "one rng per seat");
    let mut state = game.new_hand(setup);
    let mut actions = Vec::new();
    let mut legal = Vec::new();
    let mut decisions = 0;
    loop {
        let action = match game.to_act(&state) {
            ToAct::Terminal => break,
            ToAct::Chance => game.sample_chance(&state, deal_rng),
            ToAct::Seat(p) => {
                game.legal_actions(&state, &mut legal);
                let obs = game.observe(&state, p);
                let i = p.index();
                let a = loop {
                    let a = seats[i].act(game, &obs, &legal, &mut seat_rngs[i]);
                    match game.validate(&state, a) {
                        Ok(()) => break a,
                        Err(e) if seats[i].on_rejected(&e) => continue,
                        Err(e) => panic!("{} in {p} chose {a:?}: {e}", seats[i].name()),
                    }
                };
                decisions += 1;
                a
            }
        };
        game.apply(&mut state, action);
        actions.push(action);
    }
    let payoffs = game.payoffs(&state);
    for (policy, &p) in seats.iter_mut().zip(&payoffs) {
        policy.on_hand_end(p);
    }
    HandResult {
        actions,
        payoffs,
        decisions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::random::RandomPolicy;
    use engine::replay::replay;
    use engine::rng::{Stream, derive_rng};
    use games::kuhn::{Config, Kuhn};

    #[test]
    fn random_vs_random_kuhn_replays_and_is_deterministic() {
        let game = Kuhn::new(Config {});
        let setup = HandSetup::simple(2, Chips(100));
        let run = |hand: u64| {
            let (mut a, mut b) = (RandomPolicy, RandomPolicy);
            let mut seats: [&mut dyn Policy<Kuhn>; 2] = [&mut a, &mut b];
            let mut deal = derive_rng(1, hand, Stream::Deal);
            let mut rngs = [
                derive_rng(1, hand, Stream::Seat(0)),
                derive_rng(1, hand, Stream::Seat(1)),
            ];
            play_hand(&game, &setup, &mut seats, &mut deal, &mut rngs)
        };
        for hand in 0..100 {
            let r = run(hand);
            assert_eq!(r, run(hand), "same seeds, same hand");
            let s = replay(&game, &setup, &r.actions).unwrap();
            assert_eq!(game.payoffs(&s), r.payoffs);
            assert!(r.decisions >= 2);
        }
    }
}
