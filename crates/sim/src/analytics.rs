//! Player analytics from stored hands (PRD FR-PA-1…4). Human players only (PRD §5.6b).
//!
//! Everything is computed by **replaying** each hand record and classifying the player's
//! decisions with `Game::classify`, so the same definitions work for every game:
//!
//! - **Betting round**: decisions between chance events. Round 0 is the first betting round
//!   (preflop in Hold'em, 3rd street in FTQ).
//! - **VPIP**: put chips in voluntarily in round 0 (a call that cost something, or a raise).
//!   Checking the big blind doesn't count.
//! - **PFR**: raised in round 0. **3-bet**: raised in round 0 facing exactly one raise.
//! - **AF** (aggression factor): bets+raises ÷ calls, rounds ≥ 1.
//! - **C-bet**: the last round-0 raiser bets round 1 when it's checked to them (or they act first).
//! - **Fold to c-bet**: folded to that c-bet.
//! - **WTSD**: reached showdown, given they saw round 1. **W$SD**: won chips at showdown.
//! - **Fold to pressure**: folds ÷ decisions facing a bet (any round).
//!
//! Every frequency comes with its sample size and a 95% interval, because at the hand counts a
//! person actually plays, most of these are noisy: VPIP over 200 hands is ±~6 points.

use std::collections::BTreeMap;

use serde::Serialize;

use engine::{ActionClass, Card, Chips, Game, HandSetup, Rank, Seat, ToAct};

use crate::stats::Welford;

/// `hits` out of `chances`, e.g. VPIP.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Ratio {
    pub hits: u64,
    pub chances: u64,
}

impl Ratio {
    fn record(&mut self, hit: bool) {
        self.chances += 1;
        self.hits += hit as u64;
    }

    pub fn p(&self) -> Option<f64> {
        (self.chances > 0).then(|| self.hits as f64 / self.chances as f64)
    }

    /// 95% Wilson interval. Unlike p ± 1.96·√(p(1−p)/n) it behaves sensibly for small n
    /// and for p near 0 or 1, which is exactly where these stats often live.
    pub fn ci95(&self) -> Option<(f64, f64)> {
        let n = self.chances as f64;
        let p = self.p()?;
        let z = 1.96f64;
        let denom = 1.0 + z * z / n;
        let center = (p + z * z / (2.0 * n)) / denom;
        let half = z * (p * (1.0 - p) / n + z * z / (4.0 * n * n)).sqrt() / denom;
        Some(((center - half).max(0.0), (center + half).min(1.0)))
    }

    pub fn merge(&mut self, o: &Ratio) {
        self.hits += o.hits;
        self.chances += o.chances;
    }
}

/// Everything learned about one player in one game.
#[derive(Clone, Debug, Default, Serialize)]
pub struct PlayerStats {
    pub hands: u64,
    /// Result per hand in eval units (big blinds / antes).
    pub result: Welford,
    pub vpip: Ratio,
    pub pfr: Ratio,
    pub three_bet: Ratio,
    pub cbet: Ratio,
    pub fold_to_cbet: Ratio,
    pub wtsd: Ratio,
    pub won_at_showdown: Ratio,
    pub fold_to_pressure: Ratio,
    /// Bets + raises in rounds ≥ 1.
    pub aggressive: u64,
    /// Calls (with chips owed) in rounds ≥ 1.
    pub calls: u64,
    /// VPIP and PFR by position (seats after the button: 0 = button).
    pub by_position: BTreeMap<u8, (Ratio, Ratio)>,
    /// Hold'em-style two-card hands: class (e.g. "AKs") → (VPIP, PFR).
    pub range: BTreeMap<String, (Ratio, Ratio)>,
}

impl PlayerStats {
    /// Aggression factor, or `None` with no calls.
    pub fn af(&self) -> Option<f64> {
        (self.calls > 0).then(|| self.aggressive as f64 / self.calls as f64)
    }
}

/// Where a decision happened, for grouping coach notes (FR-PA-3).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Spot {
    pub round: u8,
    pub position: u8,
    /// "fold", "check", "call", "raise", "declare", "other".
    pub action: &'static str,
}

fn action_kind(c: ActionClass) -> &'static str {
    match c {
        ActionClass::Fold => "fold",
        ActionClass::CheckOrCall { owed } if owed == Chips::ZERO => "check",
        ActionClass::CheckOrCall { .. } => "call",
        ActionClass::BetOrRaise { .. } => "raise",
        ActionClass::Declare => "declare",
        _ => "other",
    }
}

