//! Game-agnostic prompts (D-WEB-1): the options a player sees come from the game's legal actions,
//! labelled by `describe_action` and grouped by `classify`; answers map back to a `G::Action`.
//! Bet sizes become one "amount" option with the bot-menu sizes as presets; when the game can
//! parse typed amounts (`parse_action("r 2.50")`), any legal amount in range is accepted.

use engine::{ActionClass, Chips, Game};
use protocol::{Choice, Preset, PromptOption};

/// The options for `legal`, and how to map an answer back.
pub struct Built<A> {
    pub options: Vec<PromptOption>,
    /// Button id → action.
    buttons: Vec<(String, A)>,
    /// Preset amount (cents) → action.
    amounts: Vec<(i64, A)>,
    /// Typed amounts allowed in this range (cents).
    range: Option<(i64, i64)>,
}

fn class_name(c: ActionClass) -> &'static str {
    match c {
        ActionClass::Fold => "fold",
        ActionClass::CheckOrCall { owed } if owed == Chips::ZERO => "check",
        ActionClass::CheckOrCall { .. } => "call",
        ActionClass::BetOrRaise { .. } => "bet",
        ActionClass::Declare => "declare",
        ActionClass::CardChoice => "cards",
        ActionClass::Chance => "other",
    }
}

pub fn build<G: Game>(game: &G, s: &G::State, legal: &[G::Action]) -> Built<G::Action> {
    let mut options = Vec::new();
    let mut buttons = Vec::new();
    let mut amounts = Vec::new();
    for (i, &a) in legal.iter().enumerate() {
        match game.classify(s, a) {
            ActionClass::BetOrRaise { to } => amounts.push((to.0, a)),
            c => {
                let label = match c {
                    ActionClass::CheckOrCall { owed } if owed > Chips::ZERO => {
                        format!("Call {owed}")
                    }
                    ActionClass::CheckOrCall { .. } => "Check".to_string(),
                    ActionClass::Fold => "Fold".to_string(),
                    _ => capitalize(&game.describe_action(a)),
                };
                let id = format!("a{i}");
                options.push(PromptOption::Button {
                    id: id.clone(),
                    label,
                    class: class_name(c).into(),
                });
                buttons.push((id, a));
            }
        }
    }
    let mut range = None;
    if !amounts.is_empty() {
        amounts.sort_by_key(|&(c, _)| c);
        let (min, max) = (amounts[0].0, amounts[amounts.len() - 1].0);
        let typed = game
            .parse_action(
                &format!("r {}", Chips(min).to_string().trim_start_matches('$')),
                legal,
            )
            .is_some();
        if typed {
            range = Some((min, max));
        }
        options.push(PromptOption::Amount {
            id: "amount".into(),
            label: "Bet / raise to".into(),
            min,
            max,
            step: if typed { 50 } else { 0 },
            presets: amounts
                .iter()
                .map(|&(c, a)| Preset {
                    label: capitalize(&game.describe_action(a)),
                    cents: c,
                })
                .collect(),
        });
    }
    Built {
        options,
        buttons,
        amounts,
        range,
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

impl<A: Copy> Built<A> {
    /// The action for an answer, or why not. `parse` turns a typed amount into an action.
    pub fn answer(&self, choice: &Choice, parse: impl Fn(i64) -> Option<A>) -> Result<A, String> {
        match choice {
            Choice::Option { id } => self
                .buttons
                .iter()
                .find(|(b, _)| b == id)
                .map(|&(_, a)| a)
                .ok_or_else(|| "that option isn't available".into()),
            Choice::Amount { cents, .. } => {
                if let Some(&(_, a)) = self.amounts.iter().find(|(c, _)| c == cents) {
                    return Ok(a);
                }
                match self.range {
                    Some((lo, hi)) if *cents >= lo && *cents <= hi => {
                        parse(*cents).ok_or_else(|| "that amount isn't allowed".into())
                    }
                    _ => Err("pick one of the amounts offered".into()),
                }
            }
        }
    }
}

/// How an action reads to the rest of the table: card choices (discards, passes, keeps) and
/// declarations don't say which cards or which way, since others couldn't see that live.
pub fn public_label<G: Game>(game: &G, s: &G::State, a: G::Action) -> String {
    match game.classify(s, a) {
        ActionClass::CardChoice => "chooses cards".into(),
        ActionClass::Declare => "declares".into(),
        _ => game.describe_action(a),
    }
}

/// D-WEB-4: on timeout, check if possible, else fold; declarations take the first option
/// (High); anything else takes the first legal action.
pub fn default_action<G: Game>(game: &G, s: &G::State, legal: &[G::Action]) -> G::Action {
    let class = |a: &G::Action| game.classify(s, *a);
    legal
        .iter()
        .find(|a| matches!(class(a), ActionClass::CheckOrCall { owed } if owed == Chips::ZERO))
        .or_else(|| legal.iter().find(|a| matches!(class(a), ActionClass::Fold)))
        .copied()
        .unwrap_or(legal[0])
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::HandSetup;
    use games::holdem::{Config, Holdem};

    #[test]
    fn holdem_prompt_has_fold_call_and_amounts() {
        let g = Holdem::new(Config::default());
        let mut s = g.new_hand(&HandSetup::simple(2, Chips(10_000)));
        let mut rng = engine::rng::rng_from_seed(1);
        while g.to_act(&s) == engine::ToAct::Chance {
            let a = g.sample_chance(&s, &mut rng);
            g.apply(&mut s, a);
        }
        let mut legal = Vec::new();
        g.legal_actions(&s, &mut legal);
        let b = build(&g, &s, &legal);
        assert!(
            b.options
                .iter()
                .any(|o| matches!(o, PromptOption::Button { class, .. } if class == "fold"))
        );
        let (min, max) = match b
            .options
            .iter()
            .find(|o| matches!(o, PromptOption::Amount { .. }))
        {
            Some(PromptOption::Amount { min, max, .. }) => (*min, *max),
            _ => panic!("no amount option"),
        };
        assert!(min < max);
        // A typed amount between the presets maps to a legal raise.
        let mid = (min + max) / 2 / 50 * 50;
        let a = b
            .answer(
                &Choice::Amount {
                    id: "amount".into(),
                    cents: mid,
                },
                |c| {
                    g.parse_action(
                        &format!("r {}", Chips(c).to_string().trim_start_matches('$')),
                        &legal,
                    )
                },
            )
            .unwrap();
        assert!(g.validate(&s, a).is_ok());
        assert!(g.validate(&s, default_action(&g, &s, &legal)).is_ok());
    }
}
