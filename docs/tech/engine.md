# Tech Plan — `engine`

Implements: FR-EC-1…6, FR-SS-1…5 (logic; persistence is in `storage`), NFR-1, NFR-2, NFR-5.

## 1. Why replace the current `src/`

| Current | Problem | Replacement |
|---|---|---|
| `Rank::Ace = 1`, ace-low only | High poker needs ace-high, A-6 low needs ace-low, and straights need both (A-2-3-4-5) | Internal ace-high ranks 0..=12; evaluators remap |
| `HandRanking`: Quads > Straight Flush | Wrong per the Bible | `Category` enum in house order (§3) |
| `compare_hands` compares only the top card | Kickers ignored; two-pair-vs-two-pair wrong | `HandValue(u32)` encodes category + kickers; plain `>` compares |
| `Card` not `Copy`, `Vec<Card>` hands | Allocation per hand; awkward borrows | `Card(u8)`: `Copy`; `CardSet(u64)` bitmask |
| `PokerGame::BlackJack(GameRules)` in core | Game rules in engine types | Games live in `games` behind the `Game` trait |

## 2. Modules

```
engine/src/
├── lib.rs
├── card.rs        Card, Rank, Suit, CardSet, Deck
├── eval/
│   ├── mod.rs     HandValue, Category, Evaluator trait
│   ├── high.rs    high evaluator, natural + wild
│   ├── low_a6.rs  ace-to-six low, natural + wild
│   └── oracle.rs  brute-force reference evaluators (test-only, cfg(test) or feature "oracle")
├── betting.rs     BettingRound, Structure, BetOptions
├── pot.rs         contributions, side pots, hi/lo split, odd chip
├── game.rs        Game trait + core types (see docs/api/game-driver.md)
├── replay.rs      build State from setup + action log
├── session.rs     Dealer's Choice game flow (GameLength), stake model, rebuys
├── rng.rs         DetRng, seed derivation
└── testing/       conformance::<G>(), random walks, invariant checks
```

## 3. Cards and evaluation

### Representation
```rust
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Card(u8);        // index = suit * 16 + rank,  rank 0 = Two … 12 = Ace
pub struct CardSet(u64);    // bit i set ⇔ Card(i) present
```
Placing each suit in its own 16-bit lane means `(set >> (16 * suit)) & 0x1FFF` is that suit's
13-bit **rank mask**, with no loop. Flushes become `popcount(suit_mask) >= 5`. Straights are a
lookup in an 8192-entry table indexed by the rank mask. This is the standard bitboard trick from
fast evaluators, and it's still readable.

`Rank` and `Suit` stay as `enum`s for display, parsing ("As", "Td") and `match` readability, with
`From` conversions to and from `Card`.

### HandValue
```rust
pub struct HandValue(u32);  // [category:4][r1:4][r2:4][r3:4][r4:4][r5:4], higher = better
pub enum Category {          // house order, PRD §4
    HighCard, Pair, TwoPair, Trips, Straight, Flush, FullHouse,
    Quads, StraightFlush, FiveOfAKind, RoyalFlush,
}
```
Kickers are packed in significance order, so `HandValue` comparison is one integer compare.
**Low evaluators also return "higher = better"** (encoded inverted), so pot code never needs to
know which direction a hand is ranked in.

### High evaluator (natural, up to 7 cards)
Direct evaluation from the rank masks and per-rank counts. No looping over the 21 five-card
subsets. Order of checks: flush suit → straight flush/royal; rank counts → quads, full house,
flush, straight, trips, two pair, pair, high. Target ≥ 10 M evals/s (NFR-3). Expected 20–50 M on a modern core.

### High evaluator with wilds (FTQ)
Input: natural cards `n` and wild count `w` (cards of the wild rank are removed from `n`). Wilds
may become **any card, including duplicates** of cards already held. That's needed for five of a kind and
matches standard wild rules.

Algorithm: check categories top-down and return the first one achievable, constructing the
best kickers:
- Royal: some suit where `|{T,J,Q,K,A}_s ∩ n| + w ≥ 5`.
- Five of a kind: `max_count + w ≥ 5`.
- Straight flush: for each suit, for each window from high to low, `5 − |window ∩ n_s| ≤ w`.
- …continue down to high card. Leftover wilds become aces as kickers.

**Correctness strategy (falsifiable).** `oracle.rs` brute-forces every substitution of each wild
with each of the 52 cards (duplicates allowed) and takes the max of the natural evaluator. Proptest
compares the fast and oracle results on random hands for `w ∈ {0,1,2}`, and on
exhaustive small cases for `w ≥ 3`. The oracle costs 52^w evaluations, fine for tests and far
too slow for search. That gap is the reason the fast path exists.

### A-6 low
Ace = lowest. Straights and flushes count *against* you. Key identity: **A-6 low = the worst high
hand with ace treated as low**. So `low_a6(cards) = max over 5-card subsets of invert(high_ace_low(subset))`.
For 7 cards that's 21 subsets of an inverted high evaluation. Good enough to start; optimize only if
profiling says so.

With wilds: each wild takes the lowest rank not already held, then the result is checked for
straight/flush. If it makes one, the next rank is tried. Same oracle approach: brute-force over 13 ranks × suits.

