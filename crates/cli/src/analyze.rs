//! `dc analyze`: long, resumable "study" campaigns over every game, with a self-contained HTML
//! dashboard per game (`docs/tech/sim-program.md` §4.5).
//!
//! A campaign lives in `<data_root>/analysis/<name>/`:
//! ```text
//! campaign.toml            what to run (written by `dc analyze run` flags or `dc lab`)
//! status.json              progress, rewritten after every chunk
//! PAUSE                    present = stop after the current chunk
//! index.html               links to every game's dashboard
//! <game>/chunk-00000.json  one finished chunk (a RunReport with study aggregates)
//! <game>/dashboard.html    regenerated after each chunk
//! ```
//! A "hand" is one deal played to showdown or until everyone else folds. Chunks are
//! independent (each has its own seed), so a killed or paused run resumes by skipping the chunk
//! files that exist.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};

use engine::{AnalysisLabels, Game};
use games::registry::{GAME_IDS, GameVisitor, with_game};
use sim::study::{Study, StudyRows};
use sim::{BotSpec, RunSpec};
use storage::DataRoot;

use crate::CliResult;
use crate::simulate::{run_spec, set_threads};

const DASHBOARD_TEMPLATE: &str = include_str!("../assets/dashboard.html");
const INDEX_TEMPLATE: &str = include_str!("../assets/analysis_index.html");

#[derive(Subcommand)]
pub enum AnalyzeCmd {
    /// Create or update a campaign from flags, then run it (resumes where it stopped).
    Run(RunArgs),
    /// Ask a running campaign to stop after its current chunk.
    Pause {
        #[arg(long, default_value = "default")]
        name: String,
    },
    /// Show every campaign's progress.
    Status,
    /// Rebuild the dashboards from the saved chunks.
    Report {
        #[arg(long, default_value = "default")]
        name: String,
    },
}

#[derive(Args, Clone, Default)]
pub struct RunArgs {
    /// Campaign name (a folder under <data_root>/analysis/).
    #[arg(long, default_value = "default")]
    pub name: String,
    /// Games to study (default: every registered game except the solver test games).
    #[arg(long, value_delimiter = ',')]
    pub games: Option<Vec<String>>,
    /// Hands per game.
    #[arg(long)]
    pub hands: Option<u64>,
    /// Bot for every seat (or a comma list, one per seat), e.g. `ismcts:3:300`.
    #[arg(long, value_delimiter = ',')]
    pub bots: Option<Vec<String>>,
    /// Seats per table (default: 6, or the game's maximum if lower).
    #[arg(long)]
    pub seats: Option<u8>,
    /// Deals per chunk (progress and dashboards update after each chunk).
    #[arg(long)]
    pub chunk: Option<u64>,
    #[arg(long)]
    pub seed: Option<u64>,
    /// Worker threads (default: all cores but two).
    #[arg(long)]
    pub threads: Option<usize>,
}

/// `campaign.toml`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Campaign {
    pub name: String,
    pub hands_per_game: u64,
    pub chunk_deals: u64,
    pub seed: u64,
    #[serde(default)]
    pub threads: Option<usize>,
    #[serde(rename = "game")]
    pub games: Vec<CampaignGame>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CampaignGame {
    pub id: String,
    pub seats: u8,
    /// One spec (used for every seat) or one per seat.
    pub bots: Vec<String>,
}

/// `status.json`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Status {
    pub name: String,
    /// "running", "paused", "done", "failed: …"
    pub state: String,
    pub updated_unix: u64,
    pub pid: u32,
    pub games: Vec<GameStatus>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GameStatus {
    pub id: String,
    pub display_name: String,
    pub target_hands: u64,
    pub hands: u64,
    pub chunks: u64,
    pub hands_per_sec: f64,
}

pub const DEFAULT_BOT: &str = "ismcts:3:300";

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn campaign_dir(root: &DataRoot, name: &str) -> PathBuf {
    root.0.join("analysis").join(name)
}

/// Games worth studying: everything but the solver test fixtures.
pub fn default_games() -> Vec<String> {
    GAME_IDS
        .iter()
        .filter(|id| !matches!(**id, "kuhn" | "leduc"))
        .map(|s| s.to_string())
        .collect()
}

/// (display name, seat range, labels) for a game id.
pub fn game_info(
    id: &str,
) -> Result<(String, std::ops::RangeInclusive<u8>, AnalysisLabels), String> {
    struct Info;
    impl GameVisitor<(String, std::ops::RangeInclusive<u8>, AnalysisLabels)> for Info {
        fn visit<G: Game>(self, g: G) -> (String, std::ops::RangeInclusive<u8>, AnalysisLabels) {
            (
                g.meta().display_name.to_string(),
                g.meta().seats.clone(),
                g.analysis_labels(),
            )
        }
    }
    with_game(id, None, Info).map_err(|e| e.to_string())
}

