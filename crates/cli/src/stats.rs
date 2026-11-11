//! `dc stats --player NAME`: what your stored hands say about your game (FR-PA-1…4).
//!
//! Per game: HUD stats with sample sizes and 95% intervals, VPIP/PFR by position, a true range
//! chart (hole cards are known for every hand, including folds), and the EV leak report built
//! from coach notes. Then a cross-game style summary, labeled indicative only.

use std::collections::BTreeMap;

use clap::Args;

use engine::{Game, Seat};
use games::registry::{GameVisitor, with_game};
use sim::analytics::{PlayerStats, Ratio, Spot, add_hand, leak_report, range_grid};
use sim::encoding::{HandRecord, decode_all};
use storage::{DataRoot, Db, HandRow};

use crate::CliResult;

#[derive(Args)]
pub struct StatsArgs {
    #[arg(long, default_value = "you")]
    pub player: String,
    /// Only this game.
    #[arg(long)]
    pub game: Option<String>,
}

fn pct(r: &Ratio) -> String {
    match (r.p(), r.ci95()) {
        (Some(p), Some((lo, hi))) => {
            format!(
                "{:>5.1}%  [{:>4.1}–{:>5.1}]  n={}",
                p * 100.0,
                lo * 100.0,
                hi * 100.0,
                r.chances
            )
        }
        _ => "    –   (no chances yet)".into(),
    }
}

pub fn run(args: StatsArgs, root: &DataRoot) -> CliResult {
    let db = Db::open(&root.db()).map_err(|e| e.to_string())?;
    let player = db
        .player_id(&args.player)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("no player named {:?}", args.player))?;
    let hands = db.hands(player).map_err(|e| e.to_string())?;
    if hands.is_empty() {
        println!(
            "{} has no stored hands yet (play with `dc play --player {}`).",
            args.player, args.player
        );
        return Ok(());
    }
    let mut by_game: BTreeMap<String, Vec<HandRow>> = BTreeMap::new();
    for row in hands {
        if args.game.as_ref().is_none_or(|g| *g == row.game_id) {
            by_game.entry(row.game_id.clone()).or_default().push(row);
        }
    }

    let mut overall = PlayerStats::default();
    for (game_id, rows) in &by_game {
        let notes = db.coach_notes(player, game_id).map_err(|e| e.to_string())?;
        let visitor = GameStats {
            rows,
            notes: &notes,
        };
        let stats = with_game(game_id, None, visitor).map_err(|e| e.to_string())??;
        overall.aggressive += stats.aggressive;
        overall.calls += stats.calls;
        overall.fold_to_pressure.merge(&stats.fold_to_pressure);
    }

    println!("\n=== Style across all games (indicative only: games differ) ===");
    println!(
        "  aggression factor   {}",
        overall.af().map_or("–".into(), |a| format!("{a:.2}"))
    );
    println!("  fold to pressure    {}", pct(&overall.fold_to_pressure));
    Ok(())
}

struct GameStats<'a> {
    rows: &'a [HandRow],
    notes: &'a [(i64, storage::Note)],
}

impl GameVisitor<Result<PlayerStats, String>> for GameStats<'_> {
    fn visit<G: Game>(self, game: G) -> Result<PlayerStats, String> {
        let mut stats = PlayerStats::default();
        let mut spots_by_hand: BTreeMap<i64, BTreeMap<usize, Spot>> = BTreeMap::new();
        let mut skipped = 0;
        for row in self.rows {
            let records: Vec<HandRecord<G::Action>> = decode_all(&row.record)?;
            let Some(rec) = records.into_iter().next() else {
                continue;
            };
            if rec.game_version != game.meta().version {
                skipped += 1; // recorded under different rules: don't mix them in
                continue;
            }
            let spots = add_hand(&game, &rec.setup, &rec.actions, Seat(row.seat), &mut stats);
            spots_by_hand.insert(row.id, spots);
        }

        let m = game.meta();
        println!(
            "\n=== {} ({} hands{}) ===",
            m.display_name,
            stats.hands,
            if skipped > 0 {
                format!(", {skipped} from older rules skipped")
            } else {
                String::new()
            }
        );
        println!(
            "  result              {:+.2} ± {:.2} {} per 100 hands",
            stats.result.mean * 100.0,
            stats.result.ci95() * 100.0,
            game.eval_unit()
        );
        println!("  VPIP                {}", pct(&stats.vpip));
        println!("  PFR / first raise   {}", pct(&stats.pfr));
        println!("  3-bet               {}", pct(&stats.three_bet));
        println!(
            "  aggression factor   {}",
            stats.af().map_or("–".into(), |a| format!(
                "{a:.2}  ({} bets+raises / {} calls)",
                stats.aggressive, stats.calls
            ))
        );
        println!("  c-bet               {}", pct(&stats.cbet));
        println!("  fold to c-bet       {}", pct(&stats.fold_to_cbet));
        println!("  went to showdown    {}", pct(&stats.wtsd));
        println!("  won at showdown     {}", pct(&stats.won_at_showdown));
        println!("  fold to pressure    {}", pct(&stats.fold_to_pressure));
        println!("  by position (0 = dealer/button):");
        for (pos, (v, p)) in &stats.by_position {
            println!("    {pos}: VPIP {}   PFR {}", pct(v), pct(p));
        }
        if !stats.range.is_empty() {
            println!("  VPIP % by starting hand (suited above the diagonal, offsuit below):");
            print!("{}", range_grid(&stats));
        }

        let leaks = leak_report(self.notes.iter().filter_map(|(hand, n)| {
            let spot = spots_by_hand.get(hand)?.get(&n.decision_idx)?.clone();
            Some((spot, n.ev_loss, n.se, n.flagged))
        }));
        if leaks.is_empty() {
            println!("  leaks: none significant (> 2 SE) yet");
        } else {
            println!(
                "  biggest leaks (EV lost vs the coach's model, {} units; caveat: bots.md §7):",
                game.eval_unit()
            );
            for l in leaks.iter().take(8) {
                println!(
                    "    round {} pos {} {:<7}  lost {:>7.2} ± {:<5.2}  over {:>4} decisions  (best play {}/{})",
                    l.spot.round,
                    l.spot.position,
                    l.spot.action,
                    l.total_loss,
                    l.total_se,
                    l.decisions,
                    l.matched_best,
                    l.decisions
                );
            }
        }
        Ok(stats)
    }
}
