# Change Log

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
