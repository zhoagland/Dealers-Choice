# Tech Plan — `sim`

Implements FR-SM-1…4 and the M3 rate measurement that gates FR-ST-2.

```
sim/src/
├── runner.rs        RunSpec → parallel hand loop
├── seating.rs       seat rotation, duplicate deals
├── stats.rs         online mean/variance, CI, per-seat aggregates
├── encoding.rs      compact binary hand records
├── instrument.rs    hands/s, decisions/s, bytes/hand
└── sink.rs          trait OutputSink (implemented by storage)
```

## 1. RunSpec

```toml
game = "holdem_nl"
hands = 1_000_000
seed = 42
duplicate = true          # §3
history_sample = 0.01     # FR-SM-4: fraction of hands whose full record is kept
[[seat]]  bot = "ismcts"  preset = 7
[[seat]]  bot = "equity"
```
Bots in sims **must** use `Budget::Playouts` (determinism, `tech/README.md` rule 1). The
runner rejects `Time` budgets.

## 2. Execution

`(0..hands).into_par_iter().map(|i| play_hand(i))`. Each hand derives its RNG from
`(run_seed, i)` (engine §7), so results are identical regardless of thread count. Per-thread
`Stats` are merged at the end (Welford's algorithm, with the parallel/Chan combination formula for merging).

Bots that keep state between decisions (ISMCTS tables) are created per hand, per seat.
ISMCTS tables are cleared after each decision anyway, and MCCFR blueprints are shared read-only via `Arc`.

## 3. Variance: why sims need more than "run more hands"

Win-rate noise in NL Hold'em is large. Typical σ ≈ 80–100 bb/100 (i.e. per 100 hands). The CI
half-width after *n* hands is `1.96 · σ / √(n/100)`. To resolve a ±1 bb/100 difference:
`n ≈ 100 · (1.96 · 90)² ≈ 3.1 M hands`.

At ISMCTS preset 7 (~0.5 s × ~6 decisions per hand on 1 core), that's ~3 s/hand, so ~3.1 M hands ≈ 108
core-days. **Brute force is infeasible for comparing strong bots.** Mitigations, in order of cost:
1. **Duplicate deals** (v1): every deal is played once per seat rotation, so each bot gets
   each set of cards. Card luck largely cancels. Typically cuts variance several-fold. Cheap to implement.
2. **Lower budgets for sims** than for play. Measure how strength scales with budget instead of
   assuming it.
3. **AIVAT-style control variates** (Burch et al., 2018): subtract estimated luck using a value
   function. v2; needs a good value estimator.

The report always prints the CI, so "A beats B" is never claimed without one.

## 4. Output tiers (FR-SM-4)

- **Aggregates** (always): per seat, `n, mean, M2` in eval units, action frequencies by
  `ActionClass` and street, showdown %, win %. Written once per run.
- **Hand records** (sampled at `history_sample`): `encoding.rs` format
  `[varint len][u8 api_ver][u16 game_ver][u64 hand_seed][setup][varint n_actions][actions…]`,
  actions encoded as bytes per game (`Serialize` → `postcard`). The seed plus setup plus actions
  is enough for exact replay. Hidden cards are recoverable from the seed, so they aren't stored twice.

## 5. M3 rate measurement (gates the storage decision)

`dc bench-sim` runs fixed workloads and prints a markdown table like the one below.

**Measured 2026-10-10** (`dc bench-sim --deals 2000`, release build, 16 threads on the dev
machine). Record size is raw postcard before compression:

| Workload | hands/s | decisions/s | B/hand | Full history per day |
|---|---|---|---|---|
| Kuhn, random ×2 | 2.06 M | 4.6 M | 34 | 6.1 TB |
| HU Hold'em, random ×2 | 1.59 M | 5.7 M | 52 | 7.1 TB |
| 6-max Hold'em, random ×6 | 1.31 M | 11.7 M | 92 | 10.5 TB |
| FTQ, random ×4 | 738 k | 10.1 M | 94 | 6.0 TB |
| HU Hold'em, ISMCTS:7 (2k playouts) vs random | 485 | 1.9 k | 52 | 2.2 GB |
| FTQ, ISMCTS:7 (2k) vs random ×3 | 27 | 445 | 101 | 238 MB |
| 6-max Hold'em, ISMCTS:7 (2k) ×6 | 45 | 356 | 89 | 346 MB |

The pre-measurement estimate was ~4×10⁵ random hands/s; actual is ~4× higher. The cheap-vs-strong gap
is ~4.5 orders of magnitude, as predicted. ISMCTS speed: ~84 k playouts/s per thread in Hold'em,
~55 k in FTQ. The wild A-6 low evaluator is not a bottleneck.

**Storage decision (closes FR-ST-2):** analytics run on human players only (PRD §5.6b), so bot
simulation records never need per-hand columnar queries. **No Parquet.** Simulation output = JSON
summary + zstd-compressed length-prefixed postcard records. Cheap-bot runs use
`history_sample` ≪ 1 (aggregates are the product); strong-bot runs can keep 100%.
