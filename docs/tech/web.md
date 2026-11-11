# Tech Plan — Web Multiplayer (Phase 2, track W)

| Field | Value |
|---|---|
| Status | Draft v0.2 for owner review (2026-11-10) |
| Implements | PRD FR-WB-1…12, milestones W1–W6 |
| Builds on | FR-UI-2 (serde I/O), `cli.md` "Path to the web UI", API v2 `view()` (`game-catalog.md` §2) |

Goal: the home-game group plays Dealer's Choice together **over the internet** from phones and laptops. Each
person signs in with **Discord**. The server is **self-hosted on the owner's server**. v1 tables are
**humans only** (bots later). It includes **table chat** and **hand history**, and is **mobile-friendly**.

Non-goals for web v1: bots at web tables, coach/hints, spectators, real money, tournaments, native apps.

---

## 1. Architecture

```
 browser (React + TS, mobile-first)
     │  HTTPS: static assets, /auth/*, /api/*        WSS: /ws (one socket per tab)
     ▼
 reverse proxy (owner's server: TLS)
     ▼
 server crate (axum + tokio) ──────────────────────────────────────────────┐
   ├─ auth: Discord OAuth2 → session cookie                                 │
   ├─ lobby: list/create/join tables (REST)                                 │
   ├─ table actors: one tokio task per table, owns engine `Table` + the     │
   │   current game's `State` (generic G via games::registry visitor)       │
   ├─ protocol: ServerMsg / ClientMsg (crate `protocol`, TS types generated)│
   └─ storage: SQLite (existing crate, migration 0002)                      │
                                                                            │
 engine / games  (unchanged consumers: server is "a transport adapter, not a rewrite", FR-UI-2)
```

New workspace members:
- `crates/protocol`: serde message types + `TableView`. TypeScript types generated with `ts-rs`, checked in
  CI (a diff fails the build), so client and server can't drift.
- `crates/server`: the `dc-server` binary.
- `web/`: Vite + React + TypeScript app. Built to static files that the server serves (single deploy unit).

Dependency direction: `server → {protocol, games, engine, storage}`; `protocol → engine` (for `Chips`, `Card`).
`bots`/`sim` are not linked into the server in v1.

## 2. Table actor (server-authoritative)

- One task per table holds the only copy of the full `State`. Clients **never** receive `State`, only
  `view(observe(state, seat))` for their own seat (FR-EC-5). That's the existing guarantee, so there's no new
  filtering code to get wrong.
- Game selection: when the chooser picks a game id, the actor calls `games::registry::with_game` with a
  visitor that runs `run_game::<G>()` until the game ends (SingleHand/Orbit/UntilDone), then returns to the
  Dealer's Choice loop (FR-SS-4).
- Chance: each hand's `DetRng` seed comes from the OS CSPRNG (`getrandom`), never derived from table ids or
  time, so outcomes can't be predicted. The seed is stored with the hand record for replay, and is never sent to
  clients during the hand.
- Input: a client sends a choice (below); the actor converts it to `G::Action`, calls `validate`, then `apply`.
  Invalid input → `ServerMsg::Error` to that client only.
- Simultaneous decisions (declarations, in/out, drop, sealed bids): the actor collects all of them, then applies
  them in seat order in one step and broadcasts once, so order and timing reveal nothing (API doc §1 rule 2).
- Persistence: after every hand the record and ledger rows are written in one transaction (storage.md §2). On
  restart, tables in progress resume at the start of the interrupted hand, whose bets are refunded (D-WEB-6).

## 3. Protocol

```rust
// client → server
enum ClientMsg {
  Hello { table: TableId, resume_from: Option<u64> },   // event sequence number for reconnect
  Sit { seat: u8 }, Stand, SitOut(bool),
  ChooseGame { game_id: String, config: Option<toml::Value> },
  Act { prompt_id: u64, choice: Choice },               // must match the current prompt
  Chat { text: String },
  Rebuy, LeaveGame,
  Ping,
}
enum Choice {
  Option { id: String },                                // a button from the prompt
  Amount { id: String, cents: i64 },                    // bet/raise to, or bid
  Cards  { id: String, cards: Vec<Card> },              // discard / pass / keep / arrange (ordered)
}

// server → client
enum ServerMsg {
  Snapshot { seq: u64, view: TableView, prompt: Option<Prompt>, lobby: TableInfo },
  Event { seq: u64, event: TableEvent },                // action taken, card dealt, pot awarded, chat …
  Prompt(Prompt),                                       // your turn
  HandSummary(HandSummary),                             // after each hand, viewer-filtered
  Error { code: String, message: String },
  Pong,
}
struct Prompt { id: u64, deadline_ms: u64, options: Vec<PromptOption> }
enum PromptOption {
  Button { id, label, class: ActionClass },             // Fold, Check, Call $2, Declare High …
  Amount { id, label, min, max, step, presets: Vec<(String, Chips)> },  // ⅓ pot, pot, all-in
  CardPick { id, label, from: Vec<Card>, count: RangeInclusive<u8>, ordered: bool },
}
```