/// The game's result unit as money, e.g. "$1.00".
fn game_unit(id: &str) -> Result<String, String> {
    struct Unit;
    impl GameVisitor<String> for Unit {
        fn visit<G: Game>(self, g: G) -> String {
            g.eval_unit().to_string()
        }
    }
    with_game(id, None, Unit).map_err(|e| e.to_string())
}

impl Campaign {
    pub fn load(dir: &Path) -> Result<Campaign, String> {
        let p = dir.join("campaign.toml");
        let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        toml::from_str(&text).map_err(|e| format!("{}: {e}", p.display()))
    }

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let text = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(dir.join("campaign.toml"), text).map_err(|e| e.to_string())
    }

    /// Defaults, then any flags. Validates games, seats and bot specs.
    pub fn from_args(existing: Option<Campaign>, a: &RunArgs) -> Result<Campaign, String> {
        let mut c = existing.unwrap_or_else(|| Campaign {
            name: a.name.clone(),
            hands_per_game: 1_000_000,
            chunk_deals: 5_000,
            seed: 2026,
            threads: None,
            games: Vec::new(),
        });
        if let Some(h) = a.hands {
            c.hands_per_game = h;
        }
        if let Some(k) = a.chunk {
            c.chunk_deals = k.max(1);
        }
        if let Some(s) = a.seed {
            c.seed = s;
        }
        if a.threads.is_some() {
            c.threads = a.threads;
        }
        let rebuild =
            c.games.is_empty() || a.games.is_some() || a.bots.is_some() || a.seats.is_some();
        if rebuild {
            let ids = a.games.clone().unwrap_or_else(default_games);
            let mut games = Vec::new();
            for id in ids {
                let (_, range, _) = game_info(&id)?;
                let want = a.seats.unwrap_or(6);
                let seats = want.clamp(*range.start(), *range.end());
                let bots = a.bots.clone().unwrap_or_else(|| vec![DEFAULT_BOT.into()]);
                games.push(CampaignGame { id, seats, bots });
            }
            c.games = games;
        }
        c.validate()?;
        Ok(c)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.games.is_empty() {
            return Err("a campaign needs at least one game".into());
        }
        for g in &self.games {
            let (_, range, _) = game_info(&g.id)?;
            if !range.contains(&g.seats) {
                return Err(format!(
                    "{} supports {range:?} seats, not {}",
                    g.id, g.seats
                ));
            }
            if g.bots.len() != 1 && g.bots.len() != g.seats as usize {
                return Err(format!(
                    "{}: give one bot for every seat or exactly {} bots",
                    g.id, g.seats
                ));
            }
            for b in &g.bots {
                b.parse::<BotSpec>().map_err(|e| format!("{}: {e}", g.id))?;
            }
        }
        Ok(())
    }
}

impl CampaignGame {
    fn bots(&self) -> Vec<BotSpec> {
        let one = |s: &String| s.parse::<BotSpec>().expect("validated");
        if self.bots.len() == 1 {
            vec![one(&self.bots[0]); self.seats as usize]
        } else {
            self.bots.iter().map(one).collect()
        }
    }
}

pub fn run(cmd: AnalyzeCmd, root: &DataRoot) -> CliResult {
    match cmd {
        AnalyzeCmd::Run(args) => {
            let dir = campaign_dir(root, &args.name);
            let existing = Campaign::load(&dir).ok();
            let c = Campaign::from_args(existing, &args)?;
            c.save(&dir)?;
            run_campaign(&c, &dir)
        }
        AnalyzeCmd::Pause { name } => {
            let dir = campaign_dir(root, &name);
            std::fs::write(dir.join("PAUSE"), b"").map_err(|e| e.to_string())?;
            println!("{name}: will pause after the current chunk");
            Ok(())
        }
        AnalyzeCmd::Status => {
            for s in list_status(root) {
                println!("{} — {}", s.name, s.state);
                for g in &s.games {
                    println!(
                        "  {:<28} {:>12} / {:<12} hands  ({:.0} hands/s)",
                        g.display_name, g.hands, g.target_hands, g.hands_per_sec
                    );
                }
            }
            Ok(())
        }
        AnalyzeCmd::Report { name } => {
            let dir = campaign_dir(root, &name);
            let c = Campaign::load(&dir)?;
            for g in &c.games {
                let agg = load_chunks(&dir.join(&g.id))?;
                write_dashboard(&c, g, &agg, &dir)?;
            }
            write_index(&dir)?;
            println!("dashboards: {}", dir.join("index.html").display());
            Ok(())
        }
    }
}

