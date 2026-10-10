//! ISMCTS tests: exact spots with a known right answer, a strength check against random, and a
//! measured Kuhn exploitability baseline (bots.md §2.3).

use super::*;
use crate::harness::play_hand;
use crate::mccfr::best_response::{SampledStrategy, exploitability};
use crate::policy::Policy;
use crate::random::RandomPolicy;
use crate::strategy::UniformStrategy;
use crate::strength::IsmctsBot;
use engine::replay::replay;
use engine::rng::{Stream, derive_rng, rng_from_seed};
use engine::{Chips, HandSetup};
use games::kuhn::{self, Kuhn};
use games::leduc::{self, Leduc};

/// The first player's observation holding `own` after `history`, plus its legal actions.
fn kuhn_spot(own: &str, history: &[kuhn::Action]) -> (Kuhn, kuhn::Observation, Vec<kuhn::Action>) {
    use kuhn::Action::Deal;
    let game = Kuhn::new(kuhn::Config {});
    // Button = seat 1, so seat 0 acts first. The opponent's card is a placeholder: determinize
    // resamples it, so it doesn't affect the search.
    let setup = HandSetup {
        button: Seat(1),
        ..HandSetup::simple(2, Chips(100))
    };
    let other = if own == "Js" { "Qs" } else { "Js" };
    let mut log = vec![Deal(own.parse().unwrap()), Deal(other.parse().unwrap())];
    log.extend_from_slice(history);
    let s = replay(&game, &setup, &log).unwrap();
    assert_eq!(game.to_act(&s), ToAct::Seat(Seat(0)));
    let mut legal = Vec::new();
    game.legal_actions(&s, &mut legal);
    let obs = game.observe(&s, Seat(0));
    (game, obs, legal)
}

#[test]
fn calls_with_king_and_folds_jack_facing_a_bet() {
    use kuhn::Action::*;
    let mut search = Ismcts::<Kuhn>::new(IsmctsConfig::default());
    let mut rng = rng_from_seed(1);

    // Checked, then the opponent bet. With K, calling always wins 2; folding loses 1.
    let (game, obs, legal) = kuhn_spot("Ks", &[Check, Bet]);
    let v = search.evaluate(&game, &obs, &legal, Budget::Playouts(2_000), &mut rng);
    assert_eq!(v[0].action, Call, "{v:?}");
    assert!(
        (v[0].q - 2.0).abs() < 1e-9,
        "calling with K wins exactly 2: {v:?}"
    );

    // With J, calling always loses 2; folding loses 1.
    let (game, obs, legal) = kuhn_spot("Js", &[Check, Bet]);
    let v = search.evaluate(&game, &obs, &legal, Budget::Playouts(2_000), &mut rng);
    assert_eq!(v[0].action, Fold, "{v:?}");
    assert!((v[0].q + 1.0).abs() < 1e-9);
}

#[test]
fn evaluation_mode_gives_every_action_visits() {
    use kuhn::Action::*;
    let mut search = Ismcts::<Kuhn>::new(IsmctsConfig::default());
    let (game, obs, legal) = kuhn_spot("Js", &[Check, Bet]);
    let v = search.evaluate(
        &game,
        &obs,
        &legal,
        Budget::Playouts(500),
        &mut rng_from_seed(3),
    );
    assert!(v.iter().all(|a| a.visits >= 32), "{v:?}");
}

#[test]
fn search_is_deterministic_with_a_playout_budget() {
    let (game, obs, legal) = kuhn_spot("Qs", &[]);
    let run = || {
        let mut search = Ismcts::<Kuhn>::new(IsmctsConfig::default());
        search.evaluate(
            &game,
            &obs,
            &legal,
            Budget::Playouts(1_000),
            &mut rng_from_seed(9),
        )
    };
    assert_eq!(run(), run());
}

#[test]
fn ismcts_beats_random_at_leduc() {
    let game = Leduc::new(leduc::Config {});
    let hands = 400u64;
    let mut total = 0.0;
    for h in 0..hands {
        // Alternate seats and buttons so position doesn't bias the result.
        let bot_seat = (h % 2) as usize;
        let mut bot = IsmctsBot::<Leduc>::preset(5, false);
        bot.strength.budget = Budget::Playouts(300);
        let mut random = RandomPolicy;
        let mut seats: [&mut dyn Policy<Leduc>; 2] = if bot_seat == 0 {
            [&mut bot, &mut random]
        } else {
            [&mut random, &mut bot]
        };
        let setup = HandSetup {
            button: Seat((h / 2 % 2) as u8),
            ..HandSetup::simple(2, Chips(100))
        };
        let mut deal = derive_rng(7, h, Stream::Deal);
        let mut rngs = [
            derive_rng(7, h, Stream::Seat(0)),
            derive_rng(7, h, Stream::Seat(1)),
        ];
        let r = play_hand(&game, &setup, &mut seats, &mut deal, &mut rngs);
        total += r.payoffs[bot_seat].0 as f64;
    }
    let per_hand = total / hands as f64;
    // Random play bleeds chips fast; a searching bot should win well over an ante per hand.
    assert!(
        per_hand > 1.0,
        "ISMCTS won only {per_hand} per hand vs random"
    );
}

