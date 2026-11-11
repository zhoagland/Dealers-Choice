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

/// One share of a pot (High, Low, Top board, Spade, …) and how seats rank for it.
#[derive(Clone, Copy, Debug)]
pub struct Share<'a, V> {
    pub name: &'static str,
    /// Per seat; bigger is better. `None` = can't win this share (no low, no spade…).
    pub values: &'a [Option<V>],
}

/// What a seat declared it is playing for in [`award_shares`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Claim {
    /// Contests only share `k`.
    One(u8),
    /// Contests every share and must win **each one outright** (the "Both" rule): tying or
    /// losing any share knocks the seat out of the pot entirely.
    All,
}

/// Award pots that split into named shares (PRD §4; catalog §2 `award_shares`).
///
/// - `claims = None`: no declarations; every eligible seat contests every share (Chicago:
///   high hand + high spade).
/// - `claims = Some(c)`: seat `i` contests what `c[i]` says; `None` contests nothing.
///   [`Claim::All`] seats must be the sole best in every share; failed ones are removed
///   and the pot is decided among the rest.
/// - The pot is divided evenly among the shares that still have a contestant holding a value;
///   odd chips go to the earliest shares (so High before Low). A share nobody can win rolls
///   into the others.
/// - **OQ-3 (owner decision):** if every contestant left failed a `Claim::All`, the pot is
///   split **evenly** among them.
/// - A pot with one eligible seat goes to that seat.
pub fn award_shares<V: Ord + Copy>(
    pots: &[Pot],
    shares: &[Share<'_, V>],
    claims: Option<&[Option<Claim>]>,
    order: &[Seat],
) -> Vec<Chips> {
    assert!(!shares.is_empty(), "award_shares needs at least one share");
    let n = shares[0].values.len();
    assert!(
        shares.iter().all(|sh| sh.values.len() == n),
        "share values length"
    );
    let contests = |i: usize, k: usize| -> bool {
        match claims {
            None => true,
            Some(c) => match c[i] {
                Some(Claim::All) => true,
                Some(Claim::One(j)) => j as usize == k,
                None => false,
            },
        }
    };
    let mut out = vec![Chips::ZERO; n];
    for pot in pots {
        if pot.eligible.count_ones() == 1 {
            split(pot.amount, pot.eligible, order, &mut out);
            continue;
        }
        let side = |k: usize, exclude: u16| -> u16 {
            (0..n)
                .filter(|&i| pot.eligible & !exclude & (1 << i) != 0 && contests(i, k))
                .fold(0, |m, i| m | 1 << i)
        };
        // Scoopers survive only as the sole best of every share.
        let all_claim: u16 = match claims {
            None => 0,
            Some(c) => (0..n)
                .filter(|&i| pot.eligible & (1 << i) != 0 && c[i] == Some(Claim::All))
                .fold(0, |m, i| m | 1 << i),
        };
        let mut failed = 0u16;
        for i in (0..n).filter(|&i| all_claim & (1 << i) != 0) {
            let sole_everywhere = shares
                .iter()
                .enumerate()
                .all(|(k, sh)| best(side(k, 0), sh.values) == 1 << i);
            if !sole_everywhere {
                failed |= 1 << i;
            }
        }
        let winners: Vec<u16> = shares
            .iter()
            .enumerate()
            .map(|(k, sh)| best(side(k, failed), sh.values))
            .collect();
        let live = winners.iter().filter(|&&w| w != 0).count() as i64;
        if live == 0 {
            // OQ-3: nobody left but failed scoopers (or nobody can win any share).
            let pool = if failed != 0 { failed } else { pot.eligible };
            split(pot.amount, pool, order, &mut out);
            continue;
        }
        let portion = pot.amount.0 / live;
        let mut odd = pot.amount.0 % live;
        for &w in winners.iter().filter(|&&w| w != 0) {
            let extra = if odd > 0 { 1 } else { 0 };
            odd -= extra;
            split(Chips(portion + extra), w, order, &mut out);
        }
    }
    out
}

/// Award pots in a hi/lo declare game (PRD §4, FTQ house rules): a thin wrapper over
/// [`award_shares`] with shares High and Low, where High → `Claim::One(0)`, Low →
/// `Claim::One(1)`, Both → `Claim::All`. The high half takes the odd chip. If every remaining
/// contestant failed a Both declaration, the pot is split evenly among them (OQ-3).
pub fn award_declared<V: Ord + Copy>(
    pots: &[Pot],
    declarations: &[Option<Declare>],
    high: &[Option<V>],
    low: &[Option<V>],
    order: &[Seat],
) -> Vec<Chips> {
    let claims: Vec<Option<Claim>> = declarations
        .iter()
        .map(|d| {
            d.map(|d| match d {
                Declare::High => Claim::One(0),
                Declare::Low => Claim::One(1),
                Declare::Both => Claim::All,
            })
        })
        .collect();
    let shares = [
        Share {
            name: "High",
            values: high,
        },
        Share {
            name: "Low",
            values: low,
        },
    ];
    award_shares(pots, &shares, Some(&claims), order)
}

/// Seats in odd-chip priority order: first seat left of the button, clockwise.
pub fn order_from_button(button: Seat, num_seats: u8) -> Vec<Seat> {
    (1..=num_seats)
        .map(|k| Seat((button.0 + k) % num_seats))
        .collect()
}

/// A payment a seat owes outside the betting, e.g. a stun. The game decides where it goes
/// (usually into the next pot).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Penalty {
    pub seat: Seat,
    pub amount: Chips,
}

