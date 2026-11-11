//! Per-starting-hand and per-decision aggregates, built from **every** simulated hand
//! (`docs/tech/sim-program.md` §4.5). Nothing is stored per hand: each hand adds its result to a
//! handful of running means, so millions of hands cost kilobytes.
//!
//! - **Starting hands**: result per (starting-hand class, position, first action). Comparing
//!   "fold" with "call"/"raise" for the same class and position says which hands are worth playing.
//! - **Decisions**: result per (street, board class, hand-strength bucket, action taken). The
//!   action is a fold/check/call, a bet sized relative to the pot, or a game-specific choice such
//!   as a declaration. This answers "when to bet, and how much" for each kind of board.
//!
//! These are outcomes of the simulated population's own play (the EV of hands where the bots took
//! that action), not solved values. Dashboards say so.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use engine::{ActionClass, Chips, DecisionContext, Game, Seat, SizeBucket};

use crate::stats::Welford;

/// Position relative to the button: 0 = first seat left of the button, `n-1` = the button.
pub fn position(seat: Seat, button: Seat, n: u8) -> u8 {
    (seat.0 + n - button.0 - 1) % n
}

/// Short label for a seat's first action of the hand.
pub fn first_action_label(class: ActionClass) -> &'static str {
    match class {
        ActionClass::Fold => "fold",
        ActionClass::CheckOrCall { owed } if owed == Chips::ZERO => "check",
        ActionClass::CheckOrCall { .. } => "call",
        ActionClass::BetOrRaise { .. } => "raise",
        _ => "other",
    }
}

/// Label for an action in a decision situation: bets are bucketed by size relative to the pot;
/// non-betting choices use the game's own description (e.g. "declare High").
pub fn decision_action_label<G: Game>(
    game: &G,
    class: ActionClass,
    action: G::Action,
    ctx: &DecisionContext,
) -> String {
    match class {
        ActionClass::Fold => "fold".into(),
        ActionClass::CheckOrCall { owed } if owed == Chips::ZERO => "check".into(),
        ActionClass::CheckOrCall { .. } => "call".into(),
        ActionClass::BetOrRaise { to } => {
            let all_in = to - ctx.street_committed >= ctx.stack;
            format!("bet {}", SizeBucket::of(to, ctx, all_in).label())
        }
        _ => game.describe_action(action),
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Study {
    /// (start class, position, first action) → result in eval units.
    pub start: BTreeMap<(u16, u8, String), Welford>,
    /// (street, board class, strength, action) → result in eval units.
    pub decisions: BTreeMap<(u8, u16, u8, String), Welford>,
    /// Hands counted.
    pub hands: u64,
}

impl Study {
    pub fn merge(&mut self, o: &Study) {
        for (k, w) in &o.start {
            self.start.entry(k.clone()).or_default().merge(w);
        }
        for (k, w) in &o.decisions {
            self.decisions.entry(k.clone()).or_default().merge(w);
        }
        self.hands += o.hands;
    }

    /// JSON-friendly form (maps with tuple keys don't serialize to JSON objects).
    pub fn to_rows(&self) -> StudyRows {
        StudyRows {
            hands: self.hands,
            start: self
                .start
                .iter()
                .map(|((class, pos, first), w)| StartRow {
                    class: *class,
                    pos: *pos,
                    first: first.clone(),
                    stat: *w,
                })
                .collect(),
            decisions: self
                .decisions
                .iter()
                .map(|((street, board, strength, action), w)| DecisionRow {
                    street: *street,
                    board: *board,
                    strength: *strength,
                    action: action.clone(),
                    stat: *w,
                })
                .collect(),
        }
    }

    pub fn from_rows(rows: &StudyRows) -> Study {
        Study {
            hands: rows.hands,
            start: rows
                .start
                .iter()
                .map(|r| ((r.class, r.pos, r.first.clone()), r.stat))
                .collect(),
            decisions: rows
                .decisions
                .iter()
                .map(|r| ((r.street, r.board, r.strength, r.action.clone()), r.stat))
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StudyRows {
    pub hands: u64,
    pub start: Vec<StartRow>,
    pub decisions: Vec<DecisionRow>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StartRow {
    pub class: u16,
    pub pos: u8,
    pub first: String,
    pub stat: Welford,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DecisionRow {
    pub street: u8,
    pub board: u16,
    pub strength: u8,
    pub action: String,
    pub stat: Welford,
}

/// Collects one hand's observations, then credits each with the seat's final result.
pub struct HandStudy {
    n: usize,
    first: Vec<Option<&'static str>>,
    pending: Vec<(Seat, (u8, u16, u8, String))>,
}

impl HandStudy {
    pub fn new(n: usize) -> HandStudy {
        HandStudy {
            n,
            first: vec![None; n],
            pending: Vec::new(),
        }
    }

    /// Call before applying a seat's action, with the state at that moment.
    pub fn on_decision<G: Game>(&mut self, game: &G, s: &G::State, seat: Seat, a: G::Action) {
        let class = game.classify(s, a);
        let i = seat.index();
        if self.first[i].is_none() {
            self.first[i] = Some(first_action_label(class));
        }
        if let Some(ctx) = game.decision_context(s, seat) {
            let label = decision_action_label(game, class, a, &ctx);
            self.pending
                .push((seat, (ctx.street, ctx.board, ctx.strength, label)));
        }
    }

    /// Call at Terminal with the payoffs (in eval units).
    pub fn finish<G: Game>(
        self,
        game: &G,
        terminal: &G::State,
        button: Seat,
        results: &[f64],
        into: &mut Study,
    ) {
        let n = self.n as u8;
        for (i, (&result, first)) in results.iter().zip(&self.first).enumerate() {
            let seat = Seat(i as u8);
            if let Some(class) = game.start_class(terminal, seat) {
                let first = first.unwrap_or("walk");
                into.start
                    .entry((class, position(seat, button, n), first.to_string()))
                    .or_default()
                    .add(result);
            }
        }
        for (seat, key) in self.pending {
            into.decisions
                .entry(key)
                .or_default()
                .add(results[seat.index()]);
        }
        into.hands += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_count_from_left_of_button() {
        assert_eq!(position(Seat(1), Seat(0), 6), 0);
        assert_eq!(position(Seat(0), Seat(0), 6), 5);
        assert_eq!(position(Seat(3), Seat(5), 6), 3);
    }

    #[test]
    fn rows_round_trip() {
        let mut s = Study::default();
        s.start.entry((5, 1, "call".into())).or_default().add(1.5);
        s.decisions
            .entry((1, 3, 7, "bet ~pot".into()))
            .or_default()
            .add(-2.0);
        s.hands = 1;
        let json = serde_json::to_string(&s.to_rows()).unwrap();
        let back: StudyRows = serde_json::from_str(&json).unwrap();
        assert_eq!(Study::from_rows(&back), s);
    }
}
