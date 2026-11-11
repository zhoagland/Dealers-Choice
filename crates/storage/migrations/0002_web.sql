-- Schema v2: the web app (docs/tech/web.md §9).
CREATE TABLE account (
  id          INTEGER PRIMARY KEY,
  discord_id  TEXT NOT NULL UNIQUE,
  name        TEXT NOT NULL,
  avatar      TEXT,
  is_admin    INTEGER NOT NULL DEFAULT 0,
  created_at  TEXT NOT NULL
);
-- Links the ledger and hands of a player row to a web account.
ALTER TABLE player ADD COLUMN account_id INTEGER REFERENCES account(id);

-- Sign-in sessions: only a SHA-256 of the cookie value is stored.
CREATE TABLE web_session (
  id_hash     BLOB PRIMARY KEY,
  account_id  INTEGER NOT NULL REFERENCES account(id),
  expires_at  INTEGER NOT NULL
);

CREATE TABLE web_table (
  id          TEXT PRIMARY KEY,
  name        TEXT NOT NULL,
  host        INTEGER REFERENCES account(id),
  settings    TEXT NOT NULL,
  session_id  INTEGER REFERENCES session(id),
  created_at  TEXT NOT NULL,
  closed_at   TEXT,
  -- JSON [[player_id, kind, cents]]: what to pay back if the server stops mid-game (D-WEB-6).
  refund      TEXT NOT NULL DEFAULT '[]'
);

CREATE TABLE chat (
  id          INTEGER PRIMARY KEY,
  table_id    TEXT NOT NULL,
  account_id  INTEGER NOT NULL REFERENCES account(id),
  text        TEXT NOT NULL,
  at          INTEGER NOT NULL,
  deleted     INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX chat_table ON chat(table_id, id);

-- One row per hand played at a web table, replayable for each viewer (web.md §7).
CREATE TABLE web_hand (
  id            INTEGER PRIMARY KEY,
  table_id      TEXT NOT NULL,
  game_id       TEXT NOT NULL,
  game_version  INTEGER NOT NULL,
  seats         TEXT NOT NULL,       -- JSON: account id (or null) per hand seat
  results       TEXT NOT NULL,       -- JSON: cents per hand seat
  record        BLOB NOT NULL,       -- sim::encoding HandRecord
  at            INTEGER NOT NULL
);
CREATE INDEX web_hand_table ON web_hand(table_id, id);
