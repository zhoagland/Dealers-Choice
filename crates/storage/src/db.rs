//! SQLite app state: players, the bankroll ledger, human hands, coach notes (FR-ST-1, FR-SS-1).
//! Schema: `migrations/*.sql`; design: `docs/tech/storage.md` §2.
//!
//! Plain `rusqlite` with SQL written out, no ORM, so what's stored is visible in one place.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use engine::Chips;

/// Migrations in order. `include_str!` compiles the SQL files into the binary.
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_init.sql"),
    include_str!("../migrations/0002_web.sql"),
];

#[derive(Debug)]
pub struct DbError(pub String);

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "database: {}", self.0)
    }
}

impl std::error::Error for DbError {}

impl From<rusqlite::Error> for DbError {
    fn from(e: rusqlite::Error) -> DbError {
        DbError(e.to_string())
    }
}

pub type DbResult<T> = Result<T, DbError>;

/// Ledger entry kinds (must match the CHECK constraint in the schema).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LedgerKind {
    Deposit,
    BuyIn,
    CashOut,
    Rebuy,
    Hand,
    Reset,
}

impl LedgerKind {
    fn as_str(self) -> &'static str {
        match self {
            LedgerKind::Deposit => "deposit",
            LedgerKind::BuyIn => "buy_in",
            LedgerKind::CashOut => "cash_out",
            LedgerKind::Rebuy => "rebuy",
            LedgerKind::Hand => "hand",
            LedgerKind::Reset => "reset",
        }
    }
}

/// One coach finding for one decision.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    /// Index into the hand's action log.
    pub decision_idx: usize,
    /// `None` = off-menu action, not scored.
    pub ev_loss: Option<f64>,
    pub se: f64,
    pub flagged: bool,
}

/// A stored hand, as read back.
#[derive(Clone, Debug)]
pub struct HandRow {
    pub id: i64,
    pub game_id: String,
    /// The player's seat in that hand.
    pub seat: u8,
    /// `sim::encoding` record.
    pub record: Vec<u8>,
}

/// A human hand to store, with its coach review.
pub struct StoredHand<'a> {
    pub session_id: i64,
    pub player_id: i64,
    pub game_id: &'a str,
    pub game_version: u32,
    pub seat: u8,
    pub result: Chips,
    pub record: &'a [u8],
    pub notes: Vec<Note>,
}

pub struct Db {
    conn: Connection,
}

fn now() -> String {
    // Seconds since the Unix epoch: sortable, and avoids a date-time dependency.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    secs.to_string()
}

