//! `dc sim` and `dc bench-sim` (FR-SM-1…4, `docs/tech/sim.md` §5).

use std::io::Write;
use std::path::PathBuf;

use clap::Args;

use engine::Game;
use games::registry::{GameVisitor, with_game};
use sim::{BotSpec, RecordSink, RunReport, RunSpec, run};
use storage::{DataRoot, ProfileKind, ProfileStore, ZstdRecordSink};

use crate::CliResult;

#[derive(Args)]
pub struct SimArgs {
    /// A RunSpec TOML file to start from.
    pub spec: Option<PathBuf>,
    /// Start from `config/sims/<name>.toml` (edit with `dc config`).
    #[arg(long, conflicts_with = "spec")]
    pub profile: Option<String>,
    // Flags below override the profile / file (or the defaults when neither is given).
    #[arg(long)]
    pub game: Option<String>,
    /// One bot per seat, e.g. `ismcts:5:500,random`.
    #[arg(long, value_delimiter = ',')]
    pub bots: Option<Vec<String>>,
    #[arg(long)]
    pub deals: Option<u64>,
    #[arg(long)]
    pub seed: Option<u64>,
    #[arg(long)]
    pub no_duplicate: bool,
    /// Fraction of hands whose records are kept.
    #[arg(long)]
    pub history_sample: Option<f64>,
    /// Where to write kept hand records (length-prefixed postcard).
    #[arg(long)]
    pub records: Option<PathBuf>,
    /// Save the run under <data_root>/sim/game=<id>/run=<id>/ (spec, summary, zstd records).
    #[arg(long)]
    pub save: bool,
    /// Where to write the JSON report (stdout summary always printed).
    #[arg(long)]
    pub report: Option<PathBuf>,
    /// Worker threads (default: all cores). Use fewer to keep the machine responsive.
    /// Results are identical for any thread count.
    #[arg(long)]
    pub threads: Option<usize>,
}

/// What `dc sim` runs when given nothing; also the starting point for new profiles in `dc config`.
pub fn default_spec() -> RunSpec {
    RunSpec {
        game: "holdem_nl".into(),
        bots: vec!["ismcts:3:300".parse().unwrap(), BotSpec::Random],
        deals: 1_000,
        seed: 1,
        duplicate: true,
        history_sample: 0.0,
        stack: None,
        dealer_ante: false,
    }
}

impl SimArgs {
    /// Apply the flags that were given on top of `spec`.
    fn apply(&self, mut spec: RunSpec) -> Result<RunSpec, String> {
        if let Some(g) = &self.game {
            spec.game = g.clone();
        }
        if let Some(b) = &self.bots {
            spec.bots = b
                .iter()
                .map(|b| b.parse::<BotSpec>())
                .collect::<Result<_, _>>()?;
        }
        if let Some(d) = self.deals {
            spec.deals = d;
        }
        if let Some(s) = self.seed {
            spec.seed = s;
        }
        if self.no_duplicate {
            spec.duplicate = false;
        }
        if let Some(h) = self.history_sample {
            spec.history_sample = h;
        }
        Ok(spec)
    }
}

