//! Splitting the pot: side pots, hi/lo halves, declarations, odd chips. PRD FR-EC-3, §4;
//! `docs/tech/engine.md` §5.
//!
//! Hand strengths come in as any ordered type `V` (bigger = better). The evaluators'
//! `HandValue` is the usual one, but tests can use plain integers. Keeping this module
//! independent of hand evaluation means pot logic can be tested without building real hands.

use serde::{Deserialize, Serialize};

use crate::game::{Chips, Seat};

/// One pot and who can win it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pot {
    pub amount: Chips,
    /// Bitmask of seats eligible to win this pot.
    pub eligible: u16,
}

/// A hi/lo declaration (FTQ and other declare games).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Declare {
    High,
    Low,
    Both,
}

/// Build the main pot and side pots from what each seat put in this hand.
///
/// Folded seats' chips are in the pots, but folded seats are never eligible. Method: each
/// distinct contribution level forms a layer; layer *k* holds every seat's chips between level
/// *k−1* and level *k*, and is won by live seats that reached level *k*. A layer nobody live
/// reached (only a folded seat put that much in) joins the previous pot. A bettor's uncalled
/// excess forms a layer only they're eligible for, so it simply comes back to them.
pub fn side_pots(contributed: &[Chips], folded: u16) -> Vec<Pot> {
    let mut levels: Vec<Chips> = contributed.iter().copied().filter(|c| c.0 > 0).collect();
    levels.sort();
    levels.dedup();

    let mut pots: Vec<Pot> = Vec::new();
    let mut prev = Chips::ZERO;
    let mut carry = Chips::ZERO; // chips from layers with no eligible seat yet
    for &level in &levels {
        let amount: Chips = contributed
            .iter()
            .map(|&c| c.min(level) - c.min(prev))
            .sum();
        let eligible = contributed
            .iter()
            .enumerate()
            .filter(|&(i, &c)| c >= level && folded & (1 << i) == 0)
            .fold(0u16, |m, (i, _)| m | 1 << i);
        prev = level;
        if eligible == 0 {
            match pots.last_mut() {
                Some(p) => p.amount += amount,
                None => carry += amount,
            }
        } else if pots.last().is_some_and(|p| p.eligible == eligible) {
            // Same contestants as the previous layer: no need for a separate pot.
            pots.last_mut().unwrap().amount += amount + carry;
            carry = Chips::ZERO;
        } else {
            pots.push(Pot {
                amount: amount + carry,
                eligible,
            });
            carry = Chips::ZERO;
        }
    }
    debug_assert_eq!(carry, Chips::ZERO, "chips left with no eligible winner");
    pots
}

/// [`side_pots`] from live money, plus dead money (antes) added to the main pot, which every live
/// seat may win. If there's no live money at all (everyone checked), the dead money is the only
/// pot.
pub fn build_pots(live: &[Chips], dead: Chips, folded: u16) -> Vec<Pot> {
    let mut pots = side_pots(live, folded);
    let all_live = ((1u32 << live.len()) - 1) as u16 & !folded;
    match pots.first_mut() {
        // The main pot already includes every live seat that put in live money. A live seat
        // that put in none (only possible if all-in from the ante) still shares the dead money.
        Some(main) => {
            main.amount += dead;
            main.eligible |= all_live;
        }
        None if dead > Chips::ZERO => pots.push(Pot {
            amount: dead,
            eligible: all_live,
        }),
        None => {}
    }
    pots
}

/// Split `amount` evenly among `winners`; the odd chips (cents) go one each to the winners
/// earliest in `order` (normally starting left of the button).
pub fn split(amount: Chips, winners: u16, order: &[Seat], out: &mut [Chips]) {
    let k = winners.count_ones() as i64;
    assert!(k > 0, "split with no winners");
    let share = amount.0 / k;
    let mut odd = amount.0 % k;
    for &s in order {
        if winners & (1 << s.0) != 0 {
            out[s.index()] += Chips(share + if odd > 0 { 1 } else { 0 });
            odd = odd.saturating_sub(1);
        }
    }
}

/// Seats (from `candidates`) holding the best value. `values[i] == None` counts as no hand.
fn best<V: Ord + Copy>(candidates: u16, values: &[Option<V>]) -> u16 {
    let top = (0..values.len())
        .filter(|&i| candidates & (1 << i) != 0)
        .filter_map(|i| values[i])
        .max();
    match top {
        None => 0,
        Some(top) => (0..values.len())
            .filter(|&i| candidates & (1 << i) != 0 && values[i] == Some(top))
            .fold(0, |m, i| m | 1 << i),
    }
}

/// Award pots in a high-only game. Returns winnings per seat.
pub fn award_high<V: Ord + Copy>(pots: &[Pot], values: &[Option<V>], order: &[Seat]) -> Vec<Chips> {
    let mut out = vec![Chips::ZERO; values.len()];
    for pot in pots {
        let winners = if pot.eligible.count_ones() == 1 {
            pot.eligible
        } else {
            best(pot.eligible, values)
        };
        split(pot.amount, winners, order, &mut out);
    }
    out
}

