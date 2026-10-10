//! Small subcommands: `games`, `train`, `exploit`.

use clap::Args;

use bots::mccfr::best_response::{SampledStrategy, exploitability};
use bots::mccfr::{Mccfr, MccfrConfig};
use bots::{Budget, IsmctsBot, UniformStrategy};
use engine::Game;
use engine::rng::rng_from_seed;
use games::registry::{GAME_IDS, GameVisitor, with_game};
use storage::DataRoot;

use crate::CliResult;

pub fn list_games() -> CliResult {
    struct Describe;
    impl GameVisitor<String> for Describe {
        fn visit<G: Game>(self, game: G) -> String {
            let m = game.meta();
            format!(
                "{:<12} {:<26} seats {:?}  {:?}  ({})",
                m.id, m.display_name, m.seats, m.length, m.rules_ref
            )
        }
    }
    for id in GAME_IDS {
        println!(
            "{}",
            with_game(id, None, Describe).map_err(|e| e.to_string())?
        );
    }
    Ok(())
}

#[derive(Args)]
pub struct TrainArgs {
    /// `kuhn` or `leduc` (exact exploitability needs a small game).
    #[arg(long, default_value = "kuhn")]
    pub game: String,
    #[arg(long, default_value_t = 100_000)]
    pub iters: u64,
    #[arg(long, default_value_t = 1)]
    pub seed: u64,
    /// Report exploitability every this many iterations.
    #[arg(long, default_value_t = 20_000)]
    pub report_every: u64,
    /// Save the final blueprint under <data_root>/blueprints/ (usable as bot `mccfr:<path>`).
    #[arg(long)]
    pub save: bool,
}

pub fn train(args: TrainArgs, root: &DataRoot) -> CliResult {
    struct Train<'a>(&'a TrainArgs, &'a DataRoot);
    impl GameVisitor<CliResult> for Train<'_> {
        fn visit<G: Game>(self, game: G) -> CliResult {
            let a = self.0;
            if game.meta().seats != (2..=2) {
                return Err(
                    "train reports exact exploitability, so it supports heads-up test games only"
                        .into(),
                );
            }
            let mut trainer = Mccfr::<G>::new(MccfrConfig::default());
            let mut rng = rng_from_seed(a.seed);
            println!(
                "uniform strategy exploitability: {:.5}",
                exploitability(&game, &UniformStrategy)
            );
            let mut done = 0;
            while done < a.iters {
                let step = a.report_every.min(a.iters - done);
                trainer.train(&game, step, &mut rng);
                done += step;
                let e = exploitability(&game, &trainer.blueprint(&game));
                println!(
                    "iter {done:>10}  info sets {:>6}  exploitability {e:.5}",
                    trainer.info_sets()
                );
            }
            if a.save {
                let path =
                    self.1
                        .blueprint(game.meta().id, game.meta().version, trainer.iterations());
                std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
                std::fs::write(&path, trainer.blueprint(&game).to_bytes())
                    .map_err(|e| e.to_string())?;
                println!(
                    "saved {}  (use as bot `mccfr:{}`)",
                    path.display(),
                    path.display()
                );
            }
            Ok(())
        }
    }
    with_game(&args.game, None, Train(&args, root)).map_err(|e| e.to_string())?
}

#[derive(Args)]
pub struct ExploitArgs {
    #[arg(long, default_value = "kuhn")]
    pub game: String,
    /// `random`, `ismcts:<level>`, or `mccfr:<iterations>`.
    #[arg(long, default_value = "ismcts:7")]
    pub bot: String,
    /// Samples per information set when tabulating a sampling bot.
    #[arg(long, default_value_t = 30)]
    pub samples: u32,
    #[arg(long, default_value_t = 1)]
    pub seed: u64,
}

pub fn exploit(args: ExploitArgs) -> CliResult {
    struct Exploit<'a>(&'a ExploitArgs);
    impl GameVisitor<CliResult> for Exploit<'_> {
        fn visit<G: Game>(self, game: G) -> CliResult {
            let a = self.0;
            if game.meta().seats != (2..=2) {
                return Err(
                    "exploitability is exact only for heads-up test games (kuhn, leduc)".into(),
                );
            }
            let (kind, param) = a.bot.split_once(':').unwrap_or((a.bot.as_str(), ""));
            let e = match kind {
                // A uniform-random bot *is* the uniform strategy, so it is evaluated exactly.
                "random" => exploitability(&game, &UniformStrategy),
                "ismcts" => {
                    let level: u8 = param.parse().map_err(|_| "ismcts:<level 1-10>")?;
                    let mut bot = IsmctsBot::<G>::preset(level, false);
                    // Keep this tractable: sampling every info set many times.
                    if let Budget::Playouts(p) = bot.strength.budget {
                        bot.strength.budget = Budget::Playouts(p.min(2_000));
                    }
                    exploitability(
                        &game,
                        &SampledStrategy::from_policy(&game, &mut bot, a.samples, a.seed),
                    )
                }
                "mccfr" => {
                    let iters: u64 = param.parse().map_err(|_| "mccfr:<iterations>")?;
                    let mut t = Mccfr::<G>::new(MccfrConfig::default());
                    t.train(&game, iters, &mut rng_from_seed(a.seed));
                    exploitability(&game, &t.blueprint(&game))
                }
                _ => return Err(format!("unknown bot {:?}", a.bot)),
            };
            println!(
                "{} at {}: exploitability {e:.5} per hand",
                a.bot,
                game.meta().id
            );
            Ok(())
        }
    }
    with_game(&args.game, None, Exploit(&args)).map_err(|e| e.to_string())?
}
