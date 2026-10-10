-- Schema v1 (docs/tech/storage.md §2). Applied once; tracked by PRAGMA user_version.
CREATE TABLE player (
  id          INTEGER PRIMARY KEY,
  name        TEXT NOT NULL UNIQUE,
  created_at  TEXT NOT NULL
);

-- Bankroll is never stored: it is SUM(delta_cents) over a player's ledger rows.
CREATE TABLE ledger (
  id          INTEGER PRIMARY KEY,
  player_id   INTEGER NOT NULL REFERENCES player(id),
  session_id  INTEGER REFERENCES session(id),
  kind        TEXT NOT NULL CHECK (kind IN ('deposit','buy_in','cash_out','rebuy','hand','reset')),
  delta_cents INTEGER NOT NULL,
  at          TEXT NOT NULL
);
CREATE INDEX ledger_player ON ledger(player_id);

CREATE TABLE session (
  id          INTEGER PRIMARY KEY,
  started_at  TEXT NOT NULL,
  seed        INTEGER NOT NULL
);

-- Human-played hands only (bots' hands are never post-processed; PRD §5.6b).
CREATE TABLE hand (
  id           INTEGER PRIMARY KEY,
  session_id   INTEGER NOT NULL REFERENCES session(id),
  player_id    INTEGER NOT NULL REFERENCES player(id),
  game_id      TEXT NOT NULL,
  game_version INTEGER NOT NULL,
  seat         INTEGER NOT NULL,
  result_cents INTEGER NOT NULL,
  record       BLOB NOT NULL,          -- sim::encoding HandRecord, replayable
  at           TEXT NOT NULL
);
CREATE INDEX hand_player_game ON hand(player_id, game_id);

CREATE TABLE coach_note (
  hand_id      INTEGER NOT NULL REFERENCES hand(id),
  decision_idx INTEGER NOT NULL,       -- index into the hand's action log
  ev_loss      REAL,                   -- NULL = off-menu action, not scored
  se           REAL NOT NULL,
  flagged      INTEGER NOT NULL
);
