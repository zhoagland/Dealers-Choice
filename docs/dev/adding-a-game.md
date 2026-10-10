# Adding a Game

This guide takes you from "a game in the Poker Bible" to "playable in `dc play`, simulated by
`dc sim`, searched by the bots". Read `docs/api/game-driver.md` first for the contract. This page
is the practical procedure. Worked example: `crates/games/src/kuhn.rs` (≈200 lines of logic);
a full-size example is `holdem.rs`.

You touch exactly two places:
1. a new module `crates/games/src/<game>.rs`, plus `pub mod <game>;` in `games/src/lib.rs`;
2. one `match` arm and one id in `crates/games/src/registry.rs`.

Nothing in `engine`, `bots`, `sim`, or `cli` changes (FR-GD-6). If you find you need to change
them, the API is missing something: stop and fix the API deliberately (bump `API_VERSION`).

---

## Step 0: Pin down the rules

Write down, before coding, everything the Bible leaves ambiguous. Every Bible game so far had
gaps (e.g. FTQ: Q→Q, last-upcard Queen, wilds in low, who acts first). Put the decisions in the
module doc comment and in `docs/tech/games.md`. Each one later becomes a rule test.

Checklist of questions:
- Seats (min/max)? Deck size limits (7-card stud × 8 seats runs out)?
- Forced bets: ante, blinds, dealer ante, bring-in? Who pays?
- Betting structure (`engine::betting::Structure`) and the bot bet menu?
- Deal sequence: which cards, face up/down, in what order, to whom (folded seats skipped)?
- Who acts first on each street?
- How hands are evaluated (high / low / wild cards / numbers)?
- Showdown: who must show, who can muck, splits, declarations?
- `GameLength`: single hand (default) or orbit? `StakeModel`: bankroll or table stake?

## Step 1: Types

Follow the Kuhn layout:

```rust
pub enum Action { Deal(Card), Fold, CheckCall, RaiseTo(Chips), /* game-specific */ }
pub struct Config { /* stakes, structure, bet menu */ }   // #[serde(default)] + Default = Bible rules
pub struct State { setup, cards…, betting: Betting, queue of pending deals, history… }
pub struct Observation { seat, setup, own cards, public history, reveals… }
```

Rules of thumb:
- **Chance is an action.** Every dealt card is `Action::Deal(card)`, one per step. Keep a small
  queue of "where the next card goes" (`holdem.rs`: `Slot`).
- **Betting goes through `engine::betting::Betting`.** Never add or subtract chips yourself
  (conformance C9). Use `post_dead` (antes), `post_blind`, `start_street(index, first_seat)`,
  `menu`, `validate`, `apply`.
- **Pots go through `engine::pot`.** `side_pots` + `award_high` or `award_declared`.
- **Observation = exactly what that seat knows.** Hidden cards appear in the history only as
  "a card was dealt to seat X" (`PublicAction::HoleDealt`), so the history keeps its shape
  (perfect recall) without leaking. Hidden *decisions* (declarations) appear as
  "seat X has declared" until revealed.

## Step 2: The `Game` impl, method by method

| Method | What to do | Pitfalls |
|---|---|---|
| `new`, `meta` | Fill `GameMeta`: id (stable!), version 1, seats, stake model, length, Bible ref | The id is stored in records and blueprints; never rename it |
| `new_hand` | Build `Betting`, post forced bets, `start_street(0, first)`, fill the deal queue | Heads-up special cases (blind order) |
| `to_act` | Pending deal → `Chance`; hand over → `Terminal`; else `betting.to_act()` or your own phase (declarations) | Must never return `Seat` with no legal actions |
| `apply` | Deals: pop the queue. Bets: `betting.apply`. Then **advance**: when the queue is empty and betting is closed, move to the next street / phase | Write `advance` as a loop: an all-in can skip several streets |
| `legal_actions` | Chance: every undealt card. Seat: `betting.menu(seat, &config.bet_menu)` | Clear `out` first |
| `validate` | Chance: an undealt card. Seat: `betting.validate` (accepts any legal amount) | |
| `chance_outcomes` / `sample_chance` | Uniform over undealt cards. Override `sample_chance` to pick directly (fast) | Same distribution as `chance_outcomes` |
| `payoffs` | `winnings − contributed` per seat | Don't evaluate hands when only one seat is left (it may have no board) |
| `observe`, `observer` | Build the observation | Never include hidden cards of others |
| `determinize` | Sample others' hidden cards with `engine::game::sample_hidden_hands` (card removal included), sample hidden decisions uniformly, then rebuild with `engine::replay::replay_interleaved(deals, actions)` | Every sampled card must avoid every card the observer knows |
| `classify` | Map to `ActionClass` | Rollouts and stats depend on it |
| `eval_unit` | Big blind or ante | Bot temperature τ is in these units |
| text hooks | `describe_observation`, `describe_action`, `parse_action` (optional) | |

## Step 3: Conformance first, then rules

In the module's tests:

```rust
#[test]
fn conformance() {
    check_conformance(&MyGame::new(Config::default()),
        &Conformance { hands: 200, seat_counts: vec![2, 4, 7], ..Conformance::default() });
}
```

Run `cargo test -p games my_game`. The harness plays random hands and checks C1–C8. Failure
messages name the check:

| Message starts with | Usually means |
|---|---|
| `C1` | Chance probabilities don't sum to 1, or `sample_chance` returns something not in `chance_outcomes` |
| `C2` | `legal_actions` offered something `validate` rejects (often a menu bet outside the legal range) |
| `C3` | Chips created or destroyed: you did chip math outside `betting`/`pot` |
| `C4` | **Leak**: another seat's hidden info is in this seat's observation, or `determinize` ignores the sampler |
| `C5` | Observation lacks history (two decisions look identical) |
| `C6` | `determinize` produced a state whose observation differs; usually deal order rebuilt wrong |
| `C7` | `validate` rejects something `apply` accepted during play, or apply isn't deterministic |
| `C8` | A hand that never ends (`advance` didn't move on) |

Then write **rule tests**: one per decision from Step 0, using `engine::replay::replay` with an
exact action log (deal cards explicitly, so outcomes are known). See `holdem.rs` tests.

## Step 4: Register

```rust
// games/src/registry.rs
pub const GAME_IDS: &[&str] = &["kuhn", "leduc", "holdem_nl", "my_game"];
"my_game" => Ok(visitor.visit(MyGame::new(parse_config(id, config)?))),
```

`cargo test -p games registry` checks that every id resolves.

## Step 5: Try it

```
dc games                                    # listed?
dc sim --game my_game --bots random,random --deals 20000   # fast, should break even
dc sim --game my_game --bots ismcts:5:500,random --deals 300  # bot should win clearly
dc play --game my_game --bots 5,5
```

Then add it to `--pool` defaults if bots should choose it.

## Versioning

Any change to rules or to the `Action`/`Observation` shape → bump `meta.version`. Old hand
records and blueprints then refuse to load under the new version (that's intended).
