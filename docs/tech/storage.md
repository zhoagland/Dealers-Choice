# Tech Plan — `storage`

Implements FR-ST-1…4, FR-SS-1 persistence.

## 1. Layout

```
<data_root>/                 # default ./data, override with DC_DATA_ROOT or --data-root (FR-ST-4)
├── dc.sqlite                # app state
├── sim/game=<id>/run=<uuid>/
│   ├── spec.toml            # RunSpec, exact copy
│   ├── summary.json         # aggregates
│   └── hands-000.bin.zst    # sampled records, zstd, rotated at 256 MB
└── blueprints/<game_id>/v<game_ver>-<menu_hash>.bin
```
The partitioning is hive-style (`key=value` directories), so DuckDB can query across runs with
`read_json('data/sim/*/*/summary.json', hive_partitioning=true)` without an import step.

## 2. SQLite schema (app state)

> **As implemented (M6)** in `crates/storage/migrations/0001_init.sql`, which differs from the sketch below:
> - `player` has no `kind`/`bot_preset`: bots are never stored (infinite bankroll, and analytics
>   are human-only per PRD §5.6b).
> - No `game_played` table. `hand` carries `player_id`, `game_id`, `game_version`, `seat`, and
>   `result_cents` directly, because every query in M7 is "this player's hands in this game".
> - `ledger` adds `reset`. The CLI checks after every game that the ledger sum equals the
>   in-memory bankroll, and stops with an error if not.
> - Blueprints: `<data_root>/blueprints/<game>/v<version>-<iterations>.bin`, plain postcard
>   (they're small), loaded once per process and shared.

```sql
CREATE TABLE player (
  id          INTEGER PRIMARY KEY,
  name        TEXT NOT NULL UNIQUE,
  kind        TEXT NOT NULL CHECK (kind IN ('human','bot')),
  bot_preset  TEXT,                  -- NULL for humans
  created_at  TEXT NOT NULL
);
-- Bankroll is derived, never stored: SUM(delta) over the ledger. Bots skip the ledger (infinite bankroll).
CREATE TABLE ledger (
  id          INTEGER PRIMARY KEY,
  player_id   INTEGER NOT NULL REFERENCES player(id),
  session_id  INTEGER REFERENCES session(id),
  kind        TEXT NOT NULL CHECK (kind IN ('deposit','buy_in','cash_out','rebuy','hand','reset')),
  delta_cents INTEGER NOT NULL,
  at          TEXT NOT NULL
);
CREATE TABLE session (id INTEGER PRIMARY KEY, started_at TEXT, ended_at TEXT, seed INTEGER NOT NULL);
CREATE TABLE game_played (
  id INTEGER PRIMARY KEY, session_id INTEGER NOT NULL REFERENCES session(id),
  game_id TEXT NOT NULL, game_version INTEGER NOT NULL, chooser_seat INTEGER NOT NULL
);
CREATE TABLE hand (
  id INTEGER PRIMARY KEY, game_played_id INTEGER NOT NULL REFERENCES game_played(id),
  record BLOB NOT NULL               -- sim::encoding format; replayable
);
CREATE TABLE coach_note (hand_id INTEGER REFERENCES hand(id), decision_idx INTEGER,
  ev_loss REAL, se REAL, flagged INTEGER);
```

**Why a ledger instead of a `bankroll` column:** the balance becomes an invariant you can audit
(`SUM(delta)`), every chip movement has a reason, and a crash between "hand ends" and "update
bankroll" can't create or destroy chips, as long as each hand's ledger rows go in one transaction.

Migrations: numbered `.sql` files embedded with `include_str!`, applied in order, tracked by
`PRAGMA user_version`. No ORM. Plain `rusqlite` keeps the SQL visible.

Human-played hands are stored in full (they're tiny in volume). Simulated hands never go into SQLite.

## 3. Simulation output

> **Decided after M3 (see sim.md §5):** no Parquet. The rest of this section is the
> implemented design.

`sim::sink::OutputSink` is implemented by `FsSink`, which writes the layout above. v1 format: JSON
summary + zstd-compressed binary records. **Parquet is deferred** until M3 numbers show a
need for columnar scans over hand records. For aggregates, JSON plus DuckDB is enough, and the
`arrow`/`parquet` crates are a heavy dependency to justify.

Decision rule after M3: if analysis needs per-hand queries ("all hands where the hero 3-bet the flop")
across >10⁸ records, add a Parquet writer for a flattened decision table
`(run, hand, decision_idx, seat, street, action_class, amount, q, pot)`. Otherwise keep the v1 format.

## 4. Server deployment

Same layout under a bigger `data_root`. SQLite is fine for single-node with WAL mode
(`PRAGMA journal_mode=WAL`) for concurrent readers. Moving to Postgres is only justified once there are
several writer processes (the multi-human online stage).
