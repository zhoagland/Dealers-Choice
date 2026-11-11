# Tech Plan — Full Game Catalog (Phase 2, track G)

| Field | Value |
|---|---|
| Status | Draft v0.2 for owner review (2026-11-10) |
| Implements | PRD FR-GC-1…8, milestones G0–G10 |
| Rules source | `Rules/Poker Bible.typ` (every section below cites it) |
| Procedure | `docs/dev/adding-a-game.md` for each driver; this doc decides *what* and *in which order* |

The Bible has **50 games**. Hold'em and Follow the Queen ship in v1, which leaves **48**. Many need
mechanics the v1 API doesn't have. Following the owner's direction, the plan adds each mechanic **once**,
in a foundation PR or the family that first needs it, and then implements every game that uses it in a
**single PR per family**.

---

## 1. Mechanics gap analysis

| # | Mechanic | Needed by | v1 status | Where it's added |
|---|---|---|---|---|
| K1 | Multi-hand games with state carried between hands (`UntilDone`, `Carry`) | Guts Leg, 3-2-1 Drop, 3-5-7, Scare-a-moosh, Tempest, Booray | missing (API doc §9) | **G0** (API v2) |
| K2 | **Stuns**: a penalty paid into the pot, sized "matches the pot up to $10" (Bible §Definitions) | 3-2-1 Drop, 3-5-7, Scare-a-moosh, Booray, Iron Cross / 3-2-1 variants | missing | **G0** (`engine::pot::Penalty`) |
| K3 | Jokers (54-card deck) | Bullet | `Card(u8)` 0..52, `CardSet(u64)` | **G0** (indices 52, 53; still fit in u64) |
| K4 | Pluggable evaluators: Omaha 2o2, 2-7 low, Badugi, short-deck high, 3-card (no straights/flushes), board geometry (lines/wheel/cross), numbers (blackjack values), high/low spade, single high card | most families | high, wild-high, A-6 low | **G0** adds the `Evaluator` framework; each family adds its evaluators, oracle-tested like v1 |
| K5 | Per-seat wild sets | Low in the Hole, 3-5-7 (table-wide but changing) | table-wide wild rank (FTQ) | G6 (stud) |
| K6 | N-way pot division with declarations (Top/Bottom board, 7/27, hi/lo numbers, Chicago spade half) | Double Board, Split Hold'em, numbers, Chicago | 2-way hi/lo `award_declared` | **G0** generalizes to `award_shares(shares: &[Share])` |
| K7 | Card-choice actions: discard/draw, pass N, split into pairs, choose reveal order, push a card, take/decline a card | draw, pass, stud-push, Bullet, Mama's | `ActionClass::CardChoice` exists, unused | G4 (draw) |
| K8 | Inverted information (see everyone's cards but your own) | Indian Poker, Indian Hold'em | allowed by the observation contract (§4), untested | G7 |
| K9 | Non-betting play/drop decisions (simultaneous) | 3-2-1 Drop, 3-5-7, 43, Iron Cross, Scare-a-moosh in/out | declarations exist (FTQ) | reuse the FTQ pattern |
| K10 | Variable deal order and acting order (best board first, bid order) | stud family, Bid, Bullet, Pass the Trash | FTQ action order | reuse |
| K11 | Sealed-bid auction paid into the pot | Bid | missing | G6 |
| K12 | House-banked game (one seat is the bank) | Blackjack | missing | G9 |
| K13 | Trick-taking (follow suit, trumps) | Booray | missing | G10 |
| K14 | **Generic table view** for the web UI (`TableView`) | every game in the web UI | CLI text hooks only | **G0** (see `web.md` §4) |

## 2. API v2 (milestone G0)

One `API_VERSION` bump (1 → 2), done before any family so every new driver is written once, against v2.

```rust
pub trait Game {
    // ... v1 items unchanged ...

    /// State that survives between hands of one game (K1). `()` for SingleHand/Orbit games.
    type Carry: Clone + Debug + Default + Serialize + DeserializeOwned + Send + Sync;

    /// v2: the session passes the previous hand's carry (Default on hand 1).
    fn new_hand(&self, setup: &HandSetup, carry: &Self::Carry) -> Self::State;

    /// Called at Terminal. The session feeds it into the next `new_hand`.
    fn carry_out(&self, s: &Self::State) -> Self::Carry { Default::default() }

    /// `GameLength::UntilDone` only: the session stops when this is true.
    fn game_over(&self, carry: &Self::Carry) -> bool { true }

    /// Game-agnostic table view for graphical UIs (K14). Built only from the observation,
    /// so it can't leak.
    fn view(&self, obs: &Self::Observation) -> TableView;
}

pub enum GameLength { SingleHand, Orbit, UntilDone { max_hands: u32 } }
```

Decisions:

| Decision | Alternative | Why |
|---|---|---|
| `Carry` is an associated type, `()` for existing games | `Box<dyn Any>` | Keeps static typing and replay (`Carry` is serialized into the hand record) |
| Carry-over pots are **part of `Carry`** (amount + per-seat contributions) and enter the next hand as dead money | Session-level pot | Pot ownership stays with the game. The session ledger checks chip conservation across the whole game, not per hand (NFR-2 is relaxed to *per game* for UntilDone; documented) |
| `max_hands` safety cap on `UntilDone` (default 200) | none | Guarantees termination (C8) for sims; at the cap the pot is split evenly among live seats (D-G0-1) |
| `view()` is **required**, not defaulted | default text-only view | The web UI must render every game; a missing view is a compile error, not a blank table |
| Hand record v2 = v1 + `carry_in` bytes | recompute carry from previous hands | Each hand stays independently replayable |

Engine work in G0, besides the trait:
- `Card`: indices 52 = red joker, 53 = black joker; `Deck::with_jokers()`. Evaluators treat a joker as
  "wild" only when the game passes it in the wild set. Golden RNG test unchanged for 52-card decks.
- `pot::Penalty { seat, amount }` and `stun_amounts(pot_before, stunned_in_order, base_cap, step)` per the
  stun rule in §3 (cap $10, +$N per additional stunner in the same hand).
- Both-declaration fallback (OQ-3, owner decision): if every remaining contestant failed a Both declaration,
  the pot is **split evenly** among them (today it's split as hi/lo). Changes `pot::award_declared`; FTQ's
  `meta.version` bumps.
- Optional show (G-3): losing players may Show or Muck after required shows.
- `pot::award_shares`: a pot split into named shares (High, Low, Top board, Spade, …), each with its own
  ranking and eligible set, with the existing Both-declaration rule applied per share pair. Replaces
  `award_declared` (kept as a thin wrapper).
- `eval::Evaluator` trait + registry so evaluators are reused across games and oracle-tested in one place.
- `engine::testing` conformance: C4 (no leaks) learns about inverted information (the observer's *own*
  cards must vary across determinizations in Indian games; others' cards must not).
- Session: drive `UntilDone`, persist carry between hands, and support the "if you're out you're out"
  flag (a seat that folds is excluded from later hands of that game).
- Migrate Kuhn, Leduc, Hold'em and FTQ to v2 and bump their `meta.version` (records/blueprints invalidate
  per handbook §4). Add `view()` for each.

**Exit criteria (G0):** all v1 tests pass on v2; a toy `UntilDone` game (Guts-lite) passes conformance
including carry replay; CLI play unchanged.

## 3. Defaults for games the Bible doesn't give stakes for

The Bible specifies stakes only for Hold'em ($0.50/$1 blinds, $20 table stake) and the dealer ante.
FTQ's house structure was confirmed separately (A7).

**D-ALL-1 (owner decision 2026-11-10): every game except Texas Hold'em uses the FTQ structure**:
dealer ante $2 only, spread limit with $0.50–$2 increments, at most 4 bets/raises per street, bankroll
stake model, `SingleHand` length. This includes the Omaha family (the Bible's name "Pot Limit Omaha" is kept;
`Structure::PotLimit` is available as a config flag) and the Hold'em variants. The only exceptions are sections
that state their own money rules: Tempest (D-TEM-1), Iron Cross ($1 ante), 3-5-7 and 3-2-1 Drop (antes + stuns),
Bid/Push/43/Scare-a-moosh (their payments), Blackjack (bank and max bet), Booray.

**Stuns (owner decision, OQ-9):** a stun = the pot, capped. The cap starts at **$10**. Within one hand, each
additional stunned player's cap is **$N higher** than the previous one's (1st: $10, 2nd: $10 + N, 3rd: $10 + 2N…),
and it resets every hand. The chooser picks N when choosing the game (`Config::stun_step`, default $0).
**D-STUN-1** every stun in a hand is measured against the pot *before* that hand's stuns are paid. **D-STUN-2**
stunned players are ordered starting left of the dealer. **D-G0-2** a stun larger than the seat's stack takes the stack.

Other cross-game defaults:
- **Optional show (G-3, owner decision):** after required shows, each losing player may Show or Muck
  (CLI and web; bots always muck). Shown cards are public in hand history.
- **D-ALL-2 Low hands use A-6** (house rule from PRD §4) with **no qualifier** (no "8 or better"); the Bible never
  mentions one. Exception: 2-7 games.
- **D-ALL-3 Hi/lo games declare.** Where a section's objective says "best high or best low" but its game-play
  list omits a "Declaration" step (5 Card Omaha, Courchevel), we add the simultaneous High/Low/Both declaration,
  matching every other hi/lo game in the Bible.
- **D-ALL-4 Burn cards** are omitted (API doc §1 rule 3), so "no burn card" in some sections changes nothing.
- **D-ALL-5 Max seats** = deck capacity (cards dealt per player + board) ≤ 52, capped at 9.
- **D-ALL-6 "Variations"** in the Bible become `Config` flags, default **off**, so the base game is what the
  section's game-play list says. Each flag is listed per game below and is sweepable in sims (`sim-program.md` §4).

## 4. Families, order, and PRs

Order follows the owner's "by mechanic family" choice: foundation first, then the families that reuse
Hold'em/FTQ machinery most, with the costliest new mechanics last. Each family is one PR, stacked on the
previous one.

| Milestone | Family | New mechanics | Games | # |
|---|---|---|---|---|
| G0 | API v2 foundation | K1–K4 framework, K6, K14 | — (migrates 4 existing games) | 0 |
| G1 | Omaha | Omaha 2o2 evaluator (high + A-6 low); FTQ structure (pot-limit as a config flag) | Pot Limit Omaha, 5 Card Omaha, Courchevel, Red River, PLO Double Board Bomb Pot | 5 |
| G2 | Board shapes | Geometry evaluator (named lines on a board layout) | Elevator, Wheel of Death, Spit, Industrial Elevator, Iron Cross | 5 |
| G3 | Hold'em twists | Short-deck evaluator, public folded cards | 6+ Hold'em, Showtime Hold'em, Reverse Poker, Split Hold'em | 4 |
| G4 | Draw & discard | K7 discard/draw/keep, 2-7 evaluator, Badugi evaluator | 5 Card Draw, Triple Draw, 2-7 Single Draw, 2-7 Triple Draw, Badugi, 3 Blind Mice, Crazy Pineapple, Mama's Favorite, Swap Hold'em, Fusion | 10 |
| G5 | Pass & in/out | K7 pass/arrange, K9 in/out | Pass the Trash, Man or Mouse, 43 | 3 |
| G6 | Stud variants | K5 per-seat wilds, K3 jokers in play, K11 auction, push payments | 7 Card Stud, Razz, Baseball, Chicago High, Chicago Low, Low in the Hole, Push, Bullet, Bid | 9 |
| G7 | Indian | K8 | Indian Poker, Indian Texas Hold'em | 2 |
| G8 | Multi-hand & stuns | K1 + K2 in anger, 3-card evaluator | Guts Leg, 3-2-1 Drop, 3-5-7, Scare-a-moosh, Tempest Hold'em | 5 |
| G9 | Numbers & house | numbers evaluator, K12 bank seat | Blackjack, 7/27, 5.5/21.5, Numbers | 4 |
| G10 | Trick-taking | K13 | Whooh Ray for Booray | 1 |
| | | | **Total** | **48** |

Each family PR contains: the drivers; the new evaluator(s) with oracle/brute-force tests; conformance for every
game; at least one rule test per decision (D-*) below; `view()` per game; registry entries; a row per game in
`docs/tech/games.md`; the family's decisions copied into the PRD open-questions list for owner review; and the
games added to the balance campaign (`sim-program.md` §4).

**Definition of done per game:** `conformance::<G>()` passes; rule tests pass; `dc play` can deal it; a
10 k-hand random-bot sim and a 200-hand ISMCTS sim complete without panics; `view()` renders in the web UI's
storybook (`web.md` §8) once W1 exists.

## 5. Rule decisions per game

Format: **D-<game>-n** = a decision made where the Bible is silent, ambiguous, or self-contradictory. Every one
is provisional and listed in the PR for the owner to overrule. Overruling one = change + rule test + `meta.version` bump.

### G1 Omaha
- **Pot Limit Omaha** (§Pot Limit Omaha): high only, 4 hole cards, 2o2. No decisions needed beyond D-ALL-1.
- **5 Card Omaha**: hand construction says "3 *continuous* community cards", copied from Wheel of Death.
  **D-5CO-1** standard board, any 3. **D-5CO-2** hi/lo with declaration (D-ALL-3); variation flag `high_only`.
- **Courchevel**: same as 5CO (**D-COU-1**, **D-COU-2**); first flop card face up before preflop betting.
- **Red River**: one card per street after the flop, continuing while the newest card is red. **D-RR-1** "river+1"
  is always dealt, so the hand has at least 6 board cards. **D-RR-2** hands use 2o2 + best 3 of *all* board cards.
  **D-RR-3** if the deck runs out, the board ends (termination). Declaration hi/lo.
- **PLO Double Board Bomb Pot**: two boards; declare Top/Bottom/Both per board (K6). "Bomb pot" is not defined in the
  Bible. **D-DB-1 (confirmed)** every seat antes $1 before the deal (plus the dealer ante), and the preflop betting round
  is kept. **D-DB-2 (confirmed)** declare Top, Bottom, or Both boards; each board's half goes to the best *high* hand
  among the seats that declared it. Variations: `deal_boards_separately`, `dump_two`.

### G2 Board shapes
- **Elevator**: H of 7 cards. Valid lines: left column (3), right column (3), middle row (3, through the blue
  centre). **D-ELV-1** which set (red or black) flips first is random per hand (chance node). Hi/lo declaration;
  high and low may use different lines.
- **Wheel of Death**: 9-card ring; a line is any 3 consecutive cards around the ring (9 lines). Flips: positions
  {0,3,6}, then {1,4,7}, then {2,5,8}. Variation flags `eight_cards`, `flip_one_at_a_time`.
- **Spit**: 5 hole cards, each player discards 1 face down into the middle; 9 − players extra cards are added; the 9
  middle cards are shuffled together and flipped one per street (9 betting rounds). **D-SPIT-1** discards are
  chosen simultaneously, and a player knows only that their own discard is somewhere in the middle. **D-SPIT-2**
  hands are 2o2 from the remaining 4 hole cards + any 3 middle cards. **D-SPIT-3** with more than 9 players the game
  isn't offered (max 9 seats anyway).
- **Industrial Elevator**: 5 hole cards; use 2 hole + 3 board or 3 hole + 2 board. The diagram is a vertical
  column of 4 crossed by a horizontal row of 4 (7 cards). **D-IE-1** a usable board set is any cards from one
  straight line (row or column). Flip order per the Bible (right two, left one, then elevator cards one at a time).
- **Iron Cross**: 4 hole cards, $1 ante, a 5-card cross (row of 3 and column of 3 sharing the centre). The Bible
  says "4 cards in hand and 3 community cards". **D-IC-1** best 5 from 4 hole + the row *or* the column.
  **D-IC-2** single round of betting (as written); the stun variant (orbit) is a `Config` flag that needs G8 and
  ships there.

### G3 Hold'em twists
- **6+ Hold'em**: 36-card deck; flush beats full house; A-6-7-8-9 is the lowest straight. **D-6P-1** three of a
  kind still ranks *below* a straight (the Bible changes only flush vs full house).
- **Showtime Hold'em**: folded hole cards are turned face up for everyone (observation change only).
- **Reverse Poker**: 5 hole cards, 2-card board flipped one at a time; best 5 of 7.
- **Split Hold'em**: two boards, half the pot each, with a Top/Bottom/Both declaration (the Bible lists one).

### G4 Draw & discard
- **5 Card Draw**: one draw. **D-5CD-1** up to 5 cards may be drawn (the Bible gives no limit here, unlike Guts Leg).
  **D-DRAW-1** if the deck runs out mid-draw, the discards are shuffled and used (standard).
- **Triple Draw**: A-6 low (house low), three draws.
- **2-7 Single Draw / 2-7 Triple Draw**: deuce-to-seven: aces always high, straights and flushes count against.
- **Badugi**: the objective line says "Best high poker hand", which contradicts the overview. **D-BAD-1** standard
  Badugi low ranking as described in the overview.
- **3 Blind Mice**: draw up to 3 (4 with an Ace, the Ace is shown), 3 community cards flipped after the draw;
  best 5 of 8.
- **Crazy Pineapple**: the overview text says "dealt 5 cards" but Setup says 3; the game-play list discards one
  after the flop. **D-CP-1** deal 3, discard 1 after the flop (standard Crazy Pineapple).
- **Mama's Favorite**: 6 cards split into 3 secret pairs, discard one pair after the flop and another after the
  turn. **D-MF-1** the split is a hidden choice (15 ways); discarded pairs are not shown.
- **Swap Hold'em**: "before any player folds they are given the option to swap one or both hole cards". **D-SW-1 (confirmed)**
  any time a player would fold, they may instead swap 1 or 2 hole cards for new ones from the deck and stay in
  the hand by calling. At most once per street.
- **Fusion**: overview text is a copy of Tempest; game play is Hold'em plus one extra hole card after the flop and
  after the turn. **D-FU-1** implement the game-play list (4 hole cards by the river, standard Hold'em hand
  construction: best 5 of hole + board, any number from either). Variation flag `hi_lo`.

### G5 Pass & in/out
- **Pass the Trash**: 7 cards; pass 3, bet, pass 2, bet, pass 1 in the dealer's chosen direction; discard 2;
  arrange 5 face down and roll them one at a time, with a betting round after each of the first 4. **D-PT-1**
  the chooser picks the direction as a public action at the start. **D-PT-2** "Declare (High/Low)" is the
  standard simultaneous declaration including Both.
- **Man or Mouse**: 7 cards + 1 face-down community card; pass 2, in/out declaration, bet, flip, bet,
  hi/lo declaration. **D-MM-1** in/out is simultaneous; "out" is a fold. Variation flag `five_card_start`.
- **43**: 4 cards, in/out for $1, 3 more cards, betting, declaration. **D-43-1** in/out is simultaneous, and
  "in" posts $1 into the pot.

### G6 Stud variants
- **7 Card Stud / Razz / Chicago High / Chicago Low**: FTQ structure without wilds. **D-STUD-1** first to act =
  best board showing (high games) or lowest board showing (Razz), as in FTQ. **D-CHI-1 (confirmed)** "highest/lowest spade"
  counts spades among **all 7 cards**, face up or down. **D-CHI-2 (confirmed)** in Chicago Low, Ace is high, so 2♠
  is the lowest. A player must reach showdown to win the spade half; if no player at showdown has a spade, the
  hand winner takes the whole pot. The spade half is won even if that player loses the hand half.
- **Baseball**: 3s and 6s wild; a face-up 4 gives an extra card. **D-BB-1** the player chooses whether that extra
  card is up or down. Hi/lo declaration. (No "buy the 3" rule; it isn't in the Bible.)
- **Low in the Hole**: wild = the lower of the two hole cards (Ace high), and matching up cards are wild too. On 7th
  street the player chooses up or down. **D-LH-1** the wild rank is recomputed from *all* face-down cards after 7th
  street if dealt down ("try to get a lower card to change their wild").
- **Push**: on 3rd–6th street a dealt up card can be pushed to the next live seat for $1 (into the pot); the
  receiver keeps it or pays $1 to take the next deck card instead. **D-PU-1** the original owner then receives a
  fresh card; a card can be pushed only once. Variation `buy_eighth` ($5 up, $7 down).
- **Bullet**: 2 down, then optional "take a card" rounds until everyone has 5 cards; face-up joker = dead, face-down
  joker = wild. **D-BU-1** extra cards are dealt face up except the last, which is face down. **D-BU-2** a player who
  declines to take a card while under 5 cards is still in the hand and is offered again next round. Best high
  board acts first.
- **Bid**: after preflop betting, remaining players sealed-bid for picking order; a card per remaining player is
  turned face up in the middle and picked in bid order; repeat until 5 up cards. **D-BID-1** bids are in $0.50
  units from $0 to $5, placed simultaneously. Variation `buy_eighth`.

### G7 Indian
- **Indian Poker**: one card, visible to everyone else; one betting round. **D-IP-1** the betting structure is FTQ
  spread.
- **Indian Texas Hold'em**: Hold'em with hole cards visible to everyone except the owner.

### G8 Multi-hand & stuns
- **Guts Leg**: 5 Card Draw (draw up to 3, or 4 with an Ace) repeated; the showdown winner earns a leg; first to 2
  legs takes the pot. If every player has a leg, legs reset. Button moves each hand. **D-GL-1** the pot carries over
  (`Carry`) and each hand's betting adds to it. **D-GL-2** a hand won by everyone folding still earns a leg.
  Variation `out_is_out`.
- **3-2-1 Drop**: ante, 3-card hands, red kings wild, simultaneous drop. Lone player vs the pot's dummy hand; losers
  match the pot (stun). **D-DROP-1** with 2+ players in, the best hand takes the pot and every other player who
  stayed in pays a stun into the next hand's pot. **D-DROP-2** stun = pot up to $10. Variation: orbit stun mode.
- **3-5-7**: the Bible's Setup line contradicts itself ("three cards... 4 cards and $1 ante"). **D-357-1 (confirmed)** deal 3,
  with a $1 ante per player at the start of each 3-5-7 cycle. Rounds of 3 (3s wild, no straights/flushes), 5 (5s wild), 7 cards (7s wild, best 5), then reshuffle.
  Play/drop is simultaneous; losers who played get stunned; a lone player gets a strike; 3 strikes wins the pot.
- **Scare-a-moosh**: starting left of the dealer, a player may name the game (high, low, high numbers, low numbers)
  and price (≤ $2) or pass. In/out, one draw, one bet, showdown; a declarer who doesn't win gets stunned; the pot
  carries until the declarer wins. **D-SAM-1** everyone who's in pays the price into the pot. **D-SAM-2** if all
  pass, redeal (counts toward `max_hands`).
- **Tempest Hold'em**: the Bible describes escalating antes, a Giant Blind, all-in-or-fold, and a 10-Giant-Blind
  cap, but the numbers are missing. **D-TEM-1 (confirmed)** Giant Blind $2; ante starts at $0.50 and rises $0.50 each hand
  until a hand reaches showdown, then resets; cap 10 Giant Blinds = $20 per player per hand.

### G9 Numbers & house
- **Blackjack**: the chooser is the bank and sets the max bet. **D-BJ-1 (confirmed)** the bank plays a fixed rule: hit
  16 or less, stand on all 17s. **D-BJ-2 (confirmed)** each player bets $0.50 up to the bank's max before the deal;
  no insurance; double down on any first two cards; split a pair once; doubling after splitting aces is a config
  flag (off), per the Bible's variation. This is not zero-sum between seats in the usual sense:
  the bank seat takes the other side of every bet, so payoffs still sum to zero.
- **7/27** and **5.5/21.5**: faces 0.5, Aces 1 or 11; one up one down; hit/stand rounds with betting until everyone
  stands; declaration (low target, high target, both). **D-727-1** closest *without going over* wins each target; a
  bust on one target can still win the other. **D-727-2** "Both" follows the house Both rule. Variation flags per
  the Bible (`bluff_bust`, `exact_target_wins`).
- **Numbers**: 6 cards, 4-card middle. **D-NUM-1** each flip discards from every hand all cards within ±1 rank of the
  flipped rank (Ace flip = 2 and King; matching rank also discards). **D-NUM-2** "the same as any other card" means
  a flipped rank equal to an earlier flip is replaced. Gin (empty hand) scoops immediately. Declaration High/Low/Both
  numbers.

### G10 Trick-taking
- **Whooh Ray for Booray**: 5 cards, dealer's flipped card sets trumps, in/out, one draw, 5 tricks. 3+ tricks scoops
  the pot; zero tricks = "fry" = stun; everyone else who played pays $1. The game repeats until someone scoops and
  nobody fries. **D-BOO-1** without a 3-trick winner, the pot carries and grows by the stuns and $1 payments.
  **D-BOO-2** "trump can't be led until played" applies. Variation `out_is_out`.

## 6. Bots in new games

ISMCTS is game-agnostic, so it plays every new game with no bot changes (FR-GD-6). Two known costs:
- Card-choice menus can be big (discard any subset of 5 = 32; Pass the Trash reveal order = 120; Mama's split = 15).
  These go through `legal_actions` like any action. If search quality drops, a game may offer a reduced bot menu
  (e.g. canonical reveal orders) in `legal_actions` while `validate` accepts every legal choice, the same trick
  as bet sizing (FR-BT-7).
- Rollouts use `classify`. Every new action kind must classify sensibly (draw/pass/arrange = `CardChoice`;
  in/out = `Declare`; take-a-card = `CheckOrCall`).

## 7. Risks

| Risk | Mitigation |
|---|---|
| A rule decision is wrong and invalidates sims | Every decision is a flagged config default with a rule test; sims record `game version`, so stale results are visible and re-runnable |
| API v2 churn breaks the existing games | G0 lands before any family and must pass all v1 tests |
| 48 drivers is a lot of determinize code | The replay-based determinization pattern (API doc §5) plus shared helpers in `games::common` (deal queues, stud action order, draw logic) |
| Evaluator bugs | Each new evaluator gets a brute-force oracle test like the v1 high evaluator |
