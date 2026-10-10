//! `dc play`: a Dealer's Choice session, one human against bots (FR-SS-4, FR-UI-1).
//!
//! Flow: the chooser picks a game → the table buys everyone in → hands are played for the
//! game's length → busted players rebuy or sit out → cash out → the next seat chooses.
//!
//! Switching games at runtime goes through `games::registry::with_game` with a visitor:
//! `PlayOneGame::visit::<G>` is compiled once per game type.

use std::time::Duration;

use clap::Args;
use rand::Rng;
use rand::seq::IndexedRandom;

use bots::coach::review_hand;
use bots::ismcts::{Ismcts, IsmctsConfig};
use bots::{Budget, IsmctsBot, Policy, play_hand};
use engine::replay::replay;
use engine::rng::{Stream, derive_rng, rng_from_seed};
use engine::session::{SeatInfo, SeatKind, Table};
use engine::{Chips, Game, Seat};
use games::registry::{GAME_IDS, GameVisitor, with_game};

use crate::CliResult;

use crate::human::{HumanPolicy, print_review, read_line};

#[derive(Args)]
pub struct PlayArgs {
    /// Bot strength levels (1-10), one per bot seat, e.g. `7,5,3`.
    #[arg(long, value_delimiter = ',', default_value = "7,5,3")]
    pub bots: Vec<u8>,
    /// Your starting bankroll in dollars.
    #[arg(long, default_value_t = 100.0)]
    pub bankroll: f64,
    /// Always play this game instead of rotating dealer's choice.
    #[arg(long)]
    pub game: Option<String>,
    /// Games bots may choose from.
    #[arg(long, value_delimiter = ',', default_value = "holdem_nl")]
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

pub fn run(args: PlayArgs) -> CliResult {
    for id in args.pool.iter().chain(args.game.iter()) {
        if !GAME_IDS.contains(&id.as_str()) {
            return Err(format!("unknown game {id:?}; see `dc games`"));
        }
    }
    let seed = args.seed.unwrap_or_else(|| rand::rng().random());
    println!("Session seed {seed}  (rerun with --seed {seed} to replay)");

    let mut seats = vec![SeatInfo {
        name: "you".into(),
        kind: SeatKind::Human,
        bankroll: Some(Chips::dollars(args.bankroll)),
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
        };
        with_game(&id, None, visitor).map_err(|e| e.to_string())??;
        let you = &table.seats[0];
        println!(
            "\n=== your bankroll: {} ===",
            you.bankroll.unwrap_or_default()
        );
        if you.bankroll == Some(Chips::ZERO) {
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
}

impl GameVisitor<CliResult> for PlayOneGame<'_> {
    fn visit<G: Game>(self, game: G) -> CliResult {
        let table = self.table;
        let hands = table.begin_game(game.meta());
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
            if let (Some(h), false) = (human_seat, self.args.no_review) {
                let mut search = Ismcts::<G>::new(IsmctsConfig::default());
                let mut rng = rng_from_seed(self.seed ^ hand);
                let reviews = review_hand(
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
        }
        table.end_game();
        Ok(())
    }
}
