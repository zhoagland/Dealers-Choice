//! What to simulate (`docs/tech/sim.md` §1): bots, deals, seeds, output sampling.

use serde::{Deserialize, Serialize};

use std::sync::Arc;

use bots::mccfr::{Blueprint, BlueprintPolicy};
use bots::{Budget, IsmctsBot, Policy, RandomPolicy};
use engine::Game;

/// A bot description like `"random"`, `"ismcts:7"`, or `"ismcts:7:2000"` (level 7 strength with
/// a 2,000-playout budget).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum BotSpec {
    Random,
    Ismcts {
        level: u8,
        playouts: Option<u32>,
    },
    /// A saved MCCFR blueprint file (`dc train --save`).
    Blueprint {
        path: String,
    },
}

impl BotSpec {
    /// A fresh bot. Sims always use **playout** budgets: time budgets depend on CPU speed and
    /// would make results irreproducible (tech/README rule 1).
    pub fn build<G: Game>(&self) -> Box<dyn Policy<G>> {
        match *self {
            BotSpec::Random => Box::new(RandomPolicy),
            BotSpec::Blueprint { ref path } => {
                Box::new(BlueprintPolicy::new(cached_blueprint::<G>(path)))
            }
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
        // Checked first: paths (e.g. `C:\…`) may themselves contain ':'.
        if let Some(path) = s.strip_prefix("mccfr:") {
            return Ok(BotSpec::Blueprint {
                path: path.to_string(),
            });
        }
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
            BotSpec::Blueprint { path } => write!(f, "mccfr:{path}"),
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

/// Blueprints are loaded once per process and shared: bots are built once per hand, and
/// re-reading the file each time made blueprint sims ~100× slower than necessary.
///
/// The cache must hold blueprints for *any* game type, so entries are stored type-erased as
/// `Arc<dyn Any>` and downcast back to `Blueprint<G>` on the way out. A path loaded as one game
/// and requested as another fails the downcast and is reloaded (and then rejected by the game
/// id check).
fn cached_blueprint<G: Game>(path: &str) -> Arc<Blueprint<G>> {
    use std::any::Any;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    type Cache = Mutex<HashMap<String, Arc<dyn Any + Send + Sync>>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();

    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(any) = cache.lock().unwrap().get(path).cloned()
        && let Ok(bp) = any.downcast::<Blueprint<G>>()
    {
        return bp;
    }
    let bp = Arc::new(load_blueprint::<G>(path));
    cache.lock().unwrap().insert(path.to_string(), bp.clone());
    bp
}

/// Read a blueprint file (postcard, see `bots::mccfr::blueprint`). Panics with a clear message if it
/// can't be loaded: a sim with a missing bot can't produce meaningful results.
fn load_blueprint<G: Game>(path: &str) -> Blueprint<G> {
    let raw = std::fs::read(path).unwrap_or_else(|e| panic!("blueprint {path}: {e}"));
    let game = G::new(G::Config::default());
    Blueprint::from_bytes(&game, &raw).unwrap_or_else(|e| panic!("blueprint {path}: {e}"))
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