/// Award pots in a declare game (PRD §4, FTQ house rules):
/// - High contestants declared High or Both; low contestants declared Low or Both.
/// - A Both declarer must win **both** sides outright (no ties). One who doesn't is out of the
///   pot entirely, and the pot is decided among the others.
/// - If both sides have contestants: high half (odd chip → high) to the best high, low half to
///   the best low. If only one side has contestants, that side takes the whole pot.
/// - Fallback (OQ-3): if every eligible seat was a failed Both declarer, the pot is split as
///   an ordinary hi/lo pot among them, ignoring declarations.
pub fn award_declared<V: Ord + Copy>(
    pots: &[Pot],
    declarations: &[Option<Declare>],
    high: &[Option<V>],
    low: &[Option<V>],
    order: &[Seat],
) -> Vec<Chips> {
    let n = declarations.len();
    let mask_of = |pred: &dyn Fn(Declare) -> bool, within: u16| -> u16 {
        (0..n)
            .filter(|&i| within & (1 << i) != 0 && declarations[i].is_some_and(pred))
            .fold(0, |m, i| m | 1 << i)
    };
    let mut out = vec![Chips::ZERO; n];
    for pot in pots {
        if pot.eligible.count_ones() == 1 {
            split(pot.amount, pot.eligible, order, &mut out);
            continue;
        }
        let hi_side = mask_of(&|d| d != Declare::Low, pot.eligible);
        let lo_side = mask_of(&|d| d != Declare::High, pot.eligible);
        let both = mask_of(&|d| d == Declare::Both, pot.eligible);

        // A Both declarer survives only as the *sole* best on each side.
        let (best_hi, best_lo) = (best(hi_side, high), best(lo_side, low));
        let mut failed = 0u16;
        for i in (0..n).filter(|&i| both & (1 << i) != 0) {
            let sole_hi = best_hi == 1 << i;
            let sole_lo = best_lo == 1 << i;
            if !(sole_hi && sole_lo) {
                failed |= 1 << i;
            }
        }
        let (hi_side, lo_side) = (hi_side & !failed, lo_side & !failed);

        match (hi_side != 0, lo_side != 0) {
            (true, true) => {
                let lo_half = Chips(pot.amount.0 / 2);
                let hi_half = pot.amount - lo_half; // odd chip to high
                split(hi_half, best(hi_side, high), order, &mut out);
                split(lo_half, best(lo_side, low), order, &mut out);
            }
            (true, false) => split(pot.amount, best(hi_side, high), order, &mut out),
            (false, true) => split(pot.amount, best(lo_side, low), order, &mut out),
            (false, false) => {
                // Everyone left failed a Both declaration: plain hi/lo among them.
                let lo_half = Chips(pot.amount.0 / 2);
                split(
                    pot.amount - lo_half,
                    best(pot.eligible, high),
                    order,
                    &mut out,
                );
                split(lo_half, best(pot.eligible, low), order, &mut out);
            }
        }
    }
    out
}

