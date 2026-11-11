//! Scripted poker: one driver for every game that is "deal some cards, bet, maybe draw or
//! declare, show down" (catalog §4). A game is a [`Script`]: its deal/bet steps, how hands are
//! built and ranked for each pot share, who acts first, and what each seat can see.
//!
//! Shared rules (catalog §3, D-ALL-1): the chooser's $2 dealer ante, spread limit 50¢–$2 with at
//! most 4 bets per street (pot limit as a `Config` flag), cards dealt one at a time starting left
//! of the dealer to seats still in the hand. Declarations and draws are made in turn starting left
//! of the dealer; declarations stay hidden until everyone has declared. At showdown every live
//! hand is shown (house rule, as in FTQ). Burn cards are not modelled: an unseen burn doesn't
//! change what anyone knows or the deal's distribution.
//!
//! Each game's own decisions (D-*) are in [`games`] next to its script.

pub mod games;
pub mod hand;

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use engine::betting::{BetAction, BetSize, Betting, Structure};
use engine::eval::{eval_partial_board, short_deck};
use engine::game::sample_hidden_hands;
use engine::pot::{
    Claim, Share, StunRule, award_shares, build_pots, order_from_button, split, stun_amounts,
};
use engine::replay::replay_interleaved_with;
use engine::view::{BoardLayout, BoardView, CardSlot, PotView, SeatStatus, SeatView, TableView};
use engine::{
    ActionClass, Card, CardSet, Chips, DetRng, Game, GameLength, GameMeta, HandSetup, HandValue,
    HiddenSampler, IllegalAction, Rank, Seat, StakeModel, Suit, ToAct,
};
use rand::Rng;

pub use hand::{Build, Ranking, ShareRule};

// ---------------------------------------------------------------------------------------------
// Scripts
// ---------------------------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DeckKind {
    Standard,
    /// 6 through A (6+ Hold'em).
    Short36,
    /// 52 + both jokers.
    WithJokers,
}

impl DeckKind {
    pub fn cards(self) -> CardSet {
        match self {
            DeckKind::Standard => CardSet::FULL_DECK,
            DeckKind::Short36 => short_deck(),
            DeckKind::WithJokers => CardSet::FULL_DECK_WITH_JOKERS,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// `n` rounds of one face-down card to each seat still in.
    Down(u8),
    /// `n` rounds of one face-up card to each seat still in (stud).
    Up(u8),
    /// `n` face-up cards to board `board`.
    Board { board: u8, n: u8 },
    /// A betting round.
    Bet,
    /// Each seat still in declares (hidden until all have).
    Declare,
    /// Each seat still in discards up to `max` face-down cards and is dealt replacements. With
    /// `ace_bonus`, a seat keeping an Ace may discard `max + 1` and shows the Ace.
    Draw { max: u8, ace_bonus: bool },
    /// Each seat still in discards exactly `n` face-down cards (no replacements). With `pairs`,
    /// the cards must be one of the seat's pairs (after a [`Step::PairUp`]).
    Discard { n: u8, pairs: bool },
    /// Each seat still in secretly splits its six face-down cards into three pairs (Mama's).
    PairUp,
    /// The chooser picks the passing direction (public; D-PT-1).
    ChooseDirection,
    /// Each seat still in picks `n` face-down cards; then all pass at once to the next seat in
    /// the chosen direction.
    Pass(u8),
    /// Each seat still in secretly says In or Out; revealed together. Out folds; In pays `price`
    /// into the pot (43: $1).
    InOut { price: Chips },
    /// Each seat still in secretly orders its face-down cards for revealing (Pass the Trash).
    Order,
    /// Each seat still in turns its next face-down card face up (in its chosen order).
    Reveal,
    /// Deal board `board`'s cards at these positions (shaped boards).
    Flip { board: u8, positions: &'static [u8] },
    /// Spit: each seat still in throws one face-down card into the middle pile.
    Spit,
    /// Spit: deal hidden cards into the middle pile until it holds `total`.
    FillPool { total: u8 },
    /// Spit: turn `n` random cards of the middle pile face up onto board `board`.
    FlipPool { board: u8, n: u8 },
    /// Each seat still in chooses whether its next card comes face up or down (Low in the Hole).
    UpOrDown,
    /// Push: one up card per seat, each may be pushed on for $1 (D-PU-1/2).
    PushStreet,
    /// Bullet: rounds of "take a card?" then betting, until everyone has `target` cards
    /// (at most `cap` rounds; D-BU-1..3).
    TakeRounds { target: u8, cap: u8 },
    /// Bid: one card per seat still in is turned face up in the middle; sealed bids (in 50¢
    /// units, 0..=`max_units`) set the picking order (D-BID-1).
    BidRound { max_units: u8 },
    /// Each seat still in may buy one more card: face up for `up`, face down for `down`.
    BuyCard { up: Chips, down: Chips },
    /// 3-5-7: everyone secretly plays or drops this round (dropping doesn't leave the hand);
    /// `wild` is the wild rank; `three` = 3-card ranking. A lone player gets a strike; with
    /// more, the losers are stunned (D-357-*).
    PlayRound { wild: Rank, three: bool },
    /// 3-2-1 Drop: if exactly one player is still in, deal the pot's dummy hand face up.
    DummyIfLone { n: u8 },
    /// Scare-a-moosh: from the dealer's left, each player may name the game and its price
    /// (up to `max_units` × 50¢) or pass it on.
    NameGame { max_units: u8 },
    /// 7/27: rounds of hit-or-stand (hits face up) with betting after each, until a round in
    /// which everyone stands (at most `cap` rounds).
    HitRounds { cap: u8 },
    /// Numbers: turn a middle card; a rank already turned is replaced (D-NUM-2); every player
    /// throws away its cards of that rank and the ranks next to it (D-NUM-1); an empty hand
    /// scoops the pot.
    NumbersFlip,
    /// Booray: one more card to each seat still in, face down, except the dealer's, which is
    /// dealt face up and sets the trump suit.
    DealerUp,
    /// Booray: `n` tricks; follow suit; trump can't be led until played (D-BOO-2).
    Tricks { n: u8 },
    /// Red River (D-RR-1..3): one board card then a betting round, repeated while the card just
    /// dealt is red (and cards remain).
    RedRiver { board: u8 },
}

/// Who opens each betting round.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ActOrder {
    LeftOfButton,
    /// Stud: best high board showing; ties to the seat nearest the dealer's left.
    BestBoard,
    /// Razz: lowest board showing.
    LowBoard,
}

/// What each seat sees of the others' face-down cards.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Visibility {
    Normal,
    /// Everyone sees every face-down card except their own (Indian games).
    Indian,
    /// A seat's face-down cards turn face up when it folds (Showtime Hold'em).
    Showtime,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DeclareKind {
    /// High / Low / Both: share 0 = high, share 1 = low.
    HiLo,
    /// Top / Bottom / Both boards: share 0 = top, share 1 = bottom.
    Boards,
    /// Named shares (7/27: "7", "27", "Both").
    Custom([&'static str; 3]),
}

impl DeclareKind {
    pub fn names(self) -> [&'static str; 3] {
        match self {
            DeclareKind::HiLo => ["High", "Low", "Both"],
            DeclareKind::Boards => ["Top", "Bottom", "Both"],
            DeclareKind::Custom(names) => names,
        }
    }
    fn claim(k: u8) -> Claim {
        match k {
            0 => Claim::One(0),
            1 => Claim::One(1),
            _ => Claim::All,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Script {
    pub id: &'static str,
    pub name: &'static str,
    pub version: u32,
    pub rules_ref: &'static str,
    pub max_seats: u8,
    pub deck: DeckKind,
    /// Paid by every seat before the deal (Iron Cross, Double Board bomb pot).
    pub ante_each: Chips,
    pub steps: Vec<Step>,
    pub order: ActOrder,
    pub shares: Vec<ShareRule>,
    pub declare: Option<DeclareKind>,
    /// Cards that are wild for everyone.
    pub wild: CardSet,
    pub visibility: Visibility,
    /// Number of community boards.
    pub boards: u8,
    /// The Bible lets this game be played high only (`Config::high_only`).
    pub has_high_only: bool,
    /// Swap Hold'em (D-SW-1): instead of folding, swap 1–2 face-down cards and call.
    pub swap_instead_of_fold: bool,
    /// The Bible's named variations, switched on by name in `Config::variants`.
    pub variants: Vec<Variant>,
    /// Shaped boards: how the UI lays them out.
    pub layout: BoardLayout,
    /// Board positions per board (for empty slots in the view; 0 = grows as dealt).
    pub positions: u8,
    /// Groups of board positions a hand may use together. Empty = any board cards.
    pub lines: Vec<Vec<u8>>,
    /// Baseball: a face-up 4 earns an extra card, up or down by choice (D-BB-1).
    pub four_extra: bool,
    /// Bullet: a face-up joker kills the hand.
    pub up_joker_kills: bool,
    /// Low in the Hole: each seat's lowest face-down rank (ace high) is wild for it.
    pub low_hole_wild: bool,
    /// Cards each seat ends with (Push: replacements are only offered while the stock can
    /// still cover everyone's remaining cards).
    pub hand_cards: u8,
    /// `SingleHand` or `UntilDone` (multi-hand games, G8).
    pub length: GameLength,
    /// How a hand's money is settled (and what carries to the next hand).
    pub resolution: Resolution,
    /// `ante_each` only on a game's first hand.
    pub ante_first_hand_only: bool,
    /// "If you're out you're out": seats that fold (or go Out) sit out the rest of the game.
    pub out_is_out: bool,
    /// The game lasts one orbit; on its last hand losers pay half a stun to the winner instead
    /// of into the pot (the Bible's stun variations; D-IC-3, D-DROP-5).
    pub orbit_stun: bool,
}

/// How a hand ends and what carries over (catalog G8).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Resolution {
    /// Ordinary pots.
    Normal,
    /// Guts Leg: the hand winner earns a leg; `need` legs takes the whole carried pot.
    Legs { need: u8 },
    /// 3-2-1 Drop: a lone player plays the dummy hand; with more, the best takes the pot and
    /// the others are stunned.
    Drop,
    /// 3-5-7: strikes and stuns happen during the hand; `need` strikes takes the pot.
    Strikes { need: u8 },
    /// Scare-a-moosh: the declarer must win the named game or is stunned; the pot carries.
    Declarer,
    /// Tempest Hold'em: ordinary pots; the ante rises each hand until one reaches showdown.
    Tempest,
    /// Booray: 3 tricks scoop the pot; no trick "fries" (a stun); the others who played pay $1.
    Booray,
    /// The stun variations (Iron Cross, 3-2-1 Drop): the hand's winner takes the pot, losers at
    /// showdown are stunned into the next hand's pot; see `Script::orbit_stun`.
    StunOrbit,
}

/// What a multi-hand game carries from hand to hand. Public; `Default` = first hand.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScriptCarry {
    /// Chips carried in the pot (cents).
    pub pot: i64,
    pub legs: Vec<u8>,
    pub strikes: Vec<u8>,
    /// Seats out for the rest of the game.
    pub out: u16,
    pub hands: u32,
    /// Tempest: hands since the last showdown.
    pub level: u32,
    pub over: bool,
}

/// A public decision a seat must make right now (Baseball's extra card, Push, Bullet, Bid…).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Prompt {
    /// 0 = face down, 1 = face up (Baseball extra card, Low in the Hole's 7th card).
    UpOrDown,
    /// 0 = keep the up card, 1 = pay $1 and push it on.
    PushOrKeep,
    /// 0 = keep the pushed card, 1 = pay $1 for the next card instead.
    KeepOrReplace,
    /// 0 = pass, 1 = take a card.
    Take,
    /// Pick a middle card by index.
    Pick,
    /// 0 = no, 1 = buy face up, 2 = buy face down.
    Buy,
}

/// Push street bookkeeping: seats still to be served, in order.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum PushTask {
    /// Give this seat its street card (or the pushed card waiting for it).
    Serve(Seat),
    /// Offer this seat the push of the card it was just dealt.
    Offer(Seat),
}

/// A named rule variation: a change applied to the script.
#[derive(Clone, Copy)]
pub struct Variant {
    pub name: &'static str,
    pub apply: fn(&mut Script),
}

impl std::fmt::Debug for Variant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name)
    }
}

// ---------------------------------------------------------------------------------------------
// Config, actions
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Which script; set by the registry from the game id.
    pub game: String,
    pub dealer_ante: Chips,
    pub structure: Structure,
    pub bet_menu: Vec<BetSize>,
    /// Play only the first share (e.g. "can be played high hand only").
    pub high_only: bool,
    /// Named variations to switch on (see each game's `Script::variants`).
    pub variants: Vec<String>,
    /// Stuns: how much higher each further stunned player's cap is, in a hand (OQ-9; the
    /// chooser picks it). Default $0.
    pub stun_step: Chips,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            game: String::new(),
            dealer_ante: Chips(200),
            structure: Structure::SpreadLimit {
                min_inc: Chips(50),
                max_inc: Chips(200),
                step: Chips(50),
                max_raises: 4,
            },
            bet_menu: vec![BetSize::MinRaise, BetSize::MaxRaise],
            high_only: false,
            variants: Vec::new(),
            stun_step: Chips::ZERO,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Action {
    Deal(Card),
    Fold,
    CheckCall,
    RaiseTo(Chips),
    /// Index into the declare options (0, 1, 2 = both).
    Declare(u8),
    /// Bitmask over the seat's face-down cards to throw away (draw or discard steps).
    Draw(u8),
    /// Swap Hold'em: throw these cards away for new ones and call, instead of folding.
    Swap(u8),
    /// Mama's Favorite: split the six cards into pairs, by index into [`PAIRINGS`].
    Arrange(u8),
    /// The chooser's passing direction: 0 = left (clockwise), 1 = right.
    Direction(u8),
    /// Bitmask of face-down cards to pass.
    Pass(u8),
    /// 1 = In, 0 = Out.
    InOut(u8),
    /// Reveal order, as an index into the permutations of the seat's face-down cards.
    Order(u8),
    /// An answer to the current [`Prompt`].
    Choose(u8),
    /// A sealed bid in 50¢ units (Bid).
    Bid(u8),
    /// 3-5-7: 1 = play this round, 0 = drop (hidden until all have chosen).
    Play(u8),
    /// Scare-a-moosh: 0 = pass, else `1 + game * 4 + (units - 1)` (game 0 high, 1 low, 2 high
    /// numbers, 3 low numbers; price `units` × 50¢).
    Name(u8),
    /// Booray: play this card to the trick.
    PlayCard(Card),
}

/// The `idx`-th permutation (lexicographic) of `0..k`.
pub fn permutation(k: usize, mut idx: usize) -> SmallVec<[u8; 8]> {
    let mut items: SmallVec<[u8; 8]> = (0..k as u8).collect();
    let mut out = SmallVec::new();
    let mut f: usize = (1..k).product::<usize>().max(1);
    for i in (1..=k).rev() {
        let j = idx / f;
        idx %= f;
        out.push(items.remove(j));
        if i > 1 {
            f /= i - 1;
        }
    }
    out
}

/// Inverse of [`permutation`].
pub fn permutation_index(perm: &[u8]) -> usize {
    let k = perm.len();
    let mut items: SmallVec<[u8; 8]> = (0..k as u8).collect();
    let mut idx = 0;
    let mut f: usize = (1..k).product::<usize>().max(1);
    for (i, &x) in perm.iter().enumerate() {
        let j = items.iter().position(|&y| y == x).unwrap();
        idx += j * f;
        items.remove(j);
        if k - i > 1 {
            f /= k - i - 1;
        }
    }
    idx
}

fn factorial(k: usize) -> usize {
    (1..=k).product::<usize>().max(1)
}

/// The 15 ways to split six positions into three pairs; each is the new card order, so the
/// pairs are positions (0,1), (2,3), (4,5).
pub const PAIRINGS: [[u8; 6]; 15] = {
    let mut out = [[0u8; 6]; 15];
    let mut k = 0;
    let mut b = 1;
    while b < 6 {
        // Pair 0 with b; the rest (4 positions) split into 2 pairs 3 ways.
        let mut rest = [0u8; 4];
        let mut r = 0;
        let mut i = 1;
        while i < 6 {
            if i != b {
                rest[r] = i as u8;
                r += 1;
            }
            i += 1;
        }
        let splits = [[0, 1, 2, 3], [0, 2, 1, 3], [0, 3, 1, 2]];
        let mut j = 0;
        while j < 3 {
            let sp = splits[j];
            out[k] = [
                0,
                b as u8,
                rest[sp[0]],
                rest[sp[1]],
                rest[sp[2]],
                rest[sp[3]],
            ];
            k += 1;
            j += 1;
        }
        b += 1;
    }
    out
};

