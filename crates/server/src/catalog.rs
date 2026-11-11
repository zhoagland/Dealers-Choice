//! The games offered at web tables: every Bible game (FR-GC-1), with names, families and seat
//! ranges for the chooser's picker.

use engine::Game;
use games::registry::{GAME_IDS, GameVisitor, with_game};
use protocol::GameInfo;

/// The family a game belongs to (catalog §4), for grouping the picker.
pub fn family(id: &str) -> &'static str {
    match id {
        "holdem_nl" | "holdem_6plus" | "holdem_showtime" | "reverse_poker" | "holdem_split"
        | "holdem_swap" | "fusion" | "holdem_tempest" | "holdem_indian" | "crazy_pineapple"
        | "mamas_favorite" => "Hold'em",
        "omaha_pl" | "omaha5" | "courchevel" | "red_river" | "plo_double_board" => "Omaha",
        "elevator" | "wheel_of_death" | "spit" | "industrial_elevator" | "iron_cross" => {
            "Board shapes"
        }
        "draw5" | "triple_draw" | "draw27_single" | "draw27_triple" | "badugi"
        | "three_blind_mice" | "guts_leg" | "scare_a_moosh" => "Draw",
        "ftq" | "stud7" | "razz" | "chicago_high" | "chicago_low" | "baseball"
        | "low_in_the_hole" | "push" | "bullet" | "bid" => "Stud",
        "pass_the_trash" | "man_or_mouse" | "game_43" => "Pass & in/out",
        "drop_321" | "three_five_seven" => "Drop games",
        "blackjack" | "seven_27" | "five_21" | "numbers" => "Numbers",
        "indian_poker" => "Indian",
        "booray" => "Tricks",
        _ => "Other",
    }
}

/// A one-line summary from the all-games table in `docs/tech/games.md` ("shape; hands").
fn summary(id: &str) -> Option<String> {
    const DOC: &str = include_str!("../../../docs/tech/games.md");
    let key = format!("| `{id}` |");
    let row = DOC.lines().find(|l| l.starts_with(&key))?;
    let cols: Vec<&str> = row.split('|').map(str::trim).collect();
    // ["", id, game, shape, hands, decisions, ""]
    Some(format!("{}. Hands: {}.", cols.get(3)?, cols.get(4)?))
}

/// Games in `pool` (empty = every Bible game), with their details.
pub fn playable(pool: &[String]) -> Vec<GameInfo> {
    struct Info(&'static str);
    impl GameVisitor<GameInfo> for Info {
        fn visit<G: Game>(self, g: G) -> GameInfo {
            let m = g.meta();
            GameInfo {
                id: self.0.into(),
                name: m.display_name.into(),
                family: family(self.0).into(),
                seats_min: *m.seats.start(),
                seats_max: *m.seats.end(),
                rules: summary(self.0).unwrap_or_else(|| m.rules_ref.into()),
            }
        }
    }
    GAME_IDS
        .iter()
        .filter(|id| !matches!(**id, "kuhn" | "leduc"))
        .filter(|id| pool.is_empty() || pool.iter().any(|p| p == *id))
        .filter_map(|&id| with_game(id, None, Info(id)).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_bible_game_is_playable_and_has_a_family() {
        let all = super::playable(&[]);
        assert_eq!(all.len(), 50, "all 50 Bible games");
        for g in &all {
            assert_ne!(g.family, "Other", "{} has no family", g.id);
        }
        assert!(
            all.iter()
                .find(|g| g.id == "omaha5")
                .unwrap()
                .rules
                .contains("declare")
        );
    }

    /// Writes `web/src/generated/gallery.json`: a mid-hand and a final view of every game from
    /// seat 0 after random play, for the client's `#/dev/views` gallery (web.md §8, W6).
    #[test]
    fn export_view_gallery() {
        use engine::{Chips, Game, GameCarry, HandSetup, Seat, ToAct};
        use games::registry::{GameVisitor, with_game};
        use rand::Rng;

        struct Views(u64);
        impl GameVisitor<Vec<engine::TableView>> for Views {
            fn visit<G: Game>(self, g: G) -> Vec<engine::TableView> {
                let m = g.meta();
                let n = 4u8.clamp(*m.seats.start(), *m.seats.end());
                let setup = HandSetup::simple(n, Chips(10_000));
                let carry = GameCarry::new(&g);
                let mut s = carry.new_hand(&g, &setup);
                let mut rng = engine::rng::rng_from_seed(self.0);
                let mut legal = Vec::new();
                let mut seen = Vec::new();
                let me = Seat(0);
                loop {
                    match g.to_act(&s) {
                        ToAct::Terminal => break,
                        ToAct::Chance => {
                            let a = g.sample_chance(&s, &mut rng);
                            g.apply(&mut s, a);
                        }
                        ToAct::Seat(_) => {
                            seen.push(g.view(&g.observe(&s, me)));
                            g.legal_actions(&s, &mut legal);
                            // Prefer passive actions so hands reach later streets.
                            let passive: Vec<_> = legal
                                .iter()
                                .copied()
                                .filter(|&a| {
                                    matches!(
                                        g.classify(&s, a),
                                        engine::ActionClass::CheckOrCall { .. }
                                    )
                                })
                                .collect();
                            let pick = if !passive.is_empty() && rng.random_bool(0.85) {
                                passive[rng.random_range(0..passive.len())]
                            } else {
                                legal[rng.random_range(0..legal.len())]
                            };
                            g.apply(&mut s, pick);
                        }
                    }
                }
                let mut out = Vec::new();
                if !seen.is_empty() {
                    out.push(seen[seen.len() * 2 / 3].clone());
                }
                out.push(g.view(&g.observe(&s, me)));
                out
            }
        }

        let mut entries = Vec::new();
        for (i, g) in super::playable(&[]).into_iter().enumerate() {
            let views = with_game(&g.id, None, Views(7 + i as u64)).unwrap();
            entries.push(serde_json::json!({
                "id": g.id, "name": g.name, "family": g.family, "views": views,
            }));
        }
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../web/src/generated/gallery.json");
        std::fs::write(
            &path,
            serde_json::to_string(&entries).unwrap()
                + "
",
        )
        .unwrap();
    }
}
