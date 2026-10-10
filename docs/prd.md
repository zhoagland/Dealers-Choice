# Dealers Choice Engine — Product Requirements Document

| Field   | Value                         |
|---------|-------------------------------|
| Status  | Approved v0.1 (2026-10-10); M0–M8 implemented, see `ChangeLog.md` |
| Date    | 2026-10-10                    |
| Owner   | Zachary Hoagland              |
| Rules source of truth | `Rules/Poker Bible.typ` |

Requirement IDs (`FR-*`, `NFR-*`) are stable. Tech plans and tests reference them; do not renumber.

---

## 1. Purpose

A Rust engine for the home-game card games documented in the Poker Bible that can:

1. Let a human play any implemented game against bots of selectable strength.
2. Evaluate decisions: given a situation ("spot"), rank the legal actions by expected value (EV).
3. Run large-scale bot-vs-bot simulations and store results for analysis.
4. Grow to a web UI and, eventually, multiple online humans, without rewriting the core.

Each game is hand-written as a **game driver** against a stable API. The engine, bots and
simulator are game-agnostic and only interact with games through that API.

## 2. Goals and non-goals

### Goals (v1 prototype)
- G1. A game-driver API general enough for every game family in the Bible (see §4), proven by
  three structurally different prototype games.
- G2. Two bot algorithms behind one interface: ISMCTS (default) and MCCFR.
- G3. Strength-adjustable bots, plus a coach mode that scores human decisions.
- G4. Spot analysis: "best move in this situation" with opponent ranges.
- G5. A simulation runner with a measured data-generation rate that informs the storage design.
- G6. Code a Rust learner can read, review and extend. Clarity beats cleverness except on
  measured hot paths.

### Non-goals (v1)
- Web UI (the engine I/O is designed for it, see FR-UI-2).
- Networked multiplayer, accounts, auth.
- Real money, rake.
- Solving full-scale multiway games to equilibrium (no algorithm guarantees this; see §8).
- Implementing all ~50 Bible games. v1 ships 3; the API must *admit* the rest.

## 3. Users and modes

