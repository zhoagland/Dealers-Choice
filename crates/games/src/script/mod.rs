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
    /// Each seat still in discards up to `max` face-down cards and is dealt replacements.
    Draw { max: u8 },
    /// Each seat still in discards exactly `n` face-down cards (no replacements).
    Discard(u8),
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
}

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
    dealt: CardSet,
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
                        .collect();
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
                for _ in 0..n {
                    for &p in &live {
                        s.queue.push_back(match self.script.steps[s.pc] {
                            Step::Down(_) => Slot::Down(p),
                            _ => Slot::Up(p),
                        });
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
            }
            Step::Declare | Step::Draw { .. } | Step::Discard(_) => {
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
                return;
            }
            if !s.queue.is_empty() {
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
                Step::Declare | Step::Draw { .. } | Step::Discard(_) => {
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
                        let left = self.deck().difference(s.dealt).len();
                        if red && left > 0 {
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

    /// Discard counts allowed for `seat` in the current step.
    fn draw_counts(&self, s: &State) -> (u8, u8) {
        match self.script.steps.get(s.pc) {
            Some(Step::Draw { max }) => (0, *max),
            Some(Step::Discard(n)) => (*n, *n),
            _ => (1, 0),
        }
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

    /// Who can see `p`'s face-down cards from `viewer`'s seat right now.
    fn sees_down(&self, s: &State, viewer: Seat, p: Seat) -> bool {
        let showdown_shown = s.is_showdown() && !s.betting.is_folded(p);
        match self.script.visibility {
            Visibility::Normal => viewer == p || showdown_shown,
            Visibility::Showtime => viewer == p || showdown_shown || s.betting.is_folded(p),
            Visibility::Indian => viewer != p || showdown_shown,
        }
    }
}

const DECLARE_OPTIONS: [u8; 3] = [0, 1, 2];

impl Game for Scripted {
    type State = State;
    type Action = Action;
    type Observation = Observation;
    type Config = Config;
    type Carry = ();

    fn new(config: Config) -> Scripted {
        let script = games::script(&config.game)
            .unwrap_or_else(|| panic!("unknown scripted game {:?}", config.game));
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
            dealt: CardSet::EMPTY,
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
            Step::Declare | Step::Draw { .. } | Step::Discard(_) => {
                ToAct::Seat(*s.pending.front().expect("step with nobody pending"))
            }
            _ => ToAct::Seat(s.betting.to_act().expect("advance left a closed street")),
        }
    }

    fn legal_actions(&self, s: &State, out: &mut Vec<Action>) {
        out.clear();
        match self.to_act(s) {
            ToAct::Chance => out.extend(self.deck().difference(s.dealt).iter().map(Action::Deal)),
            ToAct::Terminal => {}
            ToAct::Seat(p) => match self.script.steps[s.pc] {
                Step::Declare => out.extend(DECLARE_OPTIONS.map(Action::Declare)),
                Step::Draw { .. } | Step::Discard(_) => {
                    let k = s.down[p.index()].len() as u32;
                    let (lo, hi) = self.draw_counts(s);
                    for m in 0u32..(1 << k) {
                        let c = m.count_ones() as u8;
                        if c >= lo && c <= hi {
                            out.push(Action::Draw(m as u8));
                        }
                    }
                }
                _ => out.extend(
                    s.betting
                        .menu(p, &self.config.bet_menu)
                        .into_iter()
                        .map(Action::from_bet),
                ),
            },
        }
    }

    fn validate(&self, s: &State, a: Action) -> Result<(), IllegalAction> {
        match (self.to_act(s), a) {
            (ToAct::Chance, Action::Deal(c)) if self.deck().contains(c) && !s.dealt.contains(c) => {
                Ok(())
            }
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
                (Step::Draw { .. } | Step::Discard(_), Action::Draw(m)) => {
                    let k = s.down[p.index()].len();
                    let (lo, hi) = self.draw_counts(s);
                    let c = m.count_ones() as u8;
                    if (m as u32) >> k != 0 {
                        Err(IllegalAction("pick cards you hold".into()))
                    } else if c < lo || c > hi {
                        Err(IllegalAction(if lo == hi {
                            format!("throw away exactly {lo} card(s)")
                        } else {
                            format!("throw away at most {hi} card(s)")
                        }))
                    } else {
                        Ok(())
                    }
                }
                (Step::Draw { .. } | Step::Discard(_), _) => {
                    Err(IllegalAction("choose which cards to throw away".into()))
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
                s.dealt.insert(c);
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
            Action::Draw(m) => {
                let p = s.pending.pop_front().expect("no drawer");
                let hand = &mut s.down[p.index()];
                let kept: SmallVec<[Card; 8]> = hand
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| m & (1 << i) == 0)
                    .map(|(_, &c)| c)
                    .collect();
                *hand = kept;
                if matches!(self.script.steps[s.pc], Step::Draw { .. }) {
                    for _ in 0..m.count_ones() {
                        s.queue.push_back(Slot::Down(p));
                    }
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
        let left = self.deck().difference(s.dealt);
        let p = 1.0 / left.len() as f64;
        out.extend(left.iter().map(|c| (Action::Deal(c), p)));
    }

    fn sample_chance(&self, s: &State, rng: &mut DetRng) -> Action {
        let left = self.deck().difference(s.dealt);
        Action::Deal(left.iter().nth(rng.random_range(0..left.len())).unwrap())
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
        let history = s
            .history
            .iter()
            .map(|&(who, a)| match (who, a) {
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
                    Event::Drew(p, m.count_ones() as u8)
                }
                (Some(p), bet) => Event::Bet(p, bet.as_bet().unwrap()),
                (None, _) => unreachable!("only chance has no actor"),
            })
            .collect();
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

        // 1. Every hidden deal, as (seat, slot id), and each seat's hand as a list of slot ids,
        //    following the draws (others' discards: the lowest positions, which is arbitrary but
        //    consistent because their cards are sampled).
        let mut hand_slots: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut slot_seat: Vec<Seat> = Vec::new();
        let mut known = CardSet::EMPTY;
        let mut own_choice = obs.own_choices.iter();
        let mut draw_masks: Vec<u8> = Vec::new();
        for e in &obs.history {
            match *e {
                Event::HiddenDeal(p) => {
                    hand_slots[p.index()].push(slot_seat.len());
                    slot_seat.push(p);
                }
                Event::Up(_, c) | Event::Board(_, c) => {
                    known.insert(c);
                }
                Event::Drew(p, k) => {
                    let mask = if p == me {
                        *own_choice.next().expect("own draw missing")
                    } else {
                        ((1u16 << k) - 1) as u8
                    };
                    draw_masks.push(mask);
                    let hs = &mut hand_slots[p.index()];
                    let mut i = 0;
                    hs.retain(|_| {
                        let keep = mask & (1 << i) == 0;
                        i += 1;
                        keep
                    });
                }
                Event::Declared(p) => {
                    if p == me {
                        own_choice.next();
                    }
                }
                Event::Bet(..) => {}
            }
        }

        // 2. Cards we know for slots: our own deals, shown hands (in hand order).
        let mut slot_card: Vec<Option<Card>> = vec![None; slot_seat.len()];
        if !indian {
            let mut own = obs.own_dealt.iter();
            for (id, &p) in slot_seat.iter().enumerate() {
                if p == me {
                    slot_card[id] = own.next().copied();
                }
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
        for c in slot_card.iter().flatten() {
            known.insert(*c);
        }

        // 3. Sample the unknown current hands (through the sampler, so ranges apply), then any
        //    unknown discarded cards uniformly.
        let requests: Vec<(Seat, usize)> = (0..n)
            .map(|i| Seat(i as u8))
            .filter(|p| {
                hand_slots[p.index()]
                    .iter()
                    .any(|&id| slot_card[id].is_none())
            })
            .map(|p| (p, hand_slots[p.index()].len()))
            .collect();
        let sampled = sample_hidden_hands(sampler, &requests, self.deck().difference(known), rng);
        for (&(p, _), cards) in requests.iter().zip(sampled) {
            for (&id, c) in hand_slots[p.index()].iter().zip(cards) {
                slot_card[id] = Some(c);
                known.insert(c);
            }
        }
        let mut rest: Vec<Card> = self.deck().difference(known).iter().collect();
        for c in slot_card.iter_mut().filter(|c| c.is_none()) {
            let i = rng.random_range(0..rest.len());
            *c = Some(rest.swap_remove(i));
        }

        // 4. Rebuild the event sequence and replay.
        let mut next_slot = 0;
        let mut masks = draw_masks.into_iter();
        let mut own_decl = obs.own_choices.iter();
        let mut deals = Vec::new();
        let mut actions = Vec::new();
        for e in &obs.history {
            match *e {
                Event::HiddenDeal(_) => {
                    deals.push(Action::Deal(slot_card[next_slot].unwrap()));
                    next_slot += 1;
                }
                Event::Up(_, c) | Event::Board(_, c) => deals.push(Action::Deal(c)),
                Event::Bet(_, b) => actions.push(Action::from_bet(b)),
                Event::Drew(p, _) => {
                    if p == me {
                        own_decl.next();
                    }
                    actions.push(Action::Draw(masks.next().unwrap()));
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
        replay_interleaved(self, &obs.setup, deals, actions)
    }

    fn classify(&self, s: &State, a: Action) -> ActionClass {
        match a {
            Action::Deal(_) => ActionClass::Chance,
            Action::Fold => ActionClass::Fold,
            Action::CheckCall => {
                let owed = match s.betting.to_act() {
                    Some(p) => s.betting.owed(p).min(s.betting.stacks()[p.index()]),
                    None => Chips::ZERO,
                };
                ActionClass::CheckOrCall { owed }
            }
            Action::RaiseTo(to) => ActionClass::BetOrRaise { to },
            Action::Declare(_) => ActionClass::Declare,
            Action::Draw(_) => ActionClass::CardChoice,
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
                if let Some(d) = self.declare() {
                    let shown = obs.declarations.as_ref().and_then(|ds| ds[i]);
                    let mine = (p == obs.seat).then(|| {
                        obs.history
                            .iter()
                            .any(|e| matches!(e, Event::Declared(q) if *q == p))
                    });
                    if let Some(k) = shown {
                        v.badges.push(d.names()[k as usize].into());
                    } else if mine == Some(true) {
                        if let Some(k) = s.declarations[i] {
                            v.badges
                                .push(format!("you declared {}", d.names()[k as usize]));
                        }
                    } else if obs
                        .history
                        .iter()
                        .any(|e| matches!(e, Event::Declared(q) if *q == p))
                    {
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
        TableView {
            game: self.meta.display_name.into(),
            phase: match self.script.steps.get(s.pc) {
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
        out
    }

    fn describe_action(&self, a: Action) -> String {
        match a {
            Action::Declare(k) => self.declare().map_or(format!("declare {k}"), |d| {
                format!("declare {}", d.names()[k as usize])
            }),
            Action::Draw(0) => "stand pat".into(),
            Action::Draw(m) => format!("throw away cards {}", mask_positions(m)),
            a => format!("{a:?}"),
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
    /// This seat threw away this many face-down cards.
    Drew(Seat, u8),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Observation {
    pub seat: Seat,
    pub setup: HandSetup,
    /// Every face-down card dealt to this seat, in order (empty in Indian games).
    pub own_dealt: SmallVec<[Card; 12]>,
    /// This seat's own hidden choices in order: declarations and draw masks.
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
