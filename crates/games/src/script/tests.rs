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

#[test]
fn full_draw_tables_reshuffle_and_pass_conformance() {
    // 8 seats × 5 cards leaves 12 in the stock: random draws run out and reshuffle (D-DRAW-1),
    // which exercises the reshuffle path of `determinize`.
    for id in ["triple_draw", "draw27_triple", "draw5", "badugi"] {
        let g = game(id);
        let max = *g.meta().seats.end();
        check_conformance(
            &g,
            &Conformance {
                hands: 25,
                seat_counts: vec![max],
                determinizations: 2,
                ..Conformance::default()
            },
        );
    }
}

#[test]
fn reshuffles_happen_at_full_triple_draw_tables() {
    use engine::rng::{Stream, derive_rng};
    use rand::seq::IndexedRandom;
    let g = game("triple_draw");
    let mut seen = 0;
    for hand in 0..30 {
        let setup = HandSetup::simple(8, Chips(10_000));
        let mut s = g.new_hand(&setup);
        let mut rng = derive_rng(5, hand, Stream::Deal);
        let mut legal = Vec::new();
        while g.to_act(&s) != ToAct::Terminal {
            let a = match g.to_act(&s) {
                ToAct::Chance => g.sample_chance(&s, &mut rng),
                _ => {
                    g.legal_actions(&s, &mut legal);
                    // Never fold; draw a lot.
                    *legal
                        .iter()
                        .filter(|a| !matches!(a, Action::Fold))
                        .collect::<Vec<_>>()
                        .choose(&mut rng)
                        .unwrap()
                        .to_owned()
                }
            };
            g.apply(&mut s, a);
        }
        if !s.reshuffles.is_empty() {
            seen += 1;
        }
    }
    assert!(seen > 0, "expected at least one reshuffle");
}

#[test]
fn every_variant_passes_conformance() {
    for &id in SCRIPTED_IDS {
        for v in game(id).script().variants.clone() {
            let g = Scripted::new(Config {
                game: id.into(),
                variants: vec![v.name.into()],
                ..Config::default()
            });
            check_conformance(
                &g,
                &Conformance {
                    hands: 20,
                    seat_counts: vec![3],
                    determinizations: 2,
                    ..Conformance::default()
                },
            );
        }
    }
}

#[test]
fn ace_bonus_draws_four_and_shows_the_ace() {
    let g = game("three_blind_mice");
    let setup = HandSetup::simple(2, Chips(10_000));
    let mut s = g.new_hand(&setup);
    // s1: Ah 2c 3d 4h 5s ; s0: Kc Kd Qc Qd Jc
    for c in ["Ah", "Kc", "2c", "Kd", "3d", "Qc", "4h", "Qd", "5s", "Jc"] {
        g.apply(&mut s, Action::Deal(card(c)));
    }
    g.apply(&mut s, Action::CheckCall);
    g.apply(&mut s, Action::CheckCall);
    // s1 throws away four, keeping the Ace (position 0).
    assert!(g.validate(&s, Action::Draw(0b11110)).is_ok());
    g.apply(&mut s, Action::Draw(0b11110));
    let obs = g.observe(&s, Seat(0));
    assert!(
        obs.history
            .contains(&Event::Drew(Seat(1), 4, Some(card("Ah"))))
    );
    assert!(!s.betting.is_folded(Seat(1)));
    // s0 throws four keeping a King: the King is shown and the hand is fouled (D-3BM-1).
    g.apply(&mut s, Action::Draw(0b11110));
    assert!(s.betting.is_folded(Seat(0)));
    assert!(
        g.observe(&s, Seat(1))
            .history
            .contains(&Event::Drew(Seat(0), 4, Some(card("Kc"))))
    );
}

#[test]
fn mamas_favorite_throws_whole_pairs() {
    let g = game("mamas_favorite");
    let setup = HandSetup::simple(2, Chips(10_000));
    let mut s = g.new_hand(&setup);
    for c in [
        "Ah", "Kc", "2c", "Kd", "3d", "Qc", "4h", "Qd", "5s", "Jc", "6s", "Jd",
    ] {
        g.apply(&mut s, Action::Deal(card(c)));
    }
    let mut legal = Vec::new();
    g.legal_actions(&s, &mut legal);
    assert_eq!(legal.len(), 15, "15 ways to split six cards into pairs");
    g.apply(&mut s, Action::Arrange(0));
    g.apply(&mut s, Action::Arrange(0));
    g.apply(&mut s, Action::CheckCall);
    g.apply(&mut s, Action::CheckCall);
    for c in ["7c", "8c", "9c"] {
        g.apply(&mut s, Action::Deal(card(c)));
    }
    g.legal_actions(&s, &mut legal);
    assert_eq!(
        legal,
        vec![
            Action::Draw(0b11),
            Action::Draw(0b1100),
            Action::Draw(0b110000)
        ]
    );
}

