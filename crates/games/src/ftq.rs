//! Follow the Queen: house rules (Poker Bible §Follow the Queen). PRD §7; `docs/tech/games.md`.
//!
//! Rule decisions (Step 0 of `docs/dev/adding-a-game.md`), each covered by a test below:
//! - 7-card stud: 2 down + 1 up (3rd street), up (4th, 5th, 6th), down (7th); a betting round
//!   after each. Cards go one at a time to live seats starting left of the button; folded seats
//!   get no more cards. Max 7 seats (7 × 7 = 49 ≤ 52).
//! - Forced money: only the $2 dealer ante (chooser, first hand of the game; FTQ is one hand).
//! - Spread limit: each bet/raise adds 50¢–$2 in 50¢ steps, at most 4 per street.
//! - First to act each street: best board showing (visible upcards only, current wild, no partial
//!   straights/flushes); ties → closest to the dealer's left.
//! - Wilds: when a Queen is dealt face up, the rank of the **next upcard dealt** becomes wild for
//!   everyone. A later face-up Queen resets this. **Queens are not wild**, except: if the card after
//!   a face-up Queen is itself a Queen, Queens become wild. If the **last** upcard dealt is a Queen,
//!   there is **no** wild card. Downcards never affect wilds.
//! - Wilds play as the best possible card for high and for low (ace-to-six).
//! - After 7th-street betting: each live seat declares High / Low / Both. Declarations are hidden
//!   until everyone has declared (simultaneous, API doc §1 rule 2). Then all live hands are shown
//!   and pots are split per `engine::pot::award_declared`.

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use engine::betting::{BetAction, BetSize, Betting, Structure};
use engine::eval::{eval_high_wild, eval_low_a6_wild, eval_partial_board};
use engine::game::sample_hidden_hands;
use engine::pot::{Declare, award_declared, build_pots, order_from_button};
use engine::replay::replay_interleaved;
use engine::{
    ActionClass, Card, CardSet, Chips, DetRng, Game, GameLength, GameMeta, HandSetup, HandValue,
    HiddenSampler, IllegalAction, Rank, Seat, StakeModel, ToAct,
};
use rand::Rng;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub dealer_ante: Chips,
    pub structure: Structure,
    pub bet_menu: Vec<BetSize>,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            dealer_ante: Chips(200),
            structure: Structure::SpreadLimit {
                min_inc: Chips(50),
                max_inc: Chips(200),
                step: Chips(50),
                max_raises: 4,
            },
            // Smallest and largest increment: keeps the bots' tree narrow.
            bet_menu: vec![BetSize::MinRaise, BetSize::MaxRaise],
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Action {
    Deal(Card),
    Fold,
    CheckCall,
    RaiseTo(Chips),
    Declare(Declare),
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

/// The wild-card state machine (games.md §FTQ).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Wild {
    None,
    /// A Queen was just dealt face up; the next upcard decides.
    AwaitingNextUpcard,
    Rank(Rank),
}

impl Wild {
    /// The new state after an upcard is dealt.
    pub fn on_upcard(self, card: Card) -> Wild {
        match (self, card.rank()) {
            (Wild::AwaitingNextUpcard, r) => Wild::Rank(r), // includes Q after Q: Queens wild
            (_, Rank::Queen) => Wild::AwaitingNextUpcard,
            (w, _) => w,
        }
    }

    /// The wild rank in force. A pending Queen counts as none; if it's still pending at showdown,
    /// the Queen was the last upcard, which means no wild.
    pub fn rank(self) -> Option<Rank> {
        match self {
            Wild::Rank(r) => Some(r),
            _ => None,
        }
    }
}

/// Where a card goes.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
enum Slot {
    Down(Seat),
    Up(Seat),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
enum Phase {
    /// Dealing street 3..=7.
    Dealing(u8),
    /// Betting after street 3..=7.
    Betting(u8),
    Declare,
    Done,
}

#[derive(Clone, Debug)]
pub struct State {
    setup: HandSetup,
    down: Vec<SmallVec<[Card; 3]>>,
    up: Vec<SmallVec<[Card; 4]>>,
    dealt: CardSet,
    wild: Wild,
    phase: Phase,
    betting: Betting,
    /// Pending deals, next first.
    queue: SmallVec<[Slot; 21]>,
    /// Where each past deal went, in order (so observations can label them).
    deal_log: Vec<Slot>,
    declarations: Vec<Option<Declare>>,
    /// (actor, action); `None` actor = chance.
    history: Vec<(Option<Seat>, Action)>,
}

impl State {
    fn n(&self) -> u8 {
        self.setup.num_seats
    }

