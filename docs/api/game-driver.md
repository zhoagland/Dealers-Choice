# Game Driver API Contract

| Field | Value |
|---|---|
| Status | Draft v0.1 |
| Implements | PRD FR-GD-1 … FR-GD-6, FR-EC-5, FR-EC-6, NFR-1, NFR-5 |
| Defined in | `crates/engine/src/game.rs` |
| Implemented by | `crates/games/src/<game>/` |

This is the only interface between a game's rules and everything else (bots, simulator, CLI, future
web server). Breaking changes require bumping `API_VERSION` and updating every driver.

Prior art: the model is close to DeepMind's **OpenSpiel** (`State`, chance player,
`information_state`). Reading its docs alongside this one is useful.

---

## 1. Model

A hand is a finite, discrete state machine:

```
           ┌──────────── chance (deal a card) ───────────┐
           ▼                                             │
 new_hand ─► State ── to_act() ──► Seat(s) ── apply(a) ──► State' ── … ──► Terminal ─► payoffs()
                                   Chance  ── apply(a) ──┘
```

Rules of the model:

1. **One actor per step.** `to_act` returns either a single seat, `Chance`, or `Terminal`.
   There is no "several seats at once" node.
2. **Simultaneous moves are hidden sequential moves.** A declaration round is played seat by seat,
   but each committed declaration is **absent from other seats' observations** until the reveal.
   Information-wise this is identical to a simultaneous move (the standard reduction: a
   simultaneous game is an extensive-form game with imperfect information), and it keeps
   every algorithm single-actor. FR-GD-3 is satisfied by the observation contract (§4), not by
   a special node type.
   *Consequence:* UIs must collect simultaneous inputs without exposing order or timing. The
   engine supplies `Observation::pending_simultaneous` for this.
3. **Chance is a player.** Dealing is done one card per chance step. `chance_outcomes` lists the
   possible results with their probabilities. Burns are omitted: burning a card unseen doesn't change
   the distribution of the cards dealt, so leaving burns out is equivalent and saves steps.
4. **Payoffs are net chips per seat for this hand**, in cents, and sum to zero (NFR-2 chip
   conservation). The dealer ante is paid by a seat, so the game stays zero-sum.

## 2. Core types (engine-owned)

```rust
pub const API_VERSION: u32 = 1;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Seat(pub u8);                  // 0..num_seats, seat 0 = left of dealer

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Chips(pub i64);                // cents; negative only in payoffs

pub enum ToAct {
    Seat(Seat),
    Chance,
    Terminal,
}

pub struct HandSetup {
    pub num_seats: u8,
    pub button: Seat,                     // dealer for this hand
    pub chooser: Seat,                    // who picked the game (pays dealer ante on hand 1)
    pub first_hand_of_game: bool,
    pub stacks: Vec<Chips>,               // chips available to bet, per seat
}

pub struct GameMeta {
    pub id: &'static str,                 // "holdem_nl", "ftq", "kuhn"
    pub version: u32,                     // bump on any rules change → invalidates blueprints
    pub display_name: &'static str,
    pub seats: RangeInclusive<u8>,
    pub stake_model: StakeModel,          // Bankroll | TableStake(Chips)
    pub length: GameLength,               // SingleHand (default) | Orbit; UntilDone needs API v2
    pub rules_ref: &'static str,          // "Poker Bible §Follow the Queen"
}
```

## 3. The trait

