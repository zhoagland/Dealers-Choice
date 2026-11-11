//! Blueprint campaigns (FR-SP-7, `docs/tech/sim-program.md` §4.4).
//!
//! For each game: MCCFR training in steps, checkpointed after every step (a killed or paused
//! campaign resumes from the last checkpoint), the blueprint saved under
//! `<data_root>/blueprints/<game>/` (usable as bot `mccfr:<path>`), and a training curve:
//! - exact exploitability where a full best response is affordable (Kuhn, Leduc, Indian Poker);
//! - otherwise the blueprint's result per hand against ISMCTS (`ismcts:3:200`), duplicate deals;
//! - the abstracted heads-up Hold'em reports training size only (its exit criterion is
//!   checkpointed training, sim-program §6).

use std::path::Path;

use serde::{Deserialize, Serialize};

use bots::mccfr::best_response::exploitability;
use bots::mccfr::{Mccfr, MccfrConfig};
use engine::Game;
use engine::rng::{Stream, derive_rng};
use games::registry::{GameVisitor, with_game};
use sim::RunSpec;
use storage::DataRoot;

use crate::CliResult;
use crate::campaign::CampaignFile;
use crate::simulate::run_spec;

/// Games with an affordable exact best response.
const EXACT: &[&str] = &["kuhn", "leduc", "indian_poker"];

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Curve {
    pub game: String,
    pub points: Vec<Point>,
    pub blueprint: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Point {
    pub iterations: u64,
    pub info_sets: usize,
    /// Exact exploitability (eval units per hand), when computed.
    pub exploitability: Option<f64>,
    /// Result per hand vs ISMCTS (eval units) and its 95 % CI, when measured.
    pub vs_ismcts: Option<(f64, f64)>,
}

pub fn campaign(c: &CampaignFile, root: &DataRoot) -> CliResult {
    let dir = root.0.join("campaigns").join(&c.name);
    let _lock = crate::analyze::lock_dir(&dir)?;
    let _ = std::fs::remove_file(dir.join("PAUSE"));
    let iterations = c.iterations.unwrap_or(100_000);
    let games: Vec<String> = if c.games.iter().any(|g| g == "all") {
        vec![
            "indian_poker".into(),
            "drop_321".into(),
            "holdem_hu_abstract".into(),
        ]
    } else {
        c.games.clone()
    };
    let mut curves = Vec::new();
    for g in &games {
        let curve = with_game(
            g,
            None,
            Train {
                root,
                dir: &dir,
                iterations,
                seed: c.seed,
            },
        )
        .map_err(|e| e.to_string())??;
        curves.push(curve);
        if dir.join("PAUSE").exists() {
            let _ = std::fs::remove_file(dir.join("PAUSE"));
            println!("{}: paused", c.name);
            break;
        }
    }
    let out = root.0.join("reports").join(&c.name).join("index.html");
    write_report(&out, &c.name, &curves)?;
    println!("{}: report {}", c.name, out.display());
    Ok(())
}

struct Train<'a> {
    root: &'a DataRoot,
    dir: &'a Path,
    iterations: u64,
    seed: u64,
}

