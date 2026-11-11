//! The parallel hand loop (`docs/tech/sim.md` §2).
//!
//! **Reproducibility.** Deal `d` always uses `derive_rng(seed, d, Deal)`, and bots use streams
//! derived from `(deal, rotation, seat)`. So any deal's result is independent of thread count and
//! scheduling. Results are collected per batch **in deal order** and folded sequentially, so even
//! the floating-point sums come out bit-identical between runs.
//!
//! **Duplicate deals.** With `duplicate`, each deal is played once per rotation of bots through
//! seats, with identical cards. The unit of the statistics is the *deal* (the average over its
//! rotations), because results within one deal are strongly correlated; treating them as independent
//! hands would make the confidence interval look tighter than it is.

use std::time::Instant;

use rand::Rng;
use rayon::prelude::*;
use serde::Serialize;

use bots::{Policy, play_hand};
use engine::rng::{Stream, derive_rng};
use engine::{ActionClass, Chips, Game, HandSetup, Seat, StakeModel, ToAct};

use crate::encoding::{HandRecord, encode};
use crate::spec::RunSpec;
use crate::stats::Welford;

/// Where kept hand records go.
pub trait RecordSink {
    fn write(&mut self, record: &[u8]);
}

/// Discards records.
pub struct NullSink;
impl RecordSink for NullSink {
    fn write(&mut self, _: &[u8]) {}
}

/// Keeps records in memory (tests, small runs).
#[derive(Default)]
pub struct VecSink(pub Vec<u8>);
impl RecordSink for VecSink {
    fn write(&mut self, record: &[u8]) {
        self.0.extend_from_slice(record);
    }
}

/// Counts of each kind of action a bot took.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ActionCounts {
    pub fold: u64,
    pub check_call: u64,
    pub bet_raise: u64,
    pub other: u64,
}

impl ActionCounts {
    fn add(&mut self, c: ActionClass) {
        match c {
            ActionClass::Fold => self.fold += 1,
            ActionClass::CheckOrCall { .. } => self.check_call += 1,
            ActionClass::BetOrRaise { .. } => self.bet_raise += 1,
            _ => self.other += 1,
        }
    }

