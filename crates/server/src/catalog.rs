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
                rules: m.rules_ref.into(),
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
    }
}
