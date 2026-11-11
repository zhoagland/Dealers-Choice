//! Game-agnostic hooks that let the simulator aggregate results by starting hand and by
//! decision situation, over every simulated hand, without storing hand records
//! (`docs/tech/sim-program.md` §4.5).
//!
//! A driver describes situations with small integer ids (a starting-hand class, a street, a
//! board-texture class, a hand-strength bucket) and gives each id a display name once, in
//! [`AnalysisLabels`]. The simulator only counts and averages per id; dashboards print the names.

use serde::{Deserialize, Serialize};

use crate::game::Chips;

/// The situation a seat is in when it decides. Ids index into [`AnalysisLabels`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DecisionContext {
    /// Betting round: index into `AnalysisLabels::streets`.
    pub street: u8,
    /// Board/table texture: index into `AnalysisLabels::boards` (0 when there is no board).
    pub board: u16,
    /// The deciding seat's current hand strength: index into `AnalysisLabels::strengths`.
    pub strength: u8,
    /// Chips in the pot, including this street's bets.
    pub pot: Chips,
    /// What this seat has already put in on this street (to size raises by their increment).
    pub street_committed: Chips,
    /// Chips the seat still has behind (to recognize all-in bets).
    pub stack: Chips,
}

/// Display names for every id a game's analysis hooks can return.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisLabels {
    /// Names of `Game::start_class` ids (e.g. the 169 Hold'em hands, `"AKs"`).
    pub start_classes: Vec<String>,
    /// For drawing a starting-hand grid: `(row, column)` per start class, if the game has one
    /// (Hold'em's 13×13). Empty = draw as a ranked list.
    pub start_grid: Vec<(u8, u8)>,
    pub streets: Vec<String>,
    pub boards: Vec<String>,
    pub strengths: Vec<String>,
}

/// How big a bet or raise is relative to the pot, for "how much to bet" reports.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SizeBucket {
    /// Up to ~40 % of the pot.
    Third,
    /// Up to ~80 %.
    TwoThirds,
    /// Up to ~120 %.
    Pot,
    /// More than that.
    Over,
    /// The whole stack.
    AllIn,
}

impl SizeBucket {
    pub const ALL: [SizeBucket; 5] = [
        SizeBucket::Third,
        SizeBucket::TwoThirds,
        SizeBucket::Pot,
        SizeBucket::Over,
        SizeBucket::AllIn,
    ];

    /// Classify a raise to `to` given the pot before it and what the seat had already put in on
    /// this street. `all_in` = the raise uses the seat's whole stack.
    pub fn of(to: Chips, ctx: &DecisionContext, all_in: bool) -> SizeBucket {
        if all_in {
            return SizeBucket::AllIn;
        }
        let added = (to - ctx.street_committed).0.max(0) as f64;
        let pot = ctx.pot.0.max(1) as f64;
        match added / pot {
            r if r <= 0.4 => SizeBucket::Third,
            r if r <= 0.8 => SizeBucket::TwoThirds,
            r if r <= 1.2 => SizeBucket::Pot,
            _ => SizeBucket::Over,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SizeBucket::Third => "≤⅓ pot",
            SizeBucket::TwoThirds => "≤⅔ pot",
            SizeBucket::Pot => "~pot",
            SizeBucket::Over => "overbet",
            SizeBucket::AllIn => "all-in",
        }
    }
}
