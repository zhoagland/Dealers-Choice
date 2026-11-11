//! Play settings as a saveable profile (FR-UI-3), and the rule for combining a profile with flags:
//! **start from the profile (or defaults), then any flag the user actually typed wins.**
//!
//! Flags are `Option`s for this reason: `None` means "not given, keep the profile's value",
//! which a plain default value couldn't express.

use clap::Args;
use serde::{Deserialize, Serialize};

/// Everything `dc play` needs, as stored in `config/play/<name>.toml`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PlayProfile {
    /// Player name (bankroll and history are kept per name).
    pub player: String,
    /// Opening bankroll in dollars, used only when the player is new.
    pub bankroll: f64,
    /// Bot strength levels (1-10), one per bot seat.
    pub bots: Vec<u8>,
    /// Always play this game (empty = dealer's choice).
    pub game: String,
    /// Games bots may choose from.
    pub pool: Vec<String>,
    /// Live hints before each decision.
    pub hints: bool,
    /// Post-hand coach review.
    pub review: bool,
    /// Coach review playouts per decision (deterministic).
    pub review_playouts: u32,
    /// Most milliseconds a bot may think per decision.
    pub think_ms: u64,
    /// Stop after this many games.
    pub max_games: u32,
}

impl Default for PlayProfile {
    fn default() -> PlayProfile {
        PlayProfile {
            player: "you".into(),
            bankroll: 100.0,
            bots: vec![7, 5, 3],
            game: String::new(),
            pool: vec!["holdem_nl".into(), "ftq".into()],
            hints: false,
            review: true,
            review_playouts: 15_000,
            think_ms: 500,
            max_games: 1_000,
        }
    }
}

/// `dc play` flags. Anything given overrides the profile.
#[derive(Args)]
pub struct PlayArgs {
    /// Load `config/play/<name>.toml` (edit with `dc config`).
    #[arg(long)]
    pub profile: Option<String>,
    #[arg(long)]
    pub player: Option<String>,
    /// Opening bankroll in dollars (new players only).
    #[arg(long)]
    pub bankroll: Option<f64>,
    /// Bot levels, e.g. `7,5,3`.
    #[arg(long, value_delimiter = ',')]
    pub bots: Option<Vec<u8>>,
    /// Always play this game instead of rotating dealer's choice.
    #[arg(long)]
    pub game: Option<String>,
    #[arg(long, value_delimiter = ',')]
    pub pool: Option<Vec<String>>,
    /// Turn live hints on.
    #[arg(long)]
    pub hints: bool,
    /// Turn the post-hand coach review off.
    #[arg(long)]
    pub no_review: bool,
    #[arg(long)]
    pub review_playouts: Option<u32>,
    #[arg(long)]
    pub think_ms: Option<u64>,
    #[arg(long)]
    pub max_games: Option<u32>,
    /// Session seed (random if omitted). Not saved in profiles.
    #[arg(long)]
    pub seed: Option<u64>,
    /// Don't read or write the database. Not saved in profiles.
    #[arg(long)]
    pub no_save: bool,
}

impl PlayArgs {
    /// Apply the flags that were given on top of `base`.
    pub fn apply(&self, mut base: PlayProfile) -> PlayProfile {
        if let Some(v) = &self.player {
            base.player = v.clone();
        }
        if let Some(v) = self.bankroll {
            base.bankroll = v;
        }
        if let Some(v) = &self.bots {
            base.bots = v.clone();
        }
        if let Some(v) = &self.game {
            base.game = v.clone();
        }
        if let Some(v) = &self.pool {
            base.pool = v.clone();
        }
        if let Some(v) = self.review_playouts {
            base.review_playouts = v;
        }
        if let Some(v) = self.think_ms {
            base.think_ms = v;
        }
        if let Some(v) = self.max_games {
            base.max_games = v;
        }
        if self.hints {
            base.hints = true;
        }
        if self.no_review {
            base.review = false;
        }
        base
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Wrap {
        #[command(flatten)]
        args: PlayArgs,
    }

    fn parse(argv: &[&str]) -> PlayArgs {
        Wrap::parse_from(std::iter::once("dc").chain(argv.iter().copied())).args
    }

    #[test]
    fn flags_override_only_what_they_name() {
        let profile = PlayProfile {
            player: "zach".into(),
            think_ms: 200,
            ..PlayProfile::default()
        };
        let merged = parse(&["--bots", "9,9", "--no-review"]).apply(profile.clone());
        assert_eq!(merged.bots, vec![9, 9]);
        assert!(!merged.review);
        assert_eq!(
            merged.player, "zach",
            "untouched field keeps the profile value"
        );
        assert_eq!(merged.think_ms, 200);
        assert_eq!(parse(&[]).apply(profile.clone()), profile);
    }

    #[test]
    fn profile_toml_round_trips_and_rejects_typos() {
        let text = toml::to_string_pretty(&PlayProfile::default()).unwrap();
        let back: PlayProfile = toml::from_str(&text).unwrap();
        assert_eq!(back, PlayProfile::default());
        assert!(
            toml::from_str::<PlayProfile>("thinkms = 5").is_err(),
            "unknown keys are errors"
        );
        let partial: PlayProfile = toml::from_str("think_ms = 250").unwrap();
        assert_eq!(partial.think_ms, 250);
        assert_eq!(
            partial.bots,
            PlayProfile::default().bots,
            "missing keys take defaults"
        );
    }
}
