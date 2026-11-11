use super::games::SCRIPTED_IDS;
use super::*;
use engine::replay::replay;
use engine::testing::{Conformance, check_conformance};

fn game(id: &str) -> Scripted {
    Scripted::new(Config {
        game: id.into(),
        ..Config::default()
    })
}

fn card(s: &str) -> Card {
    s.parse().unwrap()
}

#[test]
fn every_scripted_game_passes_conformance() {
    for &id in SCRIPTED_IDS {
        let g = game(id);
        let max = g.meta().seats.end().to_owned();
        let cfg = Conformance {
            hands: 40,
            seat_counts: vec![2, 3, max.min(5)],
            determinizations: 2,
            inverted: g.script().visibility == Visibility::Indian,
            ..Conformance::default()
        };
        check_conformance(&g, &cfg);
    }
}

#[test]
fn high_only_variation_passes_conformance() {
    for id in ["omaha5", "courchevel"] {
        let g = Scripted::new(Config {
            game: id.into(),
            high_only: true,
            ..Config::default()
        });
        assert!(g.script().has_high_only);
        check_conformance(
            &g,
            &Conformance {
                hands: 20,
                seat_counts: vec![3],
                ..Conformance::default()
            },
        );
    }
}

/// Play `actions` for seats (by picking the first legal action matching `want`), dealing `deal`
/// cards in order.
fn run(
    g: &Scripted,
    n: u8,
    deal: &[&str],
    mut want: impl FnMut(&State, &[Action]) -> Action,
) -> State {
    let setup = HandSetup::simple(n, Chips(10_000));
    let mut s = g.new_hand(&setup);
    let mut cards = deal.iter().map(|c| card(c));
    let mut legal = Vec::new();
    let mut log = Vec::new();
    loop {
        let a = match g.to_act(&s) {
            ToAct::Terminal => break,
            ToAct::Chance => Action::Deal(cards.next().expect("script needs more cards")),
            ToAct::Seat(_) => {
                g.legal_actions(&s, &mut legal);
                want(&s, &legal)
            }
        };
        g.validate(&s, a).unwrap();
        g.apply(&mut s, a);
        log.push(a);
    }
    // Replays exactly.
    assert_eq!(g.payoffs(&replay(g, &setup, &log).unwrap()), g.payoffs(&s));
    s
}

fn passive(_: &State, legal: &[Action]) -> Action {
    *legal
        .iter()
        .find(|a| matches!(a, Action::CheckCall | Action::Draw(0)))
        .unwrap_or(&legal[0])
}

#[test]
fn omaha_needs_two_hole_cards_for_a_flush() {
    let g = game("omaha_pl");
    // Deal order with the button on s0: s1, s0, s1, s0, …
    // s1: As Kd Qc Jh (one spade). s0: 2c 3c 7d 8d. Board: 4s 5s 9s Ts Kh.
    // Any-five would give s1 an ace-high flush; Omaha (2 + 3) can't: s1's best is the
    // Q-J + K-T-9 straight, which still beats s0's ten-high.
    let deal = [
        "As", "2c", "Kd", "3c", "Qc", "7d", "Jh", "8d", "4s", "5s", "9s", "Ts", "Kh",
    ];
    let s = run(&g, 2, &deal, passive);
    let own = s.own(Seat(1));
    let v = hand::share_value(&g.script().shares[0], &own, &s.boards[0], CardSet::EMPTY).unwrap();
    assert_eq!(
        v.category(),
        engine::Category::Straight,
        "no flush with one spade in hand"
    );
    assert!(g.payoffs(&s)[1] > Chips::ZERO);
}

#[test]
fn chicago_spade_half_goes_to_highest_spade() {
    let g = game("chicago_high");
    // Two seats, button s0: deal order s1, s0. s1 gets the only spade (2s) but a weak hand;
    // s0 makes a pair of aces. Pot: dealer ante $2 (posted by the chooser, s0).
    let deal = [
        "2s", "Ad", // down
        "3h", "Ac", // down
        "7c", "Kd", // up 3rd
        "8d", "Qh", // 4th
        "9h", "Jc", // 5th
        "4d", "Th", // 6th
        "5c", "6h", // 7th down
    ];
    let s = run(&g, 2, &deal, passive);
    let pay = g.payoffs(&s);
    // $2 pot: $1 to s0 (hand), $1 to s1 (spade). s0 posted it, so net s0 -1, s1 +1.
    assert_eq!(pay, vec![Chips(-100), Chips(100)]);
}