/// Seats in odd-chip priority order: first seat left of the button, clockwise.
pub fn order_from_button(button: Seat, num_seats: u8) -> Vec<Seat> {
    (1..=num_seats)
        .map(|k| Seat((button.0 + k) % num_seats))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::rng_from_seed;
    use rand::Rng;

    fn c(v: &[i64]) -> Vec<Chips> {
        v.iter().map(|&x| Chips(x)).collect()
    }

    fn order(n: u8) -> Vec<Seat> {
        (0..n).map(Seat).collect()
    }

    #[test]
    fn three_way_all_in_side_pots() {
        // A 50 all-in, B 100 all-in, C 300.
        let pots = side_pots(&c(&[50, 100, 300]), 0);
        assert_eq!(
            pots,
            vec![
                Pot {
                    amount: Chips(150),
                    eligible: 0b111
                },
                Pot {
                    amount: Chips(100),
                    eligible: 0b110
                },
                Pot {
                    amount: Chips(200),
                    eligible: 0b100
                }, // C's uncalled excess comes back
            ]
        );
    }

    #[test]
    fn folded_chips_go_to_the_pot_they_reached() {
        // A 50 live all-in, B 100 folded, C 80 live.
        let pots = side_pots(&c(&[50, 100, 80]), 0b010);
        assert_eq!(
            pots,
            vec![
                Pot {
                    amount: Chips(150),
                    eligible: 0b101
                },
                Pot {
                    amount: Chips(80),
                    eligible: 0b100
                }
            ]
        );
    }

    #[test]
    fn short_stack_wins_only_main_pot() {
        // A (50) has the best hand, B (100) second, C (100) worst.
        let pots = side_pots(&c(&[50, 100, 100]), 0);
        let won = award_high(&pots, &[Some(3), Some(2), Some(1)], &order(3));
        assert_eq!(won, c(&[150, 100, 0]));
    }

    #[test]
    fn dead_money_goes_to_whoever_wins_the_main_pot() {
        // Seat 0 paid a $2 dealer ante (dead) and nobody bet: the winner (seat 1) gets it.
        let pots = build_pots(&c(&[0, 0]), Chips(200), 0);
        assert_eq!(
            pots,
            vec![Pot {
                amount: Chips(200),
                eligible: 0b11
            }]
        );
        assert_eq!(
            award_high(&pots, &[Some(1), Some(9)], &order(2)),
            c(&[0, 200])
        );
        // With live betting, dead money joins the main pot only.
        let pots = build_pots(&c(&[100, 300, 300]), Chips(200), 0);
        assert_eq!(
            pots[0],
            Pot {
                amount: Chips(500),
                eligible: 0b111
            }
        );
        assert_eq!(
            pots[1],
            Pot {
                amount: Chips(400),
                eligible: 0b110
            }
        );
    }

    #[test]
    fn odd_chip_goes_first_in_order() {
        let pots = vec![Pot {
            amount: Chips(101),
            eligible: 0b011,
        }];
        let won = award_high(
            &pots,
            &[Some(5), Some(5), None],
            &[Seat(1), Seat(2), Seat(0)],
        );
        assert_eq!(won, c(&[50, 51, 0]));
    }

    #[test]
    fn hi_lo_split_odd_chip_to_high() {
        let pots = vec![Pot {
            amount: Chips(301),
            eligible: 0b11,
        }];
        let decl = [Some(Declare::High), Some(Declare::Low)];
        let won = award_declared(
            &pots,
            &decl,
            &[Some(9), Some(1)],
            &[Some(1), Some(9)],
            &order(2),
        );
        assert_eq!(won, c(&[151, 150]));
    }

    #[test]
    fn both_declarer_who_scoops_takes_everything() {
        let pots = vec![Pot {
            amount: Chips(300),
            eligible: 0b111,
        }];
        let decl = [Some(Declare::Both), Some(Declare::High), Some(Declare::Low)];
        let won = award_declared(
            &pots,
            &decl,
            &[Some(9), Some(5), Some(1)],
            &[Some(9), Some(1), Some(5)],
            &order(3),
        );
        assert_eq!(won, c(&[300, 0, 0]));
    }

    #[test]
    fn both_declarer_who_ties_one_side_gets_nothing() {
        let pots = vec![Pot {
            amount: Chips(300),
            eligible: 0b111,
        }];
        let decl = [Some(Declare::Both), Some(Declare::High), Some(Declare::Low)];
        // Seat 0 ties seat 2 for low → seat 0 is out. Seat 1 wins high, seat 2 wins low.
        let won = award_declared(
            &pots,
            &decl,
            &[Some(9), Some(5), Some(1)],
            &[Some(7), Some(1), Some(7)],
            &order(3),
        );
        assert_eq!(won, c(&[0, 150, 150]));
    }

    #[test]
    fn failed_both_with_nobody_on_other_side_gives_challenger_the_pot() {
        // Bible: "If no one went the other way the whole pot goes to the challenger."
        let pots = vec![Pot {
            amount: Chips(200),
            eligible: 0b11,
        }];
        let decl = [Some(Declare::Both), Some(Declare::High)];
        let won = award_declared(
            &pots,
            &decl,
            &[Some(1), Some(9)],
            &[Some(9), Some(1)],
            &order(2),
        );
        assert_eq!(won, c(&[0, 200]));
    }

    #[test]
    fn side_nobody_declared_rolls_to_the_other() {
        let pots = vec![Pot {
            amount: Chips(200),
            eligible: 0b11,
        }];
        let decl = [Some(Declare::High), Some(Declare::High)];
        let won = award_declared(
            &pots,
            &decl,
            &[Some(1), Some(9)],
            &[Some(9), Some(1)],
            &order(2),
        );
        assert_eq!(won, c(&[0, 200]));
    }

    #[test]
    fn random_pots_conserve_chips() {
        let mut rng = rng_from_seed(4);
        for _ in 0..5_000 {
            let n = rng.random_range(2..=9usize);
            let contributed: Vec<Chips> =
                (0..n).map(|_| Chips(rng.random_range(0..=500))).collect();
            let mut folded: u16 = (0..n)
                .filter(|_| rng.random_bool(0.3))
                .fold(0, |m, i| m | 1 << i);
            // At least one live seat must have put in the most, as in a real hand.
            let top = (0..n).max_by_key(|&i| contributed[i]).unwrap();
            folded &= !(1 << top);
            let total: Chips = contributed.iter().copied().sum();
            let pots = side_pots(&contributed, folded);
            assert_eq!(pots.iter().map(|p| p.amount).sum::<Chips>(), total);
            let values: Vec<Option<u8>> = (0..n).map(|_| Some(rng.random_range(0..5))).collect();
            let won = award_high(&pots, &values, &order(n as u8));
            assert_eq!(won.iter().copied().sum::<Chips>(), total);
            for (i, w) in won.iter().enumerate() {
                assert!(
                    folded & (1 << i) == 0 || *w == Chips::ZERO,
                    "folded seat won chips"
                );
            }
        }
    }
}