impl Db {
    /// Open (creating if needed) and migrate.
    pub fn open(path: &Path) -> DbResult<Db> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| DbError(format!("{}: {e}", dir.display())))?;
        }
        Db::init(Connection::open(path)?)
    }

    /// A throwaway in-memory database (tests).
    pub fn in_memory() -> DbResult<Db> {
        Db::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> DbResult<Db> {
        // WAL: readers don't block the writer (storage.md §4). Foreign keys are off by default in SQLite.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let version: usize = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version) {
            conn.execute_batch(sql)?;
            conn.pragma_update(None, "user_version", i + 1)?;
        }
        Ok(Db { conn })
    }

    pub fn schema_version(&self) -> DbResult<usize> {
        Ok(self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))?)
    }

    // ----- players -----------------------------------------------------------------------

    /// Find a player by name, creating it (with an optional opening deposit) if new.
    pub fn player(&mut self, name: &str, opening_deposit: Chips) -> DbResult<i64> {
        if let Some(id) = self.player_id(name)? {
            return Ok(id);
        }
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO player (name, created_at) VALUES (?1, ?2)",
            params![name, now()],
        )?;
        let id = tx.last_insert_rowid();
        if opening_deposit > Chips::ZERO {
            tx.execute(
                "INSERT INTO ledger (player_id, kind, delta_cents, at) VALUES (?1, 'deposit', ?2, ?3)",
                params![id, opening_deposit.0, now()],
            )?;
        }
        tx.commit()?;
        Ok(id)
    }

    pub fn player_id(&self, name: &str) -> DbResult<Option<i64>> {
        Ok(self
            .conn
            .query_row("SELECT id FROM player WHERE name = ?1", [name], |r| {
                r.get(0)
            })
            .optional()?)
    }

    /// (name, bankroll) for every player.
    pub fn players(&self) -> DbResult<Vec<(String, Chips)>> {
        let mut st = self.conn.prepare(
            "SELECT p.name, COALESCE(SUM(l.delta_cents), 0) FROM player p
             LEFT JOIN ledger l ON l.player_id = p.id GROUP BY p.id ORDER BY p.name",
        )?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, Chips(r.get(1)?))))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The bankroll is derived, never stored: the sum of the ledger.
    pub fn bankroll(&self, player_id: i64) -> DbResult<Chips> {
        Ok(Chips(self.conn.query_row(
            "SELECT COALESCE(SUM(delta_cents), 0) FROM ledger WHERE player_id = ?1",
            [player_id],
            |r| r.get(0),
        )?))
    }

    /// Record money movements atomically: all rows or none.
    pub fn ledger(
        &mut self,
        player_id: i64,
        session_id: Option<i64>,
        entries: &[(LedgerKind, Chips)],
    ) -> DbResult<()> {
        let tx = self.conn.transaction()?;
        for &(kind, delta) in entries {
            tx.execute(
                "INSERT INTO ledger (player_id, session_id, kind, delta_cents, at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![player_id, session_id, kind.as_str(), delta.0, now()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Set the bankroll to `to` with a single `reset` entry.
    pub fn reset_bankroll(&mut self, player_id: i64, to: Chips) -> DbResult<()> {
        let now_bal = self.bankroll(player_id)?;
        self.ledger(player_id, None, &[(LedgerKind::Reset, to - now_bal)])
    }

    // ----- sessions and hands ------------------------------------------------------------

    pub fn start_session(&mut self, seed: u64) -> DbResult<i64> {
        // SQLite integers are i64; store the seed's bits unchanged.
        self.conn.execute(
            "INSERT INTO session (started_at, seed) VALUES (?1, ?2)",
            params![now(), seed as i64],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Store a human hand and its coach notes in one transaction.
    pub fn store_hand(&mut self, h: &StoredHand) -> DbResult<i64> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO hand (session_id, player_id, game_id, game_version, seat, result_cents, record, at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![h.session_id, h.player_id, h.game_id, h.game_version, h.seat, h.result.0, h.record, now()],
        )?;
        let id = tx.last_insert_rowid();
        for n in &h.notes {
            tx.execute(
                "INSERT INTO coach_note (hand_id, decision_idx, ev_loss, se, flagged) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, n.decision_idx as i64, n.ev_loss, n.se, n.flagged],
            )?;
        }
        tx.commit()?;
        Ok(id)
    }

    /// A player's stored hands, oldest first.
    pub fn hands(&self, player_id: i64) -> DbResult<Vec<HandRow>> {
        let mut st = self.conn.prepare(
            "SELECT id, game_id, seat, record FROM hand WHERE player_id = ?1 ORDER BY id",
        )?;
        let rows = st.query_map([player_id], |r| {
            Ok(HandRow {
                id: r.get(0)?,
                game_id: r.get(1)?,
                seat: r.get(2)?,
                record: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Stored hand records for a player (optionally one game), oldest first.
    pub fn hand_records(
        &self,
        player_id: i64,
        game_id: Option<&str>,
    ) -> DbResult<Vec<(String, Vec<u8>)>> {
        let mut st = self.conn.prepare(
            "SELECT game_id, record FROM hand WHERE player_id = ?1 AND (?2 IS NULL OR game_id = ?2) ORDER BY id",
        )?;
        let rows = st.query_map(params![player_id, game_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// All coach notes for a player's hands in one game, as (hand id, note), in order.
    pub fn coach_notes(&self, player_id: i64, game_id: &str) -> DbResult<Vec<(i64, Note)>> {
        let mut st = self.conn.prepare(
            "SELECT n.hand_id, n.decision_idx, n.ev_loss, n.se, n.flagged FROM coach_note n
             JOIN hand h ON h.id = n.hand_id WHERE h.player_id = ?1 AND h.game_id = ?2
             ORDER BY n.hand_id, n.decision_idx",
        )?;
        let rows = st.query_map(params![player_id, game_id], |r| {
            let note = Note {
                decision_idx: r.get::<_, i64>(1)? as usize,
                ev_loss: r.get(2)?,
                se: r.get(3)?,
                flagged: r.get(4)?,
            };
            Ok((r.get(0)?, note))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}

// ----- web app (schema v2, docs/tech/web.md §5, §7, §9) ----------------------------------------

/// A signed-in web account.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub id: i64,
    pub discord_id: String,
    pub name: String,
    pub avatar: Option<String>,
    pub is_admin: bool,
    /// The ledger player behind this account.
    pub player_id: i64,
}

/// A stored chat line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatRow {
    pub id: i64,
    pub account_id: i64,
    pub name: String,
    pub text: String,
    pub at: i64,
}

/// A web table row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebTableRow {
    pub id: String,
    pub name: String,
    pub host: i64,
    pub settings: String,
    pub session_id: i64,
    pub refund: String,
}

/// A hand played at a web table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebHandRow {
    pub id: i64,
    pub table_id: String,
    pub game_id: String,
    pub game_version: u32,
    pub seats: String,
    pub results: String,
    pub record: Vec<u8>,
    pub at: i64,
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

impl Db {
    /// Create or update the account for a Discord user; new accounts get a ledger player with an
    /// opening deposit of `opening` (web.md §5: $100).
    pub fn upsert_account(
        &mut self,
        discord_id: &str,
        name: &str,
        avatar: Option<&str>,
        is_admin: bool,
        opening: Chips,
    ) -> DbResult<Account> {
        let existing: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM account WHERE discord_id = ?1",
                [discord_id],
                |r| r.get(0),
            )
            .optional()?;
        let id = match existing {
            Some(id) => {
                self.conn.execute(
                    "UPDATE account SET name = ?2, avatar = ?3, is_admin = ?4 WHERE id = ?1",
                    params![id, name, avatar, is_admin],
                )?;
                id
            }
            None => {
                self.conn.execute(
                    "INSERT INTO account (discord_id, name, avatar, is_admin, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![discord_id, name, avatar, is_admin, now()],
                )?;
                let id = self.conn.last_insert_rowid();
                let player = self.player(&format!("discord:{discord_id}"), opening)?;
                self.conn.execute(
                    "UPDATE player SET account_id = ?1 WHERE id = ?2",
                    params![id, player],
                )?;
                id
            }
        };
        self.account(id)?
            .ok_or_else(|| DbError("account vanished".into()))
    }

    pub fn account(&self, id: i64) -> DbResult<Option<Account>> {
        Ok(self
            .conn
            .query_row(
                "SELECT a.id, a.discord_id, a.name, a.avatar, a.is_admin, p.id
                 FROM account a JOIN player p ON p.account_id = a.id WHERE a.id = ?1",
                [id],
                |r| {
                    Ok(Account {
                        id: r.get(0)?,
                        discord_id: r.get(1)?,
                        name: r.get(2)?,
                        avatar: r.get(3)?,
                        is_admin: r.get(4)?,
                        player_id: r.get(5)?,
                    })
                },
            )
            .optional()?)
    }

    /// Store a sign-in session (hash of the cookie value) valid for `ttl_secs`.
    pub fn create_web_session(
        &mut self,
        id_hash: &[u8],
        account_id: i64,
        ttl_secs: i64,
    ) -> DbResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO web_session (id_hash, account_id, expires_at) VALUES (?1, ?2, ?3)",
            params![id_hash, account_id, unix_now() + ttl_secs],
        )?;
        Ok(())
    }

    /// The account of a live session, sliding its expiry forward by `ttl_secs`.
    pub fn web_session(&mut self, id_hash: &[u8], ttl_secs: i64) -> DbResult<Option<i64>> {
        let now = unix_now();
        let row: Option<(i64, i64)> = self
            .conn
            .query_row(
                "SELECT account_id, expires_at FROM web_session WHERE id_hash = ?1",
                [id_hash],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        match row {
            Some((acc, exp)) if exp > now => {
                self.conn.execute(
                    "UPDATE web_session SET expires_at = ?2 WHERE id_hash = ?1",
                    params![id_hash, now + ttl_secs],
                )?;
                Ok(Some(acc))
            }
            Some(_) => {
                self.delete_web_session(id_hash)?;
                Ok(None)
            }
            None => Ok(None),
        }
    }

    pub fn delete_web_session(&mut self, id_hash: &[u8]) -> DbResult<()> {
        self.conn
            .execute("DELETE FROM web_session WHERE id_hash = ?1", [id_hash])?;
        Ok(())
    }

    pub fn create_web_table(
        &mut self,
        id: &str,
        name: &str,
        host: i64,
        settings_json: &str,
        session_id: i64,
    ) -> DbResult<()> {
        self.conn.execute(
            "INSERT INTO web_table (id, name, host, settings, session_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, name, host, settings_json, session_id, now()],
        )?;
        Ok(())
    }

    /// Open tables, oldest first.
    pub fn open_web_tables(&self) -> DbResult<Vec<WebTableRow>> {
        let mut st = self.conn.prepare(
            "SELECT id, name, host, settings, session_id, refund FROM web_table WHERE closed_at IS NULL ORDER BY created_at",
        )?;
        let rows = st.query_map([], |r| {
            Ok(WebTableRow {
                id: r.get(0)?,
                name: r.get(1)?,
                host: r.get(2)?,
                settings: r.get(3)?,
                session_id: r.get(4)?,
                refund: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// What to pay back if the server stops during the table's current game (D-WEB-6).
    pub fn set_web_table_refund(&mut self, id: &str, refund_json: &str) -> DbResult<()> {
        self.conn.execute(
            "UPDATE web_table SET refund = ?2 WHERE id = ?1",
            params![id, refund_json],
        )?;
        Ok(())
    }

    /// Every account with its bankroll (admin page).
    pub fn accounts(&self) -> DbResult<Vec<(Account, Chips)>> {
        let ids: Vec<i64> = {
            let mut st = self.conn.prepare("SELECT id FROM account ORDER BY name")?;
            let rows = st.query_map([], |r| r.get(0))?;
            rows.collect::<Result<_, _>>()?
        };
        let mut out = Vec::new();
        for id in ids {
            if let Some(a) = self.account(id)? {
                let b = self.bankroll(a.player_id)?;
                out.push((a, b));
            }
        }
        Ok(out)
    }

    pub fn close_web_table(&mut self, id: &str) -> DbResult<()> {
        self.conn.execute(
            "UPDATE web_table SET closed_at = ?2 WHERE id = ?1",
            params![id, now()],
        )?;
        Ok(())
    }

    pub fn add_chat(&mut self, table_id: &str, account_id: i64, text: &str) -> DbResult<i64> {
        self.conn.execute(
            "INSERT INTO chat (table_id, account_id, text, at) VALUES (?1, ?2, ?3, ?4)",
            params![table_id, account_id, text, unix_now()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// The last `limit` visible chat lines of a table from the last 30 days, oldest first.
    pub fn chat(&self, table_id: &str, limit: usize) -> DbResult<Vec<ChatRow>> {
        let mut st = self.conn.prepare(
            "SELECT c.id, c.account_id, a.name, c.text, c.at FROM chat c JOIN account a ON a.id = c.account_id
             WHERE c.table_id = ?1 AND c.deleted = 0 AND c.at > ?2 ORDER BY c.id DESC LIMIT ?3",
        )?;
        let rows = st.query_map(
            params![table_id, unix_now() - 30 * 86_400, limit as i64],
            |r| {
                Ok(ChatRow {
                    id: r.get(0)?,
                    account_id: r.get(1)?,
                    name: r.get(2)?,
                    text: r.get(3)?,
                    at: r.get(4)?,
                })
            },
        )?;
        let mut out: Vec<ChatRow> = rows.collect::<Result<_, _>>()?;
        out.reverse();
        Ok(out)
    }

    pub fn delete_chat(&mut self, id: i64) -> DbResult<()> {
        self.conn
            .execute("UPDATE chat SET deleted = 1 WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn store_web_hand(
        &mut self,
        table_id: &str,
        game_id: &str,
        game_version: u32,
        seats_json: &str,
        results_json: &str,
        record: &[u8],
    ) -> DbResult<i64> {
        self.conn.execute(
            "INSERT INTO web_hand (table_id, game_id, game_version, seats, results, record, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![table_id, game_id, game_version, seats_json, results_json, record, unix_now()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Hands of a table, newest first.
    pub fn web_hands(&self, table_id: &str, limit: usize) -> DbResult<Vec<WebHandRow>> {
        let mut st = self.conn.prepare(
            "SELECT id, table_id, game_id, game_version, seats, results, record, at FROM web_hand
             WHERE table_id = ?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = st.query_map(params![table_id, limit as i64], web_hand_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Hands an account played, newest first.
    pub fn web_hands_for(&self, account_id: i64, limit: usize) -> DbResult<Vec<WebHandRow>> {
        let mut st = self.conn.prepare(
            "SELECT id, table_id, game_id, game_version, seats, results, record, at FROM web_hand w
             WHERE EXISTS (SELECT 1 FROM json_each(w.seats) WHERE value = ?1) ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = st.query_map(params![account_id, limit as i64], web_hand_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn web_hand(&self, id: i64) -> DbResult<Option<WebHandRow>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, table_id, game_id, game_version, seats, results, record, at FROM web_hand WHERE id = ?1",
                [id],
                web_hand_row,
            )
            .optional()?)
    }

    /// A consistent snapshot of the whole database (nightly backups, web.md §9).
    pub fn backup_to(&self, path: &Path) -> DbResult<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| DbError(e.to_string()))?;
        }
        let _ = std::fs::remove_file(path);
        self.conn
            .execute("VACUUM INTO ?1", [path.to_string_lossy().to_string()])?;
        Ok(())
    }
}

fn web_hand_row(r: &rusqlite::Row) -> rusqlite::Result<WebHandRow> {
    Ok(WebHandRow {
        id: r.get(0)?,
        table_id: r.get(1)?,
        game_id: r.get(2)?,
        game_version: r.get(3)?,
        seats: r.get(4)?,
        results: r.get(5)?,
        record: r.get(6)?,
        at: r.get(7)?,
    })
}

#[cfg(test)]
mod web_tests {
    use super::*;

    #[test]
    fn accounts_sessions_chat_and_hands() {
        let mut db = Db::in_memory().unwrap();
        assert_eq!(db.schema_version().unwrap(), 2);
        let a = db
            .upsert_account("123", "Ann", None, true, Chips(10_000))
            .unwrap();
        assert_eq!(db.bankroll(a.player_id).unwrap(), Chips(10_000));
        let again = db
            .upsert_account("123", "Ann B", Some("abc"), true, Chips(10_000))
            .unwrap();
        assert_eq!(again.id, a.id);
        assert_eq!(
            db.bankroll(a.player_id).unwrap(),
            Chips(10_000),
            "no second deposit"
        );
        db.create_web_session(b"h", a.id, 100).unwrap();
        assert_eq!(db.web_session(b"h", 100).unwrap(), Some(a.id));
        db.delete_web_session(b"h").unwrap();
        assert_eq!(db.web_session(b"h", 100).unwrap(), None);
        let s = db.start_session(1).unwrap();
        db.create_web_table("t1", "Friday", a.id, "{}", s).unwrap();
        assert_eq!(db.open_web_tables().unwrap().len(), 1);
        let c = db.add_chat("t1", a.id, "hi").unwrap();
        assert_eq!(db.chat("t1", 10).unwrap()[0].text, "hi");
        db.delete_chat(c).unwrap();
        assert!(db.chat("t1", 10).unwrap().is_empty());
        let h = db
            .store_web_hand("t1", "ftq", 2, "[1,null]", "[100,-100]", b"rec")
            .unwrap();
        assert_eq!(db.web_hand(h).unwrap().unwrap().record, b"rec");
        assert_eq!(db.web_hands("t1", 5).unwrap().len(), 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_once() {
        let db = Db::in_memory().unwrap();
        assert_eq!(db.schema_version().unwrap(), MIGRATIONS.len());
    }

    #[test]
    fn bankroll_is_the_ledger_sum() {
        let mut db = Db::in_memory().unwrap();
        let p = db.player("zach", Chips(10_000)).unwrap();
        assert_eq!(
            db.player("zach", Chips(99)).unwrap(),
            p,
            "existing player isn't re-created"
        );
        db.ledger(
            p,
            None,
            &[
                (LedgerKind::BuyIn, Chips(-2_000)),
                (LedgerKind::CashOut, Chips(3_500)),
            ],
        )
        .unwrap();
        assert_eq!(db.bankroll(p).unwrap(), Chips(11_500));
        db.reset_bankroll(p, Chips(10_000)).unwrap();
        assert_eq!(db.bankroll(p).unwrap(), Chips(10_000));
        assert_eq!(
            db.players().unwrap(),
            vec![("zach".to_string(), Chips(10_000))]
        );
    }

    #[test]
    fn hands_and_notes_round_trip() {
        let mut db = Db::in_memory().unwrap();
        let p = db.player("zach", Chips(0)).unwrap();
        let s = db.start_session(u64::MAX).unwrap();
        let hand = StoredHand {
            session_id: s,
            player_id: p,
            game_id: "holdem_nl",
            game_version: 1,
            seat: 2,
            result: Chips(-150),
            record: &[1, 2, 3],
            notes: vec![
                Note {
                    decision_idx: 4,
                    ev_loss: Some(0.5),
                    se: 0.1,
                    flagged: true,
                },
                Note {
                    decision_idx: 9,
                    ev_loss: None,
                    se: 0.2,
                    flagged: false,
                },
            ],
        };
        db.store_hand(&hand).unwrap();
        assert_eq!(
            db.hand_records(p, Some("holdem_nl")).unwrap(),
            vec![("holdem_nl".into(), vec![1, 2, 3])]
        );
        assert!(db.hand_records(p, Some("ftq")).unwrap().is_empty());
        let notes = db.coach_notes(p, "holdem_nl").unwrap();
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[1].1.ev_loss, None, "off-menu loss stored as NULL");
    }

    #[test]
    fn database_survives_reopen() {
        let dir = std::env::temp_dir().join(format!("dc-test-{}", std::process::id()));
        let path = dir.join("dc.sqlite");
        {
            let mut db = Db::open(&path).unwrap();
            let p = db.player("zach", Chips(5_000)).unwrap();
            db.ledger(p, None, &[(LedgerKind::Hand, Chips(250))])
                .unwrap();
        }
        let db = Db::open(&path).unwrap();
        let p = db.player_id("zach").unwrap().unwrap();
        assert_eq!(db.bankroll(p).unwrap(), Chips(5_250));
        drop(db);
        let _ = std::fs::remove_dir_all(dir);
    }
}
