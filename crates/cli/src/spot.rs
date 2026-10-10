//! `dc spot <spot.toml>`: rank the hero's actions in a described Hold'em situation, with
//! opponent ranges (FR-EV-2, `docs/tech/bots.md` §5).
//!
//! The search is root-parallel: `threads` independent ISMCTS searches with different seeds,
//! combined by visit-weighted averaging. Independent estimates of the same quantity average
//! cleanly, which makes this the simplest correct way to use more cores for one decision.

use std::collections::BTreeMap;
use std::path::PathBuf;

use clap::Args;
use rayon::prelude::*;
use serde::Deserialize;

use bots::ismcts::{Ismcts, IsmctsConfig};
use bots::ranges::{Range, RangeSampler};
use bots::{Budget, Evaluator};
use engine::rng::{Stream, derive_rng};
use engine::{Card, Chips, Game, HandSetup, Seat};
use games::holdem::{Config, Holdem};

use crate::CliResult;

#[derive(Args)]
pub struct SpotArgs {
    /// Spot description (TOML). See `docs/tech/bots.md` §5 for the format.
    pub file: PathBuf,
    /// Override the file's total playouts.
    #[arg(long)]
    pub playouts: Option<u32>,
    /// Parallel searches (default: all cores).
    #[arg(long)]
    pub threads: Option<usize>,
}

#[derive(Deserialize)]
struct SpotFile {
    seats: u8,
    button: u8,
    /// Dollars per seat.
    stacks: Vec<f64>,
    hero: u8,
    hero_cards: String,
    #[serde(default)]
    board: String,
    /// Betting so far, e.g. `["r 3", "c", "c"]` (see `dc play` input).
    #[serde(default)]
    actions: Vec<String>,
    /// Seat number (as a string key) → range, e.g. `"1" = "22+, A2s+"`.
    #[serde(default)]
    ranges: BTreeMap<String, String>,
    #[serde(default = "default_playouts")]
    playouts: u32,
    #[serde(default)]
    seed: u64,
}

fn default_playouts() -> u32 {
    200_000
}

fn cards(s: &str) -> Result<Vec<Card>, String> {
    let s: String = s.split_whitespace().collect();
    (0..s.len() / 2)
        .map(|i| {
            s[2 * i..2 * i + 2]
                .parse::<Card>()
                .map_err(|e| e.to_string())
        })
        .collect()
}

pub fn run(args: SpotArgs) -> CliResult {
    let text =
        std::fs::read_to_string(&args.file).map_err(|e| format!("{}: {e}", args.file.display()))?;
    let spec: SpotFile =
        toml::from_str(&text).map_err(|e| format!("{}: {e}", args.file.display()))?;
    let game = Holdem::new(Config::default());

    if spec.stacks.len() != spec.seats as usize {
        return Err(format!(
            "{} stacks for {} seats",
            spec.stacks.len(),
            spec.seats
        ));
    }
    let setup = HandSetup {
        num_seats: spec.seats,
        button: Seat(spec.button),
        chooser: Seat(spec.button),
        first_hand_of_game: false,
        stacks: spec.stacks.iter().map(|&d| Chips::dollars(d)).collect(),
    };
    let hero = Seat(spec.hero);
    let hc = cards(&spec.hero_cards)?;
    let hero_cards: [Card; 2] = hc
        .try_into()
        .map_err(|_| "hero_cards needs exactly two cards")?;
    let board = cards(&spec.board)?;
    let bets = spec
        .actions
        .iter()
        .map(|a| {
            game.parse_action(a, &[])
                .ok_or_else(|| format!("can't read action {a:?} (use f, c, r <dollars>)"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let state = game.spot_state(&setup, hero, hero_cards, &board, &bets)?;

    let mut sampler = RangeSampler::default();
    for (seat, text) in &spec.ranges {
        let s: u8 = seat
            .parse()
            .map_err(|_| format!("range key {seat:?} must be a seat number"))?;
        sampler.ranges.insert(
            Seat(s),
            Range::parse(text).map_err(|e| format!("range for seat {s}: {e}"))?,
        );
    }

    let obs = game.observe(&state, hero);
    let mut legal = Vec::new();
    game.legal_actions(&state, &mut legal);
    println!("{}", game.describe_observation(&obs));

    let threads = args
        .threads
        .unwrap_or_else(rayon::current_num_threads)
        .max(1);
    let total = args.playouts.unwrap_or(spec.playouts);
    let per = (total / threads as u32).max(100);
    let results: Vec<_> = (0..threads)
        .into_par_iter()
        .map(|t| {
            let mut search = Ismcts::<Holdem>::new(IsmctsConfig::default());
            search.sampler = Box::new(sampler.clone());
            let mut rng = derive_rng(spec.seed, t as u64, Stream::Custom(9));
            search.evaluate(&game, &obs, &legal, Budget::Playouts(per), &mut rng)
        })
        .collect();

    // Visit-weighted mean per action; SE of a weighted mean of independent estimates.
    let mut rows: Vec<(String, f64, f64, u32)> = legal
        .iter()
        .map(|&a| {
            let parts: Vec<_> = results
                .iter()
                .filter_map(|r| r.iter().find(|v| v.action == a))
                .collect();
            let visits: u32 = parts.iter().map(|v| v.visits).sum();
            let w = |v: u32| v as f64 / visits.max(1) as f64;
            let q = parts.iter().map(|v| w(v.visits) * v.q).sum();
            let se = parts
                .iter()
                .map(|v| (w(v.visits) * v.se).powi(2))
                .sum::<f64>()
                .sqrt();
            (game.describe_action(a), q, se, visits)
        })
        .collect();
    rows.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let best = rows[0].1;
    println!(
        "\n{} playouts on {threads} threads; EV in big blinds vs the given ranges:",
        per as u64 * threads as u64
    );
    println!(
        "  {:<24} {:>9} {:>8} {:>9} {:>9}",
        "action", "EV", "± SE", "vs best", "visits"
    );
    for (name, q, se, visits) in rows {
        println!(
            "  {name:<24} {q:>+9.3} {se:>8.3} {:>+9.3} {visits:>9}",
            q - best
        );
    }
    Ok(())
}