/// "AKs", "AKo", "QQ" for a two-card hand (higher rank first).
pub fn hand_class(cards: &[Card]) -> Option<String> {
    let [a, b] = cards else { return None };
    let (hi, lo) = if a.rank() >= b.rank() { (a, b) } else { (b, a) };
    let (h, l) = (hi.rank().to_char(), lo.rank().to_char());
    Some(if hi.rank() == lo.rank() {
        format!("{h}{l}")
    } else if hi.suit() == lo.suit() {
        format!("{h}{l}s")
    } else {
        format!("{h}{l}o")
    })
}

/// Replay one hand and add `seat`'s facts to `stats`. Returns the spot of each of the seat's
/// decisions, keyed by index in the action log (to line up with coach notes).
pub fn add_hand<G: Game>(
    game: &G,
    setup: &HandSetup,
    actions: &[G::Action],
    seat: Seat,
    stats: &mut PlayerStats,
) -> BTreeMap<usize, Spot> {
    let n = setup.num_seats;
    let position = (seat.0 + n - setup.button.0) % n;
    let mut spots = BTreeMap::new();

    let mut state = game.new_hand(setup);
    let mut round: u8 = 0;
    let mut acted_since_chance = false;
    let mut raises_this_round = 0u32;
    let mut round0_aggressor: Option<Seat> = None;
    let mut first_r1_action_by: Vec<bool> = vec![false; n as usize]; // has seat acted in round 1?
    let mut cbet_live = false; // the round-0 aggressor just c-bet; others haven't answered yet
    let mut folded = vec![false; n as usize];
    let (mut vpip, mut pfr) = (false, false);
    let (mut had_r0_decision, mut saw_round1) = (false, false);
    let mut private = None;

    for (idx, &a) in actions.iter().enumerate() {
        let actor = match game.to_act(&state) {
            ToAct::Chance => {
                if acted_since_chance {
                    round = round.saturating_add(1);
                    acted_since_chance = false;
                    raises_this_round = 0;
                    if round == 1 && !folded[seat.index()] {
                        saw_round1 = true;
                    }
                }
                None
            }
            ToAct::Seat(p) => Some(p),
            ToAct::Terminal => break,
        };
        if let Some(p) = actor {
            let class = game.classify(&state, a);
            if p == seat {
                if private.is_none() {
                    private = Some(game.private_cards(&state, seat));
                }
                let facing = match class {
                    ActionClass::Fold => true,
                    ActionClass::CheckOrCall { owed } => owed > Chips::ZERO,
                    ActionClass::BetOrRaise { .. } => raises_this_round > 0,
                    _ => false,
                };
                if facing {
                    stats.fold_to_pressure.record(class == ActionClass::Fold);
                }
                let raise = matches!(class, ActionClass::BetOrRaise { .. });
                if round == 0 {
                    had_r0_decision = true;
                    vpip |= raise
                        || matches!(class, ActionClass::CheckOrCall { owed } if owed > Chips::ZERO);
                    pfr |= raise;
                    if raises_this_round == 1 {
                        stats.three_bet.record(raise);
                    }
                } else {
                    stats.aggressive += raise as u64;
                    stats.calls += matches!(class, ActionClass::CheckOrCall { owed } if owed > Chips::ZERO)
                        as u64;
                }
                if round == 1 && !first_r1_action_by[seat.index()] {
                    if round0_aggressor == Some(seat) && raises_this_round == 0 {
                        stats.cbet.record(raise);
                    } else if cbet_live {
                        stats.fold_to_cbet.record(class == ActionClass::Fold);
                    }
                }
                spots.insert(
                    idx,
                    Spot {
                        round,
                        position,
                        action: action_kind(class),
                    },
                );
            }
            // Track the table for everyone's decisions.
            if round == 1 && !first_r1_action_by[p.index()] {
                first_r1_action_by[p.index()] = true;
                if Some(p) == round0_aggressor
                    && raises_this_round == 0
                    && matches!(class, ActionClass::BetOrRaise { .. })
                {
                    cbet_live = true;
                }
            }
            if matches!(class, ActionClass::BetOrRaise { .. }) {
                if round == 0 {
                    round0_aggressor = Some(p);
                }
                if round == 1 && Some(p) != round0_aggressor {
                    cbet_live = false; // someone raised over it: no longer "facing a c-bet"
                }
                raises_this_round += 1;
            }
            if class == ActionClass::Fold {
                folded[p.index()] = true;
            }
            acted_since_chance = true;
        }
        game.apply(&mut state, a);
    }

    let payoff = game.payoffs(&state)[seat.index()];
    stats.hands += 1;
    stats
        .result
        .add(payoff.0 as f64 / game.eval_unit().0 as f64);
    if had_r0_decision {
        stats.vpip.record(vpip);
        stats.pfr.record(pfr);
        let (v, p) = stats.by_position.entry(position).or_default();
        v.record(vpip);
        p.record(pfr);
        if let Some(class) = private.as_deref().and_then(hand_class) {
            let (v, p) = stats.range.entry(class).or_default();
            v.record(vpip);
            p.record(pfr);
        }
    }
    let live = folded.iter().filter(|f| !**f).count();
    let showdown = !folded[seat.index()] && live > 1;
    if saw_round1 {
        stats.wtsd.record(showdown);
    }
    if showdown {
        stats.won_at_showdown.record(payoff > Chips::ZERO);
    }
    spots
}

