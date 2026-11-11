# Tech Plan — `cli` (`dc` binary)

Implements FR-UI-1, FR-UI-2.

## Commands

| Command | Purpose | Milestone |
|---|---|---|
| `dc play --seats 6 --bots 7,7,5,5,3` | Dealer's Choice session vs bots | M2 |
| `dc spot <spot.toml>` | Ranked actions with EV ± SE | M4 |
| `dc sim <run.toml>` | Batch simulation | M3 |
| `dc bench-sim` | Data-rate and throughput measurement | M3 |
| `dc train --game kuhn --iters 1e6` | MCCFR training → blueprint | M1 |
| `dc exploit --game kuhn --bot ismcts:7` | Exploitability of a bot (small games) | M1 |
| `dc replay <hand-id or file>` | Step through a recorded hand | M2 |
| `dc player add|list|deposit|reset` | Profile and bankroll management | M6 |

## Play loop

```
Session<G>  ──► hand loop ──► to_act():
                               Seat(human) → render(obs) → read input → validate → apply
                               Seat(bot)   → policy.act(obs, legal)            → apply
                               Chance      → sample(chance_outcomes, rng)      → apply
                               Terminal    → payoffs → ledger → coach (background)
```

- **Rendering:** via the `Game` text hooks (`describe_observation`, `describe_action`,
  `parse_action`; see the API doc §9). Humans pick a numbered menu item or type `f` / `c` / `r 6.50`.
- **Input grammar:** `f`, `c` (check/call), `r 6.50` (raise to), `b 2` (bet), `h`/`l`/`hl`
  (declare), meta commands `:hints on|off`, `:review`, `:quit`. Off-menu amounts are allowed;
  `validate` gives the error message.
- **Simultaneous declarations:** the CLI collects the human's declaration and *then* advances the bots'
  declarations, with no output between them. That way, order and timing reveal nothing (API doc §1 rule 2).
- **Game choice:** when the human is the chooser, show the registered games. Bot choosers pick
  uniformly from `config/session.toml: bot_game_pool`.

## Path to the web UI (FR-UI-2)

All play-loop messages are `serde` types: `ServerMsg::{Observation, Prompt{legal}, HandResult,
CoachReport}`, `ClientMsg::{Act(action_json), Command}`. The CLI uses them internally
(render ← `ServerMsg`, input → `ClientMsg`), so a future `server` crate (axum + WebSocket)
reuses the same loop with a different transport. Per-seat `Observation`s mean a server never
has to filter what each client sees after the fact; that's already done by the engine.
