# Change Log

## Unreleased

- Phase 2 plans: PRD v0.2 draft (simulation program, full game catalog, web multiplayer) and
  tech plans `docs/tech/{sim-program,game-catalog,web}.md`.
- Fixed: clippy `unnecessary_sort_by` on Rust 1.98 (`bots::mccfr::best_response`).
- Whooh Ray for Booray now plays the house rules (game version 2): the player left of the dealer
  shows a card to set trump, In costs $1, Out is out for the game, the draw allows all 5 cards,
  unstunned players add 50¢ (the scooper too), and the stun cap grows $5 per earlier stun.
- Booray opens a second deck when the draw runs the first out (game version 3). Its cards lose to
  the same card from the first deck, and the table draws them with orange backs and peach faces.
  Engine: `Card::second()` / `is_second()` / `base()` mark a second deck's card;
  `SeatView::second_deck` lists their positions.

## 0.1.0 — NPI build (2026-10-10)

First working version, built milestone by milestone (PRD M0–M8):

- **M0** PRD, game-driver API contract, per-crate tech plans.
- **M1** Engine core (cards, RNG, `Game` trait, replay, conformance harness), Kuhn and Leduc,
  random bot, MCCFR + exact best response (Kuhn exploitability < 0.005).
- **M2** Hand evaluators (high, wild, A-6 low; exhaustive-checked), betting and pots,
  No-Limit Hold'em, MO-ISMCTS with strength/style, Dealer's Choice sessions, the `dc` CLI with coach.
- **M3** Parallel simulation with duplicate deals, CIs, compact records; measured data rates.
- **M4** Hold'em ranges and spot analysis.
- **M5** Follow the Queen; adding-a-game guide. Fixed: dead money (dealer ante) could return to the
  chooser through side-pot layering.
- **M6** SQLite persistence (players, ledger, hands, coach notes), saved sims and blueprints,
  TOML profiles with the `dc config` TUI.
- **M7** Player analytics: HUD stats, range charts, EV leak report (`dc stats`).
- **M8** CI, sustainment handbook, README.

Versions: `API_VERSION` 1; game versions kuhn 1, leduc 1, holdem_nl 1, ftq 1; DB schema 1.
