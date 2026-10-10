//! `dc play`: a Dealer's Choice session, one human against bots (FR-SS-4, FR-UI-1).
//!
//! Flow: the chooser picks a game → the table buys everyone in → hands are played for the
//! game's length → busted players rebuy or sit out → cash out → the next seat chooses.
//!
//! Persistence (FR-SS-1, FR-ST-1): the human is a named player in `dc.sqlite`. Their bankroll
//! comes from the ledger; every money movement is written as ledger rows as it happens, and every
//! hand they play is stored (replayable record + coach notes) for later analysis. `--no-save`
//! plays an ephemeral session instead.
//!
//! Switching games at runtime goes through `games::registry::with_game` with a visitor:
//! `PlayOneGame::visit::<G>` is compiled once per game type.

use std::time::Duration;

use clap::Args;
use rand::Rng;
use rand::seq::IndexedRandom;

use bots::coach::{DecisionReview, review_hand};
use bots::ismcts::{Ismcts, IsmctsConfig};
use bots::{Budget, IsmctsBot, Policy, play_hand};
use engine::replay::replay;
use engine::rng::{Stream, derive_rng, rng_from_seed};
use engine::session::{SeatInfo, SeatKind, SessionEvent, Table};
use engine::{Chips, Game, Seat, StakeModel};
use games::registry::{GAME_IDS, GameVisitor, with_game};
use sim::encoding::{HandRecord, encode};
use storage::{DataRoot, Db, LedgerKind, StoredHand};

use crate::CliResult;
use crate::human::{HumanPolicy, print_review, read_line};

#[derive(Args)]
pub struct PlayArgs {
    /// Your player name (bankroll and history are kept per name).
    #[arg(long, default_value = "you")]
    pub player: String,
    /// Opening bankroll in dollars, used only when the player is new.
    #[arg(long, default_value_t = 100.0)]
    pub bankroll: f64,
    /// Don't read or write the database: an ephemeral session.
    #[arg(long)]
    pub no_save: bool,
    /// Bot strength levels (1-10), one per bot seat, e.g. `7,5,3`.
    #[arg(long, value_delimiter = ',', default_value = "7,5,3")]
    pub bots: Vec<u8>,
    /// Always play this game instead of rotating dealer's choice.
    #[arg(long)]
    pub game: Option<String>,
    /// Games bots may choose from.
    #[arg(long, value_delimiter = ',', default_value = "holdem_nl,ftq")]
    pub pool: Vec<String>,
    /// Show live hints (ranked actions with EV) before each of your decisions.
    #[arg(long)]
    pub hints: bool,
    /// Skip the post-hand coach review.
    #[arg(long)]
    pub no_review: bool,
    /// Coach review playouts per action (deterministic).
    #[arg(long, default_value_t = 15_000)]
    pub review_playouts: u32,
    /// Session seed (random if omitted). The same seed and the same inputs replay the session.
    #[arg(long)]
    pub seed: Option<u64>,
    /// Stop after this many games.
    #[arg(long, default_value_t = 1_000)]
    pub max_games: u32,
    /// Most milliseconds a bot may think per decision. Bots use one core, only on their own
    /// turn; nothing runs while you're deciding.
    #[arg(long, default_value_t = 500)]
    pub think_ms: u64,
}

/// The database connection plus the ids this session writes under.
struct Persist {
    db: Db,
    player_id: i64,
    session_id: i64,
}

impl Persist {
    /// Turn the table's events for the human (table index 0) into ledger rows.
    ///
    /// With a table stake, money moves at buy-in, rebuy, and cash-out; hand results only move
    /// chips on the table. With bankroll play, each hand's result moves the bankroll directly.
    fn record(&mut self, table: &mut Table, stake: StakeModel) -> CliResult {
        let mut rows = Vec::new();
        for e in table.drain_events() {
            let row = match (e, stake) {
                (SessionEvent::BuyIn { seat: 0, amount }, StakeModel::TableStake(_)) => {
                    Some((LedgerKind::BuyIn, -amount))
                }
                (SessionEvent::Rebuy { seat: 0, amount }, StakeModel::TableStake(_)) => {
                    Some((LedgerKind::Rebuy, -amount))
                }
                (SessionEvent::CashOut { seat: 0, amount }, StakeModel::TableStake(_)) => {
                    Some((LedgerKind::CashOut, amount))
                }
                (SessionEvent::HandResult { seat: 0, delta }, StakeModel::Bankroll) => {
                    Some((LedgerKind::Hand, delta))
                }
                _ => None,
            };
            rows.extend(row);
        }
        self.db
            .ledger(self.player_id, Some(self.session_id), &rows)
            .map_err(|e| e.to_string())
    }
}