**Decision (D-WEB-1):** prompts are built from the game's `legal_actions` + betting `options` by a
per-game `prompt()` helper next to `view()`, and choices map back through the same helper. The alternative,
sending each game's `Action` enum to TypeScript, would mean ~50 bespoke client schemas.

**Reconnect:** each table keeps a ring buffer of recent events; a client reconnecting with `resume_from`
gets the missing events, or a fresh `Snapshot` if the gap is too large. Snapshots are always safe because they're
computed from the observation.

## 4. TableView: one renderer for every game

`view()` (API v2) maps each game's observation to a generic structure the client draws without knowing the game:

```rust
struct TableView {
  game: GameBadge,                        // id, display name, rules_ref, hand # within game
  seats: Vec<SeatView>,                   // name, avatar, stack, bet this street, status (folded/all-in/out/away),
                                          // cards: Vec<CardSlot>, badges (dealer, chooser, legs, strikes, "declared")
  boards: Vec<BoardView>,                 // layout + slots, for every community-card shape
  pots: Vec<PotView>,                     // main/side/carry-over, with shares (High/Low/Top/Spade)
  wilds: Vec<WildView>,                   // table-wide wild ranks, or per-seat (Low in the Hole)
  phase: String,                          // "Turn — betting", "Draw 2 of 3", "Declare"
  log_tail: Vec<String>,
}
enum CardSlot { Up(Card), Down(Option<Card>) /* Some = only you can see it */, Empty, Joker }
enum BoardLayout { Row, Rows(u8), H, Wheel(u8), Cross, Elevator, Middle /* Spit/Numbers */ }
```

Inverted-information games just put `Down(None)` in the viewer's own slots and `Up` in everyone else's. The
generic test asserts that `view()` is a pure function of the observation, so no leaks are possible.

## 5. Accounts and identity (FR-WB-2)

- **Discord OAuth2**, authorization code flow with PKCE and `state`; scope `identify` (+ `guilds` only if the
  allowlist below is on). The server stores the Discord user id, username, and avatar hash. No passwords, no email.
- **Allowlist (D-WEB-2, confirmed):** only members of the owner's Discord server can sign in. `ALLOWED_GUILD_ID`
  is required in production (the server refuses to start without it unless `DEV_MODE=1`), and sign-in
  requests the `guilds` scope to check membership. Plus an `ADMIN_DISCORD_IDS` list for the owner.
- Sessions: random 256-bit id in an `HttpOnly; Secure; SameSite=Lax` cookie, stored hashed in SQLite, 30-day
  sliding expiry. The WebSocket upgrade checks the cookie and the `Origin` header.