/// Measures (not just asserts) ISMCTS exploitability at Kuhn: the M1 baseline. MO-ISMCTS isn't
/// an equilibrium method, so this won't be ~0, but it must clearly beat uniform.
/// Run with `--nocapture` to see the number.
#[test]
fn kuhn_exploitability_baseline() {
    let game = Kuhn::new(kuhn::Config {});
    let mut bot = IsmctsBot::<Kuhn>::preset(10, false);
    bot.strength.budget = Budget::Playouts(400);
    let sampled = SampledStrategy::from_policy(&game, &mut bot, 30, 11);
    let e = exploitability(&game, &sampled);
    let uniform = exploitability(&game, &UniformStrategy);
    println!("Kuhn exploitability: ISMCTS {e:.4}, uniform {uniform:.4}");
    assert!(e < uniform, "ISMCTS {e} vs uniform {uniform}");
}

/// Hold'em sanity: with the nuts on the river facing a bet, calling must beat folding.
#[test]
fn holdem_calls_with_the_nuts() {
    use games::holdem::{Action, Config, Holdem};
    let game = Holdem::new(Config::default());
    let setup = HandSetup { first_hand_of_game: false, ..HandSetup::simple(2, Chips(2_000)) };
    let d = |s: &str| s.split_whitespace().map(|c| Action::Deal(c.parse().unwrap())).collect::<Vec<_>>();
    // Seat 1 gets As Ks (cards 1 and 3); board makes a royal flush for seat 1.
    let mut log = d("As 2c Ks 7d");
    log.extend([Action::CheckCall, Action::CheckCall]);
    for (cards, acts) in [("Qs Js Ts", 2), ("3h", 2), ("4d", 0)] {
        log.extend(d(cards));
        log.extend(std::iter::repeat_n(Action::CheckCall, acts));
    }
    // River: seat 1 checks, seat 0 bets 300, back to seat 1.
    log.extend([Action::CheckCall, Action::RaiseTo(Chips(300))]);
    let s = replay(&game, &setup, &log).unwrap();
    assert_eq!(game.to_act(&s), ToAct::Seat(Seat(1)));
    let obs = game.observe(&s, Seat(1));
    let mut legal = Vec::new();
    game.legal_actions(&s, &mut legal);
    let mut search = Ismcts::<Holdem>::new(IsmctsConfig::default());
    let v = search.evaluate(&game, &obs, &legal, Budget::Playouts(1_500), &mut rng_from_seed(5));
    let q = |a: Action| v.iter().find(|x| x.action == a).unwrap().q;
    assert!(q(Action::CheckCall) > q(Action::Fold), "{v:?}");
    assert_ne!(v[0].action, Action::Fold);
}

/// NFR-3: ISMCTS playouts per 0.5 s in 6-handed Hold'em. Release mode only:
/// `cargo test --release -p bots holdem_playout_rate -- --ignored --nocapture`.
#[test]
#[ignore]
fn holdem_playout_rate() {
    use games::holdem::{Config, Holdem};
    let game = Holdem::new(Config::default());
    let setup = HandSetup::simple(6, Chips(2_000));
    let mut rng = rng_from_seed(1);
    // Advance to the first decision.
    let mut s = game.new_hand(&setup);
    while game.to_act(&s) == ToAct::Chance {
        let a = game.sample_chance(&s, &mut rng);
        game.apply(&mut s, a);
    }
    let ToAct::Seat(p) = game.to_act(&s) else { unreachable!() };
    let obs = game.observe(&s, p);
    let mut legal = Vec::new();
    game.legal_actions(&s, &mut legal);
    let mut search = Ismcts::<Holdem>::new(IsmctsConfig::default());
    search.best_action(&game, &obs, &legal, Budget::Time(std::time::Duration::from_millis(500)), &mut rng);
    println!("6-max Hold'em preflop: {} playouts in 0.5 s (target ≥ 10,000)", search.last_playouts);
}