| Mode | Actor | Description |
|---|---|---|
| Play | 1 human + 1–8 bots | CLI table. Human picks game (dealer's choice), bots fill seats. |
| Coach | Human | Post-hand review (default) and optional live hints. |
| Spot analysis | Human | Define a partial game state + opponent ranges → ranked actions with EV ± error. |
| Simulate | Human (batch) | Run N hands of game G with bot configs B, persist results. |
| Train | Human (batch) | Run MCCFR training for a game; persist the strategy ("blueprint"). |

## 4. Domain constraints from the Bible

The game catalog requires these mechanics. The API (FR-GD-*) must express all of them, even
though v1 only implements a subset.

| Mechanic | Example games | v1? |
|---|---|---|
| Community board(s), incl. multiple boards | Hold'em, PLO Double Board, Elevator, Iron Cross | single board |
| Face-up / face-down per-player cards | 7 Card Stud, Follow the Queen | yes |
| Draw / discard / pass cards | 5 Card Draw, Triple Draw, Pass the Trash | no |
| Dynamic wild cards | Follow the Queen, Baseball | yes |
| Simultaneous hidden decisions | Hi/Lo/Both declaration, Guts | yes |
| Inverted information (see others, not self) | Indian Poker, Indian Hold'em | no |
| Multiple hand evaluators | High, Low (A-6), 2-7, Numbers (7/27, 21) | high + A-6 low |
| Split pots, side pots | Hi/Lo games, any multiway all-in | yes |
| Multi-hand game state | Guts legs, "if you're out you're out" | no |

House conventions that are engine-level, not per-game:
- **High ranking:** Royal Flush > Five of a Kind > Straight Flush > Four of a Kind > Full House >
  Flush > Straight > Three of a Kind > Two Pair > One Pair > High Card. Note that Royal Flush
  is above Five of a Kind; this differs from standard wild-card rules.
- **Low ranking:** ace-to-six (ace low; straights and flushes count against). Best: 6-4-3-2-A.
- **A "game" is one hand unless its rules say otherwise.** The chooser deals and pays a
  dealer ante ($2.00) once per game. Then the player to the chooser's left picks the next game.
  Length is declared per game (`GameLength`):
  - `SingleHand`: the default (FTQ, most games).
  - `Orbit`: one trip of the button starting with the chooser (Hold'em; the "stun" variants of 3-2-1 Drop
    and Iron Cross).
  - `UntilDone`: repeat hands until a game-defined condition, carrying state between hands (Guts Leg:
    first to 2 legs; 3-2-1 Drop: the pot carries over until won). **Not in v1**: needs API v2 `Carry`.
- **Showdown:** the last aggressor shows first; the others may show or muck. Mucked cards are never
  revealed to other players.
- **"Both" declaration:** the player must win high *and* low outright. Any chop is a loss of both,
  and the pot goes to the other declarers. If no one else declared the opposite side, the whole pot
  goes to the player challenging the "Both" declarer. Edge cases: OQ-3.

## 5. Functional requirements

### 5.1 Engine core (`engine` crate)
- **FR-EC-1** Card, rank, suit, deck primitives. Cards are `Copy`, 1 byte. Decks support
  seeded shuffles and dealing without replacement.
- **FR-EC-2** Hand evaluators: High (5-of-7 and exact-5, with an optional wild-rank set),
  A-6 Low (same). Evaluators return a totally ordered `HandValue` that also encodes kickers.
- **FR-EC-3** Chip accounting in integer cents (`i64`). The pot manager computes main and side
  pots and splits them, including hi/lo halves and odd-chip assignment (rule: odd chip goes to
  high; within a side, to the first seat left of the button. Configurable per game).
- **FR-EC-4** Betting structures: No-Limit, Pot-Limit, Fixed-Limit, and **Spread-Limit with
  a cap** (`max_increment`, `max_raises_per_street`; e.g. FTQ: increments ≤ $2, at most 4
  bets/raises per street, so ≤ $8 per street). Each game selects one. Antes, blinds,
  dealer ante, bring-in are configurable. The dealer ante ($2, paid by the chooser once per game)
  applies to **every** game, including blind games.
- **FR-EC-5** Per-player **observation** (information set view): every consumer (bots, UI, logs
  shown to a player) receives only what that seat may know. Bots must never read the full state.
- **FR-EC-6** Every hand is reproducible from `(engine version, game id + version, seed,
  seat configs, action log)`.

### 5.2 Game driver API (`games` crate implements, `engine` defines)
- **FR-GD-1** A game is a state machine. The driver provides: initial state, the set of seats to
  act (one seat, several seats acting simultaneously, or chance), legal actions, apply action,
  terminal check, payoffs, per-seat observation.
- **FR-GD-2** Chance events (dealing) are explicit nodes so that bots can sample them and replays
  can record them.
- **FR-GD-3** Simultaneous decisions (declarations) are a first-class node type. No seat's choice
  is observable to the others until all have committed.
- **FR-GD-4** **Determinization:** given a seat's observation, sample a full state consistent with
  it, optionally weighted by opponent ranges. Required by ISMCTS and spot analysis.
- **FR-GD-5** Drivers declare metadata: id, version, seat range, betting structure, stake model
  (FR-SS-2), rules doc reference (Bible section).
- **FR-GD-6** Adding a game must not require modifying `engine`, `bots`, or `sim`.

### 5.3 Session, table, bankroll (`engine` crate, persisted via `storage`)
- **FR-SS-1** Player profiles have a persistent bankroll that survives across games, sessions
  and app restarts.
- **FR-SS-2** Stake model is per game:
  - `Bankroll`: players bet directly from their bankroll.
  - `TableStake(amount)`: e.g. Hold'em $20. Before the game the stake moves from bankroll to
    the table; only the stake can be bet; whatever is left returns to the bankroll when the game ends.
    Purpose: normalize stack sizes and prevent all-in bullying.
- **FR-SS-3** Rebuys happen **only after a bust**. In `TableStake` games a busted player can rebuy
  as many times as their bankroll allows. Humans can opt out (sit out the rest of the game).
  Bots have an **infinite bankroll** and rebuy automatically, so their result is tracked as
  net won/lost. Rebuy events are logged.
- **FR-SS-4** Dealer's Choice session flow: chooser selects a game → game runs for its
  `GameLength` (one hand by default; one orbit for Hold'em) → the seat to the chooser's left chooses.
  `TableStake` buy-in and cash-out wrap the whole game, not each hand. Bots that choose pick from a configurable list.
- **FR-SS-5** 2–9 seats.

### 5.4 Bots (`bots` crate)
- **FR-BT-1** One `Policy` interface: `observation → action distribution`. Every bot, including
  the human input adapter, implements it.
- **FR-BT-2** **ISMCTS** (Information Set Monte Carlo Tree Search), the default. Budgeted by playouts or
  wall-clock time. Default play budget: ~0.5 s per decision on one core.
- **FR-BT-3** **MCCFR** (Monte Carlo Counterfactual Regret Minimization). Offline training produces a
  blueprint strategy that is persisted and loaded at play time. v1 scope: Kuhn and Leduc.
  Abstracted heads-up Hold'em is a post-M6 stretch goal (see `docs/tech/bots.md` §3). Multiway MCCFR
  is allowed, but carries no equilibrium guarantee.
- **FR-BT-4** Baseline bots: uniform random, and a rule-based equity bot (Monte Carlo equity vs
  pot odds) for cheap simulations and sanity checks.
- **FR-BT-5** **Strength model.** Given the evaluator's per-action value estimates `Q(a)` in
  big blinds (or base-ante units), the bot samples
  `P(a) = exp(Q(a)/τ) / Σ_b exp(Q(b)/τ)`. Temperature τ → 0 means always best; larger τ means
  weaker. Strength levels map to `(τ, search budget)` presets. τ is in EV units, so a
  given level loses roughly the same EV per decision in every game.
- **FR-BT-6** **Style** settings (aggression bias, bluff frequency bias) are separate from strength.
- **FR-BT-8** *(post-v1)* **External policy adapter.** A `Policy` backed by an out-of-process
  decision model, first target **Jev** (TypeSafe AI's "System One" model: structured state in, typed
  calibrated probabilities out; see https://typesafe.ai/blog/introducing-system-one-models-and-jev).
  Requirements: (a) the request is the seat's serialized `Observation` plus the legal action list, and
  the response is a probability per legal action; (b) the engine samples from those probabilities with its own
  `DetRng`, and responses are cached by `(model id, observation hash)`, so sims and replays stay
  deterministic (NFR-1); (c) concurrency, timeout, and fallback-policy settings; (d) per-run cost
  and latency metrics. Also enables bots written in other languages (JSON over stdin/stdout).
- **FR-BT-7** Bet sizing abstraction for bots: {min-raise, ⅓ pot, ⅔ pot, pot, all-in}, filtered
  by the betting structure. Off-menu human bets are mapped to the nearest abstract size for
  tree purposes and translated back exactly for chip accounting.

### 5.5 Evaluation, spot analysis, coach (`bots` + `sim`)
- **FR-EV-1** `evaluate(observation, budget) → [(action, Q, standard_error)]`, sorted by Q.
- **FR-EV-2** **Spot analysis:** the user specifies a game, the visible state, the action history, the hero's
  cards, and a range for each opponent. Hold'em ranges use standard notation (`TT+, AKs, KQo`, weights
  allowed). Stud/FTQ ranges in v1 are "uniform over unseen cards" with optional card-level
  weights (see OQ-5). Output as in FR-EV-1.
- **FR-EV-3** **Coach, post-hand (default on):** after each hand, list every human decision with
  `EV_loss = Q(best) − Q(chosen) ± SE`. A decision is flagged only when `EV_loss > 2·SE`, so
  search noise isn't reported as a mistake.
- **FR-EV-4** **Coach, live hints (toggle):** before the human acts, show the ranked FR-EV-1 output.
- **FR-EV-5** Coach review uses a larger budget than in-play bots (default 5× the default bot budget) and
  runs off the hand's critical path.

### 5.6 Simulation (`sim` crate)
- **FR-SM-1** Run N hands (or N sessions) of game G with seat configs B, using
  `rayon` across cores. Deterministic per-hand seeds derived from a run seed.
- **FR-SM-2** Report per-seat win rate in bb/100 (or units/100) with 95% confidence interval.
- **FR-SM-3** Instrumentation: hands/s, decisions/s, encoded bytes/hand, per (game, bot config).
- **FR-SM-4** Output tiers: aggregates (always), full hand histories (opt-in or sampled at rate *p*).

### 5.6b Player analytics (M7)
- Scope: **human players only.** Bot hands are never post-processed or stored for analytics, which keeps
  storage small. Bot baselines in FR-PA-3 are computed on the fly when a report is built, not stored.
- **FR-PA-1** Descriptive stats per player, game, and position, computed from stored hand
  records: VPIP, PFR, 3-bet %, aggression factor, c-bet %, fold to c-bet, WTSD, W$SD. Defined via
  `ActionClass` so they apply to any game. Each stat is shown with its sample size and a 95% CI.
- **FR-PA-2** True range charts: because hand records are replayable from their seeds, the
  player's hole cards are known for **every** hand, including folds. Show the actual range per
  preflop/opening spot.
- **FR-PA-3** EV leak report: aggregate coach `EV_loss ± SE` by spot (street × position × action
  class × hand-strength bucket), rank by total EV lost, and compare frequencies with the default bot's in
  the same spots. Only spots where the loss exceeds 2·SE are reported.
- **FR-PA-4** Cross-game style summary (aggression, fold to pressure), labeled as indicative only:
  stats don't transfer cleanly between games with different structures.
- **FR-PA-5** *(post-v1)* Exploitative bots: weight determinization of a player's hidden cards by
  their observed tendencies (opponent model), with capped deviation from the default strategy
  so the bot stays hard to counter-exploit.

### 5.7 Storage (`storage` crate)
- **FR-ST-1** App state (profiles, bankrolls, bot presets, saved sessions) in SQLite.
- **FR-ST-2** Simulation output partitioned by game and run at `data/sim/game=<id>/run=<uuid>/`,
  queryable with DuckDB. v1 format: JSON aggregates + zstd binary records; Parquet only if M3 justifies it. **Final format decision is gated on
  milestone M3 rate measurements** (§9).
- **FR-ST-3** MCCFR blueprints stored as versioned files keyed by `(game id, game version,
  abstraction version)`.
- **FR-ST-4** Storage root is configurable so the same layout works locally and on a server.

### 5.8 Interfaces
- **FR-UI-1** v1: CLI (`cli` crate, `clap`) for play, spot, sim, train.
- **FR-UI-3** Saved profiles + TUI editor. Named **sim profiles** (`config/sims/<name>.toml`, a
  `RunSpec`) and **play profiles** (`config/play/<name>.toml`: bots, think-ms, hints, review budget,
  bankroll, game pool) are TOML files: readable, diffable, can be committed. `dc sim --profile <name>` /
  `dc play --profile <name>` load them; explicit flags override profile fields. `dc config` opens a
  terminal UI (ratatui) to list, create, copy, edit, validate, and delete profiles. Milestone M6.
- **FR-UI-2** All engine I/O (observations, actions, events, evaluations) is `serde`-serializable.
  A later web server (WebSocket) is a transport adapter, not a rewrite.

## 6. Non-functional requirements

- **NFR-1 Determinism.** Same inputs → identical hand, bit for bit, across runs on one platform.
  RNG: `rand_chacha`. No `HashMap` iteration order in game logic.
- **NFR-2 Correctness, checked by tests that can fail:**
  - High evaluator over all C(52,5) = 2,598,960 hands matches the known category counts
    (SF 36, Royal 4, Quads 624, FH 3,744, Flush 5,108, Straight 10,200, Trips 54,912,
    Two Pair 123,552, Pair 1,098,240, High 1,302,540).
  - MCCFR on Kuhn poker converges to P1 game value −1/18 ≈ −0.0556 and exploitability
    < 0.005 chips/hand. The learned P1 strategy satisfies `P(bet | K) ≈ 3·P(bet | J)`.
  - Chip conservation: Σ stacks + pots is invariant across every action in every simulated hand.
- **NFR-3 Performance targets (initial, revised after M3):**
  - 7-card high eval ≥ 10 M evals/s/core.
  - Random-bot Hold'em sim ≥ 50 k hands/s/core.
  - ISMCTS Hold'em ≥ 10 k playouts in 0.5 s on one core.
- **NFR-4 Readability.** Prefer concrete types, `enum` + `match`, and a small number of traits
  with associated types. Avoid macros and deep generic bounds unless they're on a measured hot path, and if
  they are, document why. Each public item has a doc comment. `clippy` clean.
- **NFR-5 Isolation.** A game driver bug cannot corrupt chip accounting: the engine validates
  every action the driver accepts against the betting rules.

## 7. Prototype games

| Game | Why | Acceptance |
|---|---|---|
| **Kuhn poker** | Known closed-form equilibrium → validates MCCFR, ISMCTS, best-response tooling | NFR-2 Kuhn criteria; ISMCTS exploitability measured and reported |
| **No-Limit Texas Hold'em** | Reference game. Community board, NL betting, side pots, ranges | Plays full Bible rules (§Texas Hold'em) with TableStake $20, $0.50/$1 blinds; spot analysis works with ranges |
| **Follow the Queen** | Stud dealing, dynamic wilds, hi/lo/both declaration, split pots | House rules below; declarations are simultaneous; pot splitting matches the Bible |

**Follow the Queen house rules (as decided 2026-10-10):**
- 7-card stud structure: 2 down, 4 up (3rd–6th street), 1 down, with a betting round after each deal.
- Forced bet: $2 dealer ante only. Betting: spread-limit, each bet/raise ≤ $2, at most 4 per
  street (≤ $8 per street). Smallest increment = $0.50 (smallest chip). Assumption A7.
- Wild cards play as any rank for **both** high and low (for low: the best non-pairing rank).
- First to act on each street: best board showing, judged by the visible upcards only, using the current wild
  rank, ranking only pairs/trips/quads/five of a kind (no straights or flushes on partial boards). Ties go to the
  seat closest to the dealer's left. Assumption A8: a visible wild counts as showing.
- When a Queen is dealt face up, the rank of the **next face-up card dealt** becomes wild for the
  whole table. A later face-up Queen replaces it.
- **Queens are not wild**, unless the card following a face-up Queen is itself a Queen; then
  Queens become the wild rank.
- Max **7 seats** (deck capacity).
- If a Queen is the **last face-up card dealt**, there is **no wild card**.
- After final betting: simultaneous High / Low / Both declaration → showdown → split.

## 8. Known technical limits (to keep expectations right)

- **"Best move" assumes something about the opponents.** EV depends on how opponents play. ISMCTS
  assumes opponents play the way its own search does; spot analysis uses the ranges you supply.
  The output is "best move against this model," not universally optimal.
- **Multiway:** Nash equilibrium is neither unique nor protective with ≥3 players. MCCFR in
  multiway games is a heuristic. Expect ISMCTS to weaken as the seat count rises (the branching
  factor grows, and so does the number of determinizations it needs).
- **ISMCTS strategy fusion:** it can rely on information it wouldn't actually have, which makes it
  exploitable. That is why it's measured against Kuhn's exact best response.
- **Estimator noise:** EV estimates have SE ∝ σ/√N. With σ of several big blinds per hand,
  distinguishing actions within ~0.1 bb needs on the order of 10⁴+ samples per action. Coach
  flags are thresholded on SE for this reason (FR-EV-3).

## 9. Milestones

| ID | Deliverable | Exit criteria |
|---|---|---|
| M0 | Docs: this PRD, tech plans per crate, game-driver API contract, "adding a game" guide | Reviewed and approved |
| M1 | `engine` core + Kuhn driver + random bot + MCCFR + best-response | NFR-2 Kuhn passes |
| M2 | High/Low evaluators, pot manager, Hold'em driver, ISMCTS, CLI play | NFR-2 evaluator and chip-conservation tests pass; human can play a Dealer's Choice session spanning a single-hand game and a full Hold'em orbit |
| M3 | `sim` crate + instrumentation | Measured hands/s and bytes/hand per game and bot; storage format decided (FR-ST-2) |
| M4 | Strength presets, coach (post-hand + live), spot analysis with ranges | Coach flags are stable across reruns with different seeds |
| M5 | Follow the Queen driver | Plays house rules; written using only the "adding a game" guide (tests the guide) |
| M6 | Storage + persistence (bankroll, sim output, blueprints); saved profiles + `dc config` TUI (FR-UI-3) | Sessions survive restart; sim output queryable; a profile made in the TUI runs with `--profile` |
| M7 | Player analytics (FR-PA-1…4) | Stats and leak report reproduce known values on synthetic players with planted leaks (e.g. a bot that always calls the river) |
| M8 | NPI → sustainment handoff: CI (fmt, clippy, tests per PR; nightly release-mode exhaustive + benchmark tests), sustainment handbook (architecture map, run/debug/replay, versioning and invalidation rules, release checklist, known limits, prioritized backlog), adding-a-game guide | Owner can add a game, change rules, cut a release, and diagnose a CI failure using only the docs |

## 10. Assumptions (with sensitivity)

| # | Assumption | If false |
|---|---|---|
| A1 | Single machine, single process for v1 | Networked multiplayer needs the server tier earlier; FR-UI-2 limits the rework |
| A2 | Human bets are integer cents, any legal amount | Coarser increments would shrink the bot trees; no API change |
| A3 | Bots that choose games pick uniformly from an allowed list | Smarter choosing is a separate policy; no API change |
| A4 | Odd chip goes to high, then to the seat closest to the button's left | Configurable per game, so cheap to change |
| A5 | One evaluation unit per game (big blind for blind games, ante for ante games) for τ and EV | Strength wouldn't feel the same from game to game |
| A6 | ~0.5 s per bot decision is acceptable during play | Strength presets would need re-tuning |
| A7 | FTQ bet/raise increments are any multiple of $0.50 from $0.50 to $2.00 (confirmed) | — |
| A8 | FTQ "best board": visible wilds count; no partial straights/flushes; ties → closest to dealer's left | Only the action-order function changes |

## 11. Open questions

- ~~OQ-1~~ Resolved: A7 confirmed (spread $0.50–$2.00). First to act on each street = best
  board showing (see §7).
- ~~OQ-2~~ Resolved: wild plays as best possible card for high and low.
- **OQ-3** "Both" declaration edge cases: exact payout when the Both declarer loses one side and nobody
  else declared that side.
- ~~OQ-4~~ Resolved: dealer ante applies to every game.
- **OQ-5** Range notation for stud games (no standard notation exists). Candidates: card-level weights,
  or hand-category weights.
- ~~OQ-6~~ Resolved: FR-SS-3.
- **OQ-7** Coach mode in multiway pots: EV against the ISMCTS opponent model only, or against a
  user-chosen opponent model?

## 12. Glossary

- **Observation / information set:** everything one seat knows at a decision point.
- **Determinization:** sampling a full hidden state consistent with an observation.
- **ISMCTS:** Monte Carlo tree search over information sets, using a fresh determinization per iteration.
- **MCCFR:** sampled counterfactual regret minimization. Converges to Nash in 2-player zero-sum games.
- **Exploitability:** how much a best-responding opponent wins against a strategy. 0 means Nash.
- **Blueprint:** a precomputed (MCCFR) strategy table loaded at play time.
- **bb/100:** big blinds won per 100 hands, the standard win-rate unit.
- **Orbit:** one full trip of the dealer button around the table. The length of an `Orbit` game such as Hold'em.
- **Game:** what the chooser picks. One hand by default; see `GameLength` (§4).
