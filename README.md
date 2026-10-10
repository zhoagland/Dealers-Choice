<div align="center">
  <img src="images/logo.png" alt="Logo" width="80" height="80">
  <h3>Dealers Choice</h3>
  <p>A card-game engine for the home games in the Poker Bible: play against bots of chosen
  strength, get coached on your decisions, analyze spots, and run large simulations.</p>
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

## Documentation

| | |
|---|---|
| `docs/prd.md` | Requirements, milestones, open questions |
| `docs/api/game-driver.md` | The game-driver contract |
| `docs/dev/adding-a-game.md` | How to add a game |
| `docs/dev/handbook.md` | Running, debugging, versioning, releasing, CI, known limits, backlog |
| `docs/tech/` | Per-crate design and measured performance |

## License

GNU GPL v3 or later. See `LICENSE`.