/// Every campaign's status (for `dc analyze status` and `dc lab`).
pub fn list_status(root: &DataRoot) -> Vec<Status> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(root.0.join("analysis")) else {
        return out;
    };
    let mut dirs: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for d in dirs {
        if let Ok(text) = std::fs::read_to_string(d.join("status.json"))
            && let Ok(s) = serde_json::from_str::<Status>(&text)
        {
            out.push(s);
        } else if let Ok(c) = Campaign::load(&d) {
            out.push(Status {
                name: c.name,
                state: "not started".into(),
                ..Status::default()
            });
        }
    }
    out
}

/// Everything merged from one game's chunk files.
#[derive(Default)]
pub struct Aggregate {
    pub chunks: u64,
    pub hands: u64,
    pub study: Study,
    pub secs: f64,
    /// Per bot spec: (hands-weighted mean result per hand, Σ (ci·hands)² for a combined CI).
    pub bots: BTreeMap<String, (f64, f64, u64)>,
}

impl Aggregate {
    fn hands_per_sec(&self) -> f64 {
        if self.secs > 0.0 {
            self.hands as f64 / self.secs
        } else {
            0.0
        }
    }
}

fn chunk_path(game_dir: &Path, k: u64) -> PathBuf {
    game_dir.join(format!("chunk-{k:05}.json"))
}

pub fn load_chunks(game_dir: &Path) -> Result<Aggregate, String> {
    let mut agg = Aggregate::default();
    let mut k = 0;
    while let Ok(text) = std::fs::read_to_string(chunk_path(game_dir, k)) {
        let r: ChunkFile = serde_json::from_str(&text).map_err(|e| format!("chunk {k}: {e}"))?;
        agg.add_stored(&r.report);
        k += 1;
    }
    Ok(agg)
}

/// A chunk on disk. `RunReport` only serializes, so the stored form is its own type.
#[derive(Serialize, Deserialize)]
struct ChunkFile {
    report: StoredReport,
}

#[derive(Serialize, Deserialize)]
struct StoredReport {
    hands: u64,
    elapsed_sec: f64,
    bots: Vec<(String, f64, f64)>,
    study: Option<StudyRows>,
}

impl Aggregate {
    fn add_stored(&mut self, r: &StoredReport) {
        self.chunks += 1;
        self.hands += r.hands;
        self.secs += r.elapsed_sec;
        if let Some(rows) = &r.study {
            self.study.merge(&Study::from_rows(rows));
        }
        for (bot, per_hand, ci) in &r.bots {
            let e = self.bots.entry(bot.clone()).or_insert((0.0, 0.0, 0));
            let h = r.hands as f64;
            e.0 += per_hand * h;
            e.1 += (ci * h).powi(2);
            e.2 += r.hands;
        }
    }
}