```rust
pub trait Game: Send + Sync + 'static {
    /// Everything about the hand, including hidden cards. Never given to a bot.
    type State: Clone + Debug + Send + Sync;
    /// Player actions *and* chance outcomes. Small and Copy.
    type Action: Copy + Debug + Eq + Hash + Ord + Serialize + DeserializeOwned + Send + Sync;
    /// One seat's view. Equality defines an information set (§4).
    type Observation: Clone + Debug + Eq + Hash + Serialize + Send + Sync;
    /// Per-game tunables (blinds, structure, bot bet menu). Loaded from config files.
    type Config: Clone + Debug + Default + Serialize + DeserializeOwned + Send + Sync;

    fn new(config: Self::Config) -> Self where Self: Sized;
    fn meta(&self) -> &GameMeta;

    fn new_hand(&self, setup: &HandSetup) -> Self::State;
    fn to_act(&self, s: &Self::State) -> ToAct;

    /// Seat to act: the *abstract* action menu bots search over (FR-BT-7).
    /// Chance: every outcome (pair with `chance_outcomes`).
    fn legal_actions(&self, s: &Self::State, out: &mut Vec<Self::Action>);

    /// Full legality check, including off-menu bets (any legal amount from a human).
    /// Also accepts/rejects chance actions, so `engine::replay` can verify a whole log.
    fn validate(&self, s: &Self::State, a: Self::Action) -> Result<(), IllegalAction>;

    /// Apply a validated action. Must panic on illegal input in debug builds.
    fn apply(&self, s: &mut Self::State, a: Self::Action);

    /// Only valid when to_act == Chance. Probabilities sum to 1 (±1e-9).
    fn chance_outcomes(&self, s: &Self::State, out: &mut Vec<(Self::Action, f64)>);

    /// Draw one chance outcome. Default: enumerate `chance_outcomes` and sample.
    /// Override for speed (same distribution required).
    fn sample_chance(&self, s: &Self::State, rng: &mut DetRng) -> Self::Action { … }

    /// Only valid when to_act == Terminal.
    fn payoffs(&self, s: &Self::State) -> Vec<Chips>;

    fn observe(&self, s: &Self::State, seat: Seat) -> Self::Observation;

    /// Whose observation this is (bots need to know their own seat).
    fn observer(&self, obs: &Self::Observation) -> Seat;

    /// Build a full State consistent with `obs`, sampling everything `seat` cannot see.
    fn determinize(
        &self,
        obs: &Self::Observation,
        sampler: &mut dyn HiddenSampler,
        rng: &mut DetRng,
    ) -> Self::State;

    /// Game-agnostic meaning of an action, for rollouts, style biases and stats.
    fn classify(&self, s: &Self::State, a: Self::Action) -> ActionClass;

    /// Value unit for EV display and bot temperature (PRD A5): big blind or ante.
    fn eval_unit(&self) -> Chips;
}

pub enum ActionClass {
    Fold,
    CheckOrCall,
    BetOrRaise { to: Chips },
    Declare,          // hi/lo/both and similar hidden commitments
    CardChoice,       // discard, pass, keep (future draw games)
    Chance,
}
```

### Why these choices

| Decision | Alternative | Reason |
|---|---|---|
| Associated types + generics (`fn run<G: Game>`) | `dyn Game` with boxed actions | Static dispatch: hot loops (MCTS playouts) inline `apply`. The cost is that the trait isn't object-safe, so runtime game selection is a `match` in `games::registry` (§7). |
| `out: &mut Vec<_>` parameters | Return `Vec` | Lets callers reuse buffers. ISMCTS calls `legal_actions` millions of times; this avoids an allocation per call. |
| `apply` mutates in place | Return new state | Same reason. Callers `clone()` when they need to branch. |
| Chance outcomes are `Action`s | Separate `ChanceOutcome` type | One action log type, so replay = fold `apply` over the log. |
| Abstract menu in `legal_actions`, full rules in `validate` | Enumerate every legal bet | NL has ~10³–10⁴ legal sizes. Bots search a menu; humans may act off-menu. ISMCTS rebuilds its tree from the current state at each decision, so off-menu history needs no translation. Only MCCFR blueprints need action translation (see `docs/tech/bots.md`). |

## 4. Observation contract (information hiding)

`observe(s, seat)` must contain **exactly** what `seat` legitimately knows:

- The seat's own private cards (or, in Indian games, everyone's *except* its own).
- All public cards, all public actions, stacks, pots.
- Its own hidden commitments (e.g. its declaration). Other seats' hidden commitments appear only
  as "committed: yes/no" until revealed.
- Mucked cards: never visible to others.

Two states whose observations for `seat` are equal **must** be indistinguishable to that seat.
Bots key their trees and regret tables on `Observation` (via `Hash + Eq`), so:

- **Leak bug** (the observation contains something hidden): bots cheat. The tests won't catch it
  unless you write the leak test below.
- **Under-specification bug** (something known is missing, e.g. bet order): different
  information sets merge, and the bots play badly in subtle ways.

Required tests, generic and provided by `engine::testing` (§6):
- **No leaks (C4):** across many determinizations from seat *q*'s view, every other seat *r*'s
  observation must change at least once. If it never does, *r*'s private cards are effectively fixed
  by *q*'s observation, i.e. leaked (or `determinize` ignores the sampler). It's a statistical
  check that catches gross leaks; subtle ones (e.g. leaking one bit of a card) still need
  game-specific tests. Also: legal actions and `to_act` must be identical across determinizations,
  because legality that depends on hidden cards is itself a leak.