pub fn run(args: PlayArgs, root: &DataRoot) -> CliResult {
    for id in args.pool.iter().chain(args.game.iter()) {
        if !GAME_IDS.contains(&id.as_str()) {
            return Err(format!("unknown game {id:?}; see `dc games`"));
        }
    }
    let seed = args.seed.unwrap_or_else(|| rand::rng().random());
    println!("Session seed {seed}  (rerun with --seed {seed} to replay)");

    let (bankroll, mut persist) = if args.no_save {
        (Chips::dollars(args.bankroll), None)
    } else {
        let mut db = Db::open(&root.db()).map_err(|e| e.to_string())?;
        let player_id = db
            .player(&args.player, Chips::dollars(args.bankroll))
            .map_err(|e| e.to_string())?;
        let session_id = db.start_session(seed).map_err(|e| e.to_string())?;
        let bankroll = db.bankroll(player_id).map_err(|e| e.to_string())?;
        println!(
            "Player {:?}: bankroll {bankroll}  (saved in {})",
            args.player,
            root.db().display()
        );
        (
            bankroll,
            Some(Persist {
                db,
                player_id,
                session_id,
            }),
        )
    };
    if bankroll <= Chips::ZERO {
        return Err(format!(
            "{}'s bankroll is {bankroll}; deposit with `dc player deposit`",
            args.player
        ));
    }

    let mut seats = vec![SeatInfo {
        name: args.player.clone(),
        kind: SeatKind::Human,
        bankroll: Some(bankroll),
        stack: Chips::ZERO,
        sitting_out: false,
    }];
    for (i, &level) in args.bots.iter().enumerate() {
        seats.push(SeatInfo {
            name: format!("bot{}:{}", i + 1, level.clamp(1, 10)),
            kind: SeatKind::Bot,
            bankroll: None,
            stack: Chips::ZERO,
            sitting_out: false,
        });
    }
    let mut table = Table::new(seats, Chips::dollars(100.0));
    let mut chooser_rng = derive_rng(seed, 0, Stream::Custom(1));
    let mut hand_counter = 0u64;

    for _ in 0..args.max_games {
        let id = choose_game(&table, &args, &mut chooser_rng)?;
        let visitor = PlayOneGame {
            table: &mut table,
            args: &args,
            seed,
            hand_counter: &mut hand_counter,
            persist: persist.as_mut(),
        };
        with_game(&id, None, visitor).map_err(|e| e.to_string())??;
        let you = table.seats[0].bankroll.unwrap_or_default();
        println!("\n=== your bankroll: {you} ===");
        if let Some(p) = &persist {
            // The ledger and the in-memory table must agree; a mismatch is an accounting bug.
            let stored = p.db.bankroll(p.player_id).map_err(|e| e.to_string())?;
            if stored != you {
                return Err(format!(
                    "ledger says {stored} but the table says {you}: please report this"
                ));
            }
        }
        if you == Chips::ZERO {
            println!("You're out of money. Thanks for playing.");
            break;
        }
    }
    Ok(())
}

/// The chooser picks the next game: the human from a menu, bots at random from the pool.
fn choose_game(table: &Table, args: &PlayArgs, rng: &mut engine::DetRng) -> Result<String, String> {
    if let Some(id) = &args.game {
        return Ok(id.clone());
    }
    let n = table.seats.len() as u8;
    let fits = |id: &str| {
        with_game(id, None, SeatRange)
            .map(|r| r.contains(&n))
            .unwrap_or(false)
    };
    let chooser = &table.seats[table.chooser];
    if chooser.kind == SeatKind::Human {
        let options: Vec<&str> = GAME_IDS.iter().copied().filter(|id| fits(id)).collect();
        println!("\nYour choice of game:");
        for (i, id) in options.iter().enumerate() {
            println!("  [{}] {id}", i + 1);
        }
        loop {
            let input = read_line("game> ");
            if let Ok(k) = input.parse::<usize>()
                && (1..=options.len()).contains(&k)
            {
                return Ok(options[k - 1].to_string());
            }
            if options.contains(&input.as_str()) {
                return Ok(input);
            }
        }
    }
    let pool: Vec<&String> = args.pool.iter().filter(|id| fits(id)).collect();
    let id = pool
        .choose(rng)
        .ok_or("no game in --pool fits this many seats")?;
    println!("\n{} chooses {id}", chooser.name);
    Ok((*id).clone())
}

/// Visitor that reports a game's seat range.
struct SeatRange;
impl GameVisitor<std::ops::RangeInclusive<u8>> for SeatRange {
    fn visit<G: Game>(self, game: G) -> std::ops::RangeInclusive<u8> {
        game.meta().seats.clone()
    }
}

/// Visitor that plays one complete game (all of its hands).
struct PlayOneGame<'a> {
    table: &'a mut Table,
    args: &'a PlayArgs,
    seed: u64,
    hand_counter: &'a mut u64,
    persist: Option<&'a mut Persist>,
}

