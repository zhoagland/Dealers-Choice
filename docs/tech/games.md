# Tech Plan — `games`

Implements PRD §7. Each game is one module that implements `engine::Game`
(contract: `docs/api/game-driver.md`). Rules source: the Poker Bible section in `meta.rules_ref`.

```
games/src/
├── lib.rs
├── registry.rs        id → concrete type (API doc §7)
├── kuhn.rs            M1
├── leduc.rs           M1 (test fixture for MCCFR / best response; not user-facing)
├── holdem/            M2
│   ├── mod.rs         Game impl
│   ├── state.rs
│   ├── action.rs
│   └── observation.rs
└── ftq/               M5
    ├── mod.rs
    ├── state.rs
    ├── wild.rs        wild-rank tracking
    ├── order.rs       best-board-showing action order
    └── declare.rs
```

Standard per-game files: `state.rs` (the full State), `action.rs`, `observation.rs`, plus
`mod.rs` wiring them into the trait. The "adding a game" guide is built around this layout.

## Common action shape

Each game defines its own `Action` enum, usually of this shape:
```rust
pub enum Action {
    Deal(Card),          // chance
    Fold,
    CheckCall,
    RaiseTo(Chips),
    // game-specific variants, e.g. Declare(Side)
}
```
`legal_actions` emits the abstract menu from `Config::bet_menu` (FR-BT-7), clamped by
`BettingRound::options`. `validate` accepts any amount `options` allows.

---

## Kuhn poker (M1)

- Deck {J, Q, K} (represented as `Card`s of one suit), 2 seats, ante 1, one card each, one bet size of 1,
  at most one bet per hand.
- Action history: ≤ 3 player actions. State and observation fit in a few bytes.
- Purpose: correctness of MCCFR (value −1/18 for P1, `P(bet|K) = 3·P(bet|J)`, Q never bets
  as P1's first action) and a measurement of ISMCTS exploitability.
- Uses chips of 1 unit; `eval_unit = 1`.

## Leduc poker (M1, test fixture)

- Deck {J, Q, K} × 2 suits, 2 seats, ante 1, one private card each, one public card after round 1.
  Fixed-limit raises: 2 in round 1, 4 in round 2, at most 2 raises per round. Pair with the board beats
  everything; otherwise high card.
- On the order of 10³ information sets → exact best response and exploitability are cheap. Catches
  multi-round MCCFR bugs that Kuhn can't.

## No-Limit Texas Hold'em (M2)

**Config** (defaults from the Bible): SB 50¢, BB 100¢, stake model `TableStake(2000¢)`, dealer ante
200¢ on the first hand of the game, posted by the chooser. Bot bet menu: `[MinRaise, Pot(1/3), Pot(2/3), Pot(1), AllIn]`.

**State**
```rust
pub struct HoldemState {
    setup: HandSetup,
    hole: [[Option<Card>; 2]; 9],
    board: SmallVec<[Card; 5]>,
    deck: CardSet,                    // remaining
    street: Street,                   // Preflop, Flop, Turn, River, Showdown
    betting: BettingRound,
    contributions: [Chips; 9],
    folded: u16,                      // bitmask
    history: SmallVec<[Action; 64]>,  // full log, chance included
    deal_queue: DealQueue,            // which slot the next chance card fills
}
```
- `to_act`: Chance while `deal_queue` is non-empty, otherwise `betting.next_to_act()`, and when a street
  closes, advance the street and refill `deal_queue`. Everyone all-in ⇒ run out the board via
  chance steps.
- Preflop order: left of BB. Postflop: first live seat left of the button (Bible: "closest
  to the left dealer").
- Showdown (Bible): the last aggressor shows first; the others show or muck. **Bots always show if
  they can win and muck if they can't.** Showing order then affects only what's revealed, never
  who wins, so it's handled in observation/rendering, not as a player action. That keeps the game tree
  smaller. Mucked cards stay hidden in observations (FR-EC-5).

**Observation**
```rust
pub struct HoldemObs {
    seat: Seat, hole: [Card; 2], board: SmallVec<[Card; 5]>,
    public_history: SmallVec<[PubAction; 64]>,   // Deal(board card) visible, Deal(hole) → DealHidden(seat)
    stacks: [Chips; 9], button: Seat, revealed: [Option<[Card; 2]>; 9],
}
```
`DealHidden(seat)` keeps the history aligned (perfect recall) without leaking the card.

**Determinize**: sample opponents' hole cards via `HiddenSampler` (ranges in spot mode,
otherwise uniform). Then replay `public_history` with those cards substituted, using `engine::replay`.

**Rule tests**: side pot with 3-way all-in of different sizes; short all-in doesn't reopen;
heads-up blind order (button = SB, acts first preflop and last postflop); split board.

## Follow the Queen (M5)

**Config**: dealer ante 200¢ only (no other antes or bring-in); `SpreadLimit { min_inc: 50,
max_inc: 200, step: 50, max_raises: 4 }`; stake model `Bankroll`; hi/lo with declaration.

**Deal sequence**: 2 down → 3rd street up → bet → 4th up → bet → 5th up → bet → 6th up → bet →
7th down → bet → declare → showdown. FTQ is capped at **7 seats** (7 × 7 = 49 ≤ 52), so the deck can't run out (G-1, resolved).

**Wild tracking (`wild.rs`)**, PRD §7:
```rust
pub enum WildState { None, AwaitingNextUpcard, Rank(Rank) }
```
- An upcard that's a Queen ⇒ `AwaitingNextUpcard` (any earlier wild is cancelled).
- The next upcard dealt, in dealing order across seats and streets ⇒ `Rank(r)`. If that card is itself
  a Queen ⇒ `Rank(Queen)`: Queens become wild (G-2, resolved). A *later* face-up Queen again
  ⇒ `AwaitingNextUpcard`.
- Downcards never trigger anything and never set the wild rank.
- At showdown, `AwaitingNextUpcard` (a Queen was the last upcard) ⇒ **no wild**.
- Queens are not wild by default (only via the Q→Q case above).
- The wild state is public, so it goes in the observation.

**Action order (`order.rs`)**: from 3rd street onward, the first to act is the best board showing.
Visible upcards are evaluated with the current wild rank, counting pairs/trips/quads/five of a kind only
(no partial straights or flushes); ties go to the seat closest to the dealer's left (A8). Third street:
everyone has 1 upcard → highest card (wild = highest). Uses a dedicated
`engine::eval::partial_board` helper. Not the full evaluator, since partial hands can't make straights.

**Declaration (`declare.rs`)**: after the 7th-street bet, each live seat in order takes
`Action::Declare(Side)` with `Side ∈ {High, Low, Both}`. Observations show other seats only as
`declared: bool` until all have declared, then reveal (API doc §1 rule 2). Payout via
`engine::pot` with declarations.

**Hand evaluation**: best high = `high_wild(naturals, w)`; best low = `low_a6_wild(naturals, w)`,
each over the seat's 7 cards. Both are computed only at showdown and for bot rollouts.

**Rule tests**: Queen as the last upcard ⇒ no wilds; Q→Q ⇒ Queens wild; Q→Q→…→Q→5 ⇒ fives wild; a Both declarer
tying low loses everything; a side nobody declared rolls to the other side.

## Open questions
- ~~G-1~~ Resolved: FTQ max 7 seats.
- ~~G-2~~ Resolved: Q followed by Q ⇒ Queens wild.
- **G-3** Hold'em: can a player choose to show a losing hand (for table image)? It doesn't affect bots;
  the CLI could offer it.
