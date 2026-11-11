//! `dc`: the Dealers Choice command line (PRD FR-UI-1, `docs/tech/cli.md`).
//!
//! `clap`'s derive API turns the structs below into an argument parser: each field is a flag,
//! each enum variant a subcommand, and the doc comments become `--help` text.

mod human;
mod play;
mod simulate;
mod spot;
mod tools;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "dc", version, about = "Dealers Choice card game engine")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List the registered games.
    Games,
    /// Play a Dealer's Choice session against bots.
    Play(play::PlayArgs),
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
    let result = match cli.command {
        Command::Games => tools::list_games(),
        Command::Play(args) => play::run(args),
        Command::Train(args) => tools::train(args),
        Command::Exploit(args) => tools::exploit(args),
        Command::Spot(args) => spot::run(args),
        Command::Sim(args) => simulate::sim(args),
        Command::BenchSim(args) => simulate::bench(args),
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

/// Errors are just messages for the user at this level.
pub type CliResult = Result<(), String>;