#[test]
fn swap_instead_of_fold() {
    let g = game("holdem_swap");
    let setup = HandSetup::simple(2, Chips(10_000));
    let mut s = g.new_hand(&setup);
    for c in ["Ah", "2c", "Kd", "7d"] {
        g.apply(&mut s, Action::Deal(card(c)));
    }
    let mut legal = Vec::new();
    // s1 opens with a bet: no swaps (nothing to fold to).
    g.legal_actions(&s, &mut legal);
    assert!(!legal.iter().any(|a| matches!(a, Action::Swap(_))));
    let bet = *legal
        .iter()
        .find(|a| matches!(a, Action::RaiseTo(_)))
        .unwrap();
    g.apply(&mut s, bet);
    // s0 faces a bet: may swap 1 or 2 cards (3 masks) instead of folding.
    g.legal_actions(&s, &mut legal);
    let swaps: Vec<_> = legal
        .iter()
        .filter(|a| matches!(a, Action::Swap(_)))
        .collect();
    assert_eq!(swaps.len(), 3);
    g.apply(&mut s, Action::Swap(0b01));
    assert_eq!(g.to_act(&s), ToAct::Chance, "replacement card comes next");
    g.apply(&mut s, Action::Deal(card("As")));
    assert_eq!(s.down[0].as_slice(), &[card("7d"), card("As")]);
    assert_eq!(
        s.betting.street_committed(Seat(0)),
        s.betting.street_committed(Seat(1)),
        "called"
    );
}

#[test]
fn pass_and_in_out_games_hold_up_over_many_hands() {
    for id in ["pass_the_trash", "man_or_mouse", "game_43"] {
        check_conformance(
            &game(id),
            &Conformance {
                hands: 120,
                seat_counts: vec![2, 4, 7],
                determinizations: 2,
                seed: 0xBEEF,
                ..Conformance::default()
            },
        );
    }
}

#[test]
fn passes_go_to_the_next_seat_in_the_chosen_direction() {
    let g = game("pass_the_trash");
    let setup = HandSetup::simple(3, Chips(10_000));
    let mut s = g.new_hand(&setup);
    g.apply(&mut s, Action::Direction(0)); // left
    // 7 rounds, dealing s1, s2, s0: s1 gets clubs, s2 diamonds, s0 hearts.
    let ranks = ["2", "3", "4", "5", "6", "7", "8"];
    for r in ranks {
        for suit in ["c", "d", "h"] {
            g.apply(&mut s, Action::Deal(card(&format!("{r}{suit}"))));
        }
    }
    for _ in 0..3 {
        g.apply(&mut s, Action::CheckCall);
    }
    // Everyone passes their first three cards.
    for _ in 0..3 {
        g.apply(&mut s, Action::Pass(0b111));
    }
    // Left = to the next seat clockwise: s0 -> s1, s1 -> s2, s2 -> s0.
    assert!(s.down[1].contains(&card("2h")), "s1 got s0's cards");
    assert!(s.down[2].contains(&card("2c")), "s2 got s1's cards");
    assert!(s.down[0].contains(&card("2d")), "s0 got s2's cards");
    let obs = g.observe(&s, Seat(0));
    assert_eq!(
        obs.own_received.as_slice(),
        &[card("2d"), card("3d"), card("4d")]
    );
}