    pub fn wild(&self) -> Wild {
        self.wild
    }

    /// Live seats in dealing order (starting left of the button).
    fn live_from_left(&self) -> Vec<Seat> {
        order_from_button(self.setup.button, self.n())
            .into_iter()
            .filter(|&s| !self.betting.is_folded(s))
            .collect()
    }

    /// Queue one card per live seat per round for `street` (3rd street = down, down, up).
    fn queue_street(&mut self, street: u8) {
        let live = self.live_from_left();
        let rounds: &[fn(Seat) -> Slot] = match street {
            3 => &[Slot::Down, Slot::Down, Slot::Up],
            4..=6 => &[Slot::Up],
            7 => &[Slot::Down],
            _ => unreachable!("FTQ has streets 3..=7"),
        };
        for make in rounds {
            self.queue.extend(live.iter().map(|&s| make(s)));
        }
    }

    /// Best board showing acts first; ties go to the seat closest to the dealer's left.
    fn first_to_act(&self) -> Seat {
        let wild = self.wild.rank();
        let mut best: Option<(HandValue, Seat)> = None;
        for s in self.live_from_left() {
            let v = eval_partial_board(&self.up[s.index()], wild);
            // Strict `>`: an equal board later in the order doesn't take over.
            if best.is_none_or(|(b, _)| v > b) {
                best = Some((v, s));
            }
        }
        best.expect("no live seat").1
    }

    fn next_declarer(&self) -> Option<Seat> {
        self.live_from_left()
            .into_iter()
            .find(|s| self.declarations[s.index()].is_none())
    }

    /// Move through phases until a seat must act, a card must be dealt, or the hand is over.
    /// It's a loop because all-ins can close several betting rounds in a row.
    fn advance(&mut self) {
        loop {
            if self.phase == Phase::Done {
                return;
            }
            if self.betting.live_count() <= 1 {
                self.phase = Phase::Done;
                return;
            }
            match self.phase {
                Phase::Dealing(n) => {
                    if !self.queue.is_empty() {
                        return;
                    }
                    let first = self.first_to_act();
                    self.betting.start_street(n - 3, first);
                    self.phase = Phase::Betting(n);
                }
                Phase::Betting(n) => {
                    if self.betting.to_act().is_some() {
                        return;
                    }
                    if n == 7 {
                        self.phase = Phase::Declare;
                    } else {
                        self.queue_street(n + 1);
                        self.phase = Phase::Dealing(n + 1);
                    }
                }
                Phase::Declare => {
                    if self.next_declarer().is_none() {
                        self.phase = Phase::Done;
                    }
                    return;
                }
                Phase::Done => return,
            }
        }
    }

    fn is_showdown(&self) -> bool {
        self.phase == Phase::Done && self.betting.live_count() > 1
    }

    /// (high, low) per seat at showdown; `None` for folded seats or without a showdown.
    fn values(&self) -> (Vec<Option<HandValue>>, Vec<Option<HandValue>>) {
        let n = self.n() as usize;
        let mut high = vec![None; n];
        let mut low = vec![None; n];
        if !self.is_showdown() {
            return (high, low);
        }
        let wild = self.wild.rank();
        for s in self.live_from_left() {
            let all: CardSet = self.down[s.index()]
                .iter()
                .chain(&self.up[s.index()])
                .copied()
                .collect();
            let wilds: CardSet = all.iter().filter(|c| Some(c.rank()) == wild).collect();
            let naturals = all.difference(wilds);
            high[s.index()] = Some(eval_high_wild(naturals, wilds.len()));
            low[s.index()] = Some(eval_low_a6_wild(naturals, wilds.len()));
        }
        (high, low)
    }

