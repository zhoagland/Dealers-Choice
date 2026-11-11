# Tech Plan — `bots`

Implements FR-BT-1…7, FR-EV-1…5. Generic over `G: Game`, with no dependency on `games`.

```
bots/src/
├── policy.rs       Policy, Evaluator traits; ActionValue
├── random.rs       uniform random (baseline, conformance)
├── equity.rs       Monte Carlo equity vs pot odds (cheap baseline)
├── ismcts/
│   ├── mod.rs      MO-ISMCTS search
│   ├── table.rs    per-seat information-set tables
│   └── rollout.rs  rollout policies
├── mccfr/
│   ├── mod.rs      external-sampling MCCFR trainer
│   ├── blueprint.rs average-strategy tables, load/save
│   └── best_response.rs exact BR + exploitability (small games only)
├── strength.rs     softmax temperature, style bias, presets
├── ranges.rs       Hold'em range parser, RangeSampler: HiddenSampler
├── spot.rs         SpotSpec → Observation → evaluate
└── coach.rs        post-hand review, live hints
```

## 1. Interfaces

```rust
pub struct ActionValue<A> { pub action: A, pub q: f64, pub se: f64, pub visits: u32 }

pub trait Evaluator<G: Game>: Send {
    /// Values in eval units (bb or ante), from the observer's perspective. Sorted by q descending.
    fn evaluate(&mut self, game: &G, obs: &G::Observation, legal: &[G::Action],
                budget: Budget, rng: &mut DetRng) -> Vec<ActionValue<G::Action>>;
}

pub trait Policy<G: Game>: Send {
    fn act(&mut self, game: &G, obs: &G::Observation, legal: &[G::Action], rng: &mut DetRng) -> G::Action;
}

pub enum Budget { Playouts(u32), Time(Duration) }   // Time is non-deterministic: interactive play only
```
`legal` is passed in by the harness, since the set of legal actions is public information. A bot never
sees `State`. It can only call `game.determinize(obs, …)`, which produces a *sampled* state.

`StrengthPolicy<E: Evaluator>` turns any evaluator into a policy (§4). The human input adapter is
also a `Policy`, so the table loop doesn't care who is sitting where.

## 2. MO-ISMCTS (default bot, FR-BT-2)

**Variant choice.** Single-observer ISMCTS (SO-ISMCTS) shares opponent nodes across
determinizations, so in effect opponents choose without looking at their own cards. In poker
that's a very bad opponent model (they never fold weak hands or bet strong ones). **Multiple-observer ISMCTS**
(Cowling, Powley & Whitehouse, 2012) keeps one tree per seat, keyed by *that seat's*
information set, so each simulated opponent decides from its own sampled cards.

**Table-based implementation (no pointer trees).**
```rust
struct Tables<G: Game> { per_seat: Vec<HashMap<G::Observation, Node<G::Action>>> }
struct Node<A> { actions: SmallVec<[(A, Stats); 8]> }
struct Stats { visits: u32, avail: u32, reward_sum: f64, reward_sq: f64 }
```
Keying on `Observation` (which is `Hash + Eq` by contract) avoids `Rc<RefCell<…>>` trees, the
part of Rust that's hardest for a learner, and makes "information set" literally the map key.

**One iteration:**
1. `s = game.determinize(root_obs, sampler, rng)`
2. Walk: at each `Seat(p)` node, `k = observe(s, p)`. If `k` is in `per_seat[p]`, select by
   **UCB1 with availability counts** over the actions legal in `s`. Otherwise expand: insert `k` and switch to rollout.
   At Chance, sample from `chance_outcomes`.
3. Rollout to terminal with the rollout policy (§2.1).
4. Backpropagate `payoffs / eval_unit` to every `(p, k, a)` visited, each with seat *p*'s own reward.

UCB1 assumes rewards in [0, 1]; poker rewards are unbounded in bb. Normalize the exploration
constant by a running reward standard deviation: `c = c₀ · σ̂`. Otherwise `c` would need re-tuning
per game and per stack depth.

### 2.1 Rollout policies
- `CheckCallDown` (default): `classify` → choose `CheckOrCall` if present, else first legal;
  `Declare` → uniform. Cheap, and measures showdown equity. **Known bias:** it ignores fold equity
  below the tree. Mitigated because the tree covers the near-term decisions, which are the ones that matter.