#[test]
fn in_pays_a_dollar_in_43_and_all_out_is_void() {
    let g = game("game_43");
    let setup = HandSetup::simple(3, Chips(10_000));
    let mut s = g.new_hand(&setup);
    for c in [
        "2c", "2d", "2h", "3c", "3d", "3h", "4c", "4d", "4h", "5c", "5d", "5h",
    ] {
        g.apply(&mut s, Action::Deal(card(c)));
    }
    // In/out in turn from the dealer's left: s1 In, s2 Out, s0 In.
    g.apply(&mut s, Action::InOut(1));
    g.apply(&mut s, Action::InOut(0));
    let obs = g.observe(&s, Seat(0));
    assert!(
        !obs.history
            .iter()
            .any(|e| matches!(e, Event::InOutResolved(_))),
        "hidden until all"
    );
    g.apply(&mut s, Action::InOut(1));
    assert!(s.betting.is_folded(Seat(2)));
    assert_eq!(s.betting.contributed()[1], Chips(100));
    assert_eq!(s.betting.contributed()[0], Chips(300), "dealer ante + $1");

    let mut v = g.new_hand(&setup);
    for c in [
        "2c", "2d", "2h", "3c", "3d", "3h", "4c", "4d", "4h", "5c", "5d", "5h",
    ] {
        g.apply(&mut v, Action::Deal(card(c)));
    }
    for _ in 0..3 {
        g.apply(&mut v, Action::InOut(0));
    }
    assert_eq!(g.to_act(&v), ToAct::Terminal);
    assert!(
        g.payoffs(&v).iter().all(|&x| x == Chips::ZERO),
        "everyone Out: all chips back"
    );
}

/// Value of seat `p` for share `k` with a hand-built board (positions in order 0..).
fn shaped_value(id: &str, own: &[&str], board: &[(u8, &str)], k: usize) -> Option<HandValue> {
    let g = game(id);
    let sc = g.script();
    let own: Vec<Card> = own.iter().map(|c| card(c)).collect();
    sc.lines
        .iter()
        .filter_map(|line| {
            let cards: Vec<Card> = board
                .iter()
                .filter(|(at, _)| line.contains(at))
                .map(|(_, c)| card(c))
                .collect();
            hand::share_value(&sc.shares[k], &own, &cards, sc.wild)
        })
        .max()
}

#[test]
fn elevator_hands_use_one_line() {
    // Spades at positions 0 (left top), 4 (right top), 3 (centre): no line holds all three, so no
    // flush even with two spades in hand.
    let board = [
        (0, "As"),
        (4, "Ks"),
        (3, "Qs"),
        (1, "2d"),
        (2, "3c"),
        (5, "8h"),
        (6, "9c"),
    ];
    let v = shaped_value("elevator", &["Js", "Ts", "4d", "5h"], &board, 0).unwrap();
    assert_ne!(v.category(), engine::Category::Flush);
    // Put them in the middle row (1, 3, 5) and it is a royal flush.
    let board = [
        (1, "As"),
        (3, "Ks"),
        (5, "Qs"),
        (0, "2d"),
        (2, "3c"),
        (4, "8h"),
        (6, "9c"),
    ];
    let v = shaped_value("elevator", &["Js", "Ts", "4d", "5h"], &board, 0).unwrap();
    assert_eq!(v.category(), engine::Category::RoyalFlush);
}

#[test]
fn wheel_of_death_needs_three_in_a_row() {
    // Trips need the pair in hand + one board 7: always possible. A straight needs 3 consecutive
    // ring cards: 8 at 0, 9 at 1, T at 2 with J Q in hand = straight.
    let board = [
        (0, "8c"),
        (1, "9d"),
        (2, "Th"),
        (3, "2c"),
        (4, "3d"),
        (5, "4h"),
        (6, "5c"),
        (7, "Kd"),
        (8, "Ah"),
    ];
    let v = shaped_value("wheel_of_death", &["Jc", "Qd", "2s", "3s"], &board, 0).unwrap();
    assert_eq!(v.category(), engine::Category::Straight);
    // Spread 8, 9, T around the ring (0, 3, 6): no three-in-a-row straight.
    let board = [
        (0, "8c"),
        (3, "9d"),
        (6, "Th"),
        (1, "2c"),
        (4, "3d"),
        (7, "4h"),
        (2, "5c"),
        (5, "Kd"),
        (8, "Ah"),
    ];
    let v = shaped_value("wheel_of_death", &["Jc", "Qd", "2s", "3s"], &board, 0).unwrap();
    assert_ne!(v.category(), engine::Category::Straight);
}