    fn winnings(&self) -> Vec<Chips> {
        let n = self.n() as usize;
        let folded = (0..n)
            .filter(|&i| self.betting.is_folded(Seat(i as u8)))
            .fold(0u16, |m, i| m | 1 << i);
        let pots = build_pots(
            &self.betting.live_contributed(),
            self.betting.dead_total(),
            folded,
        );
        let (high, low) = self.values();
        let order = order_from_button(self.setup.button, self.n());
        award_declared(&pots, &self.declarations, &high, &low, &order)
    }
}

// ---------------------------------------------------------------------------------------------
// Observation
// ---------------------------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PublicEvent {
    /// A face-down card went to this seat (the observer's own are in `own_down`).
    DownDealt(Seat),
    Up(Seat, Card),
    Bet(Seat, BetAction),
    /// This seat has declared; *what* stays hidden until everyone has.
    Declared(Seat),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Observation {
    pub seat: Seat,
    pub setup: HandSetup,
    pub own_down: SmallVec<[Card; 3]>,
    pub own_declaration: Option<Declare>,
    pub history: SmallVec<[PublicEvent; 64]>,
    /// Every seat's declaration, once the hand reaches showdown.
    pub declarations: Option<Vec<Option<Declare>>>,
    /// Other live hands' downcards at showdown.
    pub shown: Vec<Option<SmallVec<[Card; 3]>>>,
}

impl Observation {
    pub fn upcards(&self, seat: Seat) -> SmallVec<[Card; 4]> {
        self.history
            .iter()
            .filter_map(|e| match *e {
                PublicEvent::Up(s, c) if s == seat => Some(c),
                _ => None,
            })
            .collect()
    }