impl Action {
    fn as_bet(self) -> Option<BetAction> {
        match self {
            Action::Fold => Some(BetAction::Fold),
            Action::CheckCall => Some(BetAction::CheckCall),
            Action::RaiseTo(x) => Some(BetAction::RaiseTo(x)),
            _ => None,
        }
    }
    fn from_bet(b: BetAction) -> Action {
        match b {
            BetAction::Fold => Action::Fold,
            BetAction::CheckCall => Action::CheckCall,
            BetAction::RaiseTo(x) => Action::RaiseTo(x),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
enum Slot {
    Down(Seat),
    Up(Seat),
    Board(u8),
    /// Board `b`, position `pos` (shaped boards).
    BoardAt(u8, u8),
    /// A hidden card into the middle pile (Spit).
    Pool,
    /// A random middle-pile card turned face up onto board `b`.
    PoolFlip(u8),
    /// A face-up card into Bid's middle.
    Middle,
}

#[derive(Clone, Debug)]
pub struct State {
    setup: HandSetup,
    pc: usize,
    entered: bool,
    /// Red River: false = dealing, true = betting.
    red_betting: bool,
    streets: u8,
    pub down: Vec<SmallVec<[Card; 8]>>,
    pub up: Vec<SmallVec<[Card; 8]>>,
    pub boards: Vec<SmallVec<[Card; 12]>>,
    /// Cards that can still be dealt.
    stock: CardSet,
    /// Discards since the last reshuffle (D-DRAW-1: reshuffled into the stock when it runs out).
    muck: CardSet,
    /// History lengths at which the muck was reshuffled into the stock.
    reshuffles: Vec<usize>,
    /// Seats that swapped this street (Swap Hold'em: at most once per street).
    swapped: u16,
    /// (history index, Ace) for each draw that used the Ace bonus.
    ace_shows: Vec<(usize, Card)>,
    /// Passing direction: 0 = left, 1 = right.
    direction: u8,
    /// Pass selections waiting to be made together.
    pass_sel: Vec<Option<u8>>,
    /// In/Out choices waiting to be revealed together (1 = in).
    in_out: Vec<Option<u8>>,
    /// Events the rules produce (passes resolving, in/out reveals, card reveals), each with the
    /// history length when it happened.
    sys_events: Vec<(usize, Event)>,
    /// Cards each seat received by passing, in order.
    received: Vec<SmallVec<[Card; 8]>>,
    /// Everyone went Out: the hand is void and all chips go back.
    void: bool,
    /// Board position of each card in `boards` (same order).
    pub board_pos: Vec<SmallVec<[u8; 12]>>,
    /// Spit's middle pile: face-down cards not yet turned.
    pool: CardSet,
    /// History indices of Spit throws.
    spits: Vec<usize>,
    /// Public decisions waiting, first = now.
    prompts: VecDeque<(Seat, Prompt)>,
    push_tasks: VecDeque<PushTask>,
    /// A pushed card waiting for the next seat.
    incoming: Option<Card>,
    /// Bullet: rounds done, and whether the current round is betting.
    take_rounds: u8,
    take_betting: bool,
    /// Bid: the face-up middle cards, and the sealed bids.
    pub middle: SmallVec<[Card; 9]>,
    bids: Vec<Option<u8>>,
    bid_phase: u8,
    /// The carry this hand started from.
    carry: ScriptCarry,
    /// 3-5-7 strikes during the hand, and who reached the target.
    strikes: Vec<u8>,
    strike_winner: Option<Seat>,
    plays: Vec<Option<u8>>,
    /// Scare-a-moosh: (declarer, game, price units), or `None` while being named.
    named: Option<(Seat, u8, u8)>,
    /// Seats that went Out this hand.
    outs: u16,
    /// 7/27: hit rounds done, whether anyone hit this round, and whether it's betting.
    hit_rounds: u8,
    hit_any: bool,
    hit_betting: bool,
    /// Numbers: seats that threw away their whole hand (they scoop).
    pub gin: u16,
    /// Booray: trump suit, whether a trump has been played, the trick so far, tricks won.
    pub trump: Option<Suit>,
    trump_played: bool,
    trick: SmallVec<[(Seat, Card); 9]>,
    pub tricks_won: Vec<u8>,
    tricks_left: u8,
    pub betting: Betting,
    queue: VecDeque<Slot>,
    deal_log: Vec<Slot>,
    declarations: Vec<Option<u8>>,
    /// Seats still to act in the current declare/draw/discard step.
    pending: VecDeque<Seat>,
    history: Vec<(Option<Seat>, Action)>,
    done: bool,
}

impl State {
    fn n(&self) -> u8 {
        self.setup.num_seats
    }

    fn live_from_left(&self) -> Vec<Seat> {
        order_from_button(self.setup.button, self.n())
            .into_iter()
            .filter(|&s| !self.betting.is_folded(s))
            .collect()
    }

    fn is_showdown(&self) -> bool {
        self.done && self.betting.live_count() > 1
    }

    /// Where the next dealt card comes from: the middle pile for Spit's flips, else the stock.
    fn chance_pool(&self) -> CardSet {
        match self.queue.front() {
            Some(Slot::PoolFlip(_)) => self.pool,
            _ => self.stock,
        }
    }

    /// A seat's own cards (down then up).
    pub fn own(&self, seat: Seat) -> SmallVec<[Card; 16]> {
        self.down[seat.index()]
            .iter()
            .chain(&self.up[seat.index()])
            .copied()
            .collect()
    }
}

/// Lowest-board key for Razz first-to-act: fewer pairs, then lower cards (ace low). Smaller wins.
fn low_board_key(up: &[Card]) -> (u8, SmallVec<[u8; 8]>) {
    let mut ranks: SmallVec<[u8; 8]> = up.iter().map(|c| (c.rank() as u8 + 1) % 13).collect();
    ranks.sort_unstable_by(|a, b| b.cmp(a));
    let mut dupes = 0;
    for w in ranks.windows(2) {
        if w[0] == w[1] {
            dupes += 1;
        }
    }
    (dupes, ranks)
}

// ---------------------------------------------------------------------------------------------
// Game
// ---------------------------------------------------------------------------------------------

pub struct Scripted {
    config: Config,
    script: Script,
    meta: GameMeta,
}

impl Scripted {
    pub fn script(&self) -> &Script {
        &self.script
    }

    fn shares(&self) -> &[ShareRule] {
        if self.config.high_only {
            &self.script.shares[..1]
        } else {
            &self.script.shares
        }
    }

    fn declare(&self) -> Option<DeclareKind> {
        if self.config.high_only {
            None
        } else {
            self.script.declare
        }
    }

    fn deck(&self) -> CardSet {
        self.script.deck.cards()
    }

    fn step(&self, s: &State) -> Option<Step> {
        self.script.steps.get(s.pc).copied()
    }

    fn first_to_act(&self, s: &State) -> Seat {
        let live = s.live_from_left();
        let any_up = live.iter().any(|p| !s.up[p.index()].is_empty());
        match self.script.order {
            ActOrder::LeftOfButton => s.setup.button.next(s.n()),
            _ if !any_up => s.setup.button.next(s.n()),
            ActOrder::BestBoard => {
                let mut best: Option<(HandValue, Seat)> = None;
                for p in live {
                    let up: Vec<Card> = s.up[p.index()]
                        .iter()
                        .copied()
                        .filter(|c| !c.is_joker())
                        .take(4)
                        .collect();
                    if up.is_empty() {
                        continue;
                    }
                    let v = eval_partial_board(&up, None);
                    if best.is_none_or(|(b, _)| v > b) {
                        best = Some((v, p));
                    }
                }
                best.map(|b| b.1).unwrap_or(s.setup.button.next(s.n()))
            }
            ActOrder::LowBoard => live
                .into_iter()
                .min_by_key(|p| low_board_key(&s.up[p.index()]))
                .unwrap_or(s.setup.button.next(s.n())),
        }
    }

    fn enter(&self, s: &mut State) {
        s.entered = true;
        match self.script.steps[s.pc] {
            Step::Down(n) | Step::Up(n) => {
                let live = s.live_from_left();
                let up = matches!(self.script.steps[s.pc], Step::Up(_));
                for _ in 0..n {
                    for &p in &live {
                        s.queue
                            .push_back(if up { Slot::Up(p) } else { Slot::Down(p) });
                    }
                }
            }
            Step::Board { board, n } => {
                for _ in 0..n {
                    s.queue.push_back(Slot::Board(board));
                }
            }
            Step::Bet => {
                let mut first = self.first_to_act(s);
                let giant = self.script.resolution == Resolution::Tempest && s.streets == 0;
                let gb = s.live_from_left()[0];
                if giant {
                    // Preflop starts left of the Giant Blind (the first seat dealt in).
                    first = gb.next(s.n());
                }
                s.betting.start_street(s.streets, first);
                if giant {
                    let have = s.betting.stacks()[gb.index()];
                    s.betting.post_blind(gb, TEMPEST_GIANT_BLIND.min(have));
                }
                s.streets += 1;
                s.swapped = 0;
            }
            Step::Declare
            | Step::Draw { .. }
            | Step::Discard { .. }
            | Step::PairUp
            | Step::Pass(_)
            | Step::InOut { .. }
            | Step::Order => {
                s.pending = s.live_from_left().into();
            }
            Step::ChooseDirection => {
                s.pending = std::iter::once(s.setup.chooser).collect();
            }
            Step::Reveal => {
                for p in s.live_from_left() {
                    if !s.down[p.index()].is_empty() {
                        let c = s.down[p.index()].remove(0);
                        s.up[p.index()].push(c);
                        s.sys_events.push((s.history.len(), Event::Revealed(p, c)));
                    }
                }
            }
            Step::RedRiver { board } => {
                s.red_betting = false;
                s.queue.push_back(Slot::Board(board));
            }
            Step::Flip { board, positions } => {
                for &pos in positions {
                    s.queue.push_back(Slot::BoardAt(board, pos));
                }
            }
            Step::Spit => {
                s.pending = s.live_from_left().into();
            }
            Step::FillPool { total } => {
                for _ in s.pool.len()..total as usize {
                    s.queue.push_back(Slot::Pool);
                }
            }
            Step::FlipPool { board, n } => {
                for _ in 0..n.min(s.pool.len() as u8) {
                    s.queue.push_back(Slot::PoolFlip(board));
                }
            }
            Step::UpOrDown => {
                s.prompts = s
                    .live_from_left()
                    .into_iter()
                    .map(|p| (p, Prompt::UpOrDown))
                    .collect();
            }
            Step::PushStreet => {
                s.incoming = None;
                s.push_tasks = s
                    .live_from_left()
                    .into_iter()
                    .map(PushTask::Serve)
                    .collect();
            }
            Step::TakeRounds { .. } => {
                s.take_rounds = 0;
                self.start_take_round(s);
            }
            Step::BidRound { .. } => {
                s.bid_phase = 0;
                for _ in s.live_from_left() {
                    s.queue.push_back(Slot::Middle);
                }
            }
            Step::BuyCard { .. } => {
                s.prompts = s
                    .live_from_left()
                    .into_iter()
                    .map(|p| (p, Prompt::Buy))
                    .collect();
            }
            Step::PlayRound { .. } | Step::NameGame { .. } => {
                s.pending = s.live_from_left().into();
            }
            Step::DummyIfLone { n } => {
                if s.betting.live_count() == 1 {
                    for _ in 0..n {
                        s.queue.push_back(Slot::Board(0));
                    }
                }
            }
            Step::HitRounds { .. } => {
                s.hit_rounds = 0;
                self.start_hit_round(s);
            }
            Step::NumbersFlip => s.queue.push_back(Slot::Board(0)),
            Step::DealerUp => {
                let dealer = Self::trick_dealer(s);
                for p in s.live_from_left() {
                    s.queue.push_back(if Some(p) == dealer {
                        Slot::Up(p)
                    } else {
                        Slot::Down(p)
                    });
                }
            }
            Step::Tricks { n } => {
                s.tricks_left = n;
                s.trick.clear();
                let lead = Self::trick_dealer(s).unwrap_or(s.setup.button);
                s.pending = Self::order_from(s, lead).into();
            }
        }
    }

    /// Booray's dealer: the button if still in, else the first seat in to its left.
    fn trick_dealer(s: &State) -> Option<Seat> {
        let live = s.betting.live_mask();
        (0..s.n())
            .map(|k| Seat((s.setup.button.0 + k) % s.n()))
            .find(|p| live & (1 << p.0) != 0)
    }

    /// Seats still in, starting with `first` (or the next one in after it).
    fn order_from(s: &State, first: Seat) -> Vec<Seat> {
        let live = s.betting.live_mask();
        (0..s.n())
            .map(|k| Seat((first.0 + k) % s.n()))
            .filter(|p| live & (1 << p.0) != 0)
            .collect()
    }

    /// Booray: the cards `p` may play now.
    fn playable(&self, s: &State, p: Seat) -> Vec<Card> {
        let mut hand: Vec<Card> = s.own(p).into_iter().collect();
        hand.sort();
        let trump = s.trump;
        match s.trick.first() {
            None => {
                // Leading: no trump until one has been played, unless that's all you hold.
                let non_trump: Vec<Card> = hand
                    .iter()
                    .copied()
                    .filter(|c| Some(c.suit()) != trump)
                    .collect();
                if !s.trump_played && !non_trump.is_empty() {
                    non_trump
                } else {
                    hand
                }
            }
            Some(&(_, led)) => {
                let follow: Vec<Card> = hand
                    .iter()
                    .copied()
                    .filter(|c| c.suit() == led.suit())
                    .collect();
                if follow.is_empty() { hand } else { follow }
            }
        }
    }

    /// Booray: a full trick goes to the highest trump, else the highest card of the led suit.
    fn finish_trick(&self, s: &mut State) {
        let led = s.trick[0].1.suit();
        let trump = s.trump;
        let &(w, _) = s
            .trick
            .iter()
            .max_by_key(|(_, c)| (Some(c.suit()) == trump, c.suit() == led, c.rank()))
            .unwrap();
        s.tricks_won[w.index()] += 1;
        s.sys_events.push((s.history.len() + 1, Event::TrickWon(w)));
        s.trick.clear();
        s.tricks_left -= 1;
        if s.tricks_left > 0 {
            s.pending = Self::order_from(s, w).into();
        }
    }

    /// 7/27: offer everyone still in a card.
    fn start_hit_round(&self, s: &mut State) {
        s.hit_any = false;
        s.hit_betting = false;
        s.prompts = s
            .live_from_left()
            .into_iter()
            .map(|p| (p, Prompt::Take))
            .collect();
    }

    /// Numbers: the middle card just turned. A rank already turned is replaced; otherwise every
    /// hand throws away that rank and its neighbours (ace next to two and king).
    fn numbers_flip(&self, s: &mut State, c: Card) {
        let board = &s.boards[0];
        let dup = board[..board.len() - 1]
            .iter()
            .any(|b| b.rank() == c.rank());
        if dup {
            s.boards[0].pop();
            s.board_pos[0].pop();
            s.muck.insert(c);
            s.queue.push_front(Slot::Board(0));
            return;
        }
        let r = c.rank() as i32;
        let near = |x: Rank| {
            let d = (x as i32 - r).rem_euclid(13);
            d == 0 || d == 1 || d == 12
        };
        for p in s.live_from_left() {
            let hand = &mut s.down[p.index()];
            let mut kept = SmallVec::new();
            for &x in hand.iter() {
                if !x.is_joker() && near(x.rank()) {
                    s.sys_events
                        .push((s.history.len() + 1, Event::Discarded(p, x)));
                    s.muck.insert(x);
                } else {
                    kept.push(x);
                }
            }
            *hand = kept;
            if hand.is_empty() {
                s.gin |= 1 << p.0;
            }
        }
        if s.gin != 0 {
            s.done = true;
            s.queue.clear();
        }
    }

    /// 3-5-7: everyone has chosen; a lone player gets a strike, otherwise the losers among the
    /// players are stunned. Hands that played are shown.
    fn resolve_play_round(&self, s: &mut State, wild: Rank, three: bool) {
        let live = s.live_from_left();
        let ins: Vec<Seat> = live
            .iter()
            .copied()
            .filter(|p| s.plays[p.index()].take() == Some(1))
            .collect();
        let mask = ins.iter().fold(0u16, |m, p| m | 1 << p.0);
        s.sys_events
            .push((s.history.len(), Event::PlayResolved(mask)));
        let need = match self.script.resolution {
            Resolution::Strikes { need } => need,
            _ => 3,
        };
        match ins.len() {
            0 => {}
            1 => {
                let p = ins[0];
                s.strikes[p.index()] += 1;
                if s.strikes[p.index()] >= need {
                    s.strike_winner = Some(p);
                    s.done = true;
                }
            }
            _ => {
                let mut w = CardSet::EMPTY;
                for su in Suit::ALL {
                    w.insert(Card::new(wild, su));
                }
                let rule = ShareRule {
                    name: "High",
                    ranking: if three {
                        Ranking::ThreeCard
                    } else {
                        Ranking::High
                    },
                    build: Build::Own,
                    board: 0,
                };
                let vals: Vec<(Seat, Option<HandValue>)> = ins
                    .iter()
                    .map(|&p| (p, hand::share_value(&rule, &s.own(p), &[], w)))
                    .collect();
                let best = vals.iter().filter_map(|v| v.1).max();
                let losers = vals
                    .iter()
                    .filter(|v| v.1 != best)
                    .fold(0u16, |m, v| m | 1 << v.0.0);
                for &p in &ins {
                    for (j, &c) in s.down[p.index()].iter().enumerate() {
                        s.sys_events
                            .push((s.history.len(), Event::Showed(p, j as u8, c)));
                    }
                }
                let pot = s.betting.pot() + Chips(s.carry.pot);
                for pen in stun_amounts(
                    pot,
                    losers,
                    s.setup.button,
                    s.betting.stacks(),
                    self.stun_rule(),
                ) {
                    if pen.amount > Chips::ZERO {
                        s.betting.post_dead(pen.seat, pen.amount);
                    }
                }
            }
        }
    }

    fn stun_rule(&self) -> StunRule {
        StunRule {
            base_cap: Chips(1_000),
            step: self.config.stun_step,
        }
    }

    /// Bullet: offer a card to every seat still in that is short of the target.
    fn start_take_round(&self, s: &mut State) {
        let target = match self.script.steps[s.pc] {
            Step::TakeRounds { target, .. } => target as usize,
            _ => 5,
        };
        s.take_betting = false;
        s.prompts = s
            .live_from_left()
            .into_iter()
            .filter(|p| s.own(*p).len() < target)
            .map(|p| (p, Prompt::Take))
            .collect();
    }

    /// Push: serve the next seat (its street card, or the card pushed to it).
    fn push_next(&self, s: &mut State) -> bool {
        while let Some(task) = s.push_tasks.pop_front() {
            match task {
                PushTask::Serve(q) if s.betting.is_folded(q) => continue,
                PushTask::Serve(q) => {
                    if s.incoming.is_some() {
                        s.prompts.push_back((q, Prompt::KeepOrReplace));
                    } else {
                        s.queue.push_back(Slot::Up(q));
                        // The last seat served this street can't push (D-PU-2).
                        let more = s
                            .push_tasks
                            .iter()
                            .any(|t| matches!(t, PushTask::Serve(r) if !s.betting.is_folded(*r)));
                        if more {
                            s.push_tasks.push_front(PushTask::Offer(q));
                        }
                    }
                    return true;
                }
                PushTask::Offer(q) => {
                    if !s.betting.is_folded(q) && !s.stock.is_empty() {
                        s.prompts.push_back((q, Prompt::PushOrKeep));
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Pay up to `amount` into the pot (pushes, bids, bought cards).
    fn pay(s: &mut State, p: Seat, amount: Chips) {
        let a = amount.min(s.betting.stacks()[p.index()]);
        if a > Chips::ZERO {
            s.betting.post_dead(p, a);
        }
    }

    /// Everyone's pass selections are in: move the cards (all at once).
    fn resolve_passes(&self, s: &mut State) {
        let live: Vec<Seat> = s
            .setup
            .seats()
            .filter(|&p| !s.betting.is_folded(p))
            .collect();
        let k = live.len();
        let mut moving: Vec<(Seat, SmallVec<[Card; 8]>)> = Vec::new();
        for (i, &q) in live.iter().enumerate() {
            let m = s.pass_sel[q.index()].take().unwrap_or(0);
            let to = if s.direction == 0 {
                live[(i + 1) % k]
            } else {
                live[(i + k - 1) % k]
            };
            let hand = &mut s.down[q.index()];
            let mut kept = SmallVec::new();
            let mut gone = SmallVec::new();
            for (j, &c) in hand.iter().enumerate() {
                if m & (1 << j) != 0 {
                    gone.push(c);
                } else {
                    kept.push(c);
                }
            }
            *hand = kept;
            moving.push((to, gone));
        }
        for (to, cards) in moving {
            s.down[to.index()].extend(cards.iter().copied());
            s.received[to.index()].extend(cards);
        }
        s.sys_events.push((s.history.len(), Event::PassesMade));
    }

    /// Everyone's In/Out is in: reveal it; Out folds, In pays the price.
    fn resolve_in_out(&self, s: &mut State, price: Chips) {
        let price = match s.named {
            Some((_, _, units)) => Chips(50 * units as i64),
            None => price,
        };
        let mut ins = 0u16;
        for p in s.setup.seats() {
            match s.in_out[p.index()].take() {
                Some(1) => {
                    ins |= 1 << p.0;
                    if price > Chips::ZERO {
                        let pay = price.min(s.betting.stacks()[p.index()]);
                        s.betting.post_dead(p, pay);
                    }
                }
                Some(_) => {
                    s.betting.fold_out(p);
                    s.outs |= 1 << p.0;
                }
                None => {}
            }
        }
        if ins == 0 {
            s.void = true;
        }
        s.sys_events
            .push((s.history.len(), Event::InOutResolved(ins)));
    }

    /// Move on until a seat must act, a card must be dealt, or the hand is over.
    fn advance(&self, s: &mut State) {
        loop {
            if s.done {
                return;
            }
            // One player left ends the hand, except 3-2-1 Drop, where a lone player still plays
            // the dummy hand.
            let lone_plays_on = self.script.resolution == Resolution::Drop
                && s.betting.live_count() == 1
                && s.pc < self.script.steps.len();
            if s.betting.live_count() <= 1 && !lone_plays_on {
                s.done = true;
                s.queue.clear();
                return;
            }
            if !s.queue.is_empty() {
                // D-DRAW-1: out of cards, so the discards are shuffled and dealt from.
                if s.stock.is_empty() && !s.muck.is_empty() {
                    s.stock = s.muck;
                    s.muck = CardSet::EMPTY;
                    s.reshuffles.push(s.history.len());
                }
                return;
            }
            if !s.prompts.is_empty() {
                while s
                    .prompts
                    .front()
                    .is_some_and(|&(p, _)| s.betting.is_folded(p))
                {
                    s.prompts.pop_front();
                }
                if !s.prompts.is_empty() {
                    return;
                }
            }
            if s.pc >= self.script.steps.len() {
                s.done = true;
                return;
            }
            if !s.entered {
                self.enter(s);
                continue;
            }
            let finished = match self.script.steps[s.pc] {
                Step::Down(_) | Step::Up(_) | Step::Board { .. } => true,
                Step::Bet => s.betting.to_act().is_none(),
                Step::Declare
                | Step::Draw { .. }
                | Step::Discard { .. }
                | Step::PairUp
                | Step::Pass(_)
                | Step::InOut { .. }
                | Step::Order
                | Step::Spit
                | Step::ChooseDirection => {
                    while s.pending.front().is_some_and(|&p| s.betting.is_folded(p)) {
                        s.pending.pop_front();
                    }
                    let done = s.pending.is_empty();
                    if done {
                        match self.script.steps[s.pc] {
                            Step::Pass(_) => self.resolve_passes(s),
                            Step::InOut { price } => {
                                self.resolve_in_out(s, price);
                                if s.void {
                                    s.done = true;
                                    return;
                                }
                            }
                            _ => {}
                        }
                    }
                    done
                }
                Step::Reveal
                | Step::Flip { .. }
                | Step::FillPool { .. }
                | Step::FlipPool { .. } => true,
                Step::UpOrDown | Step::BuyCard { .. } | Step::DummyIfLone { .. } => true,
                Step::NumbersFlip | Step::DealerUp => true,
                Step::Tricks { .. } => {
                    if s.tricks_left > 0 {
                        return;
                    }
                    true
                }
                Step::HitRounds { cap } => {
                    if !s.hit_betting {
                        if !s.hit_any {
                            true // everyone stood
                        } else {
                            let first = self.first_to_act(s);
                            s.betting.start_street(s.streets, first);
                            s.streets += 1;
                            s.hit_betting = true;
                            continue;
                        }
                    } else if s.betting.to_act().is_some() {
                        false
                    } else {
                        s.hit_rounds += 1;
                        if s.hit_rounds >= cap {
                            true
                        } else {
                            self.start_hit_round(s);
                            continue;
                        }
                    }
                }
                Step::PlayRound { wild, three } => {
                    if !s.pending.is_empty() {
                        return;
                    }
                    self.resolve_play_round(s, wild, three);
                    if s.done {
                        return;
                    }
                    true
                }
                Step::NameGame { .. } => {
                    if s.named.is_none() && !s.pending.is_empty() {
                        return;
                    }
                    if s.named.is_none() {
                        // Everyone passed: redeal (D-SAM-2).
                        s.done = true;
                        return;
                    }
                    s.pending.clear();
                    true
                }
                Step::PushStreet => {
                    if self.push_next(s) {
                        continue;
                    }
                    true
                }
                Step::TakeRounds { target, cap } => {
                    if !s.take_betting {
                        let first = self.first_to_act(s);
                        s.betting.start_street(s.streets, first);
                        s.streets += 1;
                        s.take_betting = true;
                    }
                    if s.betting.to_act().is_some() {
                        false
                    } else {
                        s.take_rounds += 1;
                        let short: Vec<Seat> = s
                            .live_from_left()
                            .into_iter()
                            .filter(|p| s.own(*p).len() < target as usize)
                            .collect();
                        if short.is_empty() {
                            true
                        } else if s.take_rounds < cap {
                            self.start_take_round(s);
                            continue;
                        } else {
                            // D-BU-3: out of rounds; short hands are filled (up, last down).
                            for p in short {
                                let have = s.own(p).len();
                                for k in have..target as usize {
                                    s.queue.push_back(if k + 1 == target as usize {
                                        Slot::Down(p)
                                    } else {
                                        Slot::Up(p)
                                    });
                                }
                            }
                            s.pc += 1;
                            s.entered = false;
                            continue;
                        }
                    }
                }
                Step::BidRound { .. } => match s.bid_phase {
                    // Middle cards are out: collect bids.
                    0 => {
                        s.bid_phase = 1;
                        s.pending = s.live_from_left().into();
                        continue;
                    }
                    1 => {
                        while s.pending.front().is_some_and(|&p| s.betting.is_folded(p)) {
                            s.pending.pop_front();
                        }
                        if !s.pending.is_empty() {
                            return;
                        }
                        // Reveal and pay the bids; highest bid picks first, ties to the dealer's
                        // left.
                        let live = s.live_from_left();
                        let mut order: Vec<(u8, usize, Seat)> = Vec::new();
                        for (i, &p) in live.iter().enumerate() {
                            let b = s.bids[p.index()].take().unwrap_or(0);
                            Self::pay(s, p, Chips(50 * b as i64));
                            s.sys_events.push((s.history.len(), Event::BidShown(p, b)));
                            order.push((b, i, p));
                        }
                        order.sort_by_key(|&(b, i, _)| (std::cmp::Reverse(b), i));
                        s.prompts = order
                            .into_iter()
                            .map(|(_, _, p)| (p, Prompt::Pick))
                            .collect();
                        s.bid_phase = 2;
                        continue;
                    }
                    _ => true,
                },
                Step::RedRiver { board } => {
                    if !s.red_betting {
                        let first = self.first_to_act(s);
                        s.betting.start_street(s.streets, first);
                        s.streets += 1;
                        s.red_betting = true;
                    }
                    if s.betting.to_act().is_some() {
                        false
                    } else {
                        let red = s.boards[board as usize]
                            .last()
                            .is_some_and(|c| matches!(c.suit(), Suit::Hearts | Suit::Diamonds));
                        // D-RR-3: the board ends if the deck runs out.
                        if red && !s.stock.is_empty() {
                            s.queue.push_back(Slot::Board(board));
                            s.red_betting = false;
                            continue;
                        }
                        true
                    }
                }
            };
            if !finished {
                return;
            }
            s.pc += 1;
            s.entered = false;
        }
    }

    /// Legal throw-away masks for `p` in the current draw/discard step.
    fn throw_masks(&self, s: &State, p: Seat, out: &mut Vec<u8>) {
        out.clear();
        let hand = &s.down[p.index()];
        let k = hand.len() as u32;
        match self.step(s) {
            Some(Step::Draw { max, ace_bonus }) => {
                // The bonus draw is offered whatever you hold, so the menu reveals nothing
                // (C6); keeping a non-Ace fouls the hand (D-3BM-1).
                for m in 0u32..(1 << k) {
                    let c = m.count_ones() as u8;
                    if c <= max || (ace_bonus && c == max + 1) {
                        out.push(m as u8);
                    }
                }
            }
            Some(Step::Discard { n, pairs: false }) => {
                out.extend(
                    (0u32..(1 << k))
                        .filter(|m| m.count_ones() as u8 == n)
                        .map(|m| m as u8),
                );
            }
            Some(Step::Discard { pairs: true, .. }) => {
                out.extend((0..k / 2).map(|j| 0b11u8 << (2 * j)));
            }
            Some(Step::Spit) => out.extend((0..k).map(|j| 1u8 << j)),
            _ => {}
        }
    }

    /// The cards wild for `p`: the script's, plus (Low in the Hole) every card of the rank of
    /// `p`'s lowest face-down card, ace high.
    fn wild_for(&self, s: &State, p: Seat) -> CardSet {
        let mut wild = self.script.wild;
        if self.script.low_hole_wild
            && let Some(r) = s.down[p.index()]
                .iter()
                .filter(|c| !c.is_joker())
                .map(|c| c.rank())
                .min()
        {
            for su in Suit::ALL {
                wild.insert(Card::new(r, su));
            }
        }
        wild
    }

    /// Tempest: only folding, or putting in everything (raising all-in, or calling when the call
    /// takes the whole stack). Checking is allowed only with nothing owed.
    fn shove_or_fold(&self, s: &State, p: Seat, b: BetAction) -> bool {
        let stack = s.betting.stacks()[p.index()];
        let owed = s.betting.owed(p);
        match b {
            BetAction::Fold => true,
            BetAction::CheckCall => owed == Chips::ZERO || owed >= stack,
            BetAction::RaiseTo(to) => to - s.betting.street_committed(p) >= stack,
        }
    }

    /// How many answers the current prompt has.
    fn prompt_options(&self, s: &State) -> u8 {
        match s.prompts.front() {
            Some((_, Prompt::Pick)) => s.middle.len() as u8,
            Some((_, Prompt::Buy)) if s.stock.is_empty() => 1,
            Some((_, Prompt::Buy)) => 3,
            Some((_, Prompt::KeepOrReplace)) => {
                // Replacing takes one extra card: only while the stock covers everyone's
                // remaining cards (the pushed card fills the receiver's own need).
                let need: usize = s
                    .live_from_left()
                    .iter()
                    .map(|&p| (self.script.hand_cards as usize).saturating_sub(s.own(p).len()))
                    .sum::<usize>()
                    .saturating_sub(1)
                    + s.queue.len();
                if s.stock.len() > need { 2 } else { 1 }
            }
            Some(_) => 2,
            None => 0,
        }
    }

    /// Swap masks `p` may use now (Swap Hold'em), or none.
    fn swap_masks(&self, s: &State, p: Seat) -> Vec<u8> {
        if !self.script.swap_instead_of_fold
            || s.swapped & (1 << p.0) != 0
            || !s.betting.options(p).can_fold
            || s.stock.len() + s.muck.len() < 2
        {
            return Vec::new();
        }
        let k = s.down[p.index()].len() as u32;
        (1u32..(1 << k))
            .filter(|m| (1..=2).contains(&m.count_ones()))
            .map(|m| m as u8)
            .collect()
    }

    fn values(&self, s: &State) -> Vec<Vec<Option<HandValue>>> {
        let n = s.n() as usize;
        let shares = self.shares();
        let mut out = vec![vec![None; n]; shares.len()];
        if !s.is_showdown() {
            return out;
        }
        for p in s.live_from_left() {
            let own = s.own(p);
            let wild = self.wild_for(s, p);
            for (k, rule) in shares.iter().enumerate() {
                let b = rule.board as usize;
                let board: &[Card] = s.boards.get(b).map_or(&[], |x| x.as_slice());
                out[k][p.index()] = if self.script.lines.is_empty() {
                    hand::share_value(rule, &own, board, wild)
                } else {
                    // Shaped board: the board cards must come from one line.
                    let pos = &s.board_pos[b];
                    self.script
                        .lines
                        .iter()
                        .filter_map(|line| {
                            let cards: SmallVec<[Card; 8]> = board
                                .iter()
                                .zip(pos)
                                .filter(|(_, at)| line.contains(at))
                                .map(|(&c, _)| c)
                                .collect();
                            hand::share_value(rule, &own, &cards, wild)
                        })
                        .max()
                };
            }
        }
        out
    }

    fn winnings(&self, s: &State) -> Vec<Chips> {
        self.winnings_plus(s, Chips::ZERO)
    }

    /// Pot winnings with `carried` chips added to the pot.
    fn winnings_plus(&self, s: &State, carried: Chips) -> Vec<Chips> {
        if s.gin != 0 {
            // Numbers: an empty hand scoops (several split it).
            let n = s.n() as usize;
            let mut out = vec![Chips::ZERO; n];
            let total = s.betting.pot();
            split(
                total,
                s.gin,
                &order_from_button(s.setup.button, s.n()),
                &mut out,
            );
            return out;
        }
        if s.void {
            // Everyone went Out: every chip goes back.
            return s.betting.contributed().to_vec();
        }
        let n = s.n() as usize;
        let folded = (0..n)
            .filter(|&i| s.betting.is_folded(Seat(i as u8)))
            .fold(0u16, |m, i| m | 1 << i);
        let pots = build_pots(
            &s.betting.live_contributed(),
            s.betting.dead_total() + carried,
            folded,
        );
        let values = self.values(s);
        let shares: Vec<Share<'_, HandValue>> = self
            .shares()
            .iter()
            .zip(&values)
            .map(|(r, v)| Share {
                name: r.name,
                values: v,
            })
            .collect();
        let order = order_from_button(s.setup.button, s.n());
        let claims: Option<Vec<Option<Claim>>> = self.declare().map(|_| {
            s.declarations
                .iter()
                .map(|d| d.map(DeclareKind::claim))
                .collect()
        });
        award_shares(&pots, &shares, claims.as_deref(), &order)
    }

    /// Payoffs for this hand and what carries to the next (see [`Resolution`]).
    fn settle(&self, s: &State) -> (Vec<Chips>, ScriptCarry) {
        let n = s.n() as usize;
        let contributed: Vec<Chips> = s.betting.contributed().to_vec();
        let paid: Chips = contributed.iter().copied().sum();
        let total = paid + Chips(s.carry.pot);
        let mut carry = s.carry.clone();
        carry.hands += 1;
        if carry.legs.len() != n {
            carry.legs = vec![0; n];
        }
        carry.strikes = s.strikes.clone();
        let order = order_from_button(s.setup.button, s.n());
        let all = (1u16 << n) - 1;
        let net = |win: &[Chips], extra: &[Chips]| -> Vec<Chips> {
            (0..n).map(|i| win[i] - contributed[i] - extra[i]).collect()
        };
        let zeros = vec![Chips::ZERO; n];
        let live = s.betting.live_mask();
        // Orbit stun variations: is this the orbit's last hand?
        let in_game = (all & !carry.out).count_ones();
        let last_of_orbit = self.script.orbit_stun && carry.hands >= in_game;
        // Half a stun from each loser to the winners (orbit's last hand).
        let half_stuns = |losers: u16, winners: u16, total: Chips| -> (Vec<Chips>, Vec<Chips>) {
            let mut pay = vec![Chips::ZERO; n];
            let mut win = vec![Chips::ZERO; n];
            let mut collected = Chips::ZERO;
            for pen in stun_amounts(
                total,
                losers,
                s.setup.button,
                s.betting.stacks(),
                self.stun_rule(),
            ) {
                let h = Chips(pen.amount.0 / 2);
                pay[pen.seat.index()] = h;
                collected += h;
            }
            split(total + collected, winners, &order, &mut win);
            (win, pay)
        };
        if self.out_is_out() {
            carry.out |= (all & !live & !s.setup.dealt_out) | s.outs;
        }
        // Everything stays in the pot for the next hand.
        let carry_all = |mut c: ScriptCarry| {
            c.pot = total.0;
            (net(&zeros, &zeros), c)
        };
        let best = |rule: &ShareRule, seats: u16| -> (u16, Vec<Option<HandValue>>) {
            let mut vals = vec![None; n];
            for i in (0..n).filter(|i| seats & (1 << i) != 0) {
                let p = Seat(i as u8);
                vals[i] = hand::share_value(rule, &s.own(p), &[], self.wild_for(s, p));
            }
            let top = vals.iter().flatten().max().copied();
            let w = (0..n)
                .filter(|&i| vals[i].is_some() && vals[i] == top)
                .fold(0u16, |m, i| m | 1 << i);
            (w, vals)
        };
        match self.script.resolution {
            Resolution::Normal => {
                if s.void {
                    return (zeros.clone(), carry);
                }
                let win = self.winnings(s);
                (net(&win, &zeros), carry)
            }
            Resolution::Tempest => {
                let win = self.winnings(s);
                if s.is_showdown() {
                    carry.over = true;
                } else {
                    carry.level += 1;
                }
                (net(&win, &zeros), carry)
            }
            Resolution::Legs { need } => {
                let winners = if live.count_ones() == 1 {
                    live
                } else {
                    best(&self.script.shares[0], live).0
                };
                for i in (0..n).filter(|i| winners & (1 << i) != 0) {
                    carry.legs[i] += 1;
                }
                let finished = (0..n)
                    .filter(|&i| carry.legs[i] >= need)
                    .fold(0u16, |m, i| m | 1 << i);
                if finished != 0 {
                    let mut win = vec![Chips::ZERO; n];
                    split(total, finished, &order, &mut win);
                    carry.pot = 0;
                    carry.over = true;
                    return (net(&win, &zeros), carry);
                }
                // Everyone still in the game has a leg: the counts reset.
                let in_game = all & !carry.out;
                if (0..n)
                    .filter(|i| in_game & (1 << i) != 0)
                    .all(|i| carry.legs[i] > 0)
                {
                    carry.legs = vec![0; n];
                }
                carry_all(carry)
            }
            Resolution::StunOrbit => {
                let winners = if live.count_ones() <= 1 {
                    live
                } else {
                    let win = self.winnings_plus(s, Chips(s.carry.pot));
                    (0..n)
                        .filter(|&i| win[i] > Chips::ZERO)
                        .fold(0u16, |m, i| m | 1 << i)
                };
                let losers = live & !winners;
                if winners == 0 {
                    return carry_all(carry);
                }
                if last_of_orbit {
                    let (win, pay) = half_stuns(losers, winners, total);
                    carry.pot = 0;
                    carry.over = true;
                    return (net(&win, &pay), carry);
                }
                let mut win = vec![Chips::ZERO; n];
                split(total, winners, &order, &mut win);
                let mut stun = vec![Chips::ZERO; n];
                for pen in stun_amounts(
                    total,
                    losers,
                    s.setup.button,
                    s.betting.stacks(),
                    self.stun_rule(),
                ) {
                    stun[pen.seat.index()] = pen.amount;
                }
                carry.pot = stun.iter().map(|x| x.0).sum();
                (net(&win, &stun), carry)
            }
            Resolution::Drop if last_of_orbit => {
                // The orbit's last hand: the best hand takes the pot and half stuns; if nobody
                // wins it, the pot is split evenly (D-DROP-5).
                let ins = live.count_ones();
                let lone_wins = ins == 1 && {
                    let p = Seat(live.trailing_zeros() as u8);
                    let rule = self.script.shares[0];
                    hand::share_value(&rule, &s.own(p), &[], self.script.wild)
                        > hand::share_value(&rule, &s.boards[0], &[], self.script.wild)
                };
                carry.over = true;
                carry.pot = 0;
                if ins >= 2 {
                    let (winners, _) = best(&self.script.shares[0], live);
                    let (win, pay) = half_stuns(live & !winners, winners, total);
                    (net(&win, &pay), carry)
                } else if lone_wins {
                    let mut win = vec![Chips::ZERO; n];
                    split(total, live, &order, &mut win);
                    (net(&win, &zeros), carry)
                } else {
                    let mut win = vec![Chips::ZERO; n];
                    split(
                        total,
                        all & !carry.out & !s.setup.dealt_out,
                        &order,
                        &mut win,
                    );
                    (net(&win, &zeros), carry)
                }
            }
            Resolution::Drop => {
                let ins: Vec<Seat> = (0..n as u8)
                    .map(Seat)
                    .filter(|p| live & (1 << p.0) != 0)
                    .collect();
                let stacks = s.betting.stacks();
                match ins.len() {
                    0 => carry_all(carry),
                    1 => {
                        let p = ins[0];
                        let rule = self.script.shares[0];
                        let mine = hand::share_value(&rule, &s.own(p), &[], self.script.wild);
                        let dummy = hand::share_value(&rule, &s.boards[0], &[], self.script.wild);
                        if mine > dummy {
                            let mut win = vec![Chips::ZERO; n];
                            win[p.index()] = total;
                            carry.pot = 0;
                            carry.over = true;
                            (net(&win, &zeros), carry)
                        } else {
                            let mut stun = vec![Chips::ZERO; n];
                            for pen in stun_amounts(
                                total,
                                1 << p.0,
                                s.setup.button,
                                stacks,
                                self.stun_rule(),
                            ) {
                                stun[pen.seat.index()] = pen.amount;
                            }
                            carry.pot = total.0 + stun.iter().map(|x| x.0).sum::<i64>();
                            (net(&zeros, &stun), carry)
                        }
                    }
                    _ => {
                        let (winners, _) = best(&self.script.shares[0], live);
                        let mut win = vec![Chips::ZERO; n];
                        split(total, winners, &order, &mut win);
                        let mut stun = vec![Chips::ZERO; n];
                        for pen in stun_amounts(
                            total,
                            live & !winners,
                            s.setup.button,
                            stacks,
                            self.stun_rule(),
                        ) {
                            stun[pen.seat.index()] = pen.amount;
                        }
                        carry.pot = stun.iter().map(|x| x.0).sum();
                        carry.over = carry.pot == 0;
                        (net(&win, &stun), carry)
                    }
                }
            }
            Resolution::Booray => {
                if s.void || live == 0 {
                    return carry_all(carry);
                }
                let played: Vec<usize> = (0..n).filter(|i| live & (1 << i) != 0).collect();
                // D-BOO-5: a lone player takes every trick.
                let lone = played.len() == 1;
                let scooper = if lone {
                    Some(played[0])
                } else {
                    played.iter().copied().find(|&i| s.tricks_won[i] >= 3)
                };
                let fried: u16 = played
                    .iter()
                    .filter(|&&i| !lone && s.tricks_won[i] == 0)
                    .fold(0, |m, &i| m | 1 << i);
                let mut win = vec![Chips::ZERO; n];
                let mut pay = vec![Chips::ZERO; n];
                if let Some(w) = scooper {
                    win[w] = total;
                    if fried == 0 {
                        // The game ends: nobody pays into a pot that won't be played (D-BOO-3).
                        carry.pot = 0;
                        carry.over = true;
                        return (net(&win, &zeros), carry);
                    }
                }
                let base = if scooper.is_some() {
                    Chips::ZERO
                } else {
                    total
                };
                for pen in stun_amounts(
                    total,
                    fried,
                    s.setup.button,
                    s.betting.stacks(),
                    self.stun_rule(),
                ) {
                    pay[pen.seat.index()] = pen.amount;
                }
                for &i in &played {
                    if Some(i) != scooper && fried & (1 << i) == 0 {
                        pay[i] = Chips(100).min(s.betting.stacks()[i]);
                    }
                }
                carry.pot = base.0 + pay.iter().map(|x| x.0).sum::<i64>();
                (net(&win, &pay), carry)
            }
            Resolution::Strikes { .. } => {
                if let Some(w) = s.strike_winner {
                    let mut win = vec![Chips::ZERO; n];
                    win[w.index()] = total;
                    carry.pot = 0;
                    carry.over = true;
                    return (net(&win, &zeros), carry);
                }
                carry_all(carry)
            }
            Resolution::Declarer => {
                let Some((d, game, _)) = s.named else {
                    return carry_all(carry);
                };
                let ranking = [
                    Ranking::High,
                    Ranking::LowA6,
                    Ranking::NumbersHigh,
                    Ranking::NumbersLow,
                ][game as usize];
                let rule = ShareRule {
                    name: "Named",
                    ranking,
                    build: Build::Own,
                    board: 0,
                };
                let (winners, _) = best(&rule, live);
                if winners == 1 << d.0 {
                    let mut win = vec![Chips::ZERO; n];
                    win[d.index()] = total;
                    carry.pot = 0;
                    carry.over = true;
                    (net(&win, &zeros), carry)
                } else {
                    let mut stun = vec![Chips::ZERO; n];
                    for pen in stun_amounts(
                        total,
                        1 << d.0,
                        s.setup.button,
                        s.betting.stacks(),
                        self.stun_rule(),
                    ) {
                        stun[pen.seat.index()] = pen.amount;
                    }
                    carry.pot = total.0 + stun.iter().map(|x| x.0).sum::<i64>();
                    (net(&zeros, &stun), carry)
                }
            }
        }
    }

    fn out_is_out(&self) -> bool {
        self.script.out_is_out
    }

    /// Whether `viewer` can see `p`'s face-down cards right now.
    fn sees_down(&self, s: &State, viewer: Seat, p: Seat) -> bool {
        let showdown_shown = s.is_showdown() && !s.betting.is_folded(p);
        match self.script.visibility {
            Visibility::Normal => viewer == p || showdown_shown,
            Visibility::Showtime => viewer == p || showdown_shown || s.betting.is_folded(p),
            Visibility::Indian => viewer != p || showdown_shown,
        }
    }

    /// Throw away the masked face-down cards of `p` into the muck.
    fn throw(s: &mut State, p: Seat, m: u8) {
        let hand = &mut s.down[p.index()];
        let mut kept: SmallVec<[Card; 8]> = SmallVec::new();
        for (i, &c) in hand.iter().enumerate() {
            if m & (1 << i) != 0 {
                s.muck.insert(c);
            } else {
                kept.push(c);
            }
        }
        *hand = kept;
    }
}

/// The single card a seat keeps (and shows) on a bonus draw.
fn kept_card(hand: &[Card], m: u8) -> Card {
    hand.iter()
        .enumerate()
        .find(|(i, _)| m & (1 << i) == 0)
        .map(|(_, &c)| c)
        .expect("a bonus draw keeps one card")
}

const DECLARE_OPTIONS: [u8; 3] = [0, 1, 2];
/// Tempest (D-TEM-1): Giant Blind $2; at most 10 Giant Blinds per player per hand.
const TEMPEST_GIANT_BLIND: Chips = Chips(200);
const TEMPEST_CAP: Chips = Chips(2000);

/// One dealt card in a determinization: a hidden slot or a known card.
#[derive(Copy, Clone, Debug)]
enum DealRef {
    Slot(usize),
    Known(Card),
}

impl Game for Scripted {
    type State = State;
    type Action = Action;
    type Observation = Observation;
    type Config = Config;
    type Carry = ScriptCarry;

    fn new(config: Config) -> Scripted {
        let mut script = games::script(&config.game)
            .unwrap_or_else(|| panic!("unknown scripted game {:?}", config.game));
        for v in &config.variants {
            let found = script.variants.iter().find(|x| x.name == v).copied();
            match found {
                Some(x) => (x.apply)(&mut script),
                None => panic!(
                    "{}: unknown variant {v:?}; known: {:?}",
                    script.id, script.variants
                ),
            }
        }
        let meta = GameMeta {
            id: script.id,
            version: script.version,
            display_name: script.name,
            seats: 2..=script.max_seats,
            stake_model: StakeModel::Bankroll,
            length: script.length,
            rules_ref: script.rules_ref,
        };
        Scripted {
            config,
            script,
            meta,
        }
    }

    fn meta(&self) -> &GameMeta {
        &self.meta
    }

    fn new_hand_with(&self, setup: &HandSetup, carry: &ScriptCarry) -> State {
        let n = setup.num_seats as usize;
        let tempest = self.script.resolution == Resolution::Tempest;
        let structure = if tempest {
            Structure::NoLimit
        } else {
            self.config.structure
        };
        let min_bet = match structure {
            Structure::SpreadLimit { min_inc, .. } => min_inc,
            Structure::FixedLimit { small, .. } => small,
            _ => Chips(100),
        };
        // Tempest caps what anyone can wager in a hand at 10 Giant Blinds (D-TEM-1).
        let stacks: Vec<Chips> = if tempest {
            setup.stacks.iter().map(|&x| x.min(TEMPEST_CAP)).collect()
        } else {
            setup.stacks.clone()
        };
        let mut betting = Betting::new(structure, &stacks, min_bet);
        for p in setup.seats().filter(|&p| !setup.dealt_in(p)) {
            betting.fold_out(p);
        }
        let dealt_in: Vec<Seat> = setup.seats().filter(|&p| setup.dealt_in(p)).collect();
        // Tempest has its own money rules (D-TEM-1): no dealer ante.
        if setup.first_hand_of_game && self.config.dealer_ante > Chips::ZERO && !tempest {
            betting.post_dead(
                setup.chooser,
                self.config.dealer_ante.min(stacks[setup.chooser.index()]),
            );
        }
        let ante = if tempest {
            Chips(50 * (carry.level as i64 + 1))
        } else {
            self.script.ante_each
        };
        if ante > Chips::ZERO && (!self.script.ante_first_hand_only || carry.hands == 0) {
            for &p in &dealt_in {
                betting.post_dead(p, ante.min(stacks[p.index()]));
            }
        }
        let mut s = State {
            setup: setup.clone(),
            pc: 0,
            entered: false,
            red_betting: false,
            streets: 0,
            down: vec![SmallVec::new(); n],
            up: vec![SmallVec::new(); n],
            boards: vec![SmallVec::new(); self.script.boards as usize],
            stock: self.deck(),
            muck: CardSet::EMPTY,
            reshuffles: Vec::new(),
            swapped: 0,
            ace_shows: Vec::new(),
            direction: 0,
            pass_sel: vec![None; n],
            in_out: vec![None; n],
            sys_events: Vec::new(),
            received: vec![SmallVec::new(); n],
            void: false,
            board_pos: vec![SmallVec::new(); self.script.boards as usize],
            pool: CardSet::EMPTY,
            spits: Vec::new(),
            prompts: VecDeque::new(),
            push_tasks: VecDeque::new(),
            incoming: None,
            take_rounds: 0,
            take_betting: false,
            middle: SmallVec::new(),
            bids: vec![None; n],
            bid_phase: 0,
            carry: carry.clone(),
            strikes: if carry.strikes.len() == n {
                carry.strikes.clone()
            } else {
                vec![0; n]
            },
            strike_winner: None,
            plays: vec![None; n],
            named: None,
            outs: 0,
            hit_rounds: 0,
            hit_any: false,
            hit_betting: false,
            gin: 0,
            trump: None,
            trump_played: false,
            trick: SmallVec::new(),
            tricks_won: vec![0; n],
            tricks_left: 0,
            betting,
            queue: VecDeque::new(),
            deal_log: Vec::new(),
            declarations: vec![None; n],
            pending: VecDeque::new(),
            history: Vec::new(),
            done: false,
        };
        self.advance(&mut s);
        s
    }

    fn to_act(&self, s: &State) -> ToAct {
        if !s.queue.is_empty() {
            return ToAct::Chance;
        }
        if s.done {
            return ToAct::Terminal;
        }
        if let Some(&(p, _)) = s.prompts.front() {
            return ToAct::Seat(p);
        }
        match self.script.steps[s.pc] {
            Step::Bet
            | Step::RedRiver { .. }
            | Step::TakeRounds { .. }
            | Step::HitRounds { .. } => {
                ToAct::Seat(s.betting.to_act().expect("advance left a closed street"))
            }
            _ => ToAct::Seat(*s.pending.front().expect("step with nobody pending")),
        }
    }

    fn legal_actions(&self, s: &State, out: &mut Vec<Action>) {
        out.clear();
        match self.to_act(s) {
            ToAct::Chance => out.extend(s.chance_pool().iter().map(Action::Deal)),
            ToAct::Terminal => {}
            ToAct::Seat(_) if !s.prompts.is_empty() => {
                out.extend((0..self.prompt_options(s)).map(Action::Choose))
            }
            ToAct::Seat(p) if matches!(self.script.steps[s.pc], Step::Tricks { .. }) => {
                out.extend(self.playable(s, p).into_iter().map(Action::PlayCard))
            }
            ToAct::Seat(_) if matches!(self.script.steps[s.pc], Step::PlayRound { .. }) => {
                out.extend([Action::Play(1), Action::Play(0)])
            }
            ToAct::Seat(_) if matches!(self.script.steps[s.pc], Step::NameGame { .. }) => {
                if let Step::NameGame { max_units } = self.script.steps[s.pc] {
                    out.push(Action::Name(0));
                    for game in 0..4u8 {
                        for units in 1..=max_units {
                            out.push(Action::Name(1 + game * 4 + (units - 1)));
                        }
                    }
                }
            }
            ToAct::Seat(p)
                if matches!(self.script.steps[s.pc], Step::InOut { .. })
                    && s.named.is_some_and(|(d, _, _)| d == p) =>
            {
                out.push(Action::InOut(1)); // the declarer is always in
            }
            ToAct::Seat(_) if matches!(self.script.steps[s.pc], Step::BidRound { .. }) => {
                if let Step::BidRound { max_units } = self.script.steps[s.pc] {
                    out.extend((0..=max_units).map(Action::Bid));
                }
            }
            ToAct::Seat(p) => match self.script.steps[s.pc] {
                Step::Declare => out.extend(DECLARE_OPTIONS.map(Action::Declare)),
                Step::PairUp => out.extend((0..15).map(Action::Arrange)),
                Step::ChooseDirection => out.extend([Action::Direction(0), Action::Direction(1)]),
                Step::InOut { .. } => out.extend([Action::InOut(1), Action::InOut(0)]),
                Step::Order => {
                    let k = s.down[p.index()].len();
                    out.extend((0..factorial(k)).map(|i| Action::Order(i as u8)));
                }
                Step::Pass(n) => {
                    let k = s.down[p.index()].len() as u32;
                    out.extend(
                        (0u32..(1 << k))
                            .filter(|m| m.count_ones() == n as u32)
                            .map(|m| Action::Pass(m as u8)),
                    );
                }
                Step::Draw { .. } | Step::Discard { .. } | Step::Spit => {
                    let mut masks = Vec::new();
                    self.throw_masks(s, p, &mut masks);
                    out.extend(masks.into_iter().map(Action::Draw));
                }
                _ if self.script.resolution == Resolution::Tempest => {
                    out.extend(
                        s.betting
                            .menu(p, &[BetSize::AllIn])
                            .into_iter()
                            .filter(|&b| self.shove_or_fold(s, p, b))
                            .map(Action::from_bet),
                    );
                }
                _ => {
                    out.extend(
                        s.betting
                            .menu(p, &self.config.bet_menu)
                            .into_iter()
                            .map(Action::from_bet),
                    );
                    out.extend(self.swap_masks(s, p).into_iter().map(Action::Swap));
                }
            },
        }
    }

    fn validate(&self, s: &State, a: Action) -> Result<(), IllegalAction> {
        match (self.to_act(s), a) {
            (ToAct::Chance, Action::Deal(c)) if s.chance_pool().contains(c) => Ok(()),
            (ToAct::Chance, _) => Err(IllegalAction("expected a deal of an undealt card".into())),
            (ToAct::Terminal, _) => Err(IllegalAction("the hand is over".into())),
            (ToAct::Seat(_), Action::Choose(k)) if !s.prompts.is_empty() => {
                if k < self.prompt_options(s) {
                    Ok(())
                } else {
                    Err(IllegalAction(format!(
                        "choose one of the {} options",
                        self.prompt_options(s)
                    )))
                }
            }
            (ToAct::Seat(_), _) if !s.prompts.is_empty() => {
                Err(IllegalAction("answer the question first".into()))
            }
            (ToAct::Seat(p), a) if matches!(self.script.steps[s.pc], Step::Tricks { .. }) => {
                match a {
                    Action::PlayCard(c) if self.playable(s, p).contains(&c) => Ok(()),
                    _ => Err(IllegalAction(
                        "play a card you hold: follow suit if you can; no trump lead until trump \
                         has been played"
                            .into(),
                    )),
                }
            }
            (ToAct::Seat(_), a) if matches!(self.script.steps[s.pc], Step::PlayRound { .. }) => {
                match a {
                    Action::Play(x) if x < 2 => Ok(()),
                    _ => Err(IllegalAction("play or drop".into())),
                }
            }
            (ToAct::Seat(_), a) if matches!(self.script.steps[s.pc], Step::NameGame { .. }) => {
                match (self.script.steps[s.pc], a) {
                    (Step::NameGame { max_units }, Action::Name(k)) if k <= 4 * max_units => Ok(()),
                    _ => Err(IllegalAction("name the game and price, or pass".into())),
                }
            }
            (ToAct::Seat(p), Action::InOut(0)) if s.named.is_some_and(|(d, _, _)| d == p) => {
                Err(IllegalAction("you named the game: you're in".into()))
            }
            (ToAct::Seat(_), a) if matches!(self.script.steps[s.pc], Step::BidRound { .. }) => {
                match (self.script.steps[s.pc], a) {
                    (Step::BidRound { max_units }, Action::Bid(b)) if b <= max_units => Ok(()),
                    (Step::BidRound { max_units }, _) => Err(IllegalAction(format!(
                        "bid 0 to {} (in 50¢ units)",
                        max_units
                    ))),
                    _ => unreachable!(),
                }
            }
            (ToAct::Seat(p), a) => match (self.script.steps[s.pc], a) {
                (Step::Declare, Action::Declare(k)) if k < 3 => Ok(()),
                (Step::Declare, _) => Err(IllegalAction(format!(
                    "declare {}",
                    self.declare()
                        .map_or(["High", "Low", "Both"], |d| d.names())
                        .join(", ")
                ))),
                (Step::PairUp, Action::Arrange(k)) if k < 15 => Ok(()),
                (Step::ChooseDirection, Action::Direction(d)) if d < 2 => Ok(()),
                (Step::ChooseDirection, _) => Err(IllegalAction(
                    "choose the passing direction: left or right".into(),
                )),
                (Step::InOut { .. }, Action::InOut(x)) if x < 2 => Ok(()),
                (Step::InOut { .. }, _) => Err(IllegalAction("say In or Out".into())),
                (Step::Order, Action::Order(i))
                    if (i as usize) < factorial(s.down[p.index()].len()) =>
                {
                    Ok(())
                }
                (Step::Order, _) => Err(IllegalAction(
                    "choose the order to reveal your cards".into(),
                )),
                (Step::Pass(n), Action::Pass(m)) => {
                    let k = s.down[p.index()].len();
                    if (m as u32) >> k == 0 && m.count_ones() == n as u32 {
                        Ok(())
                    } else {
                        Err(IllegalAction(format!("pass exactly {n} of your cards")))
                    }
                }
                (Step::Pass(n), _) => Err(IllegalAction(format!("pass {n} cards"))),
                (Step::PairUp, _) => Err(IllegalAction(
                    "split your six cards into three pairs".into(),
                )),
                (Step::Draw { .. } | Step::Discard { .. } | Step::Spit, Action::Draw(m)) => {
                    let mut masks = Vec::new();
                    self.throw_masks(s, p, &mut masks);
                    if masks.contains(&m) {
                        Ok(())
                    } else {
                        Err(IllegalAction(match self.step(s) {
                            Some(Step::Discard { pairs: true, .. }) => {
                                "throw away one of your pairs".into()
                            }
                            Some(Step::Discard { n, .. }) => {
                                format!("throw away exactly {n} card(s)")
                            }
                            Some(Step::Draw {
                                max,
                                ace_bonus: true,
                            }) => format!(
                                "throw away at most {max} card(s), or {} keeping (and showing) an Ace",
                                max + 1
                            ),
                            Some(Step::Draw { max, .. }) => {
                                format!("throw away at most {max} card(s)")
                            }
                            _ => "choose cards to throw away".into(),
                        }))
                    }
                }
                (Step::Draw { .. } | Step::Discard { .. } | Step::Spit, _) => {
                    Err(IllegalAction("choose which cards to throw away".into()))
                }
                (_, Action::Swap(m)) => {
                    if self.swap_masks(s, p).contains(&m) {
                        Ok(())
                    } else {
                        Err(IllegalAction(
                            "swap 1 or 2 cards, only when you could fold, once per street".into(),
                        ))
                    }
                }
                (_, a) => match a.as_bet() {
                    Some(b)
                        if self.script.resolution == Resolution::Tempest
                            && !self.shove_or_fold(s, p, b) =>
                    {
                        Err(IllegalAction("Tempest: all-in or fold".into()))
                    }
                    Some(b) => s.betting.validate(p, b),
                    None => Err(IllegalAction("it's a betting round".into())),
                },
            },
        }
    }

    fn apply(&self, s: &mut State, a: Action) {
        let actor = match a {
            Action::Deal(c) => {
                let slot = s.queue.pop_front().expect("deal with no slot");
                if let Slot::PoolFlip(_) = slot {
                    s.pool.remove(c);
                } else {
                    s.stock.remove(c);
                }
                match slot {
                    Slot::Down(p) => s.down[p.index()].push(c),
                    Slot::Up(p) => s.up[p.index()].push(c),
                    Slot::Board(b) | Slot::PoolFlip(b) => {
                        let pos = s.boards[b as usize].len() as u8;
                        s.boards[b as usize].push(c);
                        s.board_pos[b as usize].push(pos);
                    }
                    Slot::BoardAt(b, pos) => {
                        s.boards[b as usize].push(c);
                        s.board_pos[b as usize].push(pos);
                    }
                    Slot::Pool => {
                        s.pool.insert(c);
                    }
                    Slot::Middle => s.middle.push(c),
                }
                if matches!(slot, Slot::Up(_)) && self.step(s) == Some(Step::DealerUp) {
                    s.trump = Some(c.suit());
                }
                if matches!(slot, Slot::Board(0)) && self.step(s) == Some(Step::NumbersFlip) {
                    self.numbers_flip(s, c);
                }
                if let Slot::Up(p) = slot {
                    if self.script.up_joker_kills && c.is_joker() {
                        s.betting.fold_out(p);
                    } else if self.script.four_extra
                        && !c.is_joker()
                        && c.rank() == Rank::Four
                        && !s.stock.is_empty()
                    {
                        s.prompts.push_front((p, Prompt::UpOrDown));
                    }
                }
                s.deal_log.push(slot);
                None
            }
            Action::Declare(k) => {
                let p = s.pending.pop_front().expect("no declarer");
                s.declarations[p.index()] = Some(k);
                Some(p)
            }
            Action::Arrange(k) => {
                let p = s.pending.pop_front().expect("no one to arrange");
                let hand = &mut s.down[p.index()];
                let old = hand.clone();
                for (i, &from) in PAIRINGS[k as usize].iter().enumerate() {
                    hand[i] = old[from as usize];
                }
                Some(p)
            }
            Action::Draw(m) if self.step(s) == Some(Step::Spit) => {
                let p = s.pending.pop_front().expect("no one to spit");
                s.spits.push(s.history.len());
                let c = s.down[p.index()][m.trailing_zeros() as usize];
                s.down[p.index()].remove(m.trailing_zeros() as usize);
                s.pool.insert(c);
                Some(p)
            }
            Action::Draw(m) => {
                let p = s.pending.pop_front().expect("no drawer");
                let mut fouled = false;
                if let Some(Step::Draw { max, .. }) = self.step(s)
                    && m.count_ones() as u8 > max
                {
                    // The kept card is shown; it must be an Ace (D-3BM-1).
                    let kept = kept_card(&s.down[p.index()], m);
                    fouled = kept.is_joker() || kept.rank() != Rank::Ace;
                    s.ace_shows.push((s.history.len(), kept));
                }
                Self::throw(s, p, m);
                if fouled {
                    s.betting.fold_out(p);
                } else if matches!(self.step(s), Some(Step::Draw { .. })) {
                    for _ in 0..m.count_ones() {
                        s.queue.push_back(Slot::Down(p));
                    }
                }
                Some(p)
            }
            Action::Choose(k) => {
                let (p, prompt) = s.prompts.pop_front().expect("no prompt");
                match prompt {
                    Prompt::UpOrDown => {
                        let slot = if k == 1 { Slot::Up(p) } else { Slot::Down(p) };
                        s.queue.push_front(slot);
                    }
                    Prompt::PushOrKeep => {
                        if k == 1 {
                            Self::pay(s, p, Chips(100));
                            s.incoming = s.up[p.index()].pop();
                            // D-PU-1: the pusher gets a fresh card (which can't be pushed).
                            s.queue.push_front(Slot::Up(p));
                        }
                    }
                    Prompt::KeepOrReplace => {
                        let c = s.incoming.take().expect("no pushed card");
                        if k == 1 {
                            Self::pay(s, p, Chips(100));
                            s.muck.insert(c);
                            s.queue.push_front(Slot::Up(p));
                        } else {
                            s.up[p.index()].push(c);
                        }
                    }
                    Prompt::Take if matches!(self.script.steps[s.pc], Step::HitRounds { .. }) => {
                        if k == 1 {
                            s.hit_any = true;
                            s.queue.push_back(Slot::Up(p));
                        }
                    }
                    Prompt::Take => {
                        if k == 1 {
                            let target = match self.script.steps[s.pc] {
                                Step::TakeRounds { target, .. } => target as usize,
                                _ => 5,
                            };
                            let last = s.own(p).len() + 1 >= target;
                            s.queue
                                .push_front(if last { Slot::Down(p) } else { Slot::Up(p) });
                        }
                    }
                    Prompt::Pick => {
                        let c = s.middle.remove(k as usize);
                        s.up[p.index()].push(c);
                    }
                    Prompt::Buy => {
                        if let Step::BuyCard { up, down } = self.script.steps[s.pc] {
                            match k {
                                1 => {
                                    Self::pay(s, p, up);
                                    s.queue.push_front(Slot::Up(p));
                                }
                                2 => {
                                    Self::pay(s, p, down);
                                    s.queue.push_front(Slot::Down(p));
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Some(p)
            }
            Action::Bid(b) => {
                let p = s.pending.pop_front().expect("no bidder");
                s.bids[p.index()] = Some(b);
                Some(p)
            }
            Action::PlayCard(c) => {
                let p = s.pending.pop_front().expect("no one to play");
                if let Some(i) = s.down[p.index()].iter().position(|&x| x == c) {
                    s.down[p.index()].remove(i);
                } else if let Some(i) = s.up[p.index()].iter().position(|&x| x == c) {
                    s.up[p.index()].remove(i);
                }
                if Some(c.suit()) == s.trump {
                    s.trump_played = true;
                }
                s.trick.push((p, c));
                if s.pending.is_empty() {
                    self.finish_trick(s);
                }
                Some(p)
            }
            Action::Play(x) => {
                let p = s.pending.pop_front().expect("no player");
                s.plays[p.index()] = Some(x);
                Some(p)
            }
            Action::Name(k) => {
                let p = s.pending.pop_front().expect("no namer");
                if k > 0 {
                    let game = (k - 1) / 4;
                    let units = (k - 1) % 4 + 1;
                    s.named = Some((p, game, units));
                    s.pending.clear();
                }
                Some(p)
            }
            Action::Direction(d) => {
                let p = s.pending.pop_front().expect("no chooser");
                s.direction = d;
                Some(p)
            }
            Action::Pass(m) => {
                let p = s.pending.pop_front().expect("no passer");
                s.pass_sel[p.index()] = Some(m);
                Some(p)
            }
            Action::InOut(x) => {
                let p = s.pending.pop_front().expect("no one to say in/out");
                s.in_out[p.index()] = Some(x);
                Some(p)
            }
            Action::Order(i) => {
                let p = s.pending.pop_front().expect("no one to order");
                let hand = &mut s.down[p.index()];
                let old = hand.clone();
                for (j, &from) in permutation(old.len(), i as usize).iter().enumerate() {
                    hand[j] = old[from as usize];
                }
                Some(p)
            }
            Action::Swap(m) => {
                let p = s.betting.to_act().expect("no seat to swap");
                Self::throw(s, p, m);
                s.swapped |= 1 << p.0;
                s.betting.apply(p, BetAction::CheckCall);
                for _ in 0..m.count_ones() {
                    s.queue.push_back(Slot::Down(p));
                }
                Some(p)
            }
            bet => {
                let p = s.betting.to_act().expect("no seat to bet");
                s.betting.apply(p, bet.as_bet().unwrap());
                Some(p)
            }
        };
        s.history.push((actor, a));
        self.advance(s);
    }

    fn chance_outcomes(&self, s: &State, out: &mut Vec<(Action, f64)>) {
        out.clear();
        let from = s.chance_pool();
        let p = 1.0 / from.len() as f64;
        out.extend(from.iter().map(|c| (Action::Deal(c), p)));
    }

    fn sample_chance(&self, s: &State, rng: &mut DetRng) -> Action {
        let from = s.chance_pool();
        Action::Deal(from.iter().nth(rng.random_range(0..from.len())).unwrap())
    }

    fn payoffs(&self, s: &State) -> Vec<Chips> {
        self.settle(s).0
    }

    fn carry_out(&self, s: &State) -> ScriptCarry {
        self.settle(s).1
    }

    fn game_over(&self, c: &ScriptCarry) -> bool {
        match self.script.length {
            GameLength::UntilDone { .. } => c.over,
            _ => true,
        }
    }

    fn carried_chips(&self, c: &ScriptCarry) -> Chips {
        Chips(c.pot)
    }

    fn seats_out(&self, c: &ScriptCarry) -> u16 {
        c.out
    }

    fn observe(&self, s: &State, seat: Seat) -> Observation {
        let mut slots = s.deal_log.iter();
        let mut own_dealt = SmallVec::new();
        let mut own_choices = SmallVec::new();
        let mut history: SmallVec<[Event; 64]> = SmallVec::new();
        let mut reshuffles = s.reshuffles.iter().peekable();
        let mut aces = s.ace_shows.iter().peekable();
        let mut sys = s.sys_events.iter().peekable();
        for (i, &(who, a)) in s.history.iter().enumerate() {
            while let Some(&(_, e)) = sys.next_if(|&&(h, _)| h == i) {
                history.push(e);
            }
            while reshuffles.next_if(|&&r| r == i).is_some() {
                history.push(Event::Reshuffle);
            }
            let e = match (who, a) {
                (None, Action::Deal(c)) => match *slots.next().expect("deal without a slot") {
                    Slot::Down(p) => {
                        if p == seat && self.script.visibility != Visibility::Indian {
                            own_dealt.push(c);
                        }
                        Event::HiddenDeal(p)
                    }
                    Slot::Up(p) => Event::Up(p, c),
                    Slot::Board(b) => Event::Board(b, c),
                    Slot::BoardAt(b, pos) => Event::BoardAt(b, pos, c),
                    Slot::Pool => Event::PoolDeal,
                    Slot::PoolFlip(b) => Event::PoolFlip(b, c),
                    Slot::Middle => Event::Middle(c),
                },
                (Some(p), Action::Declare(k)) => {
                    if p == seat {
                        own_choices.push(k);
                    }
                    Event::Declared(p)
                }
                (Some(p), Action::Draw(m)) if s.spits.contains(&i) => {
                    if p == seat {
                        own_choices.push(m);
                    }
                    Event::Spat(p)
                }
                (Some(p), Action::Draw(m)) => {
                    if p == seat {
                        own_choices.push(m);
                    }
                    let ace = aces.next_if(|&&(h, _)| h == i).map(|&(_, c)| c);
                    Event::Drew(p, m.count_ones() as u8, ace)
                }
                (Some(p), Action::Swap(m)) => {
                    if p == seat {
                        own_choices.push(m);
                    }
                    Event::Swapped(p, m.count_ones() as u8)
                }
                (Some(p), Action::Arrange(k)) => {
                    if p == seat {
                        own_choices.push(k);
                    }
                    Event::Arranged(p)
                }
                (Some(_), Action::Direction(d)) => Event::Direction(d),
                (Some(p), Action::Choose(k)) => Event::Chose(p, k),
                (Some(p), Action::Bid(b)) => {
                    if p == seat {
                        own_choices.push(b);
                    }
                    Event::BidChosen(p)
                }
                (Some(p), Action::Play(x)) => {
                    if p == seat {
                        own_choices.push(x);
                    }
                    Event::PlayChosen(p)
                }
                (Some(p), Action::Name(k)) => Event::Named(p, k),
                (Some(p), Action::PlayCard(c)) => Event::Played(p, c),
                (Some(p), Action::Pass(m)) => {
                    if p == seat {
                        own_choices.push(m);
                    }
                    Event::PassChosen(p)
                }
                (Some(p), Action::InOut(x)) => {
                    if p == seat {
                        own_choices.push(x);
                    }
                    Event::InOutChosen(p)
                }
                (Some(p), Action::Order(i)) => {
                    if p == seat {
                        own_choices.push(i);
                    }
                    Event::Ordered(p)
                }
                (Some(p), bet) => Event::Bet(p, bet.as_bet().unwrap()),
                (None, _) => unreachable!("only chance has no actor"),
            };
            history.push(e);
        }
        for &(_, e) in sys {
            history.push(e);
        }
        while reshuffles.next().is_some() {
            history.push(Event::Reshuffle);
        }
        let showdown = s.is_showdown();
        Observation {
            seat,
            setup: s.setup.clone(),
            own_dealt,
            own_choices,
            history,
            declarations: (showdown && self.declare().is_some()).then(|| s.declarations.clone()),
            shown: s
                .setup
                .seats()
                .map(|p| {
                    (p != seat && self.sees_down(s, seat, p)).then(|| s.down[p.index()].clone())
                })
                .collect(),
            own_shown: (self.script.visibility == Visibility::Indian && showdown)
                .then(|| s.down[seat.index()].clone()),
            own_received: s.received[seat.index()].clone(),
            carry: s.carry.clone(),
        }
    }

    fn observer(&self, obs: &Observation) -> Seat {
        obs.seat
    }

    fn determinize(
        &self,
        obs: &Observation,
        sampler: &mut dyn HiddenSampler,
        rng: &mut DetRng,
    ) -> State {
        let n = obs.setup.num_seats as usize;
        let me = obs.seat;
        let indian = self.script.visibility == Visibility::Indian;

        let pass_sizes: Vec<u8> = self
            .script
            .steps
            .iter()
            .filter_map(|st| match *st {
                Step::Pass(k) => Some(k),
                _ => None,
            })
            .collect();
        // Where each card this seat knows will be seen next: (event index, holder). From reveals,
        // cards passed to us (the passer held them), and hands shown at the end. Choices made up
        // for other seats below keep cards where they will be seen.
        let mut seen: std::collections::HashMap<Card, Vec<(usize, Seat)>> =
            std::collections::HashMap::new();
        {
            let all = (1u16 << n) - 1;
            let (mut dir, mut out, mut round) = (0u8, 0u16, 0usize);
            let mut recv = obs.own_received.iter();
            for (i, e) in obs.history.iter().enumerate() {
                match *e {
                    Event::Bet(p, BetAction::Fold) => out |= 1 << p.0,
                    Event::Direction(d) => dir = d,
                    Event::InOutResolved(m) => out |= all & !m,
                    Event::Drew(p, _, Some(c)) if c.is_joker() || c.rank() != Rank::Ace => {
                        out |= 1 << p.0
                    }
                    Event::Revealed(p, c) => seen.entry(c).or_default().push((i, p)),
                    Event::PassesMade => {
                        let size = pass_sizes.get(round).copied().unwrap_or(0);
                        round += 1;
                        let live: Vec<Seat> = (0..n as u8)
                            .map(Seat)
                            .filter(|q| out & (1 << q.0) == 0)
                            .collect();
                        if let Some(at) = live.iter().position(|&q| q == me) {
                            let k = live.len();
                            let from = if dir == 0 {
                                live[(at + k - 1) % k]
                            } else {
                                live[(at + 1) % k]
                            };
                            for _ in 0..size {
                                if let Some(&c) = recv.next() {
                                    seen.entry(c).or_default().push((i, from));
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            for (i, shown) in obs.shown.iter().enumerate() {
                for &c in shown.iter().flatten() {
                    seen.entry(c).or_default().push((usize::MAX, Seat(i as u8)));
                }
            }
        }
        let next_holder = |c: Card, at: usize| -> Option<Seat> {
            seen.get(&c)?
                .iter()
                .filter(|&&(i, _)| i >= at)
                .min_by_key(|&&(i, _)| i)
                .map(|&(_, h)| h)
        };
        let in_out_revealed: Option<u16> = obs.history.iter().find_map(|e| match *e {
            Event::InOutResolved(m) => Some(m),
            _ => None,
        });

        // 1. Walk the public events. Each hidden deal is a slot; each seat's hand is a list of
        //    slots, following its draws, passes and rearrangements. Choices of other seats are
        //    made up (their cards are sampled), keeping what this seat knows consistent.
        let mut hand_slots: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut slot_epoch: Vec<usize> = Vec::new();
        let mut slot_card: Vec<Option<Card>> = Vec::new();
        let mut deals: Vec<(DealRef, usize)> = Vec::new();
        // Slots thrown away, with the epoch they were thrown in.
        let mut thrown: Vec<(usize, usize)> = Vec::new();
        let mut forced: Vec<(usize, Card)> = Vec::new();
        let mut choices: Vec<Action> = Vec::new();
        let mut pass_masks: Vec<Vec<u8>> = Vec::new();
        // Spit: slots in the middle pile, not yet turned.
        let mut pool_slots: Vec<usize> = Vec::new();
        let mut own_pass: Option<u8> = None;
        let mut own = obs.own_choices.iter();
        let mut own_cards = obs.own_dealt.iter();
        let mut received = obs.own_received.iter();
        let mut folded = 0u16;
        let mut direction = 0u8;
        let mut epoch = 0;
        // Which of `p`'s slots to give up first when inventing a choice at event `at` (lower first).
        // Passing: cards that must reach someone else go first, cards `p` is seen with later stay.
        // Throwing: cards never seen again go first; cards seen later must stay; unknown slots stay
        // flexible.
        let rank_slot =
            |id: usize, p: Seat, at: usize, passing: bool, slot_card: &[Option<Card>]| -> u8 {
                match (slot_card[id], passing) {
                    (Some(c), true) => match next_holder(c, at) {
                        Some(h) if h != p => 0,
                        Some(_) => 3,
                        None => 1,
                    },
                    (None, true) => 2,
                    (Some(c), false) => match next_holder(c, at) {
                        Some(_) => 3,
                        None => 0,
                    },
                    (None, false) => 1,
                }
            };
        for (ei, e) in obs.history.iter().enumerate() {
            match *e {
                Event::HiddenDeal(p) => {
                    let id = slot_epoch.len();
                    slot_epoch.push(epoch);
                    slot_card.push(if p == me && !indian {
                        own_cards.next().copied()
                    } else {
                        None
                    });
                    hand_slots[p.index()].push(id);
                    deals.push((DealRef::Slot(id), epoch));
                }
                Event::Up(_, c)
                | Event::Board(_, c)
                | Event::BoardAt(_, _, c)
                | Event::Middle(c) => deals.push((DealRef::Known(c), epoch)),
                Event::Chose(..)
                | Event::BidShown(..)
                | Event::Named(..)
                | Event::PlayResolved(_) => {}
                Event::PlayChosen(p) => {
                    let x = if p == me {
                        *own.next().expect("own play missing")
                    } else {
                        obs.history[ei..]
                            .iter()
                            .find_map(|x| match *x {
                                Event::PlayResolved(m) => Some(((m >> p.0) & 1) as u8),
                                _ => None,
                            })
                            .unwrap_or_else(|| rng.random_range(0..2))
                    };
                    choices.push(Action::Play(x));
                }
                Event::Played(p, c) => {
                    // The dealer's face-up trump card isn't a hidden slot.
                    let was_up = obs.history[..ei]
                        .iter()
                        .any(|x| matches!(*x, Event::Up(q, u) if q == p && u == c));
                    if !was_up {
                        let hs = &mut hand_slots[p.index()];
                        let at = hs
                            .iter()
                            .position(|&id| slot_card[id] == Some(c))
                            .or_else(|| hs.iter().position(|&id| slot_card[id].is_none()));
                        if let Some(at) = at {
                            let id = hs.remove(at);
                            slot_card[id] = Some(c);
                        }
                    }
                }
                Event::TrickWon(_) => {}
                Event::Discarded(p, c) => {
                    let hs = &mut hand_slots[p.index()];
                    let at = hs
                        .iter()
                        .position(|&id| slot_card[id] == Some(c))
                        .or_else(|| hs.iter().position(|&id| slot_card[id].is_none()));
                    if let Some(at) = at {
                        let id = hs.remove(at);
                        slot_card[id] = Some(c);
                    }
                }
                Event::Showed(p, j, c) => {
                    if let Some(&id) = hand_slots[p.index()].get(j as usize)
                        && slot_card[id].is_none()
                    {
                        slot_card[id] = Some(c);
                    }
                }
                Event::BidChosen(p) => {
                    let b = if p == me {
                        *own.next().expect("own bid missing")
                    } else {
                        obs.history[ei..]
                            .iter()
                            .find_map(|x| match *x {
                                Event::BidShown(q, b) if q == p => Some(b),
                                _ => None,
                            })
                            .unwrap_or_else(|| rng.random_range(0..=10))
                    };
                    choices.push(Action::Bid(b));
                }
                Event::PoolDeal => {
                    let id = slot_epoch.len();
                    slot_epoch.push(epoch);
                    slot_card.push(None);
                    pool_slots.push(id);
                    deals.push((DealRef::Slot(id), epoch));
                }
                Event::PoolFlip(_, c) => {
                    let at = pool_slots
                        .iter()
                        .position(|&id| slot_card[id] == Some(c))
                        .or_else(|| pool_slots.iter().position(|&id| slot_card[id].is_none()));
                    if let Some(at) = at {
                        let id = pool_slots.remove(at);
                        slot_card[id] = Some(c);
                    }
                    deals.push((DealRef::Known(c), epoch));
                }
                Event::Spat(p) => {
                    let hs = hand_slots[p.index()].clone();
                    let mask = if p == me {
                        *own.next().expect("own spit missing")
                    } else {
                        let mut order: Vec<usize> = (0..hs.len()).collect();
                        order.sort_by_key(|&x| (rank_slot(hs[x], p, ei, false, &slot_card), x));
                        1u8 << order[0]
                    };
                    let x = mask.trailing_zeros() as usize;
                    pool_slots.push(hs[x]);
                    hand_slots[p.index()].remove(x);
                    choices.push(Action::Draw(mask));
                }
                Event::Reshuffle => epoch += 1,
                Event::Declared(p) => {
                    if p == me {
                        own.next();
                    }
                }
                Event::Bet(p, b) => {
                    if b == BetAction::Fold {
                        folded |= 1 << p.0;
                    }
                }
                Event::Direction(d) => direction = d,
                Event::Arranged(p) => {
                    let k = if p == me {
                        *own.next().expect("own arrangement missing")
                    } else {
                        0
                    };
                    let hs = &mut hand_slots[p.index()];
                    let old = hs.clone();
                    for (i, &from) in PAIRINGS[k as usize].iter().enumerate() {
                        hs[i] = old[from as usize];
                    }
                    choices.push(Action::Arrange(k));
                }
                Event::Ordered(p) => {
                    let hs = hand_slots[p.index()].clone();
                    let perm: SmallVec<[u8; 8]> = if p == me {
                        let i = *own.next().expect("own order missing");
                        permutation(hs.len(), i as usize)
                    } else {
                        // Put the slots holding p's later reveals first, in reveal order.
                        let reveals: Vec<Card> = obs.history[ei..]
                            .iter()
                            .filter_map(|x| match *x {
                                Event::Revealed(q, c) if q == p => Some(c),
                                _ => None,
                            })
                            .collect();
                        let mut used = vec![false; hs.len()];
                        let mut perm = SmallVec::new();
                        for c in &reveals {
                            let pos = (0..hs.len())
                                .find(|&j| !used[j] && slot_card[hs[j]] == Some(*c))
                                .or_else(|| {
                                    (0..hs.len()).find(|&j| !used[j] && slot_card[hs[j]].is_none())
                                });
                            if let Some(j) = pos {
                                used[j] = true;
                                perm.push(j as u8);
                            }
                        }
                        perm.extend((0..hs.len()).filter(|&j| !used[j]).map(|j| j as u8));
                        perm
                    };
                    let hsm = &mut hand_slots[p.index()];
                    for (j, &from) in perm.iter().enumerate() {
                        hsm[j] = hs[from as usize];
                    }
                    choices.push(Action::Order(permutation_index(&perm) as u8));
                }
                Event::Revealed(p, c) => {
                    let hs = &mut hand_slots[p.index()];
                    if !hs.is_empty() {
                        let id = hs.remove(0);
                        if slot_card[id].is_none() {
                            slot_card[id] = Some(c);
                        }
                    }
                }
                Event::PassChosen(p) => {
                    if p == me {
                        own_pass = Some(*own.next().expect("own pass missing"));
                    }
                }
                Event::PassesMade => {
                    let size = pass_sizes.get(pass_masks.len()).copied().unwrap_or(0) as usize;
                    let live: Vec<Seat> = (0..n as u8)
                        .map(Seat)
                        .filter(|q| folded & (1 << q.0) == 0)
                        .collect();
                    let k = live.len();
                    let mut masks = vec![0u8; n];
                    let mut moving: Vec<(Seat, Vec<usize>)> = Vec::new();
                    for (i, &q) in live.iter().enumerate() {
                        let to = if direction == 0 {
                            live[(i + 1) % k]
                        } else {
                            live[(i + k - 1) % k]
                        };
                        let hs = hand_slots[q.index()].clone();
                        let mask: u8 = if q == me {
                            own_pass.take().unwrap_or(0)
                        } else if to == me {
                            // Pick slots so the cards arrive exactly as we received them.
                            let chunk: Vec<Card> =
                                (0..size).filter_map(|_| received.next().copied()).collect();
                            let mut m = 0u8;
                            let mut prev: i32 = -1;
                            for (j, c) in chunk.iter().enumerate() {
                                let known_at = |card: &Card| {
                                    (0..hs.len()).find(|&x| slot_card[hs[x]] == Some(*card))
                                };
                                let pos = match known_at(c) {
                                    Some(x) if x as i32 > prev => Some(x),
                                    _ => {
                                        let limit = chunk[j + 1..]
                                            .iter()
                                            .filter_map(known_at)
                                            .min()
                                            .unwrap_or(hs.len());
                                        ((prev + 1) as usize..limit)
                                            .find(|&x| {
                                                m & (1 << x) == 0 && slot_card[hs[x]].is_none()
                                            })
                                            .or_else(|| {
                                                (0..hs.len()).find(|&x| {
                                                    m & (1 << x) == 0 && slot_card[hs[x]].is_none()
                                                })
                                            })
                                    }
                                };
                                if let Some(x) = pos {
                                    m |= 1 << x;
                                    slot_card[hs[x]] = Some(*c);
                                    prev = x as i32;
                                }
                            }
                            m
                        } else {
                            let mut order: Vec<usize> = (0..hs.len()).collect();
                            order.sort_by_key(|&x| (rank_slot(hs[x], q, ei, true, &slot_card), x));
                            order.iter().take(size).fold(0u8, |m, &x| m | 1 << x)
                        };
                        masks[q.index()] = mask;
                        let gone: Vec<usize> = (0..hs.len())
                            .filter(|&x| mask & (1 << x) != 0)
                            .map(|x| hs[x])
                            .collect();
                        hand_slots[q.index()].retain(|id| !gone.contains(id));
                        moving.push((to, gone));
                    }
                    for (to, gone) in moving {
                        hand_slots[to.index()].extend(gone);
                    }
                    pass_masks.push(masks);
                }
                Event::InOutChosen(p) => {
                    let declarer = obs.history[..ei]
                        .iter()
                        .any(|x| matches!(*x, Event::Named(q, k) if q == p && k > 0));
                    let x = if p == me {
                        *own.next().expect("own in/out missing")
                    } else if declarer {
                        1 // the declarer is always in
                    } else {
                        match in_out_revealed {
                            Some(m) => ((m >> p.0) & 1) as u8,
                            None => rng.random_range(0..2),
                        }
                    };
                    choices.push(Action::InOut(x));
                }
                Event::InOutResolved(m) => {
                    folded |= !m & ((1u16 << n) - 1);
                }
                Event::Drew(p, k, _) | Event::Swapped(p, k) => {
                    let hs = hand_slots[p.index()].clone();
                    let mask = if p == me {
                        *own.next().expect("own draw missing")
                    } else if let Event::Drew(_, _, Some(c)) = *e {
                        // A bonus draw keeps one card, the one shown.
                        let keep = (0..hs.len())
                            .find(|&x| slot_card[hs[x]] == Some(c))
                            .or_else(|| (0..hs.len()).rev().find(|&x| slot_card[hs[x]].is_none()))
                            .unwrap_or(hs.len() - 1);
                        ((1u16 << hs.len()) - 1) as u8 & !(1 << keep)
                    } else {
                        let mut order: Vec<usize> = (0..hs.len()).collect();
                        order.sort_by_key(|&x| (rank_slot(hs[x], p, ei, false, &slot_card), x));
                        order.iter().take(k as usize).fold(0u8, |m, &x| m | 1 << x)
                    };
                    let hsm = &mut hand_slots[p.index()];
                    let mut i = 0;
                    hsm.retain(|&id| {
                        let keep = mask & (1 << i) == 0;
                        if !keep {
                            thrown.push((id, epoch));
                        }
                        i += 1;
                        keep
                    });
                    if let Event::Drew(_, _, Some(c)) = *e {
                        forced.push((hsm[0], c));
                        if c.is_joker() || c.rank() != Rank::Ace {
                            folded |= 1 << p.0; // fouled (D-3BM-1)
                        }
                    }
                    choices.push(match *e {
                        Event::Drew(..) => Action::Draw(mask),
                        _ => Action::Swap(mask),
                    });
                }
            }
        }

        // 2. Cards we know for slots: shown hands (in hand order), shown Aces, and (Indian) our
        //    own cards once shown. Our own deals and received cards were filled in above.
        if indian && let Some(cards) = &obs.own_shown {
            for (&id, &c) in hand_slots[me.index()].iter().zip(cards) {
                slot_card[id] = Some(c);
            }
        }
        for (i, shown) in obs.shown.iter().enumerate() {
            if let Some(cards) = shown {
                for (&id, &c) in hand_slots[i].iter().zip(cards) {
                    if slot_card[id].is_none() {
                        slot_card[id] = Some(c);
                    }
                }
            }
        }
        for &(id, c) in &forced {
            slot_card[id] = Some(c);
        }

        if epoch == 0 {
            // 3a. No reshuffle: sample the unknown current hands through the sampler (ranges
            //     apply), then unknown thrown-away cards uniformly.
            let mut known = CardSet::EMPTY;
            for (d, _) in &deals {
                if let DealRef::Known(c) = d {
                    known.insert(*c);
                }
            }
            for c in slot_card.iter().flatten() {
                known.insert(*c);
            }
            let requests: Vec<(Seat, usize)> = (0..n)
                .map(|i| Seat(i as u8))
                .filter(|p| {
                    hand_slots[p.index()]
                        .iter()
                        .any(|&id| slot_card[id].is_none())
                })
                .map(|p| (p, hand_slots[p.index()].len()))
                .collect();
            // Known cards inside a requested hand are fixed: sample only the unknown ones.
            let unknown_counts: Vec<(Seat, usize)> = requests
                .iter()
                .map(|&(p, _)| {
                    (
                        p,
                        hand_slots[p.index()]
                            .iter()
                            .filter(|&&id| slot_card[id].is_none())
                            .count(),
                    )
                })
                .collect();
            // Numbers: a seat still in when a card was turned can't hold that rank or its
            // neighbours (it would have thrown them away), so its hidden cards avoid them.
            let mut forbidden = vec![CardSet::EMPTY; n];
            if self.script.resolution == Resolution::Booray {
                // A seat that didn't follow suit has none of it left; a seat that led trump
                // before trump was played holds only trumps.
                let suit_cards = |su: Suit| -> CardSet {
                    self.deck().iter().filter(|c| c.suit() == su).collect()
                };
                let mut trump: Option<Suit> = None;
                let mut trump_played = false;
                let mut led: Option<Suit> = None;
                for e in &obs.history {
                    match *e {
                        Event::Up(_, c) if trump.is_none() => trump = Some(c.suit()),
                        Event::Played(p, c) => {
                            match led {
                                None => {
                                    if Some(c.suit()) == trump && !trump_played {
                                        for su in Suit::ALL {
                                            if Some(su) != trump {
                                                forbidden[p.index()] =
                                                    forbidden[p.index()].union(suit_cards(su));
                                            }
                                        }
                                    }
                                    led = Some(c.suit());
                                }
                                Some(l) if c.suit() != l => {
                                    forbidden[p.index()] =
                                        forbidden[p.index()].union(suit_cards(l));
                                }
                                _ => {}
                            }
                            if Some(c.suit()) == trump {
                                trump_played = true;
                            }
                        }
                        Event::TrickWon(_) => led = None,
                        _ => {}
                    }
                }
            }
            if self.script.steps.contains(&Step::NumbersFlip) {
                let mut out = 0u16;
                for e in &obs.history {
                    match *e {
                        Event::Bet(p, BetAction::Fold) => out |= 1 << p.0,
                        Event::Board(0, f) => {
                            let near: CardSet = self
                                .deck()
                                .iter()
                                .filter(|c| {
                                    let d = (c.rank() as i32 - f.rank() as i32).rem_euclid(13);
                                    d == 0 || d == 1 || d == 12
                                })
                                .collect();
                            for (i, fb) in forbidden.iter_mut().enumerate() {
                                if out & (1 << i) == 0 {
                                    *fb = fb.union(near);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            let sampled: Vec<engine::CardVec> = if forbidden.iter().all(|f| f.is_empty()) {
                sample_hidden_hands(sampler, &unknown_counts, self.deck().difference(known), rng)
            } else {
                // Most constrained seats first; retry with a fresh shuffle if a seat runs short
                // (a valid assignment always exists: the real one).
                let base = self.deck().difference(known);
                let mut order: Vec<usize> = (0..unknown_counts.len()).collect();
                order.sort_by_key(|&i| {
                    let (p, k) = unknown_counts[i];
                    base.difference(forbidden[p.index()]).len() as i64 - k as i64
                });
                let mut result: Vec<Option<engine::CardVec>> = vec![None; unknown_counts.len()];
                'tries: for _ in 0..200 {
                    let mut avail = base;
                    for &i in &order {
                        let (p, k) = unknown_counts[i];
                        let pool = avail.difference(forbidden[p.index()]);
                        if pool.len() < k {
                            continue 'tries;
                        }
                        let got = engine::game::sample_cards(pool, k, rng);
                        for &c in &got {
                            avail.remove(c);
                        }
                        result[i] = Some(got);
                    }
                    break;
                }
                result
                    .into_iter()
                    .map(|r| r.expect("no consistent Numbers deal found"))
                    .collect()
            };
            for (&(p, _), cards) in unknown_counts.iter().zip(sampled) {
                let mut cards = cards.into_iter();
                for &id in &hand_slots[p.index()] {
                    if slot_card[id].is_none() {
                        let c = cards.next().unwrap();
                        slot_card[id] = Some(c);
                        known.insert(c);
                    }
                }
            }
            let mut rest: Vec<Card> = self.deck().difference(known).iter().collect();
            for c in slot_card.iter_mut().filter(|c| c.is_none()) {
                let i = rng.random_range(0..rest.len());
                *c = Some(rest.swap_remove(i));
            }
        } else {
            // 3b. Reshuffles happened: a card dealt after reshuffle e must be one thrown away
            //     before it. Link each such deal to a distinct thrown slot of that window (later
            //     epochs first, so known cards flow back to where they were first dealt), then
            //     sample the first-epoch cards and follow the links.
            let mut link: Vec<Option<usize>> = vec![None; slot_epoch.len()];
            for e in (1..=epoch).rev() {
                let mut window: Vec<usize> = thrown
                    .iter()
                    .filter(|&&(_, te)| te + 1 == e)
                    .map(|&(id, _)| id)
                    .collect();
                let in_epoch: Vec<DealRef> = deals
                    .iter()
                    .filter(|&&(_, de)| de == e)
                    .map(|&(d, _)| d)
                    .collect();
                // Known cards first.
                for d in &in_epoch {
                    let (target, card) = match *d {
                        DealRef::Known(c) => (None, c),
                        DealRef::Slot(id) => match slot_card[id] {
                            Some(c) => (Some(id), c),
                            None => continue,
                        },
                    };
                    let pos = window
                        .iter()
                        .position(|&w| slot_card[w] == Some(card))
                        .or_else(|| window.iter().position(|&w| slot_card[w].is_none()));
                    if let Some(pos) = pos {
                        let w = window.swap_remove(pos);
                        slot_card[w] = Some(card);
                        let _ = target;
                    }
                }
                // Then unknown hidden deals take the remaining thrown slots' cards.
                for d in &in_epoch {
                    if let DealRef::Slot(id) = *d
                        && slot_card[id].is_none()
                        && !window.is_empty()
                    {
                        link[id] = Some(window.remove(0));
                    }
                }
            }
            // Roots: first-epoch deals. Known ones are fixed; sample the rest.
            let mut known = CardSet::EMPTY;
            for &(d, de) in &deals {
                if de == 0 {
                    match d {
                        DealRef::Known(c) => {
                            known.insert(c);
                        }
                        DealRef::Slot(id) => {
                            if let Some(c) = slot_card[id] {
                                known.insert(c);
                            }
                        }
                    }
                }
            }
            let mut rest: Vec<Card> = self.deck().difference(known).iter().collect();
            for (id, c) in slot_card.iter_mut().enumerate() {
                if c.is_none() && slot_epoch[id] == 0 {
                    let i = rng.random_range(0..rest.len());
                    *c = Some(rest.swap_remove(i));
                }
            }
            // Follow links (a linked slot's source is earlier, so one forward pass suffices
            // per level; loop until stable).
            loop {
                let mut changed = false;
                for id in 0..slot_card.len() {
                    if slot_card[id].is_none()
                        && let Some(src) = link[id]
                        && let Some(c) = slot_card[src]
                    {
                        slot_card[id] = Some(c);
                        changed = true;
                    }
                }
                if !changed {
                    break;
                }
            }
        }

        // 4. Rebuild the event sequence and replay.
        let mut choices = choices.into_iter();
        let mut own_decl = obs.own_choices.iter();
        let mut deal_actions = Vec::new();
        let mut actions = Vec::new();
        let mut pass_round = 0;
        for e in &obs.history {
            match *e {
                Event::Chose(_, k) => actions.push(Action::Choose(k)),
                Event::Named(_, k) => actions.push(Action::Name(k)),
                Event::PlayResolved(_) | Event::Showed(..) | Event::Discarded(..) => {}
                Event::Played(_, c) => actions.push(Action::PlayCard(c)),
                Event::TrickWon(_) => {}
                Event::Middle(_) | Event::BidShown(..) => {}
                Event::HiddenDeal(_)
                | Event::Up(..)
                | Event::Board(..)
                | Event::BoardAt(..)
                | Event::PoolDeal
                | Event::PoolFlip(..)
                | Event::Reshuffle
                | Event::InOutResolved(_)
                | Event::Revealed(..) => {}
                Event::PassesMade => pass_round += 1,
                Event::Bet(_, b) => actions.push(Action::from_bet(b)),
                Event::Direction(d) => actions.push(Action::Direction(d)),
                Event::PassChosen(p) => {
                    if p == me {
                        own_decl.next();
                    }
                    let m = match pass_masks.get(pass_round) {
                        Some(ms) => ms[p.index()],
                        // A round still being chosen: nothing has moved yet.
                        None if p == me => own_pass.expect("own pass missing"),
                        None => {
                            let size = pass_sizes.get(pass_round).copied().unwrap_or(0);
                            ((1u16 << size) - 1) as u8
                        }
                    };
                    actions.push(Action::Pass(m));
                }
                Event::Drew(p, ..)
                | Event::Swapped(p, _)
                | Event::Arranged(p)
                | Event::Ordered(p)
                | Event::Spat(p)
                | Event::BidChosen(p)
                | Event::PlayChosen(p)
                | Event::InOutChosen(p) => {
                    if p == me {
                        own_decl.next();
                    }
                    actions.push(choices.next().unwrap());
                }
                Event::Declared(p) => {
                    let k = if p == me {
                        *own_decl.next().expect("own declaration missing")
                    } else {
                        match obs.declarations.as_ref().and_then(|d| d[p.index()]) {
                            Some(k) => k,
                            None => rng.random_range(0..3),
                        }
                    };
                    actions.push(Action::Declare(k));
                }
            }
        }
        for (d, _) in &deals {
            deal_actions.push(Action::Deal(match *d {
                DealRef::Known(c) => c,
                DealRef::Slot(id) => slot_card[id].expect("every slot has a card"),
            }));
        }
        replay_interleaved_with(self, &obs.setup, &obs.carry, deal_actions, actions)
    }

    fn classify(&self, s: &State, a: Action) -> ActionClass {
        match a {
            Action::Deal(_) => ActionClass::Chance,
            Action::Fold => ActionClass::Fold,
            Action::CheckCall | Action::Swap(_) => {
                let owed = match s.betting.to_act() {
                    Some(p) => s.betting.owed(p).min(s.betting.stacks()[p.index()]),
                    None => Chips::ZERO,
                };
                ActionClass::CheckOrCall { owed }
            }
            Action::RaiseTo(to) => ActionClass::BetOrRaise { to },
            Action::Declare(_) => ActionClass::Declare,
            Action::Draw(_) | Action::Arrange(_) | Action::Pass(_) | Action::Order(_) => {
                ActionClass::CardChoice
            }
            Action::Direction(_)
            | Action::InOut(_)
            | Action::Bid(_)
            | Action::Play(_)
            | Action::Name(_) => ActionClass::Declare,
            Action::Choose(_) | Action::PlayCard(_) => ActionClass::CardChoice,
        }
    }

    fn private_cards(&self, s: &State, seat: Seat) -> engine::CardVec {
        s.down[seat.index()].iter().copied().collect()
    }

    /// Strength for analysis and rollouts: the high-hand category of the seat's cards plus the
    /// first board (games whose first share is ranked high), by betting round.
    fn decision_context(&self, s: &State, seat: Seat) -> Option<engine::DecisionContext> {
        let rule = self.shares().first()?;
        if !matches!(rule.ranking, Ranking::High) {
            return None;
        }
        let mut cards: Vec<Card> = s.own(seat).into_iter().collect();
        if let Some(b) = s.boards.first() {
            cards.extend(b.iter().copied());
        }
        let wild = self.wild_for(s, seat);
        let (nat, w) = cards.iter().copied().collect::<CardSet>().split_wild(wild);
        let strength = if nat.len() + w >= 5 {
            let any = ShareRule {
                build: Build::Own,
                ..*rule
            };
            hand::share_value(&any, &cards, &[], wild)?.category() as u8
        } else {
            // Fewer than five cards: count pairs and trips among them.
            let mut counts = [0u8; 13];
            for c in nat.iter() {
                counts[c.rank() as usize] += 1;
            }
            let top = counts.iter().copied().max().unwrap_or(0) + w as u8;
            match top {
                0 | 1 => 0,
                2 => 1,
                3 => 3,
                _ => 7,
            }
        };
        Some(engine::DecisionContext {
            street: s.streets.saturating_sub(1),
            board: 0,
            strength,
            pot: s.betting.pot() + Chips(s.carry.pot),
            street_committed: s.betting.street_committed(seat),
            stack: s.betting.stacks()[seat.index()],
        })
    }

    fn analysis_labels(&self) -> engine::AnalysisLabels {
        let rounds = self
            .script
            .steps
            .iter()
            .filter(|st| {
                matches!(
                    st,
                    Step::Bet
                        | Step::RedRiver { .. }
                        | Step::TakeRounds { .. }
                        | Step::HitRounds { .. }
                )
            })
            .count()
            .max(1);
        engine::AnalysisLabels {
            start_classes: Vec::new(),
            start_grid: Vec::new(),
            streets: (1..=rounds.max(10))
                .map(|k| format!("betting round {k}"))
                .collect(),
            boards: vec!["any".into()],
            strengths: engine::Category::ALL
                .iter()
                .map(|c| c.to_string())
                .collect(),
        }
    }

    fn eval_unit(&self) -> Chips {
        Chips(100)
    }

    fn view(&self, obs: &Observation) -> TableView {
        let s = self.determinize(
            obs,
            &mut engine::UniformSampler,
            &mut engine::rng::rng_from_seed(0),
        );
        let b = &s.betting;
        let indian = self.script.visibility == Visibility::Indian;
        let seats = obs
            .setup
            .seats()
            .map(|p| {
                let i = p.index();
                let mut v = SeatView::new(p, b.stacks()[i], b.street_committed(p));
                v.status = if b.is_folded(p) {
                    SeatStatus::Folded
                } else if b.is_all_in(p) {
                    SeatStatus::AllIn
                } else {
                    SeatStatus::Active
                };
                let count = s.down[i].len();
                v.cards = if p == obs.seat {
                    match (&obs.own_shown, indian) {
                        (Some(c), _) => c.iter().map(|&c| CardSlot::Up(c)).collect(),
                        (None, true) => vec![CardSlot::Down(None); count],
                        (None, false) => {
                            s.down[i].iter().map(|&c| CardSlot::Down(Some(c))).collect()
                        }
                    }
                } else if let Some(cards) = &obs.shown[i] {
                    if indian && !s.is_showdown() {
                        cards.iter().map(|&c| CardSlot::Down(Some(c))).collect()
                    } else {
                        cards.iter().map(|&c| CardSlot::Up(c)).collect()
                    }
                } else {
                    vec![CardSlot::Down(None); count]
                };
                v.cards.extend(s.up[i].iter().map(|&c| CardSlot::Up(c)));
                if p == obs.setup.chooser {
                    v.badges.push("chooser".into());
                }
                for e in &obs.history {
                    match *e {
                        Event::Drew(q, _, Some(ace)) if q == p => {
                            v.badges.push(format!("showed {ace}"));
                        }
                        Event::Swapped(q, k) if q == p => v.badges.push(format!("swapped {k}")),
                        _ => {}
                    }
                }
                if let Some(d) = self.declare() {
                    let declared = obs
                        .history
                        .iter()
                        .any(|e| matches!(e, Event::Declared(q) if *q == p));
                    if let Some(k) = obs.declarations.as_ref().and_then(|ds| ds[i]) {
                        v.badges.push(d.names()[k as usize].into());
                    } else if p == obs.seat && declared {
                        if let Some(k) = s.declarations[i] {
                            v.badges
                                .push(format!("you declared {}", d.names()[k as usize]));
                        }
                    } else if declared {
                        v.badges.push("declared".into());
                    }
                }
                v
            })
            .collect();
        let names = if self.script.boards == 2 {
            vec!["Top", "Bottom"]
        } else {
            vec!["Board"]
        };
        let boards = s
            .boards
            .iter()
            .enumerate()
            .map(|(k, cards)| {
                let name = names.get(k).copied().unwrap_or("Board");
                if self.script.positions == 0 {
                    return BoardView::row(name, cards.iter().map(|&c| CardSlot::Up(c)).collect());
                }
                let mut slots = vec![CardSlot::Empty; self.script.positions as usize];
                for (&c, &at) in cards.iter().zip(&s.board_pos[k]) {
                    slots[at as usize] = CardSlot::Up(c);
                }
                BoardView {
                    name: name.into(),
                    layout: self.script.layout,
                    slots,
                    lines: self.script.lines.clone(),
                }
            })
            .collect();
        let wilds = Rank::ALL
            .iter()
            .filter(|&&r| {
                Suit::ALL
                    .iter()
                    .all(|&su| self.script.wild.contains(Card::new(r, su)))
            })
            .map(|&rank| engine::view::WildView { rank, seat: None })
            .collect();
        let mut notes = Vec::new();
        if self.script.wild.contains(Card::RED_JOKER) {
            notes.push("Jokers are wild".into());
        }
        if obs.history.contains(&Event::Reshuffle) {
            notes.push("The discards were reshuffled".into());
        }
        TableView {
            game: self.meta.display_name.into(),
            phase: match self.step(&s) {
                Some(step) if !s.done => format!("{step:?}"),
                _ => "Showdown".into(),
            },
            viewer: obs.seat,
            button: obs.setup.button,
            to_act: match self.to_act(&s) {
                ToAct::Seat(p) => Some(p),
                _ => None,
            },
            seats,
            boards,
            pots: vec![PotView {
                name: "Pot".into(),
                amount: b.pot(),
            }],
            wilds,
            notes,
        }
    }

    fn describe_observation(&self, obs: &Observation) -> String {
        let v = self.view(obs);
        let slot = |c: &CardSlot| match c {
            CardSlot::Up(c) => c.to_string(),
            CardSlot::Down(Some(c)) => format!("[{c}]"),
            CardSlot::Down(None) => "##".into(),
            CardSlot::Empty => "..".into(),
        };
        let mut out = format!("{} | {} | pot {}", v.game, v.phase, v.pots[0].amount);
        for b in &v.boards {
            out += &format!(
                "\n  {}: {}",
                b.name,
                b.slots.iter().map(slot).collect::<Vec<_>>().join(" ")
            );
        }
        for sv in &v.seats {
            let you = if sv.seat == obs.seat { " (you)" } else { "" };
            out += &format!(
                "\n  {}{you}: {} | stack {} bet {} {:?} {}",
                sv.seat,
                sv.cards.iter().map(slot).collect::<Vec<_>>().join(" "),
                sv.stack,
                sv.bet,
                sv.status,
                sv.badges.join(", ")
            );
        }
        for n in &v.notes {
            out += &format!("\n  {n}");
        }
        out
    }

    fn describe_action(&self, a: Action) -> String {
        match a {
            Action::Declare(k) => self.declare().map_or(format!("declare {k}"), |d| {
                format!("declare {}", d.names()[k as usize])
            }),
            Action::Draw(0) => "stand pat".into(),
            Action::Draw(m) => format!("throw away cards {}", mask_positions(m)),
            Action::Swap(m) => format!("swap cards {} and call", mask_positions(m)),
            Action::Direction(0) => "pass to the left".into(),
            Action::Direction(_) => "pass to the right".into(),
            Action::Pass(m) => format!("pass cards {}", mask_positions(m)),
            Action::InOut(1) => "In".into(),
            Action::InOut(_) => "Out".into(),
            Action::Order(i) => format!("reveal order #{i}"),
            Action::Bid(b) => format!("bid {}", Chips(50 * b as i64)),
            Action::PlayCard(c) => format!("play {c}"),
            Action::Fold => "fold".into(),
            Action::CheckCall => "check / call".into(),
            Action::RaiseTo(x) => format!("bet / raise to {x}"),
            Action::Deal(c) => format!("deal {c}"),
            Action::Play(1) => "play".into(),
            Action::Play(_) => "drop".into(),
            Action::Name(0) => "pass".into(),
            Action::Name(k) => {
                let game = ["high poker", "low poker", "high numbers", "low numbers"]
                    [((k - 1) / 4) as usize];
                format!("name {game} at {}", Chips(50 * ((k - 1) % 4 + 1) as i64))
            }
            Action::Choose(k) => format!("choose {k}"),
            Action::Arrange(k) => {
                let p = PAIRINGS[k as usize].map(|i| i + 1);
                format!(
                    "pairs ({},{}) ({},{}) ({},{})",
                    p[0], p[1], p[2], p[3], p[4], p[5]
                )
            }
        }
    }
}

fn mask_positions(m: u8) -> String {
    (0..8)
        .filter(|i| m & (1 << i) != 0)
        .map(|i| (i + 1).to_string())
        .collect::<Vec<_>>()
        .join(",")
}

// ---------------------------------------------------------------------------------------------
// Observation
// ---------------------------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Event {
    /// A face-down card went to this seat.
    HiddenDeal(Seat),
    Up(Seat, Card),
    Board(u8, Card),
    Bet(Seat, BetAction),
    /// This seat declared; what, stays hidden until everyone has.
    Declared(Seat),
    /// This seat threw away this many face-down cards (and showed this kept Ace, for the bonus).
    Drew(Seat, u8, Option<Card>),
    /// Swap Hold'em: this seat swapped this many cards and called.
    Swapped(Seat, u8),
    /// Mama's Favorite: this seat split its cards into pairs.
    Arranged(Seat),
    /// The stock ran out; the discards were shuffled into a new stock (D-DRAW-1).
    Reshuffle,
    /// The chooser picked the passing direction (0 = left).
    Direction(u8),
    /// This seat picked its cards to pass.
    PassChosen(Seat),
    /// Everyone passed at once.
    PassesMade,
    /// This seat said In or Out (hidden until all have).
    InOutChosen(Seat),
    /// Everyone's In/Out, revealed together (bitmask of In seats).
    InOutResolved(u16),
    /// This seat ordered its cards for revealing.
    Ordered(Seat),
    /// This seat turned this card face up.
    Revealed(Seat, Card),
    /// A board card at a position (shaped boards).
    BoardAt(u8, u8, Card),
    /// Spit: this seat threw a card into the middle pile.
    Spat(Seat),
    /// Spit: a hidden card went into the middle pile.
    PoolDeal,
    /// Spit: a middle-pile card was turned face up onto a board.
    PoolFlip(u8, Card),
    /// A public answer to a prompt.
    Chose(Seat, u8),
    /// Bid: a face-up card into the middle.
    Middle(Card),
    /// Bid: this seat placed its sealed bid.
    BidChosen(Seat),
    /// Bid: this seat's bid, revealed with everyone's.
    BidShown(Seat, u8),
    /// 3-5-7: this seat chose to play or drop (hidden until all have).
    PlayChosen(Seat),
    /// 3-5-7: who played this round (bitmask).
    PlayResolved(u16),
    /// A played hand is shown: seat, position in its face-down cards, card.
    Showed(Seat, u8, Card),
    /// Scare-a-moosh: this seat named the game (or passed: 0).
    Named(Seat, u8),
    /// Numbers: this seat threw away this card (shown).
    Discarded(Seat, Card),
    /// Booray: this seat played this card to the trick.
    Played(Seat, Card),
    /// Booray: this seat took the trick.
    TrickWon(Seat),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Observation {
    pub seat: Seat,
    pub setup: HandSetup,
    /// Every face-down card dealt to this seat, in order (empty in Indian games).
    pub own_dealt: SmallVec<[Card; 12]>,
    /// This seat's own hidden choices in order: declarations, draw/swap masks, arrangements.
    pub own_choices: SmallVec<[u8; 6]>,
    pub history: SmallVec<[Event; 64]>,
    /// Everyone's declarations, once the hand reaches showdown.
    pub declarations: Option<Vec<Option<u8>>>,
    /// Other seats' current face-down cards when this seat can see them.
    pub shown: Vec<Option<SmallVec<[Card; 8]>>>,
    /// Indian games: this seat's own cards, revealed at showdown.
    pub own_shown: Option<SmallVec<[Card; 8]>>,
    /// Cards passed to this seat, in order.
    pub own_received: SmallVec<[Card; 8]>,
    /// What this hand carried in from earlier hands of the game (public).
    pub carry: ScriptCarry,
}

#[cfg(test)]
mod tests;
