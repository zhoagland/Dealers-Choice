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
use engine::pot::{Claim, Share, award_shares, build_pots, order_from_button};
use engine::replay::replay_interleaved;
use engine::view::{BoardView, CardSlot, PotView, SeatStatus, SeatView, TableView};
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
}

impl DeclareKind {
    pub fn names(self) -> [&'static str; 3] {
        match self {
            DeclareKind::HiLo => ["High", "Low", "Both"],
            DeclareKind::Boards => ["Top", "Bottom", "Both"],
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
                let first = self.first_to_act(s);
                s.betting.start_street(s.streets, first);
                s.streets += 1;
                s.swapped = 0;
            }
            Step::Declare | Step::Draw { .. } | Step::Discard { .. } | Step::PairUp => {
                s.pending = s.live_from_left().into();
            }
            Step::RedRiver { board } => {
                s.red_betting = false;
                s.queue.push_back(Slot::Board(board));
            }
        }
    }

    /// Move on until a seat must act, a card must be dealt, or the hand is over.
    fn advance(&self, s: &mut State) {
        loop {
            if s.done {
                return;
            }
            if s.betting.live_count() <= 1 {
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
                Step::Declare | Step::Draw { .. } | Step::Discard { .. } | Step::PairUp => {
                    while s.pending.front().is_some_and(|&p| s.betting.is_folded(p)) {
                        s.pending.pop_front();
                    }
                    s.pending.is_empty()
                }
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
            _ => {}
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
            for (k, rule) in shares.iter().enumerate() {
                let board: &[Card] = s
                    .boards
                    .get(rule.board as usize)
                    .map_or(&[], |b| b.as_slice());
                out[k][p.index()] = hand::share_value(rule, &own, board, self.script.wild);
            }
        }
        out
    }

    fn winnings(&self, s: &State) -> Vec<Chips> {
        let n = s.n() as usize;
        let folded = (0..n)
            .filter(|&i| s.betting.is_folded(Seat(i as u8)))
            .fold(0u16, |m, i| m | 1 << i);
        let pots = build_pots(
            &s.betting.live_contributed(),
            s.betting.dead_total(),
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
    type Carry = ();

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
            length: GameLength::SingleHand,
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

    fn new_hand_with(&self, setup: &HandSetup, _carry: &()) -> State {
        let n = setup.num_seats as usize;
        let min_bet = match self.config.structure {
            Structure::SpreadLimit { min_inc, .. } => min_inc,
            Structure::FixedLimit { small, .. } => small,
            _ => Chips(100),
        };
        let mut betting = Betting::new(self.config.structure, &setup.stacks, min_bet);
        if setup.first_hand_of_game && self.config.dealer_ante > Chips::ZERO {
            betting.post_dead(setup.chooser, self.config.dealer_ante);
        }
        if self.script.ante_each > Chips::ZERO {
            for p in setup.seats() {
                betting.post_dead(p, self.script.ante_each.min(setup.stacks[p.index()]));
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
        match self.script.steps[s.pc] {
            Step::Declare | Step::Draw { .. } | Step::Discard { .. } | Step::PairUp => {
                ToAct::Seat(*s.pending.front().expect("step with nobody pending"))
            }
            _ => ToAct::Seat(s.betting.to_act().expect("advance left a closed street")),
        }
    }

    fn legal_actions(&self, s: &State, out: &mut Vec<Action>) {
        out.clear();
        match self.to_act(s) {
            ToAct::Chance => out.extend(s.stock.iter().map(Action::Deal)),
            ToAct::Terminal => {}
            ToAct::Seat(p) => match self.script.steps[s.pc] {
                Step::Declare => out.extend(DECLARE_OPTIONS.map(Action::Declare)),
                Step::PairUp => out.extend((0..15).map(Action::Arrange)),
                Step::Draw { .. } | Step::Discard { .. } => {
                    let mut masks = Vec::new();
                    self.throw_masks(s, p, &mut masks);
                    out.extend(masks.into_iter().map(Action::Draw));
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
            (ToAct::Chance, Action::Deal(c)) if s.stock.contains(c) => Ok(()),
            (ToAct::Chance, _) => Err(IllegalAction("expected a deal of an undealt card".into())),
            (ToAct::Terminal, _) => Err(IllegalAction("the hand is over".into())),
            (ToAct::Seat(p), a) => match (self.script.steps[s.pc], a) {
                (Step::Declare, Action::Declare(k)) if k < 3 => Ok(()),
                (Step::Declare, _) => Err(IllegalAction(format!(
                    "declare {}",
                    self.declare()
                        .map_or(["High", "Low", "Both"], |d| d.names())
                        .join(", ")
                ))),
                (Step::PairUp, Action::Arrange(k)) if k < 15 => Ok(()),
                (Step::PairUp, _) => Err(IllegalAction(
                    "split your six cards into three pairs".into(),
                )),
                (Step::Draw { .. } | Step::Discard { .. }, Action::Draw(m)) => {
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
                                "throw away at most {max} card(s), or {} keeping (and showing)                                  an Ace",
                                max + 1
                            ),
                            Some(Step::Draw { max, .. }) => {
                                format!("throw away at most {max} card(s)")
                            }
                            _ => "choose cards to throw away".into(),
                        }))
                    }
                }
                (Step::Draw { .. } | Step::Discard { .. }, _) => {
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
                    Some(b) => s.betting.validate(p, b),
                    None => Err(IllegalAction("it's a betting round".into())),
                },
            },
        }
    }

    fn apply(&self, s: &mut State, a: Action) {
        let actor = match a {
            Action::Deal(c) => {
                s.stock.remove(c);
                let slot = s.queue.pop_front().expect("deal with no slot");
                match slot {
                    Slot::Down(p) => s.down[p.index()].push(c),
                    Slot::Up(p) => s.up[p.index()].push(c),
                    Slot::Board(b) => s.boards[b as usize].push(c),
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
        let p = 1.0 / s.stock.len() as f64;
        out.extend(s.stock.iter().map(|c| (Action::Deal(c), p)));
    }

    fn sample_chance(&self, s: &State, rng: &mut DetRng) -> Action {
        Action::Deal(
            s.stock
                .iter()
                .nth(rng.random_range(0..s.stock.len()))
                .unwrap(),
        )
    }

    fn payoffs(&self, s: &State) -> Vec<Chips> {
        self.winnings(s)
            .iter()
            .zip(s.betting.contributed())
            .map(|(&w, &c)| w - c)
            .collect()
    }

    fn observe(&self, s: &State, seat: Seat) -> Observation {
        let mut slots = s.deal_log.iter();
        let mut own_dealt = SmallVec::new();
        let mut own_choices = SmallVec::new();
        let mut history: SmallVec<[Event; 64]> = SmallVec::new();
        let mut reshuffles = s.reshuffles.iter().peekable();
        let mut aces = s.ace_shows.iter().peekable();
        for (i, &(who, a)) in s.history.iter().enumerate() {
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
                },
                (Some(p), Action::Declare(k)) => {
                    if p == seat {
                        own_choices.push(k);
                    }
                    Event::Declared(p)
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
                (Some(p), bet) => Event::Bet(p, bet.as_bet().unwrap()),
                (None, _) => unreachable!("only chance has no actor"),
            };
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

        // 1. Walk the public events. Each hidden deal is a slot; each seat's hand is a list of
        //    slots, following its draws/swaps/arrangements (others': the lowest positions and no
        //    rearranging, arbitrary but consistent because their cards are sampled).
        let mut hand_slots: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut slot_epoch: Vec<usize> = Vec::new();
        let mut deals: Vec<(DealRef, usize)> = Vec::new();
        // Slots thrown away, with the epoch they were thrown in.
        let mut thrown: Vec<(usize, usize)> = Vec::new();
        let mut forced: Vec<(usize, Card)> = Vec::new();
        let mut choices: Vec<Action> = Vec::new();
        let mut own = obs.own_choices.iter();
        let mut epoch = 0;
        for e in &obs.history {
            match *e {
                Event::HiddenDeal(p) => {
                    let id = slot_epoch.len();
                    slot_epoch.push(epoch);
                    hand_slots[p.index()].push(id);
                    deals.push((DealRef::Slot(id), epoch));
                }
                Event::Up(_, c) | Event::Board(_, c) => deals.push((DealRef::Known(c), epoch)),
                Event::Reshuffle => epoch += 1,
                Event::Declared(p) => {
                    if p == me {
                        own.next();
                    }
                }
                Event::Bet(..) => {}
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
                Event::Drew(p, k, _) | Event::Swapped(p, k) => {
                    let mask = if p == me {
                        *own.next().expect("own draw missing")
                    } else {
                        ((1u16 << k) - 1) as u8
                    };
                    let hs = &mut hand_slots[p.index()];
                    let mut i = 0;
                    hs.retain(|&id| {
                        let keep = mask & (1 << i) == 0;
                        if !keep {
                            thrown.push((id, epoch));
                        }
                        i += 1;
                        keep
                    });
                    if let Event::Drew(_, _, Some(ace)) = *e {
                        forced.push((hs[0], ace));
                    }
                    choices.push(match *e {
                        Event::Drew(..) => Action::Draw(mask),
                        _ => Action::Swap(mask),
                    });
                }
            }
        }

        // 2. Cards we know for slots: our own deals, shown hands (in hand order), shown Aces.
        let mut slot_card: Vec<Option<Card>> = vec![None; slot_epoch.len()];
        if !indian {
            // Our own deals, in deal order.
            for (id, &c) in slot_owner(&obs.history, me).into_iter().zip(&obs.own_dealt) {
                slot_card[id] = Some(c);
            }
        } else if let Some(cards) = &obs.own_shown {
            for (&id, &c) in hand_slots[me.index()].iter().zip(cards) {
                slot_card[id] = Some(c);
            }
        }
        for (i, shown) in obs.shown.iter().enumerate() {
            if let Some(cards) = shown {
                for (&id, &c) in hand_slots[i].iter().zip(cards) {
                    slot_card[id] = Some(c);
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
            let sampled =
                sample_hidden_hands(sampler, &unknown_counts, self.deck().difference(known), rng);
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
        for e in &obs.history {
            match *e {
                Event::HiddenDeal(_) | Event::Up(..) | Event::Board(..) | Event::Reshuffle => {}
                Event::Bet(_, b) => actions.push(Action::from_bet(b)),
                Event::Drew(p, ..) | Event::Swapped(p, _) | Event::Arranged(p) => {
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
        replay_interleaved(self, &obs.setup, deal_actions, actions)
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
            Action::Draw(_) | Action::Arrange(_) => ActionClass::CardChoice,
        }
    }

    fn private_cards(&self, s: &State, seat: Seat) -> engine::CardVec {
        s.down[seat.index()].iter().copied().collect()
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
                BoardView::row(
                    names.get(k).copied().unwrap_or("Board"),
                    cards.iter().map(|&c| CardSlot::Up(c)).collect(),
                )
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
            Action::Arrange(k) => {
                let p = PAIRINGS[k as usize].map(|i| i + 1);
                format!(
                    "pairs ({},{}) ({},{}) ({},{})",
                    p[0], p[1], p[2], p[3], p[4], p[5]
                )
            }
            a => format!("{a:?}"),
        }
    }
}

/// Slot ids (in deal order) of every hidden deal to `seat`.
fn slot_owner(history: &[Event], seat: Seat) -> Vec<usize> {
    let mut out = Vec::new();
    let mut id = 0;
    for e in history {
        if let Event::HiddenDeal(p) = *e {
            if p == seat {
                out.push(id);
            }
            id += 1;
        }
    }
    out
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
}

#[cfg(test)]
mod tests;
