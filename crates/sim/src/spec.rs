//! What to simulate (`docs/tech/sim.md` §1): bots, deals, seeds, output sampling.

use serde::{Deserialize, Serialize};

use bots::{Budget, IsmctsBot, Policy, RandomPolicy};
use engine::Game;

/// A bot description like `"random"`, `"ismcts:7"`, or `"ismcts:7:2000"` (level 7 strength with
/// a 2,000-playout budget).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum BotSpec {
    Random,
    Ismcts { level: u8, playouts: Option<u32> },
}

impl BotSpec {
    /// A fresh bot. Sims always use **playout** budgets: time budgets depend on CPU speed and
    /// would make results irreproducible (tech/README rule 1).
    pub fn build<G: Game>(&self) -> Box<dyn Policy<G>> {
        match *self {
            BotSpec::Random => Box::new(RandomPolicy),
            BotSpec::Ismcts { level, playouts } => {
                let mut bot = IsmctsBot::<G>::preset(level, false);
                if let Some(p) = playouts {
                    bot.strength.budget = Budget::Playouts(p);
                }
                Box::new(bot)
            }
        }
    }
}

impl TryFrom<String> for BotSpec {
    type Error = String;
    fn try_from(s: String) -> Result<BotSpec, String> {
        s.parse()
    }
}

impl From<BotSpec> for String {
    fn from(b: BotSpec) -> String {
        b.to_string()
    }
}

impl std::str::FromStr for BotSpec {
    type Err = String;
    fn from_str(s: &str) -> Result<BotSpec, String> {
        let parts: Vec<&str> = s.split(':').collect();
        let num = |p: &str| {
            p.parse::<u32>()
                .map_err(|_| format!("bad number {p:?} in bot {s:?}"))
        };
        match parts.as_slice() {
            ["random"] => Ok(BotSpec::Random),
            ["ismcts", l] => Ok(BotSpec::Ismcts {
                level: num(l)?.clamp(1, 10) as u8,
                playouts: None,
            }),
            ["ismcts", l, p] => Ok(BotSpec::Ismcts {
                level: num(l)?.clamp(1, 10) as u8,
                playouts: Some(num(p)?),
            }),
            _ => Err(format!(
                "unknown bot {s:?}: use random, ismcts:<level>, or ismcts:<level>:<playouts>"
            )),
        }
    }
}

impl std::fmt::Display for BotSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BotSpec::Random => write!(f, "random"),
            BotSpec::Ismcts {
                level,
                playouts: None,
            } => write!(f, "ismcts:{level}"),
            BotSpec::Ismcts {
                level,
                playouts: Some(p),
            } => write!(f, "ismcts:{level}:{p}"),
        }
    }
}

/// A simulation run.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunSpec {
    pub game: String,
    /// One bot per seat.
    pub bots: Vec<BotSpec>,
    /// Distinct deals. With `duplicate`, each is played once per seat rotation.
    pub deals: u64,
    pub seed: u64,
    /// Play every deal with the bots rotated through every seat (sim.md §3).
    #[serde(default = "yes")]
    pub duplicate: bool,
    /// Fraction of hands whose full record is kept (FR-SM-4).
    #[serde(default)]
    pub history_sample: f64,
    /// Starting stack in cents (default: the game's table stake, else 100 eval units).
    #[serde(default)]
    pub stack: Option<i64>,
    /// Charge the dealer ante on every hand (default off: it models one-hand-per-game play).
    #[serde(default)]
    pub dealer_ante: bool,
}

fn yes() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bot_spec_round_trips() {
        for s in ["random", "ismcts:7", "ismcts:3:500"] {
            let b: BotSpec = s.parse().unwrap();
            assert_eq!(b.to_string(), s);
        }
        assert!("mcts".parse::<BotSpec>().is_err());
    }
}