- **Money (D-WEB-3):** play money only. Each account has a bankroll on the existing ledger (FR-SS-1). New accounts
  start with **$100** (the Bible's buy-in). Admins can deposit or reset. An optional per-night **settle-up page**
  shows net results, so the group can settle outside the app. The app never moves real money.

## 6. Lobby and table flow (FR-WB-3…6)

1. Signed-in user creates a table: name, seats (2–9), game pool (all implemented games or a subset), turn timer,
   private invite link (default) or listed to all allowed users.
2. Players open the link, sit, and the host starts. The first chooser is random; then it rotates left (FR-SS-4).
3. The chooser gets a game picker (searchable, grouped by family, with each game's Bible summary).
   `TableStake` buy-ins and rebuys go through the ledger (FR-SS-2/3).
4. Players can sit out (skipped from the next game) or leave between games.

**Timers (D-WEB-4, confirmed):** 30 s per decision + a 60 s time bank per game, refilled each game. On timeout: check
if possible, else fold; declarations default to High. Chooser timeout: the server picks from the pool at random.
**Disconnects:** the seat stays; the timer runs; three consecutive timeouts → sit out after this game.

## 7. Chat and hand history (FR-WB-7, FR-WB-8)

- Chat: per table, 500 chars, rate-limited (5 messages per 10 s), stored for 30 days; plain text, rendered escaped.
  Admin can delete messages.
- Hand history: a list of past hands per table and per player. Each entry replays step by step **from the
  viewer's own observations** (the record replays the full state server-side, then views are produced per viewer),
  so mucked and hidden cards never leak, even after the hand. Shown cards at showdown are public.
- Export: a hand as text, the same format the CLI prints.

## 8. Frontend

- Vite + React 18 + TypeScript, `zustand` for client state, a hand-rolled WebSocket client with reconnect/backoff,
  CSS modules. No UI kit for the table itself (cards and seats are custom SVG); a small headless component
  library (Radix) for dialogs and menus.
- **Mobile-first (FR-WB-9):** designed at 375×667 portrait first. The hero seat sits at the bottom with large cards,
  opponents in a compact arc, board in the centre, action bar pinned to the bottom with ≥ 44 px targets and a bet
  slider + presets. Landscape and desktop get a classic oval. Card pick (discard/pass/arrange) is tap-to-select
  plus a confirm button. Tested on iOS Safari and Android Chrome.
- A **storybook-style gallery page** (`/dev/views`, dev builds only) renders a recorded `TableView` for every game,
  which serves as the visual test for each family PR.
- Accessibility: card text alternatives ("Queen of hearts"), colour + symbol suits (four-colour deck toggle), focus
  order on the action bar.

## 9. Storage

SQLite stays (storage.md §4: Postgres only with multiple writer processes; this is one process). WAL mode;
one writer task; reads through a small pool. Migration `0002_web.sql`:

```sql
CREATE TABLE account (id INTEGER PRIMARY KEY, discord_id TEXT UNIQUE NOT NULL, name TEXT NOT NULL,
                      avatar TEXT, is_admin INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL);
-- player.account_id links the existing ledger/hand tables to a web account
ALTER TABLE player ADD COLUMN account_id INTEGER REFERENCES account(id);
CREATE TABLE web_session (id_hash BLOB PRIMARY KEY, account_id INTEGER NOT NULL, expires_at TEXT NOT NULL);
CREATE TABLE web_table (id TEXT PRIMARY KEY, name TEXT, host INTEGER, settings TEXT, created_at TEXT, closed_at TEXT);
CREATE TABLE chat (id INTEGER PRIMARY KEY, table_id TEXT, account_id INTEGER, text TEXT, at TEXT, deleted INTEGER);
```

Backups: nightly `VACUUM INTO` snapshot to `data/backups/` (7 kept); the handbook documents restoring one.

## 10. Deployment on the owner's server (FR-WB-10)

- One Docker image (multi-stage: Rust release build + `npm run build`; runtime is distroless/debian-slim with
  the binary and static files). `docker-compose.yml` with a volume for `DC_DATA_ROOT`.
- Config via env: `PUBLIC_URL`, `DISCORD_CLIENT_ID`, `DISCORD_CLIENT_SECRET`, `ALLOWED_GUILD_ID`,
  `ADMIN_DISCORD_IDS`, `DC_DATA_ROOT`, `BIND_ADDR`.
- **Target (confirmed, OQ-11):** a Linux x86-64 server with Docker + Compose. The compose file **bundles Caddy**,
  which gets Let's Encrypt certificates automatically for `PUBLIC_URL`'s domain (the owner sets it in `.env` at
  deploy time; nothing in the repo hard-codes it).
- `deploy/README.md` is a step-by-step guide: create the Discord application (Developer Portal → OAuth2 → add
  `https://<domain>/auth/discord/callback` as the redirect), find the guild id, fill `.env`, point DNS at the
  server, `docker compose up -d`. Secrets live only in the server's `.env`.
- Health: `GET /healthz`; graceful shutdown waits for hands in progress to end (max 5 min), then refunds.
- CI builds the image on tags; deploying is `docker compose pull && docker compose up -d` (handbook).
- Supplied by the owner at deploy time: the domain, Discord client id/secret, guild id, admin ids.

## 11. Security checklist

Server-authoritative state; per-seat views only; CSPRNG seeds; OAuth `state` + PKCE; cookie flags; WS `Origin`
check; per-connection and per-account rate limits; message size cap (16 KB); input validated by the game's
`validate`; chat escaped; no secrets in the client bundle; dependency audit (`cargo audit`, `npm audit`) in CI.

## 12. Milestones

| ID | Deliverable | Exit |
|---|---|---|
| W1 | `protocol` crate + TS codegen, `TableView`/`prompt` for Hold'em & FTQ (needs G0), table actor with a dev-only login stub, minimal React table | Two browser tabs play a Hold'em orbit and an FTQ hand locally |
| W2 | Discord OAuth, accounts, sessions, bankroll ledger, allowlist | Sign in with Discord on localhost; bankroll survives restart |
| W3 | Lobby, invites, Dealer's Choice flow, timers, sit-out, reconnect | 4 people on different networks complete a session; killing a tab mid-hand and reopening it resumes |
| W4 | Chat + hand history (viewer-filtered replay) | Leak test: replaying any stored hand as each viewer never shows a card that viewer couldn't see live |
| W5 | Mobile polish, Docker image, deploy docs, backups | Session played on phones; deployed on the owner's server behind TLS |
| W6 | Every game family's `view()` in the gallery; optional `/reports` for sim reports | Each catalog family PR adds its games to the gallery |

W1 starts after G0 (it needs `view()`). After that, web work runs in parallel with the game families and sims.