#[test]
fn iron_cross_row_or_column() {
    // Row 0-2: 2c 3d 4h ; column 3,1,4: Ks 3d Kh. Hand A A K x: full house with the column
    // (K K + A A K), not with the row.
    let board = [(0, "2c"), (1, "3d"), (2, "4h"), (3, "Ks"), (4, "Kh")];
    let v = shaped_value("iron_cross", &["As", "Ad", "Kc", "7d"], &board, 0).unwrap();
    assert_eq!(v.category(), engine::Category::FullHouse);
}

#[test]
fn spit_middle_holds_nine_including_each_discard() {
    let g = game("spit");
    let setup = HandSetup::simple(3, Chips(10_000));
    let mut s = g.new_hand(&setup);
    let deal = [
        "2c", "2d", "2h", "3c", "3d", "3h", "4c", "4d", "4h", "5c", "5d", "5h", "6c", "6d", "6h",
    ];
    for c in deal {
        g.apply(&mut s, Action::Deal(card(c)));
    }
    // Each throws its first card: s1 2c, s2 2d, s0 2h.
    for _ in 0..3 {
        g.apply(&mut s, Action::Draw(1));
    }
    // Six more hidden cards top the middle up to nine.
    for c in ["7c", "7d", "7h", "8c", "8d", "8h"] {
        assert_eq!(g.to_act(&s), ToAct::Chance);
        g.apply(&mut s, Action::Deal(card(c)));
    }
    assert_eq!(s.pool.len(), 9);
    assert!(s.pool.contains(card("2c")) && s.pool.contains(card("2h")));
    let obs = g.observe(&s, Seat(0));
    assert!(obs.history.contains(&Event::Spat(Seat(1))));
    // Flips come from the middle only.
    for _ in 0..3 {
        g.apply(&mut s, Action::CheckCall);
    }
    let mut legal = Vec::new();
    g.legal_actions(&s, &mut legal);
    assert_eq!(legal.len(), 9);
}

/// Deal the 2-down + check-around opening of a 2-seat stud game.
fn stud_open(g: &Scripted, downs: [&str; 4]) -> State {
    let setup = HandSetup::simple(2, Chips(10_000));
    let mut s = g.new_hand(&setup);
    for c in downs {
        g.apply(&mut s, Action::Deal(card(c)));
    }
    g.apply(&mut s, Action::CheckCall);
    g.apply(&mut s, Action::CheckCall);
    s
}

#[test]
fn baseball_face_up_four_earns_an_extra_card() {
    let g = game("baseball");
    let mut s = stud_open(&g, ["Ac", "Kc", "Ad", "Kd"]);
    g.apply(&mut s, Action::Deal(card("4h"))); // s1's up card: a 4
    g.apply(&mut s, Action::Deal(card("9s"))); // s0's up card
    assert_eq!(
        g.to_act(&s),
        ToAct::Seat(Seat(1)),
        "s1 chooses up or down for the extra"
    );
    g.apply(&mut s, Action::Choose(1));
    g.apply(&mut s, Action::Deal(card("2c")));
    assert_eq!(s.up[1].as_slice(), &[card("4h"), card("2c")]);
}

#[test]
fn low_in_the_hole_wild_is_the_lowest_hole_card() {
    let g = game("low_in_the_hole");
    let mut s = stud_open(&g, ["5c", "Ah", "Kd", "Ks"]);
    // s1 holds 5c + Kd down: 5s are wild for s1 (ace high, so Ah is not its low card).
    let wild = g.wild_for(&s, Seat(1));
    assert!(wild.contains(card("5d")) && !wild.contains(card("Ah")));
    // s0 holds Ah + Ks: kings wild.
    assert!(g.wild_for(&s, Seat(0)).contains(card("Kh")));
    let _ = &mut s;
}

#[test]
fn push_moves_the_card_and_the_pusher_gets_a_fresh_one() {
    let g = game("push");
    let mut s = stud_open(&g, ["Ac", "Kc", "Ad", "Kd"]);
    g.apply(&mut s, Action::Deal(card("2h"))); // s1's street card
    g.apply(&mut s, Action::Choose(1)); // push it ($1)
    g.apply(&mut s, Action::Deal(card("9s"))); // s1's fresh card
    assert_eq!(
        g.to_act(&s),
        ToAct::Seat(Seat(0)),
        "s0 decides on the pushed 2h"
    );
    g.apply(&mut s, Action::Choose(0)); // keep it
    assert_eq!(s.up[1].as_slice(), &[card("9s")]);
    assert_eq!(s.up[0].as_slice(), &[card("2h")]);
    assert_eq!(s.betting.contributed()[1], Chips(100), "push costs $1");
}