fn run_campaign(c: &Campaign, dir: &Path) -> CliResult {
    let threads = c.threads.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .saturating_sub(2)
            .max(1)
    });
    set_threads(Some(threads))?;
    let _ = std::fs::remove_file(dir.join("PAUSE"));

    let mut aggs = Vec::new();
    let mut status = Status {
        name: c.name.clone(),
        state: "running".into(),
        pid: std::process::id(),
        ..Status::default()
    };
    for g in &c.games {
        let gdir = dir.join(&g.id);
        std::fs::create_dir_all(&gdir).map_err(|e| e.to_string())?;
        let agg = load_chunks(&gdir)?;
        let (display, _, _) = game_info(&g.id)?;
        status.games.push(GameStatus {
            id: g.id.clone(),
            display_name: display,
            target_hands: c.hands_per_game,
            hands: agg.hands,
            chunks: agg.chunks,
            hands_per_sec: agg.hands_per_sec(),
        });
        aggs.push(agg);
    }
    write_status(dir, &mut status)?;
    write_index(dir)?;

    // Round-robin one chunk per game, so every game's dashboard fills in early.
    loop {
        let mut progressed = false;
        for (i, g) in c.games.iter().enumerate() {
            if aggs[i].hands >= c.hands_per_game {
                continue;
            }
            if dir.join("PAUSE").exists() {
                status.state = "paused".into();
                write_status(dir, &mut status)?;
                let _ = std::fs::remove_file(dir.join("PAUSE"));
                println!("{}: paused", c.name);
                return Ok(());
            }
            let k = aggs[i].chunks;
            let spec = RunSpec {
                game: g.id.clone(),
                bots: g.bots(),
                deals: c.chunk_deals,
                seed: c.seed.wrapping_add(1_000_003 * i as u64).wrapping_add(k),
                duplicate: false,
                history_sample: 0.0,
                stack: None,
                dealer_ante: false,
                study: true,
            };
            let report = match run_spec(&spec, None, None) {
                Ok(r) => r,
                Err(e) => {
                    status.state = format!("failed: {e}");
                    write_status(dir, &mut status)?;
                    return Err(e);
                }
            };
            let stored = StoredReport {
                hands: report.hands,
                elapsed_sec: report.throughput.elapsed_sec,
                bots: report
                    .bots
                    .iter()
                    .map(|b| (b.bot.clone(), b.per_hand, b.per_hand_ci95))
                    .collect(),
                study: report.study.clone(),
            };
            aggs[i].add_stored(&stored);
            let text =
                serde_json::to_string(&ChunkFile { report: stored }).map_err(|e| e.to_string())?;
            let gdir = dir.join(&g.id);
            // Write then rename, so a crash never leaves a half-written chunk behind.
            let tmp = gdir.join("chunk.tmp");
            std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
            std::fs::rename(&tmp, chunk_path(&gdir, k)).map_err(|e| e.to_string())?;
            write_dashboard(c, g, &aggs[i], dir)?;
            let st = &mut status.games[i];
            st.hands = aggs[i].hands;
            st.chunks = aggs[i].chunks;
            st.hands_per_sec = aggs[i].hands_per_sec();
            let rate = st.hands_per_sec;
            write_status(dir, &mut status)?;
            write_index(dir)?;
            println!(
                "{} chunk {k}: {} / {} hands ({rate:.0} hands/s)",
                g.id, aggs[i].hands, c.hands_per_game
            );
            progressed = true;
        }
        if !progressed {
            break;
        }
    }
    status.state = "done".into();
    write_status(dir, &mut status)?;
    println!(
        "{}: done. Dashboards: {}",
        c.name,
        dir.join("index.html").display()
    );
    Ok(())
}

fn write_status(dir: &Path, s: &mut Status) -> CliResult {
    s.updated_unix = now();
    let text = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("status.json"), text).map_err(|e| e.to_string())
}

/// Data embedded in a dashboard page.
#[derive(Serialize)]
struct DashboardData<'a> {
    campaign: &'a str,
    game_id: &'a str,
    game: String,
    seats: u8,
    bots: &'a [String],
    hands: u64,
    target_hands: u64,
    hands_per_sec: f64,
    updated_unix: u64,
    /// What one result unit is worth, e.g. "$1.00" (the big blind or the ante).
    unit: String,
    labels: AnalysisLabels,
    results: Vec<BotResult>,
    study: StudyRows,
}

#[derive(Serialize)]
struct BotResult {
    bot: String,
    per_hand: f64,
    ci95: f64,
}

/// Put JSON inside a `<script>` safely: `</` can't end the script element early.
fn embed(json: &str) -> String {
    json.replace("</", "<\\/")
}

pub fn write_dashboard(c: &Campaign, g: &CampaignGame, agg: &Aggregate, dir: &Path) -> CliResult {
    let (display, _, labels) = game_info(&g.id)?;
    let unit = game_unit(&g.id)?;
    let data = DashboardData {
        campaign: &c.name,
        game_id: &g.id,
        game: display,
        seats: g.seats,
        bots: &g.bots,
        hands: agg.hands,
        target_hands: c.hands_per_game,
        hands_per_sec: agg.hands_per_sec(),
        updated_unix: now(),
        unit,
        labels,
        results: agg
            .bots
            .iter()
            .map(|(bot, &(sum, ci2, h))| BotResult {
                bot: bot.clone(),
                per_hand: if h > 0 { sum / h as f64 } else { 0.0 },
                ci95: if h > 0 { ci2.sqrt() / h as f64 } else { 0.0 },
            })
            .collect(),
        study: agg.study.to_rows(),
    };
    let json = serde_json::to_string(&data).map_err(|e| e.to_string())?;
    let html = DASHBOARD_TEMPLATE.replace("\"__DC_DATA__\"", &embed(&json));
    let path = dir.join(&g.id).join("dashboard.html");
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, html).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

pub fn write_index(dir: &Path) -> CliResult {
    let status = std::fs::read_to_string(dir.join("status.json")).unwrap_or_else(|_| "null".into());
    let html = INDEX_TEMPLATE.replace("\"__DC_DATA__\"", &embed(&status));
    std::fs::write(dir.join("index.html"), html).map_err(|e| e.to_string())
}