#[test]
fn chicago_without_a_spade_hand_takes_all() {
    let g = game("chicago_high");
    let deal = [
        "2h", "Ad", "3h", "Ac", "7c", "Kd", "8d", "Qh", "9h", "Jc", "4d", "Th", "5c", "6h",
    ];
    let s = run(&g, 2, &deal, passive);
    assert_eq!(
        g.payoffs(&s),
        vec![Chips::ZERO, Chips::ZERO],
        "s0 wins its own ante back"
    );
}

#[test]
fn red_river_keeps_dealing_while_red() {
    let g = game("red_river");
    // 2 seats × 4 hole, flop/turn/river, then river+1 = 9h (red) -> another card Kd (red) ->
    // 2c (black) stops. Board ends with 3 + 1 + 1 + 3 = 8 cards.
    let mut deal: Vec<&str> = ["Ac", "Ad", "Kc", "Ks", "Qc", "Qs", "Jc", "Js"].to_vec();
    deal.extend(["2d", "3d", "4c", "5c", "6c", "9h", "Kh", "2s"]);
    let s = run(&g, 2, &deal, |s, legal| {
        // Declare High for both; otherwise check.
        if matches!(legal[0], Action::Declare(_)) {
            let _ = s;
            Action::Declare(0)
        } else {
            passive(s, legal)
        }
    });
    assert_eq!(s.boards[0].len(), 8);
}

#[test]
fn indian_poker_shows_others_but_not_yourself() {
    let g = game("indian_poker");
    let setup = HandSetup::simple(3, Chips(10_000));
    let mut s = g.new_hand(&setup);
    for c in ["Ah", "Kd", "2c"] {
        g.apply(&mut s, Action::Deal(card(c)));
    }
    // Deal order from left of button s0: s1, s2, s0.
    let obs = g.observe(&s, Seat(0));
    assert!(obs.own_dealt.is_empty(), "you never see your own card");
    assert_eq!(obs.shown[1].as_deref(), Some(&[card("Ah")][..]));
    assert_eq!(obs.shown[2].as_deref(), Some(&[card("Kd")][..]));
}

#[test]
fn showtime_reveals_folded_hands() {
    let g = game("holdem_showtime");
    let setup = HandSetup::simple(3, Chips(10_000));
    let mut s = g.new_hand(&setup);
    for c in ["Ah", "Kd", "2c", "As", "Ks", "3c"] {
        g.apply(&mut s, Action::Deal(card(c)));
    }
    // First to act: s1 (left of button s0). Bet so a fold is possible, then s2 folds.
    let mut legal = Vec::new();
    g.legal_actions(&s, &mut legal);
    let raise = *legal
        .iter()
        .find(|a| matches!(a, Action::RaiseTo(_)))
        .unwrap();
    g.apply(&mut s, raise);
    g.apply(&mut s, Action::Fold); // s2
    let obs = g.observe(&s, Seat(0));
    assert_eq!(obs.shown[2].as_deref(), Some(&[card("Kd"), card("Ks")][..]));
    assert_eq!(obs.shown[1], None, "s1 is still in: hidden");
}

#[test]
fn six_plus_flush_beats_full_house() {
    let g = game("holdem_6plus");
    // s1: Ah Kh ; s0: 9c 9d. Board: 9h 7h 6h Ks Kc -> s1 flush, s0 nines full.
    let deal = ["Ah", "9c", "Kh", "9d", "9h", "7h", "6h", "Ks", "Kc"];
    let s = run(&g, 2, &deal, passive);
    assert!(g.payoffs(&s)[1] > Chips::ZERO);
}

#[test]
fn razz_lowest_board_acts_first() {
    let g = game("razz");
    let setup = HandSetup::simple(3, Chips(10_000));
    let mut s = g.new_hand(&setup);
    let mut legal = Vec::new();
    for c in ["Kc", "Kd", "Kh", "Qc", "Qd", "Qh"] {
        g.apply(&mut s, Action::Deal(card(c)));
    }
    // First round (no upcards): everyone checks.
    for _ in 0..3 {
        g.legal_actions(&s, &mut legal);
        g.apply(&mut s, Action::CheckCall);
    }
    // 3rd street up cards: s1 9c, s2 Ac, s0 5c -> s2 (ace, lowest) acts first.
    for c in ["9c", "Ac", "5c"] {
        g.apply(&mut s, Action::Deal(card(c)));
    }
    assert_eq!(g.to_act(&s), ToAct::Seat(Seat(2)));
}