#[test]
fn bullet_face_up_joker_kills() {
    let g = game("bullet");
    let mut s = stud_open(&g, ["Ac", "Kc", "Ad", "Kd"]);
    // Take rounds: s1 takes and gets a face-up joker -> dead; s0 wins.
    g.apply(&mut s, Action::Choose(1));
    g.apply(&mut s, Action::Deal(Card::RED_JOKER));
    assert!(s.betting.is_folded(Seat(1)));
    assert_eq!(g.to_act(&s), ToAct::Terminal);
}

#[test]
fn bid_highest_bid_picks_first() {
    let g = game("bid");
    let mut s = stud_open(&g, ["Ac", "Kc", "Ad", "Kd"]);
    g.apply(&mut s, Action::Deal(card("2h")));
    g.apply(&mut s, Action::Deal(card("Ah")));
    // s1 bids $1, s0 bids $3 (hidden until both are in).
    g.apply(&mut s, Action::Bid(2));
    assert!(
        !g.observe(&s, Seat(0))
            .history
            .iter()
            .any(|e| matches!(e, Event::BidShown(..)))
    );
    g.apply(&mut s, Action::Bid(6));
    assert_eq!(
        g.to_act(&s),
        ToAct::Seat(Seat(0)),
        "s0 bid more and picks first"
    );
    g.apply(&mut s, Action::Choose(1)); // the Ah
    g.apply(&mut s, Action::Choose(0));
    assert_eq!(s.up[0].as_slice(), &[card("Ah")]);
    assert_eq!(s.up[1].as_slice(), &[card("2h")]);
    assert_eq!(
        s.betting.contributed()[0],
        Chips(200 + 300),
        "dealer ante + $3 bid"
    );
}

/// Start hand `carry.hands` of a 2-seat multi-hand game, dealing `cards` (button alternates).
fn carry_hand(g: &Scripted, carry: &ScriptCarry, cards: &[&str]) -> State {
    let mut setup = HandSetup::simple(2, Chips(10_000));
    setup.button = Seat((carry.hands % 2) as u8);
    setup.first_hand_of_game = carry.hands == 0;
    let mut s = g.new_hand_with(&setup, carry);
    for c in cards {
        assert_eq!(g.to_act(&s), ToAct::Chance);
        g.apply(&mut s, Action::Deal(card(c)));
    }
    s
}

#[test]
fn guts_leg_two_legs_take_the_carried_pot() {
    let g = game("guts_leg");
    // Hand 1 (button s0, deal order s1, s0): s0 has aces, s1 nothing; both check and stand pat.
    let h1 = ["2c", "Ac", "7d", "Ad", "9h", "Ah", "Js", "Kc", "4d", "Kd"];
    let mut s = carry_hand(&g, &ScriptCarry::default(), &h1);
    for a in [
        Action::CheckCall,
        Action::CheckCall,
        Action::Draw(0),
        Action::Draw(0),
        Action::CheckCall,
        Action::CheckCall,
    ] {
        g.apply(&mut s, a);
    }
    let c1 = g.carry_out(&s);
    assert_eq!(c1.legs, vec![1, 0]);
    assert_eq!(c1.pot, 200, "the dealer ante carries");
    assert!(!c1.over);
    assert_eq!(g.payoffs(&s), vec![Chips(-200), Chips::ZERO]);
    // Hand 2 (button s1, deal order s0, s1): s0 wins again -> 2 legs, takes the pot.
    let h2 = ["Ac", "2c", "Ad", "7d", "Ah", "9h", "Kc", "Js", "Kd", "4d"];
    let mut s = carry_hand(&g, &c1, &h2);
    for a in [
        Action::CheckCall,
        Action::CheckCall,
        Action::Draw(0),
        Action::Draw(0),
        Action::CheckCall,
        Action::CheckCall,
    ] {
        g.apply(&mut s, a);
    }
    let c2 = g.carry_out(&s);
    assert!(c2.over);
    assert_eq!(
        g.payoffs(&s),
        vec![Chips(200), Chips::ZERO],
        "s0 gets the carried ante back"
    );
}