- **Perfect recall (C5):** along one hand, a seat's observations at its own decision points
  are all distinct. Necessary, not sufficient; the stronger property ("the history can be recovered
  from the observation") is enforced by the convention that observations carry the full public
  history plus the seat's own actions.

Observation should be compact (it's hashed constantly): card sets as `u64` bitmasks, action
history as a `SmallVec` of `Action`.

## 5. Determinization contract

```rust
/// Supplies hidden cards for one seat's view of the world.
pub trait HiddenSampler {
    /// Sample the hidden private cards for `seat` from `available` (not yet seen by the observer).
    /// `n` cards. Implementations: Uniform, Range-weighted (FR-EV-2).
    fn sample_private(&mut self, seat: Seat, n: usize, available: CardSet, rng: &mut DetRng)
        -> Option<SmallVec<[Card; 7]>>;   // None → caller retries (rejection sampling)
}
```

`determinize(obs, sampler, rng)` must:

1. Assign every card the observer can't see: opponents' private cards via `sampler`, then the
   undealt deck as a uniformly shuffled remainder. Card removal: no card is used twice.
2. Fill in hidden past actions (other seats' uncommitted-to-us declarations) by sampling
   uniformly from what was legal at the time. *Known bias:* real opponents don't declare
   uniformly; v2 may sample from an opponent policy.
3. Return a State whose `observe(_, observer)` **equals** `obs`. Tested: `determinize_roundtrip`.

Recommended implementation: rebuild the state with `engine::replay`. Sample the hidden cards,
construct the deal order, then apply the public action log to a fresh `new_hand`. This reuses
`apply`, so determinized states can't drift from real ones.

## 6. Driver obligations (checked by `engine::testing::conformance::<G>()`)

| # | Invariant | Test |
|---|---|---|
| C1 | `to_act` is Chance ⇒ outcomes are non-empty and probabilities sum to 1 | random walks, 10⁴ hands |
| C2 | Every action from `legal_actions` passes `validate` | random walks |
| C3 | Terminal ⇒ Σ payoffs == 0, and each seat's loss ≤ its starting stack | random walks |
| C4 | No leaks (§4) | determinization variability |
| C5 | Perfect recall (§4), necessary condition | random walks |
| C6 | Determinize round-trip (§5) | random walks |
| C7 | Replay determinism: same setup + log ⇒ identical State and payoffs | random walks |
| C8 | Termination: every hand ends in ≤ `max_steps` (game-declared) | random walks |
| C9 | Betting delegated to `engine::betting` (NFR-5): no chip arithmetic outside it | code review |

A driver is "done" when `conformance::<G>()` passes plus game-specific rule tests.

## 7. Registration

```rust
// crates/games/src/registry.rs — the only file to touch when adding a game
pub fn with_game<R>(id: &str, cfg: &toml::Value, f: impl GameVisitor<R>) -> Result<R, UnknownGame> {
    match id {
        "kuhn"       => f.visit(Kuhn::new(cfg.clone().try_into()?)),
        "holdem_nl"  => f.visit(Holdem::new(cfg.clone().try_into()?)),
        "ftq"        => f.visit(FollowTheQueen::new(cfg.clone().try_into()?)),
        _ => Err(UnknownGame(id.into())),
    }
}
pub trait GameVisitor<R> { fn visit<G: Game>(self, game: G) -> R; }
```

The visitor pattern turns a runtime string into a compile-time type `G` once, at the CLI or
server boundary. Everything downstream is generic and monomorphized.

## 8. Serialization and versioning

- Hand record = `(API_VERSION, meta.id, meta.version, HandSetup, seed, Vec<Action>)`. That's
  sufficient for exact replay (FR-EC-6, C7).
- Changing rules ⇒ bump `meta.version`. Old records are still replayable only by the old version
  (keep the git tag). Blueprints are keyed by version and refuse to load on mismatch.

## 9. Things deliberately not in the API

- **Session logic.** The engine's `Session` drives hands according to `meta.length` and calls
  `new_hand` with the right `HandSetup`. `SingleHand` and `Orbit` games are independent hands.
  `UntilDone` games (Guts Leg legs, 3-2-1 Drop carry-over pot) need state passed between hands:
  a `type Carry` produced at terminal and fed into the next `new_hand`, plus `fn game_over(&Carry) -> bool`.
  Deferred until the first such driver (YAGNI). That would be `API_VERSION` 2.
- **Rendering.** *Changed during implementation:* the text hooks `describe_observation`,
  `describe_action`, `parse_action` are default methods on `Game` (defaults: `Debug`, none).
  A separate `Render<G>` trait in the CLI would need its own per-game registry, because
  `games::registry` only provides `G: Game`. A web UI should render from the serialized
  `Observation` instead.
- **Bot hints.** No `heuristic_value` hook. Bots must work from the API alone (FR-GD-6).
  `classify` is the only semantic hint. It's enough for a "check/call down" rollout policy
  (pick a `CheckOrCall` action, otherwise the first legal action) and for aggression biases.