- `Random`: for conformance/sanity only. Random folding makes values meaningless.

### 2.2 Play mode vs evaluation mode
UCB deliberately starves bad actions, so a bad action ends up with few visits and a huge SE. That's
fine for *playing* and useless for *coaching*, where the human's (possibly bad) action needs a tight estimate.
- **Play mode:** UCB at the root as well.
- **Evaluation mode** (coach, spot, strength sampling): the root allocates round-robin until every
  action has ≥ `n_min` visits, then UCB. `q = reward_sum / visits`, `se = sqrt(var / visits)`.

### 2.3 Known limitations (stated, measured)
- **Strategy fusion / non-locality:** ISMCTS doesn't converge to equilibrium. M1 measures its Kuhn
  exploitability against the exact best response (it'll be > 0; the number is the baseline).
- **Determinization bias:** opponents' hidden cards are sampled from ranges or uniformly, *not*
  conditioned on their past actions (a player who raised preflop is more likely to hold strong
  cards). Spot mode addresses this with user ranges. v2 option: weight determinizations by
  the likelihood of the opponent's actions under the bot's own policy (Bayesian inference over ranges).
- **Multiway:** each seat added multiplies the information sets per tree level, so the budget
  needed per decision grows quickly. Expect weaker play at 9 seats than heads-up at the same 0.5 s.

### 2.4 Budget
Default play: `Time(500 ms)`. NFR-3 target ≥ 10⁴ playouts in that time for Hold'em. Cost per
playout ≈ determinize + ~10–30 `apply` calls + `observe`/hash at tree nodes + one evaluation.
`observe` allocating is the likely bottleneck. Measure first.

## 3. MCCFR (FR-BT-3)

- **External-sampling MCCFR** (Lanctot et al., 2009). Traverser enumerates own actions; opponents and
  chance are sampled. Regret matching; optionally regret matching⁺ with linear averaging (faster
  convergence in practice).
- Tables: `HashMap<G::Observation, InfoSetData { regret: SmallVec<f64>, strat_sum: SmallVec<f64> }>`, the same keying as ISMCTS.
- **Output:** the average strategy is the blueprint (`blueprint.rs`), serialized with `meta.id`,
  `meta.version`, and the bet-menu hash. It refuses to load on mismatch (API doc §8).
- **Best response** (`best_response.rs`): exact, by enumerating the whole tree with chance
  probabilities. Feasible only for Kuhn and Leduc. `exploitability = (BR₀ + BR₁)/2`.
- **Validation (NFR-2):** Kuhn exploitability < 0.005 after training; P1 value → −1/18.
- **Hold'em scope:** full HUNL needs card abstraction (equity buckets) plus action abstraction.
  Without them it is intractable: Johanson (2013) counts ~10¹⁶¹ information sets for 200 bb HUNL; 20 bb stacks shrink that by many orders of magnitude, but it's still far beyond tabular. **This is a stretch goal after M6.** It
  needs an `Abstraction` trait mapping `Observation → bucket key`, which the table key would use instead
  of the raw observation. Planned, not v1.
- **Off-menu actions** (a human bets 0.8 pot when the menu has ⅔ and 1): map to the menu
  with pseudo-harmonic mapping (Ganzfried & Sandholm, 2013). Only needed for MCCFR bots;
  ISMCTS searches from the true state.

## 4. Strength and style (FR-BT-5, FR-BT-6)

```rust
pub struct Strength { pub tau: f64, pub budget: Budget, pub n_min: u32 }
pub struct Style    { pub aggression: f64, pub bluff: f64 }   // biases in eval units
```
Sampling: `P(a) ∝ exp((Q(a) + bias(a)) / τ)`, where `bias(a) = aggression` for `BetOrRaise`,
plus `bluff` when the bot's sampled equity is below a threshold. τ = 0 ⇒ argmax with deterministic
tie-breaking (lowest `Ord` action).

**Why τ is in eval units:** a τ of 0.5 bb means "an action 0.5 bb worse is 1/e as likely as the
best." That's game-independent and interpretable, and the expected EV loss per decision grows
monotonically with τ. Presets (initial, tuned in M4 by measuring the EV lost per decision):

| Level | τ (bb) | Playouts (sim) | Time (play) |
|---|---|---|---|
| 1 | 8.0 | 500 | 50 ms |
| 3 | 2.0 | 1 000 | 100 ms |
| 5 | 0.5 | 3 000 | 250 ms |
| 7 | 0.15 | 10 000 | 500 ms |
| 10 | 0 | 30 000 | 1 500 ms |

The τ budget also absorbs search noise: Q estimates have SE > 0, so even τ = 0 is noisy. Smaller budgets
make the bot weaker in a second, natural way.

MCCFR bots: there's no Q from the blueprint, so strength = power transform `π_T(a) ∝ π(a)^{1/T}`
(T > 1 flattens toward uniform). Different units than τ; presets are mapped separately.

## 5. Ranges and spot analysis (FR-EV-2)

- `ranges.rs`: parse `"TT+, AKs, KQo:0.5, A5s-A2s"` → weights over the 1,326 Hold'em combos.
  `RangeSampler` implements `HiddenSampler`. It samples a combo by weight, and rejects and
  resamples if it collides with known cards or with cards already given to another seat. Rejection is fine at 2–4
  opponents. With very narrow multiway ranges, the acceptance rate drops; fall back to enumerating the
  remaining combos.
- Stud/FTQ (v1): uniform over unseen cards (PRD OQ-5).
- `spot.rs`:
```toml
game = "holdem_nl"
seats = 3
button = 2
stacks = [2000, 2000, 2000]
hero = 0
hero_cards = "AhKh"
board = "Qh7h2c"
actions = ["s1 raise_to 300", "s2 call", "s0 call", "flop", "s1 check", "s2 bet_to 600"]
[ranges]
s1 = "22+, A2s+, KTs+, ATo+"
s2 = "TT-55, AJs-A9s, KQo"
budget_playouts = 200000
```
Built into a hero `Observation` by replaying the actions with placeholder opponent cards, then
`observe(hero)`. The placeholders don't matter because `determinize` resamples them from the ranges.
Output: the FR-EV-1 table.

## 6. External policies (FR-BT-8, post-v1)

```rust
pub struct ExternalPolicy<T: Transport> { transport: T, cache: ResponseCache, fallback: Box<dyn …>, … }
pub trait Transport { fn decide(&mut self, req: &DecisionRequest) -> Result<Vec<f64>, TransportError>; }
// impls: HttpJev (vendor API), StdioJsonLines (any local program), Mock (tests)
```
- `DecisionRequest = { model, game_id, game_version, observation (serde JSON), legal: [action JSON] }`.
  The legal moves form a closed set, which matches Jev's "typed outcomes defined up front" model.
- **Determinism:** the response is a distribution. Sample it with the seat's `DetRng` stream. The cache key is
  `(model, blake3(observation JSON))`. A sim re-run with a warm cache makes zero network calls, giving
  identical results and $0.
- **Throughput:** this is latency-bound, not CPU-bound. Running hands as `rayon` tasks blocked on I/O wastes
  threads, so the external runner uses an async pool with N concurrent in-flight hands (N set
  per run, to respect rate limits). This is a separate sim mode; the CPU path stays sync.
- **Failure:** on timeout or error → `fallback` policy (default: ISMCTS preset 3). Logged per decision
  so results are never silently contaminated.
- **Evaluation:** an external policy has no Q values, so strength and coach don't apply to it. It's
  measured like any bot: bb/100 vs ISMCTS presets with duplicate deals, plus Kuhn exploitability
  (exact, cheap: ~12 infosets).

## 7. Coach (FR-EV-3…5)

- **Post-hand:** when a hand ends, the hand record (`engine::replay` format) goes to a background worker
  (`std::thread` + `mpsc`). For each human decision: replay to that point, `observe(human)`,
  evaluate in evaluation mode with `5 × play budget` as **playouts** (deterministic and repeatable),
  then report `EV_loss = q_best − q_chosen`, `se = sqrt(se_best² + se_chosen²)`. Flag if
  `EV_loss > 2·se`. The result is a table printed after the hand (or when ready, if the next hand has started).
- **Live hints:** the same evaluation, run before prompting the human. Toggle `:hints on|off` in the CLI.
- **Opponent model caveat (OQ-7):** the evaluation assumes opponents play like MO-ISMCTS does. The
  coach says "against this bot's model," not "GTO."
