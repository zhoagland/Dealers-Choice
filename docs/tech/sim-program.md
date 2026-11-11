# Tech Plan — Simulation Program (Phase 2, track S)

| Field | Value |
|---|---|
| Status | Draft v0.2 for owner review (2026-11-10) |
| Implements | PRD FR-SP-1…8, milestones S1–S6 |
| Builds on | `sim.md` (runner, duplicate deals, encoding), `storage.md` (data root layout) |

v1 can run one `RunSpec` at a time and print a summary. Phase 2 turns that into a **program**: queued
**campaigns** that answer four questions the owner picked, run unattended in the background on the dev PC, and
produce an **HTML report** per run and per campaign.

| Question | Campaign type | First games |
|---|---|---|
| How does each Bible game play? Variance, pot sizes, showdowns, position, house-rule what-ifs | **Balance** | Hold'em, FTQ, then each family as it lands |
| Do strength levels mean the same thing in every game? | **Ladder** | Hold'em, FTQ |
| What should *I* do? Starting-hand EV, key decisions | **Study** | Hold'em, FTQ |
| Can bots play from a precomputed strategy? | **Blueprint** | Kuhn/Leduc (exists), Indian Poker, 3-2-1 Drop, HU Hold'em (abstracted) |

---

## 1. Campaigns

A campaign is a TOML file under `config/campaigns/` that expands to many `RunSpec`s.

```toml
name = "balance-v1"
kind = "balance"                 # balance | ladder | study | blueprint
games = ["holdem_nl", "ftq"]     # or "all"
seats = [2, 4, 6]
hands_per_run = 200_000
seed = 2026
[population]                     # which bots sit at the table
mix = ["random", "equity", "ismcts:3", "ismcts:5"]
[sweep]                          # optional: house-rule what-ifs (D-ALL-6 config flags)
"ftq.max_raises_per_street" = [3, 4, 5]
```

`dc campaign plan <file>` prints the expanded runs, an estimated wall-clock time (from the measured rates in
`sim.md` §5 and `dc bench-sim`), and the disk estimate, **before** anything runs.

## 2. Background execution on the dev PC (FR-SP-2)

```
dc campaign run config/campaigns/balance-v1.toml --threads 12 --nice
dc campaign status            # progress, ETA, per-run state
dc campaign pause|resume <name>
```

- **Frozen binary.** `campaign run` copies the current `dc.exe` to `data/bin/dc-<git sha>.exe` and runs that
  copy. Windows locks a running `.exe`, so without this, `cargo build` fails while a sim runs, and results
  would mix code versions. The sha goes into every summary.
- **Chunked and resumable.** A run is split into chunks of 10 k hands (seeds are per hand index, `sim.md` §2, so
  chunks are independent). Each finished chunk writes `chunk-NNNN.json` (merged Welford stats). Resume
  skips finished chunks. A crash, reboot, or `pause` loses at most one chunk per thread.
- **Polite.** `--nice` lowers process priority (Windows `BELOW_NORMAL_PRIORITY_CLASS`); `--threads` defaults to
  cores − 2 so the PC stays usable for development and the web dev server. Optional `--only-idle` waits when the
  user is active (later; not in S1).
- **Queue.** Runs execute in order from `data/campaigns/<name>/queue.json`; status is a file, not a daemon.
  Starting `dc campaign run` in a detached terminal, or as a Windows Scheduled Task for overnight runs, is
  documented in the handbook.
- **Determinism.** Same campaign file + same frozen binary ⇒ identical results (NFR-1). Time budgets stay
  forbidden in sims (`sim.md` §1).

## 3. Reports (FR-SP-3)

`dc report <run-or-campaign>` writes one **self-contained HTML file** (no server, opens from disk, works offline):
`data/reports/<campaign>/index.html` + one page per run.

- Rendering: `minijinja` templates in the `sim` crate (new module `report/`), data embedded as JSON, charts drawn by
  a small vendored charting script (inlined, pinned version). No Node toolchain needed for reports.