/// One row of the EV leak report (FR-PA-3).
#[derive(Clone, Debug, Serialize)]
pub struct Leak {
    pub spot: Spot,
    pub decisions: u64,
    /// Sum of EV lost (eval units) and its standard error.
    pub total_loss: f64,
    pub total_se: f64,
    /// Decisions where the chosen action was the evaluator's best.
    pub matched_best: u64,
    pub flagged: u64,
}

/// Group (spot, ev_loss, se, flagged) per decision and keep spots whose total loss is
/// significant (> 2 SE), worst first. SE of a sum of independent estimates = √Σ se².
pub fn leak_report(notes: impl IntoIterator<Item = (Spot, Option<f64>, f64, bool)>) -> Vec<Leak> {
    let mut by_spot: BTreeMap<Spot, Leak> = BTreeMap::new();
    for (spot, loss, se, flagged) in notes {
        let Some(loss) = loss else { continue }; // off-menu actions have no estimate
        let e = by_spot.entry(spot.clone()).or_insert_with(|| Leak {
            spot,
            decisions: 0,
            total_loss: 0.0,
            total_se: 0.0,
            matched_best: 0,
            flagged: 0,
        });
        e.decisions += 1;
        e.total_loss += loss;
        e.total_se += se * se; // sum of variances for now; square-rooted below
        e.matched_best += (loss.abs() < 1e-12) as u64;
        e.flagged += flagged as u64;
    }
    let mut leaks: Vec<Leak> = by_spot
        .into_values()
        .map(|mut l| {
            l.total_se = l.total_se.sqrt();
            l
        })
        .filter(|l| l.total_loss > 2.0 * l.total_se && l.total_loss > 0.0)
        .collect();
    leaks.sort_by(|a, b| b.total_loss.partial_cmp(&a.total_loss).unwrap());
    leaks
}