#[test]
fn drop_lone_player_plays_the_dummy() {
    let g = game("drop_321");
    // s1: trip 2s; s0: junk. s1 plays alone; dummy is A-K-Q high: s1 wins the antes.
    let mut s = carry_hand(
        &g,
        &ScriptCarry::default(),
        &["2c", "3d", "2d", "7h", "2h", "9s"],
    );
    g.apply(&mut s, Action::InOut(1)); // s1 holds
    g.apply(&mut s, Action::InOut(0)); // s0 drops
    for c in ["Ac", "Ks", "Qd"] {
        g.apply(&mut s, Action::Deal(card(c)));
    }
    let c = g.carry_out(&s);
    assert!(c.over);
    // Pot: $2 dealer ante (s0, the chooser) + $1 + $1 antes = $4 to s1.
    assert_eq!(g.payoffs(&s), vec![Chips(-300), Chips(300)]);
}

#[test]
fn three_five_seven_lone_player_gets_a_strike() {
    let g = game("three_five_seven");
    let mut s = carry_hand(
        &g,
        &ScriptCarry::default(),
        &["2c", "3d", "2d", "7h", "2h", "9s"],
    );
    g.apply(&mut s, Action::Play(1)); // s1 plays
    g.apply(&mut s, Action::Play(0)); // s0 drops
    assert_eq!(s.strikes, vec![0, 1]);
}

#[test]
fn scare_a_moosh_declarer_who_loses_is_stunned() {
    let g = game("scare_a_moosh");
    // s1 names high poker at $1 with junk; s0 has aces and is in.
    let deal = ["2c", "Ac", "7d", "Ad", "9h", "Ah", "Js", "Kc", "4d", "Kd"];
    let mut s = carry_hand(&g, &ScriptCarry::default(), &deal);
    g.apply(&mut s, Action::Name(2)); // s1: high poker (game 0), price 2 × 50¢
    g.apply(&mut s, Action::InOut(1)); // s1 (declarer) in
    g.apply(&mut s, Action::InOut(1)); // s0 in
    g.apply(&mut s, Action::Draw(0));
    g.apply(&mut s, Action::Draw(0));
    g.apply(&mut s, Action::CheckCall);
    g.apply(&mut s, Action::CheckCall);
    let c = g.carry_out(&s);
    assert!(!c.over, "the declarer didn't win: the pot carries");
    // Pot $2 + $1 + $1 = $4; s1's stun = $4 (under the $10 cap).
    assert_eq!(c.pot, 400 + 400);
    assert_eq!(g.payoffs(&s), vec![Chips(-300), Chips(-500)]);
}

#[test]
fn tempest_ante_rises_until_a_showdown_and_only_shoves_or_folds() {
    let g = game("holdem_tempest");
    let mut s = carry_hand(&g, &ScriptCarry::default(), &["2c", "3d", "7h", "8s"]);
    // Ante 50¢ each + $2 Giant Blind from s1 (left of the button). s0 acts first.
    let mut legal = Vec::new();
    g.legal_actions(&s, &mut legal);
    assert!(
        legal
            .iter()
            .all(|a| matches!(a, Action::Fold | Action::RaiseTo(_) | Action::CheckCall))
    );
    assert!(
        legal
            .iter()
            .any(|a| matches!(a, Action::RaiseTo(x) if *x == Chips(2000) - Chips(50))),
        "{legal:?} stacks {:?}",
        s.betting.stacks()
    );
    g.apply(&mut s, Action::Fold);
    let c = g.carry_out(&s);
    assert_eq!(c.level, 1, "no showdown: the ante goes up");
    assert!(!c.over);
    let s2 = carry_hand(&g, &c, &["2c", "3d", "7h", "8s"]);
    assert_eq!(
        s2.betting.contributed()[0] + s2.betting.contributed()[1],
        Chips(100 + 100 + 200)
    );
}

#[test]
fn numbers_conformance() {
    check_conformance(
        &game("numbers"),
        &Conformance {
            hands: 60,
            seat_counts: vec![2, 3, 5],
            determinizations: 2,
            ..Conformance::default()
        },
    );
}