/// Size rayon's global thread pool. Must run before any parallel work.
fn set_threads(threads: Option<usize>) -> CliResult {
    if let Some(n) = threads {
        rayon::ThreadPoolBuilder::new()
            .num_threads(n.max(1))
            .build_global()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Appends records to a file through a buffer.
struct FileSink(std::io::BufWriter<std::fs::File>);
impl RecordSink for FileSink {
    fn write(&mut self, record: &[u8]) {
        self.0.write_all(record).expect("writing hand records");
    }
}

pub fn sim(args: SimArgs, root: &DataRoot, profiles: &ProfileStore) -> CliResult {
    set_threads(args.threads)?;
    let base: RunSpec = match (&args.spec, &args.profile) {
        (Some(path), _) => {
            let text =
                std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
            toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?
        }
        (None, Some(name)) => profiles.load(ProfileKind::Sim, name)?,
        (None, None) => default_spec(),
    };
    let spec = args.apply(base)?;
    let save_dir = args
        .save
        .then(|| root.sim_run(&spec.game, &storage::files::run_id(spec.seed)));
    let report = run_spec(&spec, args.records.as_ref(), save_dir.as_ref())?;
    if let Some(dir) = &save_dir {
        storage::files::write_run_files(dir, &spec, &report).map_err(|e| e.to_string())?;
        println!("saved run to {}", dir.display());
    }
    print_report(&report);
    if let Some(path) = &args.report {
        let json = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("report written to {}", path.display());
    }
    Ok(())
}

/// Run a spec. Records go to `records` (a plain file) if given, else into `save_dir` as zstd.
fn run_spec(
    spec: &RunSpec,
    records: Option<&PathBuf>,
    save_dir: Option<&PathBuf>,
) -> Result<RunReport, String> {
    struct Run<'a> {
        spec: &'a RunSpec,
        records: Option<&'a PathBuf>,
        save_dir: Option<&'a PathBuf>,
    }
    impl GameVisitor<Result<RunReport, String>> for Run<'_> {
        fn visit<G: Game>(self, game: G) -> Result<RunReport, String> {
            let n = self.spec.bots.len() as u8;
            if !game.meta().seats.contains(&n) {
                return Err(format!(
                    "{} supports {:?} seats, not {n}",
                    game.meta().id,
                    game.meta().seats
                ));
            }
            let mut sink: Box<dyn RecordSink> = match self.records {
                Some(p) => Box::new(FileSink(std::io::BufWriter::new(
                    std::fs::File::create(p).map_err(|e| format!("{}: {e}", p.display()))?,
                ))),
                None => match self.save_dir {
                    Some(dir) => Box::new(
                        ZstdRecordSink::new(dir).map_err(|e: std::io::Error| e.to_string())?,
                    ),
                    None => Box::new(sim::NullSink),
                },
            };
            // Progress only on an interactive terminal; piped/logged output stays clean.
            let tty = std::io::IsTerminal::is_terminal(&std::io::stderr());
            Ok(run(&game, self.spec, sink.as_mut(), |done, total| {
                if tty {
                    eprint!("\r  {done}/{total} deals");
                }
            }))
        }
    }
    let r = with_game(
        &spec.game,
        None,
        Run {
            spec,
            records,
            save_dir,
        },
    )
    .map_err(|e| e.to_string())?;
    eprintln!();
    r
}

fn print_report(r: &RunReport) {
    println!(
        "{}: {} hands ({} deals{}), seed {}",
        r.spec.game,
        r.hands,
        r.spec.deals,
        if r.spec.duplicate { ", duplicate" } else { "" },
        r.spec.seed
    );
    println!(
        "  {:<18} {:>12} {:>12}   fold / call / raise",
        "bot", "per 100", "95% CI"
    );
    for b in &r.bots {
        println!(
            "  {:<18} {:>+12.2} {:>12.2}   {} / {} / {}",
            b.bot,
            b.per_100,
            b.per_100_ci95,
            b.actions.fold,
            b.actions.check_call,
            b.actions.bet_raise
        );
    }
    let t = &r.throughput;
    println!(
        "  {:.0} hands/s, {:.0} decisions/s on {} threads; {:.0} B/hand → {:.1} MB/day at 100% history",
        t.hands_per_sec,
        t.decisions_per_sec,
        t.threads,
        t.bytes_per_hand,
        t.mb_per_day_full_history
    );
}

#[derive(Args)]
pub struct BenchArgs {
    /// Deals per workload (bigger = steadier numbers).
    #[arg(long, default_value_t = 2_000)]
    pub deals: u64,
    /// Worker threads (default: all cores).
    #[arg(long)]
    pub threads: Option<usize>,
}

/// Fixed workloads for the M3 data-rate table (sim.md §5).
pub fn bench(args: BenchArgs) -> CliResult {
    set_threads(args.threads)?;
    let workloads: [(&str, &[&str], u64); 5] = [
        ("kuhn", &["random", "random"], args.deals * 50),
        ("holdem_nl", &["random"; 2], args.deals * 10),
        ("holdem_nl", &["random"; 6], args.deals * 10),
        ("holdem_nl", &["ismcts:7:2000", "random"], args.deals / 20),
        (
            "holdem_nl",
            &["ismcts:7:2000"; 6],
            (args.deals / 200).max(2),
        ),
    ];
    println!("| game | bots | hands/s | decisions/s | B/hand | MB/day @100% |");
    println!("|---|---|---|---|---|---|");
    for (game, bots, deals) in workloads {
        let spec = RunSpec {
            game: game.into(),
            bots: bots.iter().map(|b| b.parse()).collect::<Result<_, _>>()?,
            deals: deals.max(1),
            seed: 1,
            duplicate: true,
            history_sample: 0.0,
            stack: None,
            dealer_ante: false,
        };
        let r = run_spec(&spec, None, None)?;
        let t = &r.throughput;
        println!(
            "| {game} | {} | {:.0} | {:.0} | {:.0} | {:.1} |",
            bots.join(","),
            t.hands_per_sec,
            t.decisions_per_sec,
            t.bytes_per_hand,
            t.mb_per_day_full_history
        );
    }
    Ok(())
}