impl GameVisitor<CliResult> for PlayOneGame<'_> {
    fn visit<G: Game>(mut self, game: G) -> CliResult {
        let table = self.table;
        let stake = game.meta().stake_model;
        let hands = table.begin_game(game.meta());
        if let Some(p) = self.persist.as_deref_mut() {
            p.record(table, stake)?;
        }
        println!(
            "--- {} ({} hand{}) ---",
            game.meta().display_name,
            hands,
            if hands == 1 { "" } else { "s" }
        );

        while let Some((setup, map)) = table.next_hand() {
            *self.hand_counter += 1;
            let hand = *self.hand_counter;

            // One policy per hand seat. `Box<dyn Policy<G>>` lets the human and bots share a Vec.
            let mut policies: Vec<Box<dyn Policy<G>>> = map
                .iter()
                .map(|&t| -> Box<dyn Policy<G>> {
                    let info = &table.seats[t];
                    match info.kind {
                        SeatKind::Human => Box::new(HumanPolicy::<G>::new(
                            self.args.hints,
                            Budget::Playouts(5_000),
                        )),
                        SeatKind::Bot => {
                            let level = info
                                .name
                                .rsplit(':')
                                .next()
                                .and_then(|l| l.parse().ok())
                                .unwrap_or(5);
                            let mut bot = IsmctsBot::<G>::preset(level, true);
                            // Strong presets would think longer than the table's cap (PRD A6, --think-ms).
                            if let Budget::Time(t) = bot.strength.budget {
                                bot.strength.budget =
                                    Budget::Time(t.min(Duration::from_millis(self.args.think_ms)));
                            }
                            Box::new(bot)
                        }
                    }
                })
                .collect();
            let mut seats: Vec<&mut dyn Policy<G>> = policies
                .iter_mut()
                // `as` shortens the trait object's lifetime from 'static to this borrow; `&mut` is
                // invariant, so Rust won't do that implicitly when collecting into a Vec.
                .map(|p| p.as_mut() as &mut dyn Policy<G>)
                .collect();
            let mut deal = derive_rng(self.seed, hand, Stream::Deal);
            let mut rngs: Vec<_> = (0..map.len())
                .map(|i| derive_rng(self.seed, hand, Stream::Seat(i as u8)))
                .collect();
            let result = play_hand(&game, &setup, &mut seats, &mut deal, &mut rngs);

            // Show the end of the hand from the human's seat.
            let human_seat = map
                .iter()
                .position(|&t| table.seats[t].kind == SeatKind::Human)
                .map(|i| Seat(i as u8));
            let end = replay(&game, &setup, &result.actions).map_err(|e| e.to_string())?;
            if let Some(h) = human_seat {
                println!("\n{}", game.describe_observation(&game.observe(&end, h)));
            }
            for (i, &t) in map.iter().enumerate() {
                println!(
                    "  {:<10} {:>+9}",
                    table.seats[t].name,
                    result.payoffs[i].to_string()
                );
            }
            let mut reviews: Vec<DecisionReview<G::Action>> = Vec::new();
            if let (Some(h), false) = (human_seat, self.args.no_review) {
                let mut search = Ismcts::<G>::new(IsmctsConfig::default());
                let mut rng = rng_from_seed(self.seed ^ hand);
                reviews = review_hand(
                    &game,
                    &setup,
                    &result.actions,
                    h,
                    &mut search,
                    Budget::Playouts(self.args.review_playouts),
                    &mut rng,
                );
                print_review(&game, &reviews);
            }
            if let (Some(p), Some(h)) = (self.persist.as_deref_mut(), human_seat) {
                let record = encode(&HandRecord::new(
                    game.meta().id,
                    game.meta().version,
                    hand,
                    0,
                    map.iter().map(|&t| t as u8).collect(),
                    setup.clone(),
                    result.actions.clone(),
                ));
                let stored = StoredHand {
                    session_id: p.session_id,
                    player_id: p.player_id,
                    game_id: game.meta().id,
                    game_version: game.meta().version,
                    seat: h.0,
                    result: result.payoffs[h.index()],
                    record: &record,
                    notes: reviews
                        .iter()
                        .map(|r| (r.step, r.ev_loss, r.se, r.flagged))
                        .collect(),
                };
                p.db.store_hand(&stored).map_err(|e| e.to_string())?;
            }
            table.settle_hand(&map, &result.payoffs);

            for t in table.busted() {
                if table.seats[t].kind == SeatKind::Bot {
                    let _ = table.rebuy(t);
                    continue;
                }
                let answer = read_line("You're busted. Rebuy? [y/N] ");
                if answer.eq_ignore_ascii_case("y") {
                    match table.rebuy(t) {
                        Ok(a) => println!("  rebought for {a}"),
                        Err(e) => {
                            println!("  can't rebuy: {e:?}");
                            table.sit_out(t);
                        }
                    }
                } else {
                    table.sit_out(t);
                }
            }
            if let Some(p) = self.persist.as_deref_mut() {
                p.record(table, stake)?;
            }
        }
        table.end_game();
        if let Some(p) = self.persist.as_deref_mut() {
            p.record(table, stake)?;
        }
        Ok(())
    }
}