#[test]
fn seven_27_scores_closest_without_going_over() {
    let r = |half| ShareRule {
        name: "t",
        ranking: Ranking::Target {
            half,
            bust_ok: false,
        },
        build: Build::Own,
        board: 0,
    };
    let v = |cards: &str, half| {
        let own: Vec<Card> = cards.split_whitespace().map(card).collect();
        hand::share_value(&r(half), &own, &[], CardSet::EMPTY)
    };
    // 7 target (14 half points): K + 6 = 6.5 beats 5 + A = 6 (ace as 1; 16 would bust).
    assert!(v("Kc 6d", 14) > v("5c Ad", 14));
    // 27 target: A + A + 5 = 27 exactly (11 + 11 + 5).
    assert_eq!(v("Ac Ad 5h", 54).unwrap().0, 1_000);
    // Bust on 7 still makes a 27 hand.
    assert_eq!(v("9c 8d", 14), None);
    assert!(v("9c 8d", 54).is_some());
}

#[test]
fn numbers_throws_neighbours_and_gin_scoops() {
    let g = game("numbers");
    let setup = HandSetup::simple(2, Chips(10_000));
    let mut s = g.new_hand(&setup);
    // s1: all 7s and 8s and a 6 (gins on a 7); s0: kings and queens.
    let deal = [
        "7c", "Kc", "7d", "Kd", "8c", "Kh", "8d", "Qc", "6c", "Qd", "6d", "Qh",
    ];
    for c in deal {
        g.apply(&mut s, Action::Deal(card(c)));
    }
    g.apply(&mut s, Action::CheckCall);
    g.apply(&mut s, Action::CheckCall);
    g.apply(&mut s, Action::Deal(card("7h")));
    assert_eq!(s.gin, 0b10, "s1 threw away every card");
    assert_eq!(g.to_act(&s), ToAct::Terminal);
    // s1 scoops the $2 dealer ante (posted by s0, the chooser).
    assert_eq!(g.payoffs(&s), vec![Chips(-200), Chips(200)]);
}

#[test]
fn booray_follow_suit_trump_and_scoring() {
    let g = game("booray");
    // 2 seats, button s0 = dealer. Deal order s1, s0 for 4 down each, then the dealer's card up.
    // s1: 2c 3c 4c 5c (+ 6c) ; s0: Ah Kh Qh 2d (+ Jh up -> hearts are trump).
    let mut s = carry_hand(
        &g,
        &ScriptCarry::default(),
        &["2c", "Ah", "3c", "Kh", "4c", "Qh", "5c", "2d", "6c", "Jh"],
    );
    assert_eq!(s.trump, Some(engine::Suit::Hearts));
    g.apply(&mut s, Action::InOut(1));
    g.apply(&mut s, Action::InOut(1));
    g.apply(&mut s, Action::Draw(0));
    g.apply(&mut s, Action::Draw(0));
    // The dealer (s0) leads and may not lead trump before it's played: only 2d.
    let mut legal = Vec::new();
    g.legal_actions(&s, &mut legal);
    assert_eq!(legal, vec![Action::PlayCard(card("2d"))]);
    g.apply(&mut s, Action::PlayCard(card("2d")));
    // s1 has no diamonds: any card.
    g.legal_actions(&s, &mut legal);
    assert_eq!(legal.len(), 5);
    g.apply(&mut s, Action::PlayCard(card("6c")));
    // s0 won (the only diamond); s0 leads again, now any card (still no trump played: hearts
    // only left, so trump may be led).
    assert_eq!(s.tricks_won, vec![1, 0]);
    for _ in 0..4 {
        g.legal_actions(&s, &mut legal);
        g.apply(&mut s, legal[0]);
        g.legal_actions(&s, &mut legal);
        g.apply(&mut s, legal[0]);
    }
    assert_eq!(s.tricks_won, vec![5, 0]);
    let c = g.carry_out(&s);
    // s0 scooped but s1 fried: the game goes on with s1's stun in the pot.
    assert!(!c.over);
    assert!(c.pot > 0);
}

#[test]
fn booray_lone_player_scoops() {
    let g = game("booray");
    let mut s = carry_hand(
        &g,
        &ScriptCarry::default(),
        &["2c", "Ah", "3c", "Kh", "4c", "Qh", "5c", "2d", "6c", "Jh"],
    );
    g.apply(&mut s, Action::InOut(1));
    g.apply(&mut s, Action::InOut(0));
    let c = g.carry_out(&s);
    assert!(c.over);
    // Pot: $2 dealer ante (s0) + $1 + $1 antes = $4 to s1.
    assert_eq!(g.payoffs(&s), vec![Chips(-300), Chips(300)]);
}
