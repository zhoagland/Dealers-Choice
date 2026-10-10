//! `dc player`: manage named players and their bankrolls (FR-SS-1, FR-SS-3).

use clap::Subcommand;

use engine::Chips;
use storage::{DataRoot, Db, LedgerKind};

use crate::CliResult;

#[derive(Subcommand)]
pub enum PlayerCmd {
    /// Create a player with an opening deposit (dollars).
    Add {
        name: String,
        #[arg(long, default_value_t = 100.0)]
        deposit: f64,
    },
    /// List players and bankrolls.
    List,
    /// Add money to a player's bankroll (dollars).
    Deposit { name: String, amount: f64 },
    /// Set a player's bankroll to an amount (dollars), recorded as a reset.
    Reset { name: String, amount: f64 },
}

pub fn run(cmd: PlayerCmd, root: &DataRoot) -> CliResult {
    let mut db = Db::open(&root.db()).map_err(|e| e.to_string())?;
    let find = |db: &Db, name: &str| -> Result<i64, String> {
        db.player_id(name)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("no player named {name:?}"))
    };
    match cmd {
        PlayerCmd::Add { name, deposit } => {
            if db.player_id(&name).map_err(|e| e.to_string())?.is_some() {
                return Err(format!("{name:?} already exists"));
            }
            db.player(&name, Chips::dollars(deposit))
                .map_err(|e| e.to_string())?;
            println!("added {name} with {}", Chips::dollars(deposit));
        }
        PlayerCmd::List => {
            for (name, bankroll) in db.players().map_err(|e| e.to_string())? {
                println!("{name:<20} {bankroll:>12}");
            }
        }
        PlayerCmd::Deposit { name, amount } => {
            let id = find(&db, &name)?;
            db.ledger(id, None, &[(LedgerKind::Deposit, Chips::dollars(amount))])
                .map_err(|e| e.to_string())?;
            println!("{name}: {}", db.bankroll(id).map_err(|e| e.to_string())?);
        }
        PlayerCmd::Reset { name, amount } => {
            let id = find(&db, &name)?;
            db.reset_bankroll(id, Chips::dollars(amount))
                .map_err(|e| e.to_string())?;
            println!("{name}: {}", db.bankroll(id).map_err(|e| e.to_string())?);
        }
    }
    Ok(())
}