/// The 13×13 range grid as text: pairs on the diagonal, suited above, offsuit below. Each cell
/// is the VPIP % for that class, or `·` if never dealt.
pub fn range_grid(stats: &PlayerStats) -> String {
    let ranks: Vec<Rank> = Rank::ALL.iter().rev().copied().collect(); // A..2
    let mut out = String::from("     ");
    for r in &ranks {
        out += &format!("{:>4}", r.to_char());
    }
    out += "\n";
    for (i, r1) in ranks.iter().enumerate() {
        out += &format!("  {}  ", r1.to_char());
        for (j, r2) in ranks.iter().enumerate() {
            let class = match i.cmp(&j) {
                std::cmp::Ordering::Equal => format!("{}{}", r1.to_char(), r2.to_char()),
                std::cmp::Ordering::Less => format!("{}{}s", r1.to_char(), r2.to_char()),
                std::cmp::Ordering::Greater => format!("{}{}o", r2.to_char(), r1.to_char()),
            };
            match stats.range.get(&class).and_then(|(v, _)| v.p()) {
                Some(p) => out += &format!("{:>4.0}", p * 100.0),
                None => out += "   ·",
            }
        }
        out += "\n";
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use bots::baseline::{CallingStation, Maniac};
    use bots::{Policy, play_hand};
    use engine::rng::{Stream, derive_rng};
    use games::holdem::{self, Holdem};

    /// Play `hands` of 3-handed Hold'em with `hero` in seat 0 against random bots and collect
    /// seat 0's stats.
    fn stats_for(hero: &mut dyn Policy<Holdem>, hands: u64) -> PlayerStats {
        let game = Holdem::new(holdem::Config::default());
        let mut stats = PlayerStats::default();
        for h in 0..hands {
            let setup = HandSetup {
                button: Seat((h % 3) as u8),
                first_hand_of_game: false,
                ..HandSetup::simple(3, Chips(2_000))
            };
            let (mut a, mut b) = (bots::RandomPolicy, bots::RandomPolicy);
            let mut seats: [&mut dyn Policy<Holdem>; 3] = [&mut *hero, &mut a, &mut b];
            let mut deal = derive_rng(5, h, Stream::Deal);
            let mut rngs: Vec<_> = (0..3).map(|s| derive_rng(5, h, Stream::Seat(s))).collect();
            let r = play_hand(&game, &setup, &mut seats, &mut deal, &mut rngs);
            add_hand(&game, &setup, &r.actions, Seat(0), &mut stats);
        }
        stats
    }

    /// M7 exit criterion: planted leaks show up exactly.
    #[test]
    fn calling_station_never_raises_or_folds() {
        let s = stats_for(&mut CallingStation, 600);
        assert_eq!(s.hands, 600);
        assert_eq!(s.pfr.hits, 0);
        assert_eq!(s.aggressive, 0);
        assert_eq!(s.af(), Some(0.0));
        assert_eq!(s.fold_to_pressure.hits, 0, "{:?}", s.fold_to_pressure);
        // It calls whenever it owes something preflop: every hand except a BB check-through.
        assert!(s.vpip.p().unwrap() > 0.6, "{:?}", s.vpip);
    }

    #[test]
    fn maniac_raises_whenever_it_can() {
        let s = stats_for(&mut Maniac, 600);
        // It raises every time raising is legal. When a random opponent has already shoved
        // and nobody is left to raise against, it can only call, so PFR is high but not 100%.
        assert!(s.pfr.p().unwrap() > 0.75, "{:?}", s.pfr);
        assert!(
            s.vpip.p().unwrap() > 0.95,
            "never folds preflop: {:?}",
            s.vpip
        );
        assert_eq!(s.fold_to_pressure.hits, 0);
        assert!(
            s.af().is_none_or(|af| af > 3.0),
            "postflop it's almost all bets: {:?}",
            s.af()
        );
    }

    #[test]
    fn range_chart_counts_every_dealt_class() {
        let s = stats_for(&mut CallingStation, 400);
        let dealt: u64 = s.range.values().map(|(v, _)| v.chances).sum();
        assert_eq!(
            dealt, s.vpip.chances,
            "one range entry per hand with a preflop decision"
        );
        assert!(range_grid(&s).lines().count() == 14);
        assert_eq!(
            hand_class(&["As".parse().unwrap(), "Ks".parse().unwrap()]),
            Some("AKs".into())
        );
        assert_eq!(
            hand_class(&["2c".parse().unwrap(), "Ad".parse().unwrap()]),
            Some("A2o".into())
        );
        assert_eq!(
            hand_class(&["7h".parse().unwrap(), "7d".parse().unwrap()]),
            Some("77".into())
        );
    }

    #[test]
    fn wilson_interval_is_sane() {
        let r = Ratio {
            hits: 0,
            chances: 10,
        };
        let (lo, hi) = r.ci95().unwrap();
        assert_eq!(lo, 0.0);
        assert!(hi > 0.2 && hi < 0.35, "0/10 can't rule out ~30%: {hi}");
        let r = Ratio {
            hits: 300,
            chances: 1_000,
        };
        let (lo, hi) = r.ci95().unwrap();
        assert!(
            (lo - 0.272).abs() < 0.005 && (hi - 0.329).abs() < 0.005,
            "{lo} {hi}"
        );
    }

    #[test]
    fn leak_report_keeps_only_significant_spots() {
        let s = |action| Spot {
            round: 3,
            position: 0,
            action,
        };
        let notes = vec![
            (s("call"), Some(1.0), 0.1, true),
            (s("call"), Some(0.8), 0.1, true),
            (s("raise"), Some(0.05), 0.2, false), // noise
            (s("fold"), None, 0.1, false),        // off-menu: ignored
            (s("check"), Some(0.0), 0.05, false), // matched best
        ];
        let leaks = leak_report(notes);
        assert_eq!(leaks.len(), 1);
        assert_eq!(leaks[0].spot.action, "call");
        assert!((leaks[0].total_loss - 1.8).abs() < 1e-12);
        assert_eq!(leaks[0].flagged, 2);
    }
}
