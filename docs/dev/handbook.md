# Sustainment Handbook

The handoff document for owning Dealers Choice after the NPI build (PRD M8). It assumes you've
skimmed `docs/prd.md` and `docs/api/game-driver.md`. Everything here should let you add a game,
change rules, cut a release, and diagnose a CI failure without outside help. If something isn't
covered, that's a bug in this document: fix it here.

| Doc | Read when |
|---|---|
| `docs/prd.md` | What and why: requirements (FR/NFR IDs), milestones, open questions |
| `docs/api/game-driver.md` | The `Game` trait contract and invariants C1–C9 |
| `docs/dev/adding-a-game.md` | Writing a new game driver, step by step |
| `docs/tech/*.md` | Per-crate design and the measured numbers |
| this file | Running, debugging, versioning, releasing, CI, known limits, backlog |

---

## 1. Architecture map

```
                engine ─────────────── cards, eval, betting, pots, Game trait, replay,
              ↗   ↑   ↖                session/table, RNG, conformance harness
         games  bots   │
       (rules) (AI)    │
              ↖   ↑    │
               sim ────┘ ───────────── parallel runner, stats, encoding, analytics
                ↑
             storage ───────────────── SQLite, data dir, profiles
                ↑
               cli (dc) ────────────── everything user-facing
```

`bots` never depends on `games` (bots are game-agnostic, FR-GD-6); it only uses them in tests.

| I want to change… | Look in |
|---|---|
| How a game plays | `crates/games/src/<game>.rs` (+ rule tests there) |
| Chip rules: min raise, all-ins, caps | `crates/engine/src/betting.rs` |
| Side pots, splits, declarations, dead money | `crates/engine/src/pot.rs` |
| Hand rankings | `crates/engine/src/eval/` (oracle-checked) |
| Orbit / buy-in / rebuy flow | `crates/engine/src/session.rs` |
| Bot search | `crates/bots/src/ismcts/`; strength presets in `strength.rs` |
| Solver | `crates/bots/src/mccfr/` |
| Coach scoring | `crates/bots/src/coach.rs` |
| Simulation | `crates/sim/src/runner.rs` |
| Your HUD stats | `crates/sim/src/analytics.rs` |
| DB schema | `crates/storage/migrations/*.sql` (+ `db.rs`) |
| A command or flag | `crates/cli/src/<command>.rs` |

## 2. Build, run, test

```powershell
cargo build --release                      # binary: target/release/dc(.exe)
cargo test --workspace                     # ~20 s; what CI runs on every PR
cargo test --workspace --release -- --ignored --nocapture   # slow: exhaustive eval, benchmarks
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

Everyday commands:

```text
dc games                                   list games
dc player add NAME --deposit 100           players and bankrolls (dc player list/deposit/reset)
dc play --profile evening                  play; flags override the profile
dc config                                  TUI editor for sim/play profiles (config/)
dc sim --profile quick-holdem --save       simulate; --save writes data/sim/...
dc spot config/spots/example.toml          rank actions in a described Hold'em spot
dc stats --player NAME                     your stats, range chart, leaks
dc train --game kuhn --iters 200000 --save     MCCFR blueprint (test games)
dc exploit --game kuhn --bot ismcts:7      exploitability of a bot
dc bench-sim                               throughput / data-rate table
```

Locations: `--data-root` / `$DC_DATA_ROOT` / `./data` (DB, sim runs, blueprints, gitignored);
`--config-root` / `$DC_CONFIG_ROOT` / `./config` (profiles, spots; committed).

**Machine quirks (this dev machine):**
- Corporate TLS proxy: cargo downloads fail on certificate revocation checks. Set
  `$env:CARGO_HTTP_CHECK_REVOKE='false'` for the command (or `[http] check-revoke = false` in
  `~/.cargo/config.toml`, which disables the check globally; your call).
- Run cargo from PowerShell. Under Git Bash, builds died silently with exit 127 partway through.
- Builds occasionally get killed (exit 0xE0000027) while downloading or compiling; just rerun.
  Progress is kept.

## 3. Debugging and replay

Every hand is reproducible from **(game id + version, setup, action log)**, chance included
(FR-EC-6). Practical recipes:

- **A conformance test fails**: the message names the check (C1–C8), the seat count, hand number,
  and step. `docs/dev/adding-a-game.md` §3 has a table of what each check usually means. Re-run
  with `cargo test -p games <name> -- --nocapture`.
- **A sim result looks wrong**: rerun with the same `--seed`; results are bit-identical for any
  `--threads`. Keep records with `--history-sample 1 --records hands.bin` (or `--save`), then
  decode in a test with `sim::encoding::decode_all` and `engine::replay::replay`.
- **A played hand looks wrong**: `dc play` prints its session seed; the same seed and the same
  inputs replay it. Stored hands are in the `hand` table as replayable records.
- **Chip accounting**: chips are integer cents, and all movement goes through `betting`/`pot`.
  The CLI cross-checks the ledger against the table after every game and stops on a mismatch.
- **A bot does something odd**: `dc spot` for Hold'em, or an `Evaluator::evaluate` call in a test,
  shows Q ± SE for every action. Check whether the gap is bigger than the SE before assuming a bug.

## 4. Versioning and what invalidates what

| You change… | Bump | Consequence |
|---|---|---|
| The `Game` trait or its contract | `engine::API_VERSION` | Every driver must be updated; stored records carry the API version |
| A game's rules, `Action`, or `Observation` shape | that game's `meta.version` | Old hand records and blueprints for that game refuse to load or are skipped by `dc stats`. That's intended: they describe a different game |
| A hand evaluator's ranking | the version of every game using it | Same as above |
| `rand` / `rand_chacha` version | nothing automatically | The golden RNG test fails if the random stream changed. If so, saved seeds deal different cards: treat it like a rules change |
| DB schema | add `migrations/000N_*.sql` (never edit an applied one) | `PRAGMA user_version` applies it once on next open |
| `PlayProfile` fields | — | Unknown keys are rejected (`deny_unknown_fields`) and missing keys take defaults, so renaming a field breaks old profiles loudly. Prefer adding fields |
| `RunSpec` fields | — | Missing optional fields take defaults; renaming breaks old sim profiles |
| `HandRecord` (postcard) layout | `API_VERSION` | Postcard isn't self-describing: old records won't decode |

## 5. Release checklist

1. `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo test --workspace` all clean (CI green on both OSes).
2. Nightly job green, or run the `--release -- --ignored` tests locally.
3. Bumped versions per §4 if rules, records, or the API changed; note it in `ChangeLog.md`.
4. `dc bench-sim`: compare with `docs/tech/sim.md` §5; investigate drops > ~20%.
5. Smoke test: `dc play --no-save --max-games 1`, `dc sim --profile quick-holdem`,
   `dc stats --player <you>`.
6. Tag: `git tag vX.Y.Z && git push --tags`.

## 6. CI

`.github/workflows/ci.yml`:

| Job | When | Guards | If it fails |
|---|---|---|---|
| `fmt` | every push/PR | formatting | run `cargo fmt --all`, commit |
| `clippy` | every push/PR | lints as errors | fix the lint; avoid `#[allow]` unless the lint is wrong, and say why in a comment |
| `test` (Linux + Windows) | every push/PR | all unit, conformance, and statistical tests | see below |
| `nightly` | 07:00 UTC daily, or manual | exhaustive 7-card eval, benchmarks, `bench-sim` table (artifact) | a real regression in eval or a big slowdown |

