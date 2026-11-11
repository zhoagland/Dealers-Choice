//! Deliberately bad, predictable bots with known leaks. Useful as sim opponents and as
//! **ground truth for player analytics** (M7 exit criterion): their stats are known in advance.
//!
//! - [`CallingStation`]: never folds, never raises; checks or calls everything.
//! - [`Maniac`]: always takes the largest bet/raise on the menu; calls when it can't raise.

use engine::{ActionClass, Chips, DetRng, Game, UniformSampler};

use crate::policy::Policy;

/// Classify each legal action. Action classes are public, so any determinization of the
/// observation classifies them the same way.
fn classes<G: Game>(
    game: &G,
    obs: &G::Observation,
    legal: &[G::Action],
    rng: &mut DetRng,
) -> Vec<ActionClass> {
    let s = game.determinize(obs, &mut UniformSampler, rng);
    legal.iter().map(|&a| game.classify(&s, a)).collect()
}

#[derive(Copy, Clone, Debug, Default)]
pub struct CallingStation;

impl<G: Game> Policy<G> for CallingStation {
    fn act(
        &mut self,
        game: &G,
        obs: &G::Observation,
        legal: &[G::Action],
        rng: &mut DetRng,
    ) -> G::Action {
        let c = classes(game, obs, legal, rng);
        match c
            .iter()
            .position(|c| matches!(c, ActionClass::CheckOrCall { .. }))
        {
            Some(i) => legal[i],
            None => legal[0], // e.g. a declaration: first option
        }
    }

    fn name(&self) -> String {
        "callstation".into()
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub struct Maniac;

impl<G: Game> Policy<G> for Maniac {
    fn act(
        &mut self,
        game: &G,
        obs: &G::Observation,
        legal: &[G::Action],
        rng: &mut DetRng,
    ) -> G::Action {
        let c = classes(game, obs, legal, rng);
        let biggest = c
            .iter()
            .enumerate()
            .filter_map(|(i, c)| match c {
                ActionClass::BetOrRaise { to } => Some((*to, i)),
                _ => None,
            })
            .max_by_key(|&(to, _): &(Chips, usize)| to);
        if let Some((_, i)) = biggest {
            return legal[i];
        }
        match c
            .iter()
            .position(|c| matches!(c, ActionClass::CheckOrCall { .. }))
        {
            Some(i) => legal[i],
            None => legal[0],
        }
    }

    fn name(&self) -> String {
        "maniac".into()
    }
}