    fn merge(&mut self, o: &ActionCounts) {
        self.fold += o.fold;
        self.check_call += o.check_call;
        self.bet_raise += o.bet_raise;
        self.other += o.other;
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct BotReport {
    pub bot: String,
    /// Mean result per hand, in eval units (big blinds / antes).
    pub per_hand: f64,
    /// 95% CI half-width of `per_hand`.
    pub per_hand_ci95: f64,
    /// The same, per 100 hands (bb/100 for blind games).
    pub per_100: f64,
    pub per_100_ci95: f64,
    pub actions: ActionCounts,
}

#[derive(Clone, Debug, Serialize)]
pub struct Throughput {
    pub elapsed_sec: f64,
    pub threads: usize,
    pub hands_per_sec: f64,
    pub decisions_per_sec: f64,
    /// Average encoded record size, measured on every hand (kept or not).
    pub bytes_per_hand: f64,
    /// Storage per day if every hand's record were kept at this rate (FR-SM-3, sim.md §5).
    pub mb_per_day_full_history: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct RunReport {
    pub spec: RunSpec,
    pub hands: u64,
    pub bots: Vec<BotReport>,
    pub throughput: Throughput,
    pub records_kept: u64,
}

/// Everything one deal produced.
struct DealOutcome {
    /// Average result per bot over this deal's rotations (eval units).
    per_bot: Vec<f64>,
    actions: Vec<ActionCounts>,
    hands: u64,
    decisions: u64,
    bytes: u64,
    kept: Vec<Vec<u8>>,
}

/// Run `spec` on `game`. `on_progress(done, total)` is called after each batch.
pub fn run<G: Game>(
    game: &G,
    spec: &RunSpec,
    sink: &mut dyn RecordSink,
    mut on_progress: impl FnMut(u64, u64),
) -> RunReport {
    let n = spec.bots.len();
    assert!(
        game.meta().seats.contains(&(n as u8)),
        "{} seats not supported by {}",
        n,
        game.meta().id
    );
    let stack = match (spec.stack, game.meta().stake_model) {
        (Some(c), _) => Chips(c),
        (None, StakeModel::TableStake(x)) => x,
        (None, StakeModel::Bankroll) => Chips(game.eval_unit().0 * 100),
    };
    let unit = game.eval_unit().0 as f64;
    let rotations = if spec.duplicate { n } else { 1 };

    let start = Instant::now();
    let mut results = vec![Welford::default(); n];
    let mut actions = vec![ActionCounts::default(); n];
    let (mut hands, mut decisions, mut bytes, mut kept) = (0u64, 0u64, 0u64, 0u64);

    const BATCH: u64 = 1_024;
    let mut d0 = 0;
    while d0 < spec.deals {
        let d1 = (d0 + BATCH).min(spec.deals);
        // `into_par_iter` spreads the deals across all cores; `collect` keeps them in order.
        let batch: Vec<DealOutcome> = (d0..d1)
            .into_par_iter()
            .map(|d| play_deal(game, spec, d, n, rotations, stack, unit))
            .collect();
        for out in batch {
            for b in 0..n {
                results[b].add(out.per_bot[b]);
                actions[b].merge(&out.actions[b]);
            }
            hands += out.hands;
            decisions += out.decisions;
            bytes += out.bytes;
            for r in &out.kept {
                sink.write(r);
                kept += 1;
            }
        }
        d0 = d1;
        on_progress(d0, spec.deals);
    }

    let secs = start.elapsed().as_secs_f64().max(1e-9);
    let bytes_per_hand = bytes as f64 / hands.max(1) as f64;
    let hands_per_sec = hands as f64 / secs;
    RunReport {
        spec: spec.clone(),
        hands,
        bots: (0..n)
            .map(|b| BotReport {
                bot: spec.bots[b].to_string(),
                per_hand: results[b].mean,
                per_hand_ci95: results[b].ci95(),
                per_100: results[b].mean * 100.0,
                per_100_ci95: results[b].ci95() * 100.0,
                actions: actions[b],
            })
            .collect(),
        throughput: Throughput {
            elapsed_sec: secs,
            threads: rayon::current_num_threads(),
            hands_per_sec,
            decisions_per_sec: decisions as f64 / secs,
            bytes_per_hand,
            mb_per_day_full_history: hands_per_sec * bytes_per_hand * 86_400.0 / 1e6,
        },
        records_kept: kept,
    }
}

fn play_deal<G: Game>(
    game: &G,
    spec: &RunSpec,
    d: u64,
    n: usize,
    rotations: usize,
    stack: Chips,
    unit: f64,
) -> DealOutcome {
    let mut out = DealOutcome {
        per_bot: vec![0.0; n],
        actions: vec![ActionCounts::default(); n],
        hands: 0,
        decisions: 0,
        bytes: 0,
        kept: Vec::new(),
    };
    let button = Seat((d % n as u64) as u8);
    let setup = HandSetup {
        button,
        chooser: button,
        first_hand_of_game: spec.dealer_ante,
        ..HandSetup::simple(n as u8, stack)
    };
    let keep = derive_rng(spec.seed, d, Stream::Custom(7)).random::<f64>() < spec.history_sample;
    for r in 0..rotations {
        // seating[s] = which bot sits in seat s for this rotation.
        let seating: Vec<usize> = (0..n).map(|s| (s + r) % n).collect();
        let mut policies: Vec<Box<dyn Policy<G>>> =
            seating.iter().map(|&b| spec.bots[b].build::<G>()).collect();
        let mut seats: Vec<&mut dyn Policy<G>> = policies
            .iter_mut()
            .map(|p| p.as_mut() as &mut dyn Policy<G>)
            .collect();
        let mut deal_rng = derive_rng(spec.seed, d, Stream::Deal);
        let hand_id = d * rotations as u64 + r as u64;
        let mut rngs: Vec<_> = (0..n)
            .map(|s| derive_rng(spec.seed, hand_id, Stream::Seat(s as u8)))
            .collect();
        let result = play_hand(game, &setup, &mut seats, &mut deal_rng, &mut rngs);

        for (&bot, pay) in seating.iter().zip(&result.payoffs) {
            out.per_bot[bot] += pay.0 as f64 / unit / rotations as f64;
        }
        // Classify each decision (replaying: `classify` needs the state at that moment).
        let mut state = game.new_hand(&setup);
        for &a in &result.actions {
            if let ToAct::Seat(p) = game.to_act(&state) {
                out.actions[seating[p.index()]].add(game.classify(&state, a));
            }
            game.apply(&mut state, a);
        }
        let record = encode(&HandRecord::new(
            game.meta().id,
            game.meta().version,
            d,
            r as u8,
            seating.iter().map(|&b| b as u8).collect(),
            setup.clone(),
            result.actions,
        ));
        out.bytes += record.len() as u64;
        if keep {
            out.kept.push(record);
        }
        out.hands += 1;
        out.decisions += result.decisions as u64;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoding::decode_all;
    use crate::spec::BotSpec;
    use engine::replay::replay;
    use games::holdem::{self, Holdem};
    use games::kuhn::{self, Kuhn};

    fn spec(game: &str, bots: &[&str], deals: u64) -> RunSpec {
        RunSpec {
            game: game.into(),
            bots: bots.iter().map(|b| b.parse::<BotSpec>().unwrap()).collect(),
            deals,
            seed: 3,
            duplicate: true,
            history_sample: 0.0,
            stack: None,
            dealer_ante: false,
        }
    }

    #[test]
    fn identical_bots_break_even_and_results_are_reproducible() {
        let game = Holdem::new(holdem::Config::default());
        let s = spec("holdem_nl", &["random", "random", "random"], 2_000);
        let a = run(&game, &s, &mut NullSink, |_, _| {});
        let b = run(&game, &s, &mut NullSink, |_, _| {});
        assert_eq!(a.hands, 6_000);
        for (x, y) in a.bots.iter().zip(&b.bots) {
            assert_eq!(
                x.per_hand.to_bits(),
                y.per_hand.to_bits(),
                "bit-identical across runs"
            );
            assert!(
                x.per_hand.abs() < 3.0 * x.per_hand_ci95.max(1e-9) + 1e-9,
                "{x:?}"
            );
        }
        // Zero-sum: the bots' results cancel.
        let total: f64 = a.bots.iter().map(|b| b.per_hand).sum();
        assert!(total.abs() < 1e-9, "{total}");
    }

    #[test]
    fn duplicate_cancels_card_luck_between_identical_bots() {
        // With duplicate deals each bot plays both sides of the same cards, so the card luck
        // in a deal largely cancels and the CI is narrower than for the same number of
        // independent hands.
        let game = Kuhn::new(kuhn::Config {});
        let mut s = spec("kuhn", &["random", "random"], 4_000);
        let dup = run(&game, &s, &mut NullSink, |_, _| {});
        s.duplicate = false;
        s.deals = 8_000; // same number of hands
        let plain = run(&game, &s, &mut NullSink, |_, _| {});
        assert!(
            dup.bots[0].per_hand_ci95 < plain.bots[0].per_hand_ci95,
            "dup {:?} plain {:?}",
            dup.bots[0],
            plain.bots[0]
        );
    }

    #[test]
    fn ismcts_beats_random_with_significance() {
        let game = Kuhn::new(kuhn::Config {});
        let s = spec("kuhn", &["ismcts:5:200", "random"], 1_500);
        let r = run(&game, &s, &mut NullSink, |_, _| {});
        let bot = &r.bots[0];
        assert!(bot.per_hand - bot.per_hand_ci95 > 0.0, "{bot:?}");
    }

    #[test]
    fn kept_records_replay_exactly() {
        let game = Holdem::new(holdem::Config::default());
        let mut s = spec("holdem_nl", &["random", "random"], 200);
        s.history_sample = 0.25;
        let mut sink = VecSink::default();
        let r = run(&game, &s, &mut sink, |_, _| {});
        let records: Vec<HandRecord<holdem::Action>> = decode_all(&sink.0).unwrap();
        assert_eq!(records.len() as u64, r.records_kept);
        assert!(r.records_kept > 0 && r.records_kept < r.hands);
        for rec in records {
            let state = replay(&game, &rec.setup, &rec.actions).unwrap();
            assert_eq!(game.to_act(&state), ToAct::Terminal);
        }
        assert!(r.throughput.bytes_per_hand > 10.0);
    }
}