Diagnosing `test` failures:
- **Fails on one OS only**: a determinism or platform issue (NFR-1). Suspect iteration over a
  `HashMap` in logic, or float formatting.
- **Statistical tests** (uniformity, CI-based assertions) use fixed seeds and > 5σ margins, so
  they don't flake. If one fails, behavior changed.
- **Golden RNG test** (`engine::rng`): see §4. The random stream changed.
- **Kuhn convergence** (`bots::mccfr`): the exploitability target is 0.005. A failure means MCCFR
  or best-response logic changed.

## 7. Known limits (measured, not hypothetical)

| Limit | Evidence | Mitigation / backlog |
|---|---|---|
| ISMCTS isn't an equilibrium method | Kuhn exploitability 0.145 (MCCFR < 0.005) | MCCFR where tractable; better opponent models |
| Coach over-rewards aggression | Smoke test: "raise 9-high" scored ~12 bb over calling; ISMCTS opponent nodes over-fold | Better rollouts; opponent-model determinization (backlog #1, #2) |
| Check undervalued in spots | Rollouts check down, so they capture no future value after a check | Same |
| Determinization ignores opponents' past actions | Uniform hidden cards unless ranges are given | Range inference (backlog #2) |
| MCCFR only for Kuhn/Leduc | HUNL needs card + action abstraction | Backlog #7 |
| Multiway play is weaker | Info sets grow fast with seats | Bigger budgets; neural values (backlog #8) |
| FTQ declarations sampled uniformly in determinization | `ftq.rs` determinize | Opponent model |
| A live seat all-in from the ante only shares dead money, not live layers | `pot::build_pots` | Rare; revisit if a game makes it common |
| Coach review is synchronous | runs after each hand | Background thread (backlog #9) |

Throughput reference (16 threads, release): random Hold'em 1.3–1.6 M hands/s; ISMCTS ~84 k
playouts/s/thread (Hold'em), ~55 k (FTQ). Full table: `docs/tech/sim.md` §5.

## 8. Backlog (prioritized)

1. **Better rollout policy** (equity-aware instead of check-down). Fixes the most visible coach bias.
2. **FR-PA-5 exploitative bots / opponent model**: weight determinization by the player's
   observed tendencies (from `dc stats`), with capped deviation.
3. **Web UI** (axum + WebSocket): engine I/O is already serde; observations are per seat.
4. **FR-BT-8 external policy adapter (Jev)**: protocol in `docs/tech/bots.md` §6.
5. **`UntilDone` games** (Guts Leg, 3-2-1 Drop): `type Carry` in the API (v2), game doc §9.
6. **More Bible games** via `docs/dev/adding-a-game.md`.
7. **HUNL MCCFR** with card/action abstraction (`Abstraction` trait; bots.md §3).
8. **GPU / neural**: equity on GPU (`wgpu`/`cudarc`), value networks (`candle`/`burn`) for
   depth-limited search. A 3080 is enough for small networks.
9. **Coach off the critical path** (background thread).
10. **ISMCTS perf**: `observe` allocates per node; an incremental info-set key would help.

Open questions still in the PRD: OQ-3 (Both-declaration edge payouts), OQ-5 (stud range
notation), OQ-7 (coach opponent model), games.md G-3 (showing losing hands).
