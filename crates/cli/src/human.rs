//! The human at the keyboard, as a `Policy` (so the table loop treats them like any bot).

use std::io::{BufRead, Write};

use bots::ismcts::{Ismcts, IsmctsConfig};
use bots::{Budget, Evaluator, Policy};
use engine::{Chips, DetRng, Game, IllegalAction};

pub struct HumanPolicy<G: Game> {
    /// Live hints (FR-EV-4): evaluate before each decision.
    pub hints: Option<(Ismcts<G>, Budget)>,
}

impl<G: Game> HumanPolicy<G> {
    pub fn new(hints: bool, budget: Budget) -> HumanPolicy<G> {
        HumanPolicy {
            hints: hints.then(|| (Ismcts::new(IsmctsConfig::default()), budget)),
        }
    }
}

/// Read one trimmed line from stdin. End of input ends the program cleanly.
pub fn read_line(prompt: &str) -> String {
    print!("{prompt}");
    std::io::stdout().flush().ok();
    let mut line = String::new();
    if std::io::stdin().lock().read_line(&mut line).unwrap_or(0) == 0 {
        println!("\n(end of input)");
        std::process::exit(0);
    }
    line.trim().to_string()
}

impl<G: Game> Policy<G> for HumanPolicy<G> {
    fn act(
        &mut self,
        game: &G,
        obs: &G::Observation,
        legal: &[G::Action],
        rng: &mut DetRng,
    ) -> G::Action {
        println!("\n{}", game.describe_observation(obs));
        if let Some((search, budget)) = self.hints.as_mut() {
            let values = search.evaluate(game, obs, legal, *budget, rng);
            println!("  hints (EV in {} units, ± 1 SE):", game.eval_unit());
            for v in &values {
                println!(
                    "    {:<24} {:+8.3} ± {:.3}",
                    game.describe_action(v.action),
                    v.q,
                    v.se
                );
            }
        }
        for (i, &a) in legal.iter().enumerate() {
            println!("  [{}] {}", i + 1, game.describe_action(a));
        }
        loop {
            let input = read_line("your action> ");
            if let Ok(k) = input.parse::<usize>()
                && (1..=legal.len()).contains(&k)
            {
                return legal[k - 1];
            }
            if let Some(a) = game.parse_action(&input, legal) {
                return a;
            }
            println!(
                "  enter a number from the list, or e.g. `f`, `c`, `r 6.50` where the game supports it"
            );
        }
    }

    fn name(&self) -> String {
        "you".into()
    }

    fn on_rejected(&mut self, error: &IllegalAction) -> bool {
        println!("  {error}");
        true
    }

    fn on_hand_end(&mut self, payoff: Chips) {
        println!("  hand result for you: {payoff}");
    }
}

/// Print the post-hand coach report (FR-EV-3).
pub fn print_review<G: Game>(game: &G, reviews: &[bots::coach::DecisionReview<G::Action>]) {
    if reviews.is_empty() {
        return;
    }
    println!(
        "\n  coach review (EV lost in {} units; ⚠ = outside 2 SE):",
        game.eval_unit()
    );
    for r in reviews {
        let best = &r.values[0];
        let loss = match r.ev_loss {
            Some(l) => format!("{l:.3} ± {:.3}", r.se),
            None => "off-menu, not scored".into(),
        };
        println!(
            "  {} you: {:<22} best: {:<22} lost {}",
            if r.flagged { "⚠" } else { " " },
            game.describe_action(r.chosen),
            game.describe_action(best.action),
            loss
        );
    }
}