### Validation (NFR-2)
- Exhaustive over all 2,598,960 five-card hands: category counts match the published table in the PRD.
- Exhaustive over all 133,784,560 seven-card hands (release-mode test, `#[ignore]` by default):
  category counts match published 7-card frequencies.

## 4. Betting (`betting.rs`) — NFR-5 boundary

```rust
pub enum Structure {
    NoLimit,
    PotLimit,
    FixedLimit { small: Chips, big: Chips, big_from_round: u8, max_raises: u8 },
    SpreadLimit { min_inc: Chips, max_inc: Chips, step: Chips, max_raises: u8 }, // FTQ: 50,200,50,4
}

pub struct BettingRound { /* per-seat committed this street, current bet, last full raise,
                             raises this street, who has acted since the last full raise, all-in flags */ }

pub struct BetOptions {
    pub call: Option<Chips>,      // None ⇒ check available
    pub raise_to: Option<(Chips, Chips)>, // inclusive min..=max, None ⇒ cannot raise
    pub step: Chips,              // amount granularity (1 cent NL, 50 cents spread)
}

impl BettingRound {
    pub fn options(&self, seat: Seat) -> BetOptions;
    pub fn apply(&mut self, seat: Seat, a: BetAction) -> Result<(), IllegalAction>;
    pub fn next_to_act(&self) -> Option<Seat>;   // None ⇒ street closed
}
pub enum BetAction { Fold, CheckCall, RaiseTo(Chips) }
```
Rules encoded once, used by every game:
- NL minimum raise = size of the last *full* raise. A short all-in **does not reopen** betting
  to players who already acted (standard rule; a common source of bugs).
- PL: let `P` = the whole pot including this street's bets, and `C` = the amount to call. The maximum raise
  *by* is `P + C` (the pot after you've called), so the max raise-*to* is `current_bet + P + C`.
- Raise caps count bets and raises together (FTQ: 4 per street).
- Drivers never do chip arithmetic. They call `options` / `apply` (conformance check C9).

Property tests: chip conservation; a street always terminates; `options` is consistent with `apply`
(every amount in range accepted, every amount outside rejected).

## 5. Pots (`pot.rs`)

Inputs: total contribution per seat, the set of seats still in, and a showdown `HandValue` per side per seat.

1. **Side pots** by contribution levels: sort the distinct contributions `c₁ < c₂ < …`. Pot *k*
   = Σ_seats min(contrib, c_k) − min(contrib, c_{k−1}). Eligible seats: not folded and contribution ≥ c_k.
2. **Per pot:** if the game is hi/lo, split into high and low halves (odd chip → high). Each half goes to the
   best eligible hand declared for that side. Ties split evenly; odd chip → first tied seat
   left of the button.
3. **Declarations** (FTQ): a seat that declared Both is eligible on both sides only if it wins both
   outright (no ties); otherwise it's ineligible for that pot entirely (PRD §4). A side nobody
   declared rolls to the other side.

Output: `Vec<Chips>` winnings per seat. Driver payoff = winnings − contribution. Property
test: Σ payoffs = 0.

## 6. Session (`session.rs`)

Pure logic, no I/O. `storage` persists its events.

```rust
pub struct Session { seats: Vec<SeatInfo>, chooser: Seat, button: Seat, hands_left_in_game: u8, … }
pub enum SessionEvent { GameChosen{..}, BuyIn{..}, HandComplete{record, deltas}, Rebuy{..}, CashOut{..}, SitOut{..} }
```
- Game start: `button = chooser`, `hands_left_in_game = 1` for `SingleHand`, `num_seats` for `Orbit`
  (button advances each hand). At game end, `chooser += 1` and the next game starts (FR-SS-4).
- `StakeModel::TableStake(x)`: buy-in at game start, cash-out at game end. `Bankroll`:
  stack = bankroll.
- Rebuy only when the table stack is 0 (FR-SS-3). Bots: infinite bankroll, automatic rebuy.
  Humans: prompted, can sit out.
- Dealer ante: charged by the driver on `first_hand_of_game` (it's part of the hand's
  pot, so it goes through `betting`/`pot` like any chip movement).

Sessions are generic over `G: Game` for one game at a time. Switching games = the CLI drops the
`Session<G>` and builds a `Session<G2>` carrying the `SeatInfo`s (bankrolls) over.

## 7. RNG and seeds (`rng.rs`)

`DetRng = ChaCha8Rng`. Hand *i* of a run uses `ChaCha8Rng::seed_from_u64(run_seed)` with
`set_stream(i)`, so the streams are independent and order-independent. That's what makes
`rayon` sims reproducible no matter which thread runs which hand. Bots get their own stream
per (hand, seat), so a bot's randomness doesn't shift the deal.

## 8. Testing (`testing/`)

`conformance::<G>(config, n_hands)` runs random walks (random legal actions, sampled chance) and
checks C1–C8 from the API doc. Every driver calls it in its tests. This is the "standard library" of
correctness: write the driver, run conformance, fix until green.

## 9. Open items
- Leduc poker as a test-only driver (`engine` dev or `games`). Small enough for exact best
  response; bigger than Kuhn, so it catches MCCFR bugs Kuhn can't (multiple rounds, public card).