impl GameVisitor<Result<Curve, String>> for Train<'_> {
    fn visit<G: Game>(self, game: G) -> Result<Curve, String> {
        let id = game.meta().id;
        if !game.meta().seats.contains(&2) {
            return Err(format!("{id}: blueprints are trained heads-up"));
        }
        let ckpt = self.dir.join("blueprints").join(format!("{id}.ckpt"));
        let curve_path = self.dir.join("blueprints").join(format!("{id}.json"));
        std::fs::create_dir_all(ckpt.parent().unwrap()).map_err(|e| e.to_string())?;
        let config = MccfrConfig::default();
        let mut trainer = match std::fs::read(&ckpt) {
            Ok(bytes) => Mccfr::<G>::restore(config.clone(), &bytes)?,
            Err(_) => Mccfr::<G>::new(config.clone()),
        };
        let mut curve: Curve = std::fs::read_to_string(&curve_path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or(Curve {
                game: id.into(),
                ..Curve::default()
            });
        let step = (self.iterations / 10).max(1);
        while trainer.iterations() < self.iterations {
            if self.dir.join("PAUSE").exists() {
                break;
            }
            let n = step.min(self.iterations - trainer.iterations());
            let mut rng = derive_rng(self.seed, trainer.iterations(), Stream::Custom(77));
            trainer.train(&game, n, &mut rng);
            std::fs::write(&ckpt, trainer.checkpoint()).map_err(|e| e.to_string())?;
            let bp = trainer.blueprint(&game);
            let path = self
                .root
                .blueprint(id, game.meta().version, trainer.iterations());
            std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::write(&path, bp.to_bytes()).map_err(|e| e.to_string())?;
            curve.blueprint = path.display().to_string();
            let mut point = Point {
                iterations: trainer.iterations(),
                info_sets: trainer.info_sets(),
                ..Point::default()
            };
            if EXACT.contains(&id) {
                point.exploitability = Some(exploitability(&game, &bp));
            } else if id != "holdem_hu_abstract" {
                let spec = RunSpec {
                    game: id.into(),
                    bots: vec![
                        format!("mccfr:{}", path.display()).parse()?,
                        "ismcts:3:200".parse()?,
                    ],
                    deals: 150,
                    seed: self.seed ^ trainer.iterations(),
                    duplicate: true,
                    history_sample: 0.0,
                    stack: None,
                    dealer_ante: false,
                    study: false,
                    balance: false,
                    config: None,
                };
                let r = run_spec(&spec, None, None)?;
                point.vs_ismcts = Some((r.bots[0].per_hand, r.bots[0].per_hand_ci95));
            }
            println!(
                "{id}: {} iterations, {} info sets{}{}",
                point.iterations,
                point.info_sets,
                point
                    .exploitability
                    .map_or(String::new(), |e| format!(", exploitability {e:.4}")),
                point.vs_ismcts.map_or(String::new(), |(m, ci)| format!(
                    ", vs ISMCTS {m:+.3} ±{ci:.3}"
                ))
            );
            curve.points.push(point);
            std::fs::write(
                &curve_path,
                serde_json::to_vec_pretty(&curve).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(curve)
    }
}

fn write_report(out: &Path, name: &str, curves: &[Curve]) -> CliResult {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;");
    let mut body = String::new();
    for c in curves {
        body += &format!(
            "<h2>{}</h2><p class=note>Blueprint: {}</p><table><tr><th>Iterations</th><th>Info sets</th><th>Exploitability</th><th>vs ISMCTS (per hand, ±95 %)</th></tr>",
            esc(&c.game),
            esc(&c.blueprint)
        );
        for p in &c.points {
            body += &format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                p.iterations,
                p.info_sets,
                p.exploitability.map_or("–".into(), |e| format!("{e:.4}")),
                p.vs_ismcts
                    .map_or("–".into(), |(m, ci)| format!("{m:+.3} ±{ci:.3}"))
            );
        }
        body += "</table>";
    }
    let html = format!(
        "<!doctype html><html lang=en><head><meta charset=utf-8><meta name=viewport content='width=device-width, initial-scale=1'><title>Blueprints</title><style>:root{{color-scheme:light dark}}body{{font:14px/1.45 system-ui,sans-serif;max-width:900px;margin:0 auto;padding:24px 16px;background:Canvas;color:CanvasText}}table{{border-collapse:collapse;width:100%;font-variant-numeric:tabular-nums}}td,th{{padding:5px 8px;border-top:1px solid #8884;text-align:left}}.note{{opacity:.7;font-size:12px;word-break:break-all}}</style></head><body><h1>Blueprints · {}</h1>{body}</body></html>",
        esc(name)
    );
    std::fs::create_dir_all(out.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(out, html).map_err(|e| e.to_string())
}
