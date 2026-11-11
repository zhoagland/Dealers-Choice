//! `dc campaign`: the simulation program's campaigns (`docs/tech/sim-program.md` §1–§5,
//! PRD FR-SP-1…8).
//!
//! A campaign file (`config/campaigns/<name>.toml`) expands to many runs. `dc campaign run`
//! copies the current binary to `<data_root>/bin/dc-<sha>.exe` and runs that frozen copy, so a
//! long campaign never blocks `cargo build` and never mixes code versions. Runs are split into
//! chunks with independent seeds; finished chunks are files, so a killed or paused campaign
//! resumes where it stopped. `dc campaign report` writes one self-contained HTML page.
//!
//! Layout under `<data_root>/campaigns/<name>/`:
//! ```text
//! campaign.toml          the campaign as run
//! status.json            progress, rewritten after every chunk
//! run.lock / PAUSE       running lock (OS-held) / stop after the current chunk
//! runs/<id>/chunk-NNNN.json, summary.json
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};

use engine::Game;
use games::registry::{GameVisitor, with_game};
use sim::balance::BalanceStats;
use sim::{BotSpec, RunSpec};
use storage::DataRoot;

use crate::CliResult;
use crate::analyze::{self, default_games, game_info};
use crate::simulate::{run_spec, set_threads};

/// The commit this binary was built from (build.rs).
pub const GIT_SHA: &str = env!("DC_GIT_SHA");

const REPORT_TEMPLATE: &str = include_str!("../assets/campaign_report.html");

#[derive(Subcommand)]
pub enum CampaignCmd {
    /// Print the runs a campaign expands to, with time and disk estimates.
    Plan { file: PathBuf },
    /// Run (or resume) a campaign from its file, on a frozen copy of this binary.
    Run(RunArgs),
    /// Every campaign's progress.
    Status,
    /// Ask a running campaign to stop after its current chunk.
    Pause { name: String },
    /// Continue a paused or interrupted campaign.
    Resume(ResumeArgs),
    /// Write the campaign's HTML report (also done after each run finishes).
    Report { name: String },
}

#[derive(Args, Clone)]
pub struct RunArgs {
    pub file: PathBuf,
    #[command(flatten)]
    pub exec: ExecArgs,
}

#[derive(Args, Clone)]
pub struct ResumeArgs {
    pub name: String,
    #[command(flatten)]
    pub exec: ExecArgs,
}

#[derive(Args, Clone, Default)]
pub struct ExecArgs {
    /// Worker threads (default: all cores but two).
    #[arg(long)]
    pub threads: Option<usize>,
    /// Run at lower priority so the PC stays responsive.
    #[arg(long)]
    pub nice: bool,
    /// Internal: this process is the frozen copy.
    #[arg(long, hide = true)]
    pub frozen: bool,
}