- Every number carries its sample size and 95 % CI (FR-SM-2). Differences are only called out when CIs don't overlap.
- Reports are regenerated from stored summaries, so templates can improve without re-running sims.
- The web app (track W) later serves the same files under `/reports` for logged-in users (W6, optional).

Sections by campaign kind:

| Kind | Report contents |
|---|---|
| Balance | Per game: $ won/lost per hand distribution (std dev = "swinginess"), average and P90 pot, showdown %, hands per game (UntilDone), win rate by position/seat, hand category at showdown that wins, hi/lo/both declaration mix and success, effect of each swept config flag. Cross-game table ranking games by swinginess and dealer-ante share |
| Ladder | Rating per (game, bot preset) with CI, head-to-head matrix, EV lost per decision vs the strongest preset (tests FR-BT-5's "same τ means the same loss in every game"), recommended preset retuning |
| Study | Starting-hand EV grid (Hold'em 13×13; FTQ/stud: 3-card start classes; Omaha: hand buckets), position overlays, decision studies (batch spot analysis over a grid of situations, e.g. FTQ "declare Both with X") |
| Blueprint | Training curve (exploitability where exact best response exists, else head-to-head vs ISMCTS), size on disk, version key |

## 4. Campaign designs

### 4.1 Balance (S3)
Population of mixed bots at 2, 4 and 6 seats (9 for games that allow it). Cheap bots (random, equity, baselines)
give large samples; low-budget ISMCTS (preset 3–5) gives realistic behaviour. Report both, since random bots
exaggerate showdown rates. Every family PR adds its games to `balance-v1.toml`.

House-rule what-ifs use `[sweep]` over the `Config` flags each driver exposes (catalog D-ALL-6), e.g. FTQ max raises,
Red River "river+1", Guts Leg `out_is_out`.

### 4.2 Strength ladder (S4)
Heads-up ladders don't fit multiway games, and a full round robin of 10 presets is 45 pairings per game. Instead:
- **Population tables:** 6-seat tables with presets drawn from {1,3,5,7,9} + baselines, duplicate deals, seats
  rotated.
- **Fit:** a linear model `result_seat = β[preset_seat] + noise` across all hands (least squares with CIs),
  giving each preset's value in eval units per hand relative to preset 5. Heads-up matches confirm the top
  two presets.
- **Budget:** ISMCTS is ~4.5 orders of magnitude slower than random (sim.md §5). Ladder runs use the
  playout budgets from `strength.rs`, scaled down, and the report states the scaling. Target: 1 game overnight
  (~8 h on 12 threads).

### 4.3 Strategy study (S5)
- **Starting hands:** for each starting-hand class, the hero's EV against a population, with 100 % hand records
  kept so the class is recoverable (records are tiny, 50–100 B/hand).
- **Decision grids:** `dc spot` in batch: a TOML grid of spots (stack/pot/position/hand) → ranked actions with
  EV ± SE per cell. Uses the coach's budget (FR-EV-5).
- Known bias: ISMCTS opponent models over-fold and under-value checks (handbook §7). Study reports print that
  caveat, and S5 depends on backlog #1 (better rollouts) for credible numbers. **S5 starts after rollout
  improvements land.**

### 4.4 Blueprints (S6)
MCCFR needs small games or abstraction (bots.md §3).
- Tractable now: Kuhn, Leduc (exist), **Indian Poker** (1 card per seat, one betting round), **3-2-1 Drop**
  play/drop decision (3-card hand classes × pot size buckets).
- With abstraction work (backlog #7): **heads-up NL Hold'em** with equity-histogram card buckets and the FR-BT-7
  bet menu. Multi-day CPU training; checkpointed (same chunk/resume model).
- Blueprints are stored per FR-ST-3 and loaded by an MCCFR policy in play and sims.

### 4.5 Study campaigns: `dc analyze` (built ahead of S5, 2026-11-10)

Owner request (2026-11-10): "a script to start a simulation that tests every game for millions of hands … a
dashboard per game describing the best starting hand, the hands that should and shouldn't be played … then board
variants to see hand strength, when to bet, how much to bet." A **hand** = one deal played to showdown or until
everyone else folds. **Every** simulated hand feeds the aggregates; nothing is sampled.

- `dc analyze run [--games a,b] [--hands N] [--bots spec,...] [--seats N] [--chunk N] [--seed N] [--threads N]`
  creates or updates `<data_root>/analysis/<name>/campaign.toml` and runs it. Defaults: every registered game
  except the solver fixtures, 6 seats (or the game's max), `ismcts:3:300` in every seat, all cores but two.
  `pause`, `status`, `report` (rebuild dashboards from chunks).
- Chunks have independent seeds; a killed or paused run resumes by skipping finished `chunk-*.json` files.
  `status.json` is rewritten after every chunk, and each game's `dashboard.html` is regenerated.
- **Storage:** no per-hand records. `sim::study` keeps running means (Welford) per key, so millions of hands cost
  kilobytes per chunk.
- **Starting hands:** result per (starting-hand class, position from the button, first action). Comparing fold
  with call/raise for the same class and position ranks hands as play / don't play.
- **Decisions:** result per (street, board texture, hand-strength bucket, action). Bets are bucketed by size
  relative to the pot (`SizeBucket`: ⅓, ⅔, pot, over, all-in); non-betting choices use the game's label (e.g.
  "declare High"). The dashboard pivots rows by hand strength or by board texture.
- Games opt in through the analysis hooks on `Game` (starting-hand class, decision context, labels); Hold'em and
  FTQ have them. Each new family PR adds them so its games appear in `dc analyze`.
- Caveat printed on every dashboard: these are outcomes of the bot population's own play, not solved values,
  and inherit the ISMCTS biases in §4.3.

### 4.6 Local campaign UI: `dc lab` (requested 2026-11-10, not built)

Owner request: "a local HTML interface for configuring and starting simulation campaigns."
- `dc lab` starts a localhost-only HTTP server (bound to 127.0.0.1, random free port, opens the browser) that
  serves one page: pick games, hands, bots per seat, seats, threads; save as `campaign.toml`; start / pause /
  resume; live progress from `status.json`; links to each `dashboard.html` and `index.html`.
- Runs are spawned as a detached `dc analyze run --name <n>` process, so closing the page or the server never
  kills a campaign. The UI is a thin editor over the same files the CLI uses.
- No auth (loopback only); the server refuses non-loopback `Host` headers.
- Later: same page drives Phase 2 campaign kinds (§1) once `dc campaign` exists.

## 5. Storage

Unchanged layout (`storage.md` §1) plus:
```
data/campaigns/<name>/{campaign.toml, queue.json, status.json}
data/sim/game=<id>/run=<uuid>/{spec.toml, summary.json, chunk-*.json, hands-*.bin.zst}
data/reports/<campaign>/index.html
data/bin/dc-<sha>.exe
```
`summary.json` gains `binary_sha`, `game_version`, `campaign`, `sweep` values, so stale runs (rule changes) are
visible and can be filtered or re-run.

## 6. Milestones

| ID | Deliverable | Exit |
|---|---|---|
| S1 | Campaign files, `plan`/`run`/`status`/`pause`/`resume`, frozen binary, chunked resume | Killing the process mid-run and resuming gives byte-identical summaries |
| S2 | HTML reports (`dc report`) for runs and campaigns | Balance report for Hold'em + FTQ opens offline; every stat shows n and CI |
| S3 | Balance campaign, extended per game family | Report covers every implemented game |
| S4 | Strength ladder | Preset ratings with CIs for Hold'em and FTQ; retuning recommendation |
| S5 | Strategy study (after rollout improvement) | Starting-hand grids for Hold'em and FTQ, a batch spot grid |
| S6 | Blueprints for Indian Poker and 3-2-1 Drop; HU Hold'em abstraction | Indian Poker blueprint beats ISMCTS:7 with CI; HUNL training checkpoints |

S1–S3 start right after the stack merge (M9) using Hold'em and FTQ, and grow as game families land.