    /// The wild state, replayed from the public upcards.
    pub fn wild(&self) -> Wild {
        self.history.iter().fold(Wild::None, |w, e| match *e {
            PublicEvent::Up(_, c) => w.on_upcard(c),
            _ => w,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Game
// ---------------------------------------------------------------------------------------------

pub struct FollowTheQueen {
    config: Config,
    meta: GameMeta,
}

const DECLARES: [Declare; 3] = [Declare::High, Declare::Low, Declare::Both];

impl Game for FollowTheQueen {
    type State = State;
    type Action = Action;
    type Observation = Observation;
    type Config = Config;

    fn new(config: Config) -> FollowTheQueen {
        FollowTheQueen {
            config,
            meta: GameMeta {
                id: "ftq",
                version: 1,
                display_name: "Follow the Queen (hi/lo declare)",
                seats: 2..=7,
                stake_model: StakeModel::Bankroll,
                length: GameLength::SingleHand,
                rules_ref: "Poker Bible §Follow the Queen; docs/tech/games.md §FTQ",
            },
        }
    }

    fn meta(&self) -> &GameMeta {
        &self.meta
    }

    fn new_hand(&self, setup: &HandSetup) -> State {
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
        let mut s = State {
            setup: setup.clone(),
            down: vec![SmallVec::new(); n],
            up: vec![SmallVec::new(); n],
            dealt: CardSet::EMPTY,
            wild: Wild::None,
            phase: Phase::Dealing(3),
            betting,
            queue: SmallVec::new(),
            deal_log: Vec::new(),
            declarations: vec![None; n],
            history: Vec::new(),
        };
        s.queue_street(3);
        s
    }

    fn to_act(&self, s: &State) -> ToAct {
        if !s.queue.is_empty() {
            return ToAct::Chance;
        }
        match s.phase {
            Phase::Done => ToAct::Terminal,
            Phase::Declare => {
                ToAct::Seat(s.next_declarer().expect("declare phase with no declarer"))
            }
            _ => ToAct::Seat(s.betting.to_act().expect("advance left a closed street")),
        }
    }

    fn legal_actions(&self, s: &State, out: &mut Vec<Action>) {
        out.clear();
        match (self.to_act(s), s.phase) {
            (ToAct::Chance, _) => out.extend(
                CardSet::FULL_DECK
                    .difference(s.dealt)
                    .iter()
                    .map(Action::Deal),
            ),
            (ToAct::Seat(_), Phase::Declare) => out.extend(DECLARES.map(Action::Declare)),
            (ToAct::Seat(p), _) => out.extend(
                s.betting
                    .menu(p, &self.config.bet_menu)
                    .into_iter()
                    .map(Action::from_bet),
            ),
            (ToAct::Terminal, _) => {}
        }
    }

    fn validate(&self, s: &State, a: Action) -> Result<(), IllegalAction> {
        match (self.to_act(s), s.phase, a) {
            (ToAct::Chance, _, Action::Deal(c)) if !s.dealt.contains(c) => Ok(()),
            (ToAct::Chance, _, _) => {
                Err(IllegalAction("expected a deal of an undealt card".into()))
            }
            (ToAct::Seat(_), Phase::Declare, Action::Declare(_)) => Ok(()),
            (ToAct::Seat(_), Phase::Declare, _) => {
                Err(IllegalAction("declare high, low, or both".into()))
            }
            (ToAct::Seat(p), _, a) => match a.as_bet() {
                Some(b) => s.betting.validate(p, b),
                None => Err(IllegalAction("it's a betting round".into())),
            },
            (ToAct::Terminal, _, _) => Err(IllegalAction("the hand is over".into())),
        }
    }

    fn apply(&self, s: &mut State, a: Action) {
        let actor = match a {
            Action::Deal(c) => {
                s.dealt.insert(c);
                let slot = s.queue.remove(0);
                match slot {
                    Slot::Down(seat) => s.down[seat.index()].push(c),
                    Slot::Up(seat) => {
                        s.up[seat.index()].push(c);
                        s.wild = s.wild.on_upcard(c);
                    }
                }
                s.deal_log.push(slot);
                None
            }
            Action::Declare(d) => {
                let seat = s.next_declarer().expect("no one left to declare");
                s.declarations[seat.index()] = Some(d);
                Some(seat)
            }
            bet => {
                let seat = s.betting.to_act().expect("no seat to bet");
                s.betting.apply(seat, bet.as_bet().unwrap());
                Some(seat)
            }
        };
        s.history.push((actor, a));
        s.advance();
    }

    fn chance_outcomes(&self, s: &State, out: &mut Vec<(Action, f64)>) {
        out.clear();
        let left = CardSet::FULL_DECK.difference(s.dealt);
        let p = 1.0 / left.len() as f64;
        out.extend(left.iter().map(|c| (Action::Deal(c), p)));
    }

    fn sample_chance(&self, s: &State, rng: &mut DetRng) -> Action {
        let left = CardSet::FULL_DECK.difference(s.dealt);
        Action::Deal(left.iter().nth(rng.random_range(0..left.len())).unwrap())
    }

    fn payoffs(&self, s: &State) -> Vec<Chips> {
        s.winnings()
            .iter()
            .zip(s.betting.contributed())
            .map(|(&w, &c)| w - c)
            .collect()
    }

    fn observe(&self, s: &State, seat: Seat) -> Observation {
        let mut slots = s.deal_log.iter();
        let history = s
            .history
            .iter()
            .map(|&(who, a)| match (who, a) {
                (None, Action::Deal(c)) => match slots.next().expect("deal without a slot") {
                    Slot::Down(p) => PublicEvent::DownDealt(*p),
                    Slot::Up(p) => PublicEvent::Up(*p, c),
                },
                (Some(p), Action::Declare(_)) => PublicEvent::Declared(p),
                (Some(p), bet) => PublicEvent::Bet(p, bet.as_bet().unwrap()),
                (None, _) => unreachable!("only chance has no actor"),
            })
            .collect();
        let showdown = s.is_showdown();
        Observation {
            seat,
            setup: s.setup.clone(),
            own_down: s.down[seat.index()].clone(),
            own_declaration: s.declarations[seat.index()],
            history,
            declarations: showdown.then(|| s.declarations.clone()),
            shown: (0..s.n())
                .map(Seat)
                .map(|p| {
                    (showdown && !s.betting.is_folded(p) && p != seat)
                        .then(|| s.down[p.index()].clone())
                })
                .collect(),
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
        let mut downs_dealt = vec![0usize; n];
        let mut known: CardSet = obs.own_down.iter().copied().collect();
        for e in &obs.history {
            match *e {
                PublicEvent::DownDealt(p) => downs_dealt[p.index()] += 1,
                PublicEvent::Up(_, c) => {
                    known.insert(c);
                }
                _ => {}
            }
        }
        for cards in obs.shown.iter().flatten() {
            known = known.union(cards.iter().copied().collect());
        }
        let requests: Vec<(Seat, usize)> = (0..n)
            .map(|i| Seat(i as u8))
            .filter(|&p| {
                p != obs.seat && obs.shown[p.index()].is_none() && downs_dealt[p.index()] > 0
            })
            .map(|p| (p, downs_dealt[p.index()]))
            .collect();
        let sampled = sample_hidden_hands(
            sampler,
            &requests,
            CardSet::FULL_DECK.difference(known),
            rng,
        );

        let mut downs: Vec<SmallVec<[Card; 3]>> = vec![SmallVec::new(); n];
        downs[obs.seat.index()] = obs.own_down.clone();
        for (i, shown) in obs.shown.iter().enumerate() {
            if let Some(cards) = shown {
                downs[i] = cards.clone();
            }
        }
        for (&(p, _), cards) in requests.iter().zip(sampled) {
            downs[p.index()] = cards.into_iter().collect();
        }

        // Rebuild the event sequence. Hidden declarations are sampled uniformly (known bias,
        // API doc §5); our own and revealed ones are used as-is.
        let mut next_down = vec![0usize; n];
        let mut deals = Vec::new();
        let mut actions = Vec::new();
        for e in &obs.history {
            match *e {
                PublicEvent::DownDealt(p) => {
                    deals.push(Action::Deal(downs[p.index()][next_down[p.index()]]));
                    next_down[p.index()] += 1;
                }
                PublicEvent::Up(_, c) => deals.push(Action::Deal(c)),
                PublicEvent::Bet(_, b) => actions.push(Action::from_bet(b)),
                PublicEvent::Declared(p) => {
                    let d = if p == obs.seat {
                        obs.own_declaration.expect("own declaration missing")
                    } else {
                        match obs.declarations.as_ref().and_then(|d| d[p.index()]) {
                            Some(d) => d,
                            None => DECLARES[rng.random_range(0..3)],
                        }
                    };
                    actions.push(Action::Declare(d));
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
                    Some(seat) => s.betting.owed(seat).min(s.betting.stacks()[seat.index()]),
                    None => Chips::ZERO,
                };
                ActionClass::CheckOrCall { owed }
            }
            Action::RaiseTo(to) => ActionClass::BetOrRaise { to },
            Action::Declare(_) => ActionClass::Declare,
        }
    }

    fn private_cards(&self, s: &State, seat: Seat) -> engine::CardVec {
        s.down[seat.index()].iter().copied().collect()
    }
    /// $1: between the 50¢ minimum and $2 maximum increment.
    fn eval_unit(&self) -> Chips {
        Chips(100)
    }

    fn describe_observation(&self, obs: &Observation) -> String {
        // Stacks and pot are public, so any determinization reports them correctly.
        let s = self.determinize(
            obs,
            &mut engine::UniformSampler,
            &mut engine::rng::rng_from_seed(0),
        );
        let b = &s.betting;
        let cards = |c: &[Card]| {
            c.iter()
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join(" ")
        };
        let wild = match obs.wild() {
            Wild::None => "none".to_string(),
            Wild::AwaitingNextUpcard => "a Queen is showing: the next upcard decides".to_string(),
            Wild::Rank(r) => format!("{}s", r.to_char()),
        };
        let mut out = format!("Wild: {wild}   Pot: {}\n", b.pot());
        for i in 0..obs.setup.num_seats {
            let p = Seat(i);
            let downs = if p == obs.seat {
                format!("[{}]", cards(&obs.own_down))
            } else if let Some(c) = &obs.shown[i as usize] {
                format!("[{}]", cards(c))
            } else {
                "[hidden]".to_string()
            };
            let decl = match (&obs.declarations, p == obs.seat) {
                (Some(d), _) => d[i as usize]
                    .map(|d| format!("  declared {d:?}"))
                    .unwrap_or_default(),
                (None, true) => obs
                    .own_declaration
                    .map(|d| format!("  you declared {d:?}"))
                    .unwrap_or_default(),
                (None, false) if obs.history.contains(&PublicEvent::Declared(p)) => {
                    "  declared".into()
                }
                _ => String::new(),
            };
            out += &format!(
                "  {p}{}: up [{}] down {downs}  stack {}{}{decl}\n",
                if p == obs.setup.button {
                    " (dealer)"
                } else {
                    ""
                },
                cards(&obs.upcards(p)),
                b.stacks()[i as usize],
                if b.is_folded(p) { "  folded" } else { "" },
            );
        }
        if s.phase != Phase::Declare && b.to_act() == Some(obs.seat) {
            out += &format!("To call: {}\n", b.owed(obs.seat));
        }
        out
    }

    fn describe_action(&self, a: Action) -> String {
        match a {
            Action::Deal(c) => format!("deal {c}"),
            Action::Fold => "fold".into(),
            Action::CheckCall => "check/call".into(),
            Action::RaiseTo(x) => format!("bet/raise to {x}"),
            Action::Declare(d) => format!("declare {d:?}").to_lowercase(),
        }
    }

    /// `f`, `c`, `r 1.50`, and for declarations `hi` / `lo` / `both`.
    fn parse_action(&self, input: &str, _legal: &[Action]) -> Option<Action> {
        let mut w = input.split_whitespace();
        match (w.next()?, w.next()) {
            ("f", None) => Some(Action::Fold),
            ("c" | "k", None) => Some(Action::CheckCall),
            ("r" | "b", Some(x)) => {
                let dollars: f64 = x.trim_start_matches('$').parse().ok()?;
                Some(Action::RaiseTo(Chips::dollars(dollars)))
            }
            ("hi" | "h" | "high", None) => Some(Action::Declare(Declare::High)),
            ("lo" | "l" | "low", None) => Some(Action::Declare(Declare::Low)),
            ("both", None) => Some(Action::Declare(Declare::Both)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::replay::replay;
    use engine::testing::{Conformance, check_conformance};

    fn c(s: &str) -> Card {
        s.parse().unwrap()
    }

    fn game() -> FollowTheQueen {
        FollowTheQueen::new(Config::default())
    }

    #[test]
    fn conformance_2_4_7_seats() {
        let cfg = Conformance {
            hands: 60,
            seat_counts: vec![2, 4, 7],
            stack: Chips(5_000),
            ..Conformance::default()
        };
        check_conformance(&game(), &cfg);
    }

    #[test]
    fn conformance_short_stacks() {
        let cfg = Conformance {
            hands: 100,
            seat_counts: vec![3],
            stack: Chips(400),
            ..Conformance::default()
        };
        check_conformance(&game(), &cfg);
    }

    #[test]
    fn wild_state_machine() {
        let w = |cards: &[&str]| cards.iter().fold(Wild::None, |w, s| w.on_upcard(c(s)));
        assert_eq!(
            w(&["9s", "Qh", "5c"]),
            Wild::Rank(Rank::Five),
            "rank after the Queen"
        );
        assert_eq!(
            w(&["Qh", "5c", "Kd"]),
            Wild::Rank(Rank::Five),
            "stays until another Queen"
        );
        assert_eq!(
            w(&["Qh", "Qd"]),
            Wild::Rank(Rank::Queen),
            "Q then Q: Queens wild"
        );
        assert_eq!(
            w(&["Qh", "5c", "Qd"]),
            Wild::AwaitingNextUpcard,
            "a later Queen resets"
        );
        assert_eq!(
            w(&["Qh", "5c", "Qd"]).rank(),
            None,
            "Queen as last upcard: no wild"
        );
        assert_eq!(w(&["Qh", "5c", "Qd", "7h"]), Wild::Rank(Rank::Seven));
    }

    /// Heads-up, button = seat 0, so seat 1 is dealt first in every round. Seat 0's Queen on 3rd
    /// street makes seat 1's next upcard (5c) set fives wild. Seat 1 then has K + two wild fives
    /// = trip kings, which beats seat 0's pair of aces. Without the wild, seat 1 would lose.
    fn wild_hand(decl1: Declare, decl0: Declare) -> Vec<Action> {
        use Action::*;
        let d = |s: &str| Deal(c(s));
        vec![
            // 3rd street: downs to s1, s0; downs to s1, s0; ups to s1, s0.
            d("2c"),
            d("Ah"),
            d("3d"),
            d("Ad"),
            d("9s"),
            d("Qh"),
            CheckCall,
            CheckCall,
            d("5c"),
            d("Js"),
            CheckCall,
            CheckCall, // 4th: 5c follows the Queen → fives wild
            d("5h"),
            d("7c"),
            CheckCall,
            CheckCall, // 5th
            d("Kd"),
            d("4d"),
            CheckCall,
            CheckCall, // 6th
            d("8c"),
            d("Tc"),
            CheckCall,
            CheckCall, // 7th (down)
            Declare(decl1),
            Declare(decl0),
        ]
    }

    #[test]
    fn wild_cards_decide_the_showdown() {
        let g = game();
        let setup = HandSetup::simple(2, Chips(5_000)); // chooser seat 0 pays the $2 dealer ante
        let s = replay(&g, &setup, &wild_hand(Declare::High, Declare::High)).unwrap();
        assert_eq!(g.to_act(&s), ToAct::Terminal);
        assert_eq!(s.wild(), Wild::Rank(Rank::Five));
        assert_eq!(
            g.payoffs(&s),
            vec![Chips(-200), Chips(200)],
            "trip kings (wild fives) beat aces"
        );
    }

    #[test]
    fn hi_lo_declarations_split_the_pot() {
        let g = game();
        let setup = HandSetup::simple(2, Chips(5_000));
        // Seat 1 declares high, seat 0 low: $1 each from the $2 pot that seat 0 paid in.
        let s = replay(&g, &setup, &wild_hand(Declare::High, Declare::Low)).unwrap();
        assert_eq!(g.payoffs(&s), vec![Chips(-100), Chips(100)]);
    }

    #[test]
    fn declarations_stay_hidden_until_everyone_has_declared() {
        let g = game();
        let setup = HandSetup::simple(2, Chips(5_000));
        let mut a = wild_hand(Declare::High, Declare::High);
        a.pop(); // seat 0 hasn't declared yet
        let mut b = wild_hand(Declare::Low, Declare::High);
        b.pop();
        let (sa, sb) = (
            replay(&g, &setup, &a).unwrap(),
            replay(&g, &setup, &b).unwrap(),
        );
        assert_eq!(g.to_act(&sa), ToAct::Seat(Seat(0)));
        assert_eq!(
            g.observe(&sa, Seat(0)),
            g.observe(&sb, Seat(0)),
            "seat 1's choice must not leak"
        );
        let done = replay(&g, &setup, &wild_hand(Declare::Low, Declare::High)).unwrap();
        assert_eq!(
            g.observe(&done, Seat(0)).declarations.unwrap()[1],
            Some(Declare::Low)
        );
    }

    #[test]
    fn best_board_acts_first() {
        use Action::*;
        let g = game();
        let setup = HandSetup::simple(2, Chips(5_000));
        let d = |s: &str| Deal(c(s));
        // 3rd street upcards: seat 1 shows 9s, seat 0 shows Kh → seat 0 (higher) acts first.
        let log = vec![d("2c"), d("Ah"), d("3d"), d("Ad"), d("9s"), d("Kh")];
        let s = replay(&g, &setup, &log).unwrap();
        assert_eq!(g.to_act(&s), ToAct::Seat(Seat(0)));
        // 4th street: seat 1 pairs its nines → seat 1 acts first.
        let mut log2 = log.clone();
        log2.extend([CheckCall, CheckCall, d("9h"), d("2h")]);
        let s = replay(&g, &setup, &log2).unwrap();
        assert_eq!(g.to_act(&s), ToAct::Seat(Seat(1)));
    }

    #[test]
    fn spread_limit_cap_per_street() {
        use Action::*;
        let g = game();
        let setup = HandSetup::simple(2, Chips(5_000));
        let d = |s: &str| Deal(c(s));
        let mut log = vec![d("2c"), d("Ah"), d("3d"), d("Ad"), d("9s"), d("Kh")];
        log.extend([
            RaiseTo(Chips(200)),
            RaiseTo(Chips(400)),
            RaiseTo(Chips(600)),
            RaiseTo(Chips(800)),
        ]);
        let s = replay(&g, &setup, &log).unwrap();
        let mut legal = Vec::new();
        g.legal_actions(&s, &mut legal);
        assert_eq!(
            legal,
            vec![Fold, CheckCall],
            "4 bets/raises = $8 cap reached"
        );
    }
}