// ---------------------------------------------------------------------------------------------
// Campaign files
// ---------------------------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// FR-SP-4: how each game plays.
    Balance,
    /// FR-SP-5: strength presets rated against each other.
    Ladder,
    /// FR-SP-6: starting-hand and decision studies (runs `dc analyze`).
    Study,
    /// FR-SP-7: MCCFR blueprints.
    Blueprint,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CampaignFile {
    pub name: String,
    pub kind: Kind,
    /// Game ids, or `["all"]` for every Bible game.
    pub games: Vec<String>,
    #[serde(default = "default_seats")]
    pub seats: Vec<u8>,
    #[serde(default = "default_hands")]
    pub hands_per_run: u64,
    #[serde(default = "default_chunk")]
    pub chunk_hands: u64,
    #[serde(default)]
    pub seed: u64,
    /// Play every deal with the bots rotated through every seat.
    #[serde(default = "yes")]
    pub duplicate: bool,
    #[serde(default)]
    pub population: Population,
    /// House-rule what-ifs: `"<game>.<config field>" = [values…]` (`"*.<field>"` for every game).
    #[serde(default)]
    pub sweep: BTreeMap<String, Vec<toml::Value>>,
    /// Blueprint campaigns: MCCFR iterations per game.
    #[serde(default)]
    pub iterations: Option<u64>,
    /// Study campaigns: a spot grid (`dc spot` file with `[[spots]]`) evaluated into the report.
    #[serde(default)]
    pub spots: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Population {
    /// Bot specs. Balance: dealt around the table in order. Ladder: tables are windows over it.
    #[serde(default)]
    pub mix: Vec<String>,
}

fn default_seats() -> Vec<u8> {
    vec![2, 4, 6]
}
fn default_hands() -> u64 {
    20_000
}
fn default_chunk() -> u64 {
    10_000
}
fn yes() -> bool {
    true
}

impl CampaignFile {
    pub fn load(path: &Path) -> Result<CampaignFile, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let c: CampaignFile =
            toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if c.name.is_empty()
            || !c
                .name
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
        {
            return Err("campaign names use letters, digits, - and _".into());
        }
        Ok(c)
    }

    fn game_ids(&self) -> Vec<String> {
        if self.games.iter().any(|g| g == "all") {
            default_games()
        } else {
            self.games.clone()
        }
    }

    fn mix(&self) -> Vec<String> {
        if self.population.mix.is_empty() {
            vec!["random".into(), "callstation".into(), "maniac".into()]
        } else {
            self.population.mix.clone()
        }
    }

    /// Every combination of sweep values that applies to `game`, as config tables (one empty
    /// table when nothing is swept).
    fn sweeps_for(&self, game: &str) -> Vec<(BTreeMap<String, toml::Value>, Option<toml::Table>)> {
        let keys: Vec<(&String, &Vec<toml::Value>, String)> = self
            .sweep
            .iter()
            .filter_map(|(k, v)| {
                let (g, field) = k.split_once('.')?;
                (g == game || g == "*").then(|| (k, v, field.to_string()))
            })
            .collect();
        let mut combos: Vec<BTreeMap<String, toml::Value>> = vec![BTreeMap::new()];
        for (k, values, _) in &keys {
            combos = combos
                .into_iter()
                .flat_map(|c| {
                    values.iter().map(move |v| {
                        let mut c = c.clone();
                        c.insert((*k).clone(), v.clone());
                        c
                    })
                })
                .collect();
        }
        combos
            .into_iter()
            .map(|c| {
                if c.is_empty() {
                    return (c, None);
                }
                let mut t = toml::Table::new();
                for (k, v) in &c {
                    let field = k.split_once('.').map(|x| x.1).unwrap_or(k);
                    t.insert(field.to_string(), v.clone());
                }
                (c, Some(t))
            })
            .collect()
    }

    /// The runs this campaign expands to.
    pub fn expand(&self) -> Result<Vec<PlannedRun>, String> {
        let mix = self.mix();
        for b in &mix {
            b.parse::<BotSpec>()
                .map_err(|e| format!("population: {e}"))?;
        }
        let mut out = Vec::new();
        for game in self.game_ids() {
            let (_, range, _) = game_info(&game)?;
            for &s in &self.seats {
                if !range.contains(&s) {
                    continue;
                }
                let tables: Vec<Vec<String>> = match self.kind {
                    Kind::Ladder => (0..mix.len())
                        .map(|r| {
                            (0..s as usize)
                                .map(|i| mix[(r + i) % mix.len()].clone())
                                .collect()
                        })
                        .collect(),
                    _ => vec![
                        (0..s as usize)
                            .map(|i| mix[i % mix.len()].clone())
                            .collect(),
                    ],
                };
                for (ti, bots) in tables.into_iter().enumerate() {
                    for (si, (sweep, config)) in self.sweeps_for(&game).into_iter().enumerate() {
                        out.push(PlannedRun {
                            id: format!("{game}-s{s}-t{ti}-w{si}"),
                            game: game.clone(),
                            seats: s,
                            bots: bots.clone(),
                            sweep: sweep.into_iter().map(|(k, v)| (k, v.to_string())).collect(),
                            config,
                        });
                    }
                }
            }
        }
        if out.is_empty() {
            return Err("the campaign expands to no runs (check games and seats)".into());
        }
        Ok(out)
    }

    fn rotations(&self, seats: u8) -> u64 {
        if self.duplicate { seats as u64 } else { 1 }
    }

    fn deals_per_run(&self, seats: u8) -> u64 {
        (self.hands_per_run / self.rotations(seats)).max(1)
    }

    fn deals_per_chunk(&self, seats: u8) -> u64 {
        (self.chunk_hands / self.rotations(seats)).max(1)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlannedRun {
    pub id: String,
    pub game: String,
    pub seats: u8,
    pub bots: Vec<String>,
    /// Swept keys and their values for this run.
    pub sweep: BTreeMap<String, String>,
    pub config: Option<toml::Table>,
}

// ---------------------------------------------------------------------------------------------
// Results
// ---------------------------------------------------------------------------------------------

/// One finished chunk.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Chunk {
    hands: u64,
    deals: u64,
    elapsed_sec: f64,
    /// (bot, mean per hand, 95 % CI half-width) in eval units.
    bots: Vec<(String, f64, f64)>,
    balance: Option<BalanceStats>,
}

/// A run's merged result (FR-SP-8: records the binary and game version).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Summary {
    pub id: String,
    pub game: String,
    pub game_name: String,
    pub game_version: u32,
    pub binary_sha: String,
    pub campaign: String,
    pub seats: u8,
    pub sweep: BTreeMap<String, String>,
    pub hands: u64,
    pub deals: u64,
    pub elapsed_sec: f64,
    /// Per seat's bot: (bot, mean per hand, 95 % CI half-width).
    pub bots: Vec<(String, f64, f64)>,
    pub balance: Option<BalanceStats>,
    pub eval_unit: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Status {
    pub name: String,
    pub kind: String,
    pub state: String,
    pub binary_sha: String,
    pub updated_unix: u64,
    pub runs: Vec<RunStatus>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RunStatus {
    pub id: String,
    pub chunks_done: u64,
    pub chunks: u64,
    pub hands: u64,
    pub hands_per_sec: f64,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn dir(root: &DataRoot, name: &str) -> PathBuf {
    root.0.join("campaigns").join(name)
}

fn write_json<T: Serialize>(path: &Path, v: &T) -> CliResult {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(
        &tmp,
        serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

// ---------------------------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------------------------

pub fn run(cmd: CampaignCmd, root: &DataRoot) -> CliResult {
    match cmd {
        CampaignCmd::Plan { file } => plan(&CampaignFile::load(&file)?),
        CampaignCmd::Run(a) => {
            let c = CampaignFile::load(&a.file)?;
            start(c, a.exec, root)
        }
        CampaignCmd::Resume(a) => {
            let d = dir(root, &a.name);
            let text = std::fs::read_to_string(d.join("campaign.toml"))
                .map_err(|_| format!("no campaign {:?} under {}", a.name, root.0.display()))?;
            let c: CampaignFile = toml::from_str(&text).map_err(|e| e.to_string())?;
            start(c, a.exec, root)
        }
        CampaignCmd::Status => {
            let Ok(rd) = std::fs::read_dir(root.0.join("campaigns")) else {
                println!("no campaigns yet");
                return Ok(());
            };
            for e in rd.flatten() {
                if let Ok(s) = read_json::<Status>(&e.path().join("status.json")) {
                    let running = analyze::is_running(&e.path());
                    let done: u64 = s.runs.iter().map(|r| r.chunks_done).sum();
                    let total: u64 = s.runs.iter().map(|r| r.chunks).sum();
                    println!(
                        "{} ({}) — {} — {done}/{total} chunks — binary {}",
                        s.name,
                        s.kind,
                        if running { "running" } else { &s.state },
                        s.binary_sha
                    );
                }
            }
            Ok(())
        }
        CampaignCmd::Pause { name } => {
            std::fs::write(dir(root, &name).join("PAUSE"), b"").map_err(|e| e.to_string())?;
            println!("{name}: will pause after the current chunk");
            Ok(())
        }
        CampaignCmd::Report { name } => {
            let path = report(root, &name)?;
            println!("report: {}", path.display());
            Ok(())
        }
    }
}

fn plan(c: &CampaignFile) -> CliResult {
    if c.kind == Kind::Study || c.kind == Kind::Blueprint {
        println!("{} ({:?}): games {:?}", c.name, c.kind, c.game_ids());
        return Ok(());
    }
    let runs = c.expand()?;
    let mut hands = 0u64;
    println!("{} ({:?}) expands to {} runs:", c.name, c.kind, runs.len());
    for r in &runs {
        let h = c.deals_per_run(r.seats) * c.rotations(r.seats);
        hands += h;
        let sweep = if r.sweep.is_empty() {
            String::new()
        } else {
            format!(" {:?}", r.sweep)
        };
        println!(
            "  {:<36} {} seats, {} hands, bots {}{sweep}",
            r.id,
            r.seats,
            h,
            r.bots.join(",")
        );
    }
    // Rough rates from sim.md §5: random bots ~100 k hands/s on 12 threads; ISMCTS ~1 k.
    let slow = runs
        .iter()
        .any(|r| r.bots.iter().any(|b| b.starts_with("ismcts")));
    let rate = if slow { 1_500.0 } else { 100_000.0 };
    println!(
        "total {hands} hands; estimated {:.1} min at ~{rate} hands/s; disk ~{} KB of summaries",
        hands as f64 / rate / 60.0,
        runs.len() * 8
    );
    Ok(())
}

/// Start on a frozen copy of this binary (unless we are it).
fn start(c: CampaignFile, exec: ExecArgs, root: &DataRoot) -> CliResult {
    let d = dir(root, &c.name);
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    std::fs::write(
        d.join("campaign.toml"),
        toml::to_string_pretty(&c).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if exec.frozen {
        return execute(&c, &exec, root);
    }
    let bin_dir = root.0.join("bin");
    std::fs::create_dir_all(&bin_dir).map_err(|e| e.to_string())?;
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let ext = me
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let frozen = bin_dir.join(format!("dc-{GIT_SHA}{ext}"));
    if !frozen.exists() || GIT_SHA.ends_with("-dirty") {
        std::fs::copy(&me, &frozen).map_err(|e| format!("freezing the binary: {e}"))?;
    }
    println!("{}: running on {}", c.name, frozen.display());
    let mut cmd = Command::new(&frozen);
    cmd.arg("--data-root")
        .arg(&root.0)
        .args(["campaign", "resume", &c.name, "--frozen"]);
    if let Some(t) = exec.threads {
        cmd.args(["--threads", &t.to_string()]);
    }
    if exec.nice {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
            cmd.creation_flags(BELOW_NORMAL_PRIORITY_CLASS);
        }
        #[cfg(not(windows))]
        {
            let mut nice = Command::new("nice");
            nice.arg("-n")
                .arg("10")
                .arg(cmd.get_program())
                .args(cmd.get_args());
            cmd = nice;
        }
    }
    let status = cmd.status().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("campaign process exited with {status}"))
    }
}

/// Do the work: every run's missing chunks, in order.
fn execute(c: &CampaignFile, exec: &ExecArgs, root: &DataRoot) -> CliResult {
    let d = dir(root, &c.name);
    match c.kind {
        Kind::Study => return study(c, root),
        Kind::Blueprint => return crate::blueprint::campaign(c, root),
        _ => {}
    }
    let threads = exec.threads.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .saturating_sub(2)
            .max(1)
    });
    set_threads(Some(threads))?;
    let _lock = analyze::lock_dir(&d)?;
    let _ = std::fs::remove_file(d.join("PAUSE"));
    let runs = c.expand()?;
    write_json(&d.join("queue.json"), &runs)?;
    let mut status = Status {
        name: c.name.clone(),
        kind: format!("{:?}", c.kind).to_lowercase(),
        state: "running".into(),
        binary_sha: GIT_SHA.into(),
        updated_unix: now(),
        runs: runs
            .iter()
            .map(|r| RunStatus {
                id: r.id.clone(),
                chunks: c
                    .deals_per_run(r.seats)
                    .div_ceil(c.deals_per_chunk(r.seats)),
                ..RunStatus::default()
            })
            .collect(),
    };
    for (ri, r) in runs.iter().enumerate() {
        let rdir = d.join("runs").join(&r.id);
        let total_deals = c.deals_per_run(r.seats);
        let per_chunk = c.deals_per_chunk(r.seats);
        let chunks = total_deals.div_ceil(per_chunk);
        for k in 0..chunks {
            let path = rdir.join(format!("chunk-{k:04}.json"));
            if path.exists() {
                status.runs[ri].chunks_done = k + 1;
                continue;
            }
            if d.join("PAUSE").exists() {
                let _ = std::fs::remove_file(d.join("PAUSE"));
                status.state = "paused".into();
                status.updated_unix = now();
                write_json(&d.join("status.json"), &status)?;
                println!("{}: paused", c.name);
                return Ok(());
            }
            let deals = per_chunk.min(total_deals - k * per_chunk);
            let spec = RunSpec {
                game: r.game.clone(),
                bots: r
                    .bots
                    .iter()
                    .map(|b| b.parse().expect("validated"))
                    .collect(),
                deals,
                seed: c
                    .seed
                    .wrapping_add((ri as u64).wrapping_mul(1_000_003))
                    .wrapping_add(k.wrapping_mul(7_919)),
                duplicate: c.duplicate,
                history_sample: 0.0,
                stack: None,
                dealer_ante: false,
                study: false,
                balance: c.kind == Kind::Balance,
                config: r.config.clone(),
            };
            let rep = run_spec(&spec, None, None)?;
            let chunk = Chunk {
                hands: rep.hands,
                deals,
                elapsed_sec: rep.throughput.elapsed_sec,
                bots: rep
                    .bots
                    .iter()
                    .map(|b| (b.bot.clone(), b.per_hand, b.per_hand_ci95))
                    .collect(),
                balance: rep.balance.clone(),
            };
            write_json(&path, &chunk)?;
            let st = &mut status.runs[ri];
            st.chunks_done = k + 1;
            st.hands += rep.hands;
            st.hands_per_sec = rep.throughput.hands_per_sec;
            status.updated_unix = now();
            write_json(&d.join("status.json"), &status)?;
            println!(
                "{} {}: chunk {}/{} ({:.0} hands/s)",
                c.name,
                r.id,
                k + 1,
                chunks,
                rep.throughput.hands_per_sec
            );
        }
        write_json(&rdir.join("summary.json"), &summarize(c, r, &rdir)?)?;
    }
    status.state = "done".into();
    status.updated_unix = now();
    write_json(&d.join("status.json"), &status)?;
    let path = report(root, &c.name)?;
    println!("{}: done. Report: {}", c.name, path.display());
    Ok(())
}

/// Merge a run's chunks.
fn summarize(c: &CampaignFile, r: &PlannedRun, rdir: &Path) -> Result<Summary, String> {
    struct Meta;
    impl GameVisitor<(String, u32, String)> for Meta {
        fn visit<G: Game>(self, g: G) -> (String, u32, String) {
            (
                g.meta().display_name.to_string(),
                g.meta().version,
                g.eval_unit().to_string(),
            )
        }
    }
    let (game_name, game_version, eval_unit) =
        with_game(&r.game, r.config.as_ref(), Meta).map_err(|e| e.to_string())?;
    let mut s = Summary {
        id: r.id.clone(),
        game: r.game.clone(),
        game_name,
        game_version,
        binary_sha: GIT_SHA.into(),
        campaign: c.name.clone(),
        seats: r.seats,
        sweep: r.sweep.clone(),
        eval_unit,
        ..Summary::default()
    };
    // Bots: deal-weighted means; CIs combine as independent chunk estimates.
    let mut acc: Vec<(String, f64, f64, u64)> = Vec::new();
    let mut k = 0;
    while let Ok(ch) = read_json::<Chunk>(&rdir.join(format!("chunk-{k:04}.json"))) {
        s.hands += ch.hands;
        s.deals += ch.deals;
        s.elapsed_sec += ch.elapsed_sec;
        for (i, (b, m, ci)) in ch.bots.iter().enumerate() {
            if acc.len() <= i {
                acc.push((b.clone(), 0.0, 0.0, 0));
            }
            let w = ch.deals as f64;
            acc[i].1 += m * w;
            acc[i].2 += (ci * w).powi(2);
            acc[i].3 += ch.deals;
        }
        if let Some(b) = &ch.balance {
            s.balance.get_or_insert_with(BalanceStats::default).merge(b);
        }
        k += 1;
    }
    s.bots = acc
        .into_iter()
        .map(|(b, sum, sq, n)| {
            let n = n.max(1) as f64;
            (b, sum / n, sq.sqrt() / n)
        })
        .collect();
    Ok(s)
}

/// Study campaigns run `dc analyze` (per-starting-hand and per-decision dashboards).
fn study(c: &CampaignFile, root: &DataRoot) -> CliResult {
    let mix = c.mix();
    let args = analyze::RunArgs {
        name: c.name.clone(),
        games: Some(c.game_ids()),
        hands: Some(c.hands_per_run),
        bots: Some(mix),
        seats: c.seats.first().copied(),
        chunk: Some(c.chunk_hands),
        seed: Some(c.seed),
        threads: None,
    };
    analyze::run(analyze::AnalyzeCmd::Run(args), root)?;
    if let Some(grid) = &c.spots {
        let out = root.0.join("reports").join(&c.name).join("spots.html");
        let threads = rayon::current_num_threads();
        let path = crate::spot::run_grid(grid, &out, None, threads)?;
        println!("spot grid: {}", path.display());
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Reports (FR-SP-3)
// ---------------------------------------------------------------------------------------------

/// Preset ratings from ladder runs (FR-SP-5): `result = β[bot] − mean(β at the table)`, solved by
/// weighted least squares with the middle preset fixed at 0.
#[derive(Clone, Debug, Serialize)]
pub struct Rating {
    pub game: String,
    pub bot: String,
    pub rating: f64,
    pub ci95: f64,
    pub tables: usize,
}

pub fn ladder_fit(summaries: &[Summary]) -> Vec<Rating> {
    let mut by_game: BTreeMap<String, Vec<&Summary>> = BTreeMap::new();
    for s in summaries {
        by_game.entry(s.game.clone()).or_default().push(s);
    }
    let mut out = Vec::new();
    for (game, runs) in by_game {
        let mut bots: Vec<String> = Vec::new();
        for r in &runs {
            for (b, _, _) in &r.bots {
                if !bots.contains(b) {
                    bots.push(b.clone());
                }
            }
        }
        let k = bots.len();
        if k < 2 {
            continue;
        }
        let reference = k / 2;
        let idx = |b: &str| bots.iter().position(|x| x == b).unwrap();
        // Unknowns: β for every bot except the reference.
        let m = k - 1;
        let col = |j: usize| {
            if j < reference {
                Some(j)
            } else if j == reference {
                None
            } else {
                Some(j - 1)
            }
        };
        let mut ata = vec![vec![0.0; m]; m];
        let mut atb = vec![0.0; m];
        let mut tables = vec![0usize; k];
        for r in &runs {
            let at: Vec<usize> = r.bots.iter().map(|(b, _, _)| idx(b)).collect();
            let s = at.len() as f64;
            for (row, (_, mean, ci)) in r.bots.iter().enumerate() {
                let se = (ci / 1.96).max(1e-6);
                let w = 1.0 / (se * se);
                // Row coefficients: +1 for this bot, −1/s for every bot at the table.
                let mut a = vec![0.0; k];
                a[at[row]] += 1.0;
                for &j in &at {
                    a[j] -= 1.0 / s;
                }
                let a: Vec<f64> = (0..k).filter_map(|j| col(j).map(|_| a[j])).collect();
                for i in 0..m {
                    atb[i] += w * a[i] * mean;
                    for j in 0..m {
                        ata[i][j] += w * a[i] * a[j];
                    }
                }
                tables[at[row]] += 1;
            }
        }
        let Some((beta, cov)) = solve_with_inverse(ata, atb) else {
            continue;
        };
        for (j, b) in bots.iter().enumerate() {
            let (rating, var) = match col(j) {
                Some(c) => (beta[c], cov[c][c]),
                None => (0.0, 0.0),
            };
            out.push(Rating {
                game: game.clone(),
                bot: b.clone(),
                rating,
                ci95: 1.96 * var.max(0.0).sqrt(),
                tables: tables[j],
            });
        }
    }
    out
}

/// Solve `A x = b` and return `(x, A⁻¹)` by Gauss-Jordan elimination (`A` small, symmetric).
fn solve_with_inverse(mut a: Vec<Vec<f64>>, b: Vec<f64>) -> Option<(Vec<f64>, Vec<Vec<f64>>)> {
    let n = a.len();
    let mut inv: Vec<Vec<f64>> = (0..n)
        .map(|i| (0..n).map(|j| (i == j) as u8 as f64).collect())
        .collect();
    for c in 0..n {
        let p = (c..n).max_by(|&x, &y| a[x][c].abs().total_cmp(&a[y][c].abs()))?;
        if a[p][c].abs() < 1e-12 {
            return None;
        }
        a.swap(c, p);
        inv.swap(c, p);
        let d = a[c][c];
        for j in 0..n {
            a[c][j] /= d;
            inv[c][j] /= d;
        }
        for r in 0..n {
            if r != c {
                let f = a[r][c];
                if f != 0.0 {
                    for j in 0..n {
                        a[r][j] -= f * a[c][j];
                        inv[r][j] -= f * inv[c][j];
                    }
                }
            }
        }
    }
    let x = (0..n)
        .map(|i| (0..n).map(|j| inv[i][j] * b[j]).sum())
        .collect();
    Some((x, inv))
}

#[derive(Serialize)]
struct ReportData {
    name: String,
    kind: String,
    binary_sha: String,
    generated_unix: u64,
    runs: Vec<ReportRun>,
    ratings: Vec<Rating>,
}

#[derive(Serialize)]
struct ReportRun {
    #[serde(flatten)]
    summary: Summary,
    swing: f64,
    pot_mean: f64,
    pot_p90: f64,
    showdown: f64,
    showdown_ci: Option<(f64, f64)>,
    position: Vec<(f64, f64)>,
    hands_per_game: f64,
}

/// Write `<data_root>/reports/<name>/index.html`.
pub fn report(root: &DataRoot, name: &str) -> Result<PathBuf, String> {
    let d = dir(root, name);
    let c: CampaignFile = toml::from_str(
        &std::fs::read_to_string(d.join("campaign.toml")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let mut summaries = Vec::new();
    if let Ok(rd) = std::fs::read_dir(d.join("runs")) {
        let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        dirs.sort();
        for rdir in dirs {
            let id = rdir.file_name().unwrap().to_string_lossy().to_string();
            let planned = c
                .expand()
                .ok()
                .and_then(|rs| rs.into_iter().find(|r| r.id == id));
            if let Some(r) = planned
                && let Ok(s) = summarize(&c, &r, &rdir)
                && s.hands > 0
            {
                summaries.push(s);
            }
        }
    }
    let ratings = if c.kind == Kind::Ladder {
        ladder_fit(&summaries)
    } else {
        Vec::new()
    };
    let runs = summaries
        .into_iter()
        .map(|s| {
            let b = s.balance.clone().unwrap_or_default();
            let (rate, ci) = b.showdown_rate();
            ReportRun {
                swing: b.result.variance().sqrt(),
                pot_mean: b.pot.mean,
                pot_p90: b.pot_quantile(0.9),
                showdown: rate,
                showdown_ci: ci,
                position: b.by_position.iter().map(|w| (w.mean, w.ci95())).collect(),
                hands_per_game: b.hands_per_game.mean,
                summary: s,
            }
        })
        .collect();
    let data = ReportData {
        name: c.name.clone(),
        kind: format!("{:?}", c.kind).to_lowercase(),
        binary_sha: GIT_SHA.into(),
        generated_unix: now(),
        runs,
        ratings,
    };
    let json = serde_json::to_string(&data).map_err(|e| e.to_string())?;
    let html = REPORT_TEMPLATE.replace("\"__DC_DATA__\"", &json.replace("</", "<\\/"));
    let out = root.0.join("reports").join(name).join("index.html");
    std::fs::create_dir_all(out.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&out, html).map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ladder_fit_recovers_known_ratings() {
        // Three bots with true strengths 1, 0, -1 (middle = reference), seen at 2-seat tables.
        let mk = |a: (&str, f64), b: (&str, f64)| Summary {
            game: "g".into(),
            bots: vec![
                (a.0.into(), a.1 - (a.1 + b.1) / 2.0, 0.01),
                (b.0.into(), b.1 - (a.1 + b.1) / 2.0, 0.01),
            ],
            ..Summary::default()
        };
        let runs = vec![
            mk(("hi", 1.0), ("mid", 0.0)),
            mk(("mid", 0.0), ("lo", -1.0)),
            mk(("lo", -1.0), ("hi", 1.0)),
        ];
        let r = ladder_fit(&runs);
        let get = |b: &str| r.iter().find(|x| x.bot == b).unwrap().rating;
        assert!((get("hi") - 1.0).abs() < 1e-6, "{r:?}");
        assert!((get("lo") + 1.0).abs() < 1e-6, "{r:?}");
        assert_eq!(get("mid"), 0.0);
    }

    #[test]
    fn sweeps_expand_to_every_combination() {
        let c: CampaignFile = toml::from_str(
            r#"
            name = "t"
            kind = "balance"
            games = ["fusion"]
            seats = [3]
            [sweep]
            "fusion.variants" = [[], ["hi_lo"]]
            "*.dealer_ante" = [0, 200]
            "#,
        )
        .unwrap();
        let runs = c.expand().unwrap();
        assert_eq!(runs.len(), 4);
        assert!(runs.iter().all(|r| r.config.is_some()));
    }
}
