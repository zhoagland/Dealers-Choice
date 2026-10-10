//! SQLite app state: players, the bankroll ledger, human hands, coach notes (FR-ST-1, FR-SS-1).
//! Schema: `migrations/*.sql`; design: `docs/tech/storage.md` §2.
//!
//! Plain `rusqlite` with SQL written out, no ORM, so what's stored is visible in one place.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use engine::Chips;

/// Migrations in order. `include_str!` compiles the SQL files into the binary.
const MIGRATIONS: &[&str] = &[include_str!("../migrations/0001_init.sql")];

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

/// A human hand to store, with its coach review.
pub struct StoredHand<'a> {
    pub session_id: i64,
    pub player_id: i64,
    pub game_id: &'a str,
    pub game_version: u32,
    pub seat: u8,
    pub result: Chips,
    pub record: &'a [u8],
    /// (decision index, EV loss or None for off-menu, SE, flagged)
    pub notes: Vec<(usize, Option<f64>, f64, bool)>,
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
        for &(idx, loss, se, flagged) in &h.notes {
            tx.execute(
                "INSERT INTO coach_note (hand_id, decision_idx, ev_loss, se, flagged) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, idx as i64, loss, se, flagged],
            )?;
        }
        tx.commit()?;
        Ok(id)
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

    /// All coach notes for a player's hands in one game: (hand id, decision idx, loss, se, flagged).
    pub fn coach_notes(
        &self,
        player_id: i64,
        game_id: &str,
    ) -> DbResult<Vec<(i64, usize, Option<f64>, f64, bool)>> {
        let mut st = self.conn.prepare(
            "SELECT n.hand_id, n.decision_idx, n.ev_loss, n.se, n.flagged FROM coach_note n
             JOIN hand h ON h.id = n.hand_id WHERE h.player_id = ?1 AND h.game_id = ?2 ORDER BY n.hand_id, n.decision_idx",
        )?;
        let rows = st.query_map(params![player_id, game_id], |r| {
            Ok((
                r.get(0)?,
                r.get::<_, i64>(1)? as usize,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
            ))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
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
            notes: vec![(4, Some(0.5), 0.1, true), (9, None, 0.2, false)],
        };
        db.store_hand(&hand).unwrap();
        assert_eq!(
            db.hand_records(p, Some("holdem_nl")).unwrap(),
            vec![("holdem_nl".into(), vec![1, 2, 3])]
        );
        assert!(db.hand_records(p, Some("ftq")).unwrap().is_empty());
        let notes = db.coach_notes(p, "holdem_nl").unwrap();
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[1].2, None, "off-menu loss stored as NULL");
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
