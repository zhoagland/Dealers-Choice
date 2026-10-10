//! The interfaces every bot implements (PRD FR-BT-1, FR-EV-1). `docs/tech/bots.md` §1.

use std::time::Duration;

use engine::{DetRng, Game};

/// Something that picks an action for one seat: a bot, or the human input adapter.
///
/// The trait is generic over the game (`Policy<G>`) rather than having a generic method, so it
/// stays *object-safe*: a table can hold `Box<dyn Policy<G>>` for seats with different bot types.
pub trait Policy<G: Game>: Send {
    /// Choose one of `legal`. `obs` is everything this seat knows. `legal` comes from the table
    /// (the legal set is public), so a policy never needs the full state to know what's allowed.
    fn act(
        &mut self,
        game: &G,
        obs: &G::Observation,
        legal: &[G::Action],
        rng: &mut DetRng,
    ) -> G::Action;

    /// Short label for logs and reports, e.g. `"ismcts:7"`.
    fn name(&self) -> String;
}

/// One action's estimated value from an [`Evaluator`].
#[derive(Clone, Debug, PartialEq)]
pub struct ActionValue<A> {
    pub action: A,
    /// Expected value in eval units (big blinds or antes), from the observer's point of view.
    pub q: f64,
    /// Standard error of `q`. Zero for exact methods.
    pub se: f64,
    /// Samples behind the estimate.
    pub visits: u32,
}

/// How much work an evaluator may do.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Budget {
    /// A fixed number of iterations: deterministic, so used in sims, coach, spot, and tests.
    Playouts(u32),
    /// Wall-clock time: depends on CPU speed, so interactive play only (tech/README rule 1).
    Time(Duration),
}

/// Something that can value every legal action (ISMCTS, later external models). Strength
/// settings and the coach are built on top of this.
pub trait Evaluator<G: Game>: Send {
    /// Values for every action in `legal`, sorted best first.
    fn evaluate(
        &mut self,
        game: &G,
        obs: &G::Observation,
        legal: &[G::Action],
        budget: Budget,
        rng: &mut DetRng,
    ) -> Vec<ActionValue<G::Action>>;
}
