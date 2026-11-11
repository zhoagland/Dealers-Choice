# Technical Plans — Index

| Doc | Crate | Milestones | PRD coverage |
|---|---|---|---|
| [engine.md](engine.md) | `engine` | M1, M2 | FR-EC-*, FR-SS-*, NFR-1, NFR-2, NFR-5 |
| [../api/game-driver.md](../api/game-driver.md) | `engine::game` | M1 | FR-GD-* |
| [games.md](games.md) | `games` | M1, M2, M5 | §7 prototype games |
| [bots.md](bots.md) | `bots` | M1, M2, M4 | FR-BT-*, FR-EV-* |
| [sim.md](sim.md) | `sim` | M3 | FR-SM-* |
| [storage.md](storage.md) | `storage` | M3, M6 | FR-ST-* |
| [cli.md](cli.md) | `cli` | M2, M4 | FR-UI-* |

## Workspace layout

```
Dealers-Choice/
├── Cargo.toml              # [workspace] members = ["crates/*"]
├── crates/
│   ├── engine/             # cards, eval, betting, pots, Game trait, session, rng, testing
│   ├── games/              # one module per game + registry.rs
│   ├── bots/               # Policy, Evaluator, random, equity, ismcts, mccfr, strength, coach, ranges
│   ├── sim/                # runner, stats, encoding, instrumentation
│   ├── storage/            # sqlite (app state), sim output writer, blueprint files
│   └── cli/                # `dc` binary
├── config/                 # TOML: game configs, bot presets
├── data/                   # gitignored: dc.sqlite, sim/, blueprints/
└── docs/
```

The existing `src/` is replaced by `crates/engine`. Reasons are in `engine.md` §1.

## Dependency direction (no cycles, enforced by Cargo)

```
          engine
        ↗   ↑   ↖
   games  bots  storage
        ↖   ↑   ↗
           sim
            ↑
           cli  (depends on all)
```

- `bots` does **not** depend on `games`. That's what makes bots game-agnostic (FR-GD-6).
  It uses `games` only as a dev-dependency for tests.
- `sim` depends on `bots` and `engine`, but not `games`. It's generic over `G: Game`. The CLI
  picks the concrete game via `games::registry`.

## Shared dependencies

| Crate | Use | Why this one |
|---|---|---|
| `rand`, `rand_chacha` | `DetRng = ChaCha8Rng` | Portable, seedable, supports streams → per-hand reproducible seeds |
| `serde`, `toml`, `serde_json` | config, I/O messages | FR-UI-2 |
| `smallvec` | action lists, card lists | avoids heap allocation in hot loops |
| `thiserror` | error enums | readable error types without boilerplate |
| `rayon` | sim parallelism | data-parallel `par_iter`, no manual threads |
| `clap` (derive) | CLI | |
| `rusqlite` (`bundled`) | SQLite | bundles SQLite → no system install on Windows |
| `proptest` (dev) | property tests | evaluator differential tests, betting invariants |
| `criterion` (dev) | benchmarks | NFR-3 numbers |

## Cross-cutting rules

1. **Determinism (NFR-1).** All randomness flows from a `DetRng` passed in explicitly. No
   `thread_rng()`. No `HashMap` iteration in logic that affects outcomes; use `BTreeMap` or sort
   first. Time budgets are *non-deterministic* (they depend on CPU speed), so:
   - sims, coach, spot analysis, and tests use **playout budgets**;
   - interactive play may use a **time budget**. Determinism there comes from the recorded
     action log, not from re-running the search.
2. **Units.** Chips are `i64` cents everywhere. Floats appear only in bot values and probabilities.
3. **Errors.** Library crates return `Result<_, thiserror enum>`. `panic!` only for violated
   internal invariants (bugs), never for bad user input.
4. **Readability (NFR-4).** One concept per module. A `//!` module doc at the top of each file saying
   what it does and which PRD IDs it implements.
