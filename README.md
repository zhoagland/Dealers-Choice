<div align="center">
  <img src="images/logo.png" alt="Logo" width="80" height="80">
  <h3>Dealers Choice</h3>
  <p>A card-game engine for the home games in the Poker Bible: play against bots of chosen
  strength, get coached on your decisions, analyze spots, run large simulations, and (coming next)
  play Dealer's Choice with friends in the browser.</p>
</div>

## What's here

- **Games:** No-Limit Texas Hold'em (orbit, $20 table stake), Follow the Queen (wild cards,
  hi/lo/both declarations), plus Kuhn and Leduc poker as solver test games. Rules follow
  `Rules/Poker Bible.typ`; add more with `docs/dev/adding-a-game.md`.
- **Bots:** ISMCTS (default, strength levels 1–10) and an MCCFR solver with exact
  exploitability measurement on small games. Baselines: random, calling station, maniac.
- **Coach:** after each hand, every decision you made scored by EV lost ± standard error;
  optional live hints.
- **Spot analysis:** "what's the best play here?" against opponent ranges (`dc spot`).
- **Simulation:** parallel, bit-reproducible, duplicate deals, confidence intervals, saved runs.
- **Your stats:** VPIP/PFR/AF/…, true range charts, and an EV leak report from your stored hands.
- **Persistence:** named players with bankrolls (SQLite ledger), saved profiles with a TUI editor.

## Quick start

Requires Rust (stable) and a C compiler (MSVC Build Tools on Windows) for bundled SQLite/zstd.

```sh
cargo build --release
./target/release/dc games
./target/release/dc player add me --deposit 100
./target/release/dc play --player me --bots 7,5,3          # f / c / r 6.50, or pick a number
./target/release/dc sim --profile quick-holdem
./target/release/dc spot config/spots/example.toml
./target/release/dc stats --player me
./target/release/dc config                                  # edit saved profiles
```

## Roadmap (Phase 2)

Three tracks run in parallel once the v1 build is merged. Requirements: `docs/prd.md` §2.1, §5.9–5.11, §9.1.

| Track | What | Plan |
|---|---|---|
| **Simulations** | Background campaigns on your PC with HTML reports: game balance for every Bible game, a bot strength ladder, starting-hand and decision studies, solver blueprints | `docs/tech/sim-program.md` |
| **Full catalog** | All 50 Bible games, poker and non-poker, built by mechanic family (API v2 adds multi-hand games, stuns, jokers, new evaluators) | `docs/tech/game-catalog.md` |
| **Web multiplayer** | Play Dealer's Choice with friends over the internet: Discord sign-in, mobile-friendly table, chat, hand history, self-hosted, also as a Discord Activity | `docs/tech/web.md`, deploy: `deploy/README.md` |

### Game catalog status

| Family | Games | Status |
|---|---|---|
| Texas Hold'em | Texas Hold'em | ✅ |
| Stud | Follow the Queen | ✅ |
| Foundation | API v2: multi-hand games, stuns, jokers, table view | ✅ (G0) |
| Omaha | Pot Limit Omaha, 5 Card Omaha, Courchevel, Red River, PLO Double Board Bomb Pot | ✅ (G1) |
| Board shapes | Elevator, Wheel of Death, Spit, Industrial Elevator, Iron Cross | ✅ (G2) |
| Hold'em twists | 6+ Hold'em, Showtime Hold'em, Reverse Poker, Split Hold'em | ✅ (G3) |
| Draw & discard | 5 Card Draw, Triple Draw, 2-7 Single Draw, 2-7 Triple Draw, Badugi, 3 Blind Mice, Crazy Pineapple, Mama's Favorite, Swap Hold'em, Fusion | ✅ (G4) |
| Pass & in/out | Pass the Trash, Man or Mouse, 43 | ✅ (G5) |
| Stud variants | 7 Card Stud, Razz, Baseball, Chicago High, Chicago Low, Low in the Hole, Push, Bullet, Bid | ✅ (G6) |
| Indian | Indian Poker, Indian Texas Hold'em | ✅ (G7) |
| Multi-hand & stuns | Guts Leg, 3-2-1 Drop, 3-5-7, Scare-a-moosh, Tempest Hold'em | ✅ (G8) |
| Numbers & house | Blackjack, 7/27, 5.5/21.5, Numbers | ✅ (G9) |
| Trick-taking | Whooh Ray for Booray | ✅ (G10) |

Where the Bible is ambiguous, each game's rule decisions are listed in `docs/tech/game-catalog.md` §5 for
review.

## Documentation

| | |
|---|---|
| `docs/prd.md` | Requirements, milestones, open questions |
| `docs/api/game-driver.md` | The game-driver contract |
| `docs/dev/adding-a-game.md` | How to add a game |
| `docs/dev/handbook.md` | Running, debugging, versioning, releasing, CI, known limits, backlog |
| `docs/tech/` | Per-crate design, measured performance, and the Phase 2 plans |
| `ChangeLog.md` | What changed in each release |

## License

GNU GPL v3 or later. See `LICENSE`.
