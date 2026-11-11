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
    /// Rollout policy below the search tree: `heuristic` (default) or `check-call` (the old one).
    #[arg(long, default_value = "heuristic")]
    pub rollout: String,
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

/// One evaluated action: (description, EV, SE, visits).
pub type Row = (String, f64, f64, u32);

/// Evaluate one spot: (what the hero sees, actions ranked best first).
fn evaluate(
    spec: &SpotFile,
    playouts: u32,
    threads: usize,
    check_call: bool,
) -> Result<(String, Vec<Row>), String> {
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
        dealt_out: 0,
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
    let per = (playouts / threads as u32).max(100);
    let results: Vec<_> = (0..threads)
        .into_par_iter()
        .map(|t| {
            let mut config = IsmctsConfig::default();
            if check_call {
                config.rollout = bots::ismcts::Rollout::CheckCallDown;
            }
            let mut search = Ismcts::<Holdem>::new(config);
            search.sampler = Box::new(sampler.clone());
            let mut rng = derive_rng(spec.seed, t as u64, Stream::Custom(9));
            search.evaluate(&game, &obs, &legal, Budget::Playouts(per), &mut rng)
        })
        .collect();
    // Visit-weighted mean per action; SE of a weighted mean of independent estimates.
    let mut rows: Vec<Row> = legal
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
    Ok((game.describe_observation(&obs), rows))
}

/// A batch of spots (FR-SP-6): `[[spots]]` entries, each a spot plus a `name`.
#[derive(Deserialize)]
struct GridFile {
    spots: Vec<NamedSpot>,
}

#[derive(Deserialize)]
struct NamedSpot {
    name: String,
    #[serde(flatten)]
    spot: SpotFile,
}

/// Evaluate every spot of a grid file and write an HTML table (returns its path).
pub fn run_grid(
    file: &std::path::Path,
    out: &std::path::Path,
    playouts: Option<u32>,
    threads: usize,
) -> Result<std::path::PathBuf, String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let grid: GridFile = toml::from_str(&text).map_err(|e| format!("{}: {e}", file.display()))?;
    let mut body = String::new();
    for (i, ns) in grid.spots.iter().enumerate() {
        let total = playouts.unwrap_or(ns.spot.playouts);
        let (_, rows) = evaluate(&ns.spot, total, threads, false)?;
        println!(
            "[{}/{}] {}: best {} ({:+.3})",
            i + 1,
            grid.spots.len(),
            ns.name,
            rows[0].0,
            rows[0].1
        );
        let best = rows[0].1;
        body += &format!(
            "<h2>{}</h2><p class=note>hero {} · board {} · {} playouts · EV in big blinds vs the given ranges</p><table><tr><th>Action</th><th>EV</th><th>± SE</th><th>vs best</th><th>Visits</th></tr>",
            esc(&ns.name),
            esc(&ns.spot.hero_cards),
            esc(if ns.spot.board.is_empty() {
                "-"
            } else {
                &ns.spot.board
            }),
            total
        );
        for (a, q, se, v) in &rows {
            body += &format!(
                "<tr><td>{}</td><td>{q:+.3}</td><td>{se:.3}</td><td>{:+.3}</td><td>{v}</td></tr>",
                esc(a),
                q - best
            );
        }
        body += "</table>";
    }
    let html = format!(
        "<!doctype html><html lang=en><head><meta charset=utf-8><meta name=viewport content='width=device-width, initial-scale=1'><title>Spot Grid</title><style>:root{{color-scheme:light dark}}body{{font:14px/1.45 system-ui,sans-serif;max-width:900px;margin:0 auto;padding:24px 16px;background:Canvas;color:CanvasText}}table{{border-collapse:collapse;width:100%;font-variant-numeric:tabular-nums}}td,th{{padding:5px 8px;border-top:1px solid #8884;text-align:left}}.note{{opacity:.7;font-size:12px}}</style></head><body><h1>Spot grid</h1>{body}</body></html>"
    );
    if let Some(p) = out.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    std::fs::write(out, html).map_err(|e| e.to_string())?;
    Ok(out.to_path_buf())
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn run(args: SpotArgs) -> CliResult {
    let threads = args
        .threads
        .unwrap_or_else(rayon::current_num_threads)
        .max(1);
    let check_call = match args.rollout.as_str() {
        "heuristic" => false,
        "check-call" => true,
        r => {
            return Err(format!(
                "unknown rollout {r:?}: use heuristic or check-call"
            ));
        }
    };
    let text =
        std::fs::read_to_string(&args.file).map_err(|e| format!("{}: {e}", args.file.display()))?;
    if text.contains("[[spots]]") {
        let out = args.file.with_extension("grid.html");
        let path = run_grid(&args.file, &out, args.playouts, threads)?;
        println!("grid: {}", path.display());
        return Ok(());
    }
    let spec: SpotFile =
        toml::from_str(&text).map_err(|e| format!("{}: {e}", args.file.display()))?;
    let total = args.playouts.unwrap_or(spec.playouts);
    let (seen, rows) = evaluate(&spec, total, threads, check_call)?;
    println!("{seen}");
    let best = rows[0].1;
    let per = (total / threads as u32).max(100);
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