/// How stuns are sized (owner decision OQ-9, `docs/tech/game-catalog.md` §3).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StunRule {
    /// Cap for the first stunned player in a hand. House default $10.
    pub base_cap: Chips,
    /// How much higher each further stunned player's cap is, in the same hand. Picked by the
    /// chooser (`Config::stun_step`); default $0.
    pub step: Chips,
}

impl Default for StunRule {
    fn default() -> StunRule {
        StunRule {
            base_cap: Chips(1_000),
            step: Chips::ZERO,
        }
    }
}

/// What each stunned seat pays this hand: the pot, capped.
///
/// - The `k`-th stunned seat (k = 0, 1, …) is capped at `base_cap + k·step`.
/// - **D-STUN-1:** every stun is measured against `pot_before` (the pot before this hand's
///   stuns), not the pot growing as stuns are paid.
/// - **D-STUN-2:** stunned seats are taken in order starting left of the dealer (`button`).
/// - **D-G0-2:** a stun larger than the seat's stack takes the whole stack.
///
/// `stunned` is a bitmask over seats; `stacks[i]` is what seat `i` can pay.
pub fn stun_amounts(
    pot_before: Chips,
    stunned: u16,
    button: Seat,
    stacks: &[Chips],
    rule: StunRule,
) -> Vec<Penalty> {
    let n = stacks.len() as u8;
    order_from_button(button, n)
        .into_iter()
        .filter(|s| stunned & (1 << s.0) != 0)
        .enumerate()
        .map(|(k, seat)| {
            let cap = rule.base_cap + Chips(rule.step.0 * k as i64);
            let amount = pot_before
                .min(cap)
                .min(stacks[seat.index()])
                .max(Chips::ZERO);
            Penalty { seat, amount }
        })
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
    fn everyone_failing_both_splits_the_pot_evenly_oq3() {
        // Two Both declarers each win one side: both fail, nobody else is left.
        let pots = vec![Pot {
            amount: Chips(301),
            eligible: 0b11,
        }];
        let decl = [Some(Declare::Both), Some(Declare::Both)];
        let won = award_declared(
            &pots,
            &decl,
            &[Some(9), Some(1)],
            &[Some(1), Some(9)],
            &order(2),
        );
        assert_eq!(won, c(&[151, 150]), "even split, odd chip left of button");
    }

    #[test]
    fn shares_without_declarations_chicago_style() {
        // High hand and high spade, half each; nobody declares.
        let pots = vec![Pot {
            amount: Chips(400),
            eligible: 0b111,
        }];
        let shares = [
            Share {
                name: "High",
                values: &[Some(9), Some(5), Some(1)][..],
            },
            Share {
                name: "Spade",
                values: &[None, Some(3), Some(12)][..],
            },
        ];
        let won = award_shares(&pots, &shares, None, &order(3));
        assert_eq!(won, c(&[200, 0, 200]));
        // Nobody holds a spade: that half rolls to the high hand.
        let no_spade = [
            shares[0],
            Share {
                name: "Spade",
                values: &[None, None, None][..],
            },
        ];
        assert_eq!(
            award_shares(&pots, &no_spade, None, &order(3)),
            c(&[400, 0, 0])
        );
    }

    #[test]
    fn three_shares_with_claims_and_a_scooper() {
        let pots = vec![Pot {
            amount: Chips(301),
            eligible: 0b111,
        }];
        let a = [Some(5), Some(9), Some(1)];
        let b = [Some(9), Some(1), Some(1)];
        let d = [Some(9), Some(1), Some(5)];
        let shares = [
            Share {
                name: "A",
                values: &a[..],
            },
            Share {
                name: "B",
                values: &b[..],
            },
            Share {
                name: "C",
                values: &d[..],
            },
        ];
        // s0 scoops but loses A to s1 -> out; s1 takes A, s2 alone contests C, B rolls over.
        let claims = [Some(Claim::All), Some(Claim::One(0)), Some(Claim::One(2))];
        let won = award_shares(&pots, &shares, Some(&claims), &order(3));
        assert_eq!(won, c(&[0, 151, 150]));
        // If s0 also had the best A, it scoops everything.
        let a2 = [Some(10), Some(9), Some(1)];
        let shares2 = [
            Share {
                name: "A",
                values: &a2[..],
            },
            shares[1],
            shares[2],
        ];
        assert_eq!(
            award_shares(&pots, &shares2, Some(&claims), &order(3)),
            c(&[301, 0, 0])
        );
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

    #[test]
    fn stuns_are_the_pot_capped_rising_per_stunner_from_left_of_dealer() {
        let rule = StunRule {
            base_cap: Chips(1_000),
            step: Chips(200),
        };
        let stacks = c(&[5_000, 5_000, 5_000, 5_000]);
        // Button s2: order is s3, s0, s1. Stunned: s0, s1, s3. Pot $25.
        let p = stun_amounts(Chips(2_500), 0b1011, Seat(2), &stacks, rule);
        let got: Vec<(u8, i64)> = p.iter().map(|x| (x.seat.0, x.amount.0)).collect();
        assert_eq!(got, vec![(3, 1_000), (0, 1_200), (1, 1_400)]);
        // Small pot: everyone pays the pot measured before stuns (D-STUN-1), not a growing pot.
        let p = stun_amounts(Chips(300), 0b1011, Seat(2), &stacks, rule);
        assert!(p.iter().all(|x| x.amount == Chips(300)));
        // Short stack pays what it has (D-G0-2).
        let short = c(&[5_000, 150, 5_000, 5_000]);
        let p = stun_amounts(Chips(2_500), 0b0010, Seat(0), &short, rule);
        assert_eq!(
            p,
            vec![Penalty {
                seat: Seat(1),
                amount: Chips(150)
            }]
        );
        // Default rule: flat $10 cap.
        let p = stun_amounts(Chips(9_999), 0b0011, Seat(3), &stacks, StunRule::default());
        assert!(p.iter().all(|x| x.amount == Chips(1_000)));
        assert!(stun_amounts(Chips(500), 0, Seat(0), &stacks, rule).is_empty());
    }
}
