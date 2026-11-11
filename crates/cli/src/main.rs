//! `dc`: the Dealers Choice command line (PRD FR-UI-1, `docs/tech/cli.md`).
//!
//! `clap`'s derive API turns the structs below into an argument parser: each field is a flag,
//! each enum variant a subcommand, and the doc comments become `--help` text.

mod config_tui;
mod human;
mod play;
mod player;
mod settings;
mod simulate;
mod spot;
mod tools;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use storage::{DataRoot, ProfileStore};

#[derive(Parser)]
#[command(name = "dc", version, about = "Dealers Choice card game engine")]
struct Cli {
    /// Where the database, sim runs, and blueprints live (default: $DC_DATA_ROOT or ./data).
    #[arg(long, global = true)]
    data_root: Option<PathBuf>,
    /// Where saved profiles live (default: $DC_CONFIG_ROOT or ./config).
    #[arg(long, global = true)]
    config_root: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List the registered games.
    Games,
    /// Edit saved sim and play profiles in a terminal UI.
    Config,
    /// Manage players and bankrolls.
    #[command(subcommand)]
    Player(player::PlayerCmd),
    /// Play a Dealer's Choice session against bots.
    Play(settings::PlayArgs),
    /// Train an MCCFR strategy (small games) and report its exploitability.
    Train(tools::TrainArgs),
    /// Measure a bot's exploitability (Kuhn / Leduc only).
    Exploit(tools::ExploitArgs),
    /// Rank the actions in a described Hold'em spot, against opponent ranges.
    Spot(spot::SpotArgs),
    /// Run a bot-vs-bot simulation.
    Sim(simulate::SimArgs),
    /// Measure throughput and data rate on fixed workloads.
    BenchSim(simulate::BenchArgs),
}

fn main() {
    let cli = Cli::parse();
    let root = DataRoot::resolve(cli.data_root.as_deref());
    let profiles = ProfileStore::resolve(cli.config_root.as_deref());
    let result = match cli.command {
        Command::Games => tools::list_games(),
        Command::Config => config_tui::run(profiles.clone()),
        Command::Player(cmd) => player::run(cmd, &root),
        Command::Play(args) => play::run(args, &root, &profiles),
        Command::Train(args) => tools::train(args, &root),
        Command::Exploit(args) => tools::exploit(args),
        Command::Spot(args) => spot::run(args),
        Command::Sim(args) => simulate::sim(args, &root, &profiles),
        Command::BenchSim(args) => simulate::bench(args),
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

/// Errors are just messages for the user at this level.
pub type CliResult = Result<(), String>;
