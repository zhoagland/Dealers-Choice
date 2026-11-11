//! Blackjack (Poker Bible §Blackjack; catalog G9). The chooser is the bank: it sets the max bet,
//! takes the other side of every bet, and plays a fixed rule. Payoffs still sum to zero because the
//! bank seat pays and collects.
//!
//! Rules and decisions:
//! - **D-BJ-1 (owner)** the bank hits 16 or less and stands on all 17s.
//! - **D-BJ-2 (owner)** each player bets 50¢ up to the bank's max before the deal; no insurance;
//!   double down on any first two cards; split a pair once; doubling after splitting aces is a
//!   `Config` flag (off), per the Bible's variation.
//! - **D-BJ-3** the bank picks its max from $1, $2, $5, $10.
//! - **D-BJ-4** players' cards are dealt face up (as the Bible says); the bank gets one card up and
//!   one face down, turned at its turn.
//! - **D-BJ-5** no peek: a bank blackjack is revealed at the bank's turn and beats every hand
//!   except a player blackjack (push). Blackjack (ace + ten-value card on a hand's first two cards,
//!   not after a split) pays 3:2.
//! - **D-BJ-6** a hand reaching 21 stands automatically; if the deck runs out, nobody can take more
//!   cards (hit, double and split are no longer offered; the bank stands).
//! - **D-BJ-7** the bank never pays more than its stack: if short, winning hands are paid in seat
//!   order from the bank's left until the bank's stack is gone.

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use engine::pot::order_from_button;
use engine::replay::replay_interleaved;
use engine::view::{CardSlot, PotView, SeatStatus, SeatView, TableView};
use engine::{
    ActionClass, Card, CardSet, Chips, DetRng, Game, GameLength, GameMeta, HandSetup,
    HiddenSampler, IllegalAction, Rank, Seat, StakeModel, ToAct,
};
use rand::Rng;

/// Bank max-bet choices, in 50¢ units ($1, $2, $5, $10).
pub const MAX_CHOICES: [u8; 4] = [2, 4, 10, 20];

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// The Bible's variation: may a player double after splitting aces?
    pub double_after_split_aces: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Action {
    Deal(Card),
    /// Bank: index into [`MAX_CHOICES`].
    SetMax(u8),
    /// Player: wager in 50¢ units.
    Wager(u8),
    Hit,
    Stand,
    Double,
    Split,
}

/// Where the next card goes.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Slot {
    Hand(u8),
    BankUp,
    BankHole,
    BankHit,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlayerHand {
    pub seat: Seat,
    pub cards: SmallVec<[Card; 8]>,
    /// Wager in 50¢ units (doubled hands count twice).
    pub units: u8,
    pub from_split: bool,
    pub split_aces: bool,
    pub done: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
enum Phase {
    SetMax,
    Wagers,
    Play,
    Bank,
    Done,
}

#[derive(Clone, Debug)]
pub struct State {
    setup: HandSetup,
    bank: Seat,
    max_units: u8,
    players: Vec<Seat>,
    wagers: Vec<u8>,
    phase: Phase,
    pub hands: Vec<PlayerHand>,
    cur: usize,
    pub bank_cards: SmallVec<[Card; 8]>,
    hole_shown: bool,
    stock: CardSet,
    queue: SmallVec<[Slot; 16]>,
    deal_log: Vec<Slot>,
    history: Vec<(Option<Seat>, Action)>,
    split_used: u16,
}

/// Best blackjack total (aces 1 or 11) and whether it's soft.
pub fn total(cards: &[Card]) -> (u32, bool) {
    let mut sum = 0;
    let mut aces = 0;
    for c in cards {
        sum += match c.rank() {
            Rank::Ace => {
                aces += 1;
                1
            }
            Rank::Ten | Rank::Jack | Rank::Queen | Rank::King => 10,
            r => r as u32 + 2,
        };
    }
    if aces > 0 && sum + 10 <= 21 {
        (sum + 10, true)
    } else {
        (sum, false)
    }
}

fn is_natural(cards: &[Card], from_split: bool) -> bool {
    !from_split && cards.len() == 2 && total(cards).0 == 21
}

impl State {
    fn wager_seat(&self) -> Option<Seat> {
        self.players
            .iter()
            .copied()
            .find(|p| self.wagers[p.index()] == 0)
    }
}

pub struct Blackjack {
    config: Config,
    meta: GameMeta,
}

impl Blackjack {
    /// Move on until someone must act, a card must be dealt, or the hand is over.
    fn advance(&self, s: &mut State) {
        loop {
            if !s.queue.is_empty() {
                if s.stock.is_empty() {
                    // D-BJ-6: no cards left.
                    let slot = s.queue.remove(0);
                    if let Slot::Hand(i) = slot {
                        s.hands[i as usize].done = true;
                    }
                    continue;
                }
                return;
            }
            match s.phase {
                Phase::SetMax | Phase::Done => return,
                Phase::Wagers => {
                    if s.wager_seat().is_some() {
                        return;
                    }
                    // Deal: a card to each player from the bank's left, the bank's up card, a
                    // second card to each, the bank's hole card.
                    for &p in &s.players {
                        s.hands.push(PlayerHand {
                            seat: p,
                            cards: SmallVec::new(),
                            units: s.wagers[p.index()],
                            from_split: false,
                            split_aces: false,
                            done: false,
                        });
                    }
                    for i in 0..s.players.len() {
                        s.queue.push(Slot::Hand(i as u8));
                    }
                    s.queue.push(Slot::BankUp);
                    for i in 0..s.players.len() {
                        s.queue.push(Slot::Hand(i as u8));
                    }
                    s.queue.push(Slot::BankHole);
                    s.phase = Phase::Play;
                    s.cur = 0;
                }
                Phase::Play => {
                    while s.cur < s.hands.len() {
                        let h = &mut s.hands[s.cur];
                        if !h.done && h.cards.len() >= 2 && total(&h.cards).0 >= 21 {
                            h.done = true; // bust, or 21 stands (D-BJ-6)
                        }
                        if h.split_aces
                            && h.cards.len() >= 2
                            && !self.config.double_after_split_aces
                        {
                            h.done = true;
                        }
                        if h.done {
                            s.cur += 1;
                        } else if h.cards.len() < 2 {
                            // A split hand waiting for its second card.
                            let i = s.cur as u8;
                            s.queue.push(Slot::Hand(i));
                            break;
                        } else {
                            return;
                        }
                    }
                    if !s.queue.is_empty() {
                        continue;
                    }
                    s.phase = Phase::Bank;
                    s.hole_shown = true;
                }
                Phase::Bank => {
                    if total(&s.bank_cards).0 < 17 && !s.stock.is_empty() {
                        s.queue.push(Slot::BankHit);
                        return;
                    }
                    s.phase = Phase::Done;
                    return;
                }
            }
        }
    }

    fn can_double(&self, s: &State, i: usize) -> bool {
        let h = &s.hands[i];
        h.cards.len() == 2
            && !s.stock.is_empty()
            && (!h.split_aces || self.config.double_after_split_aces)
            && self.stack_left(s, h.seat) >= Chips(50 * h.units as i64)
    }

    fn can_split(&self, s: &State, i: usize) -> bool {
        let h = &s.hands[i];
        h.cards.len() == 2
            && h.cards[0].rank() == h.cards[1].rank()
            && s.split_used & (1 << h.seat.0) == 0
            && s.stock.len() >= 2
            && self.stack_left(s, h.seat) >= Chips(50 * h.units as i64)
    }

    /// What `p` still has behind its wagers.
    fn stack_left(&self, s: &State, p: Seat) -> Chips {
        let staked: i64 = s
            .hands
            .iter()
            .filter(|h| h.seat == p)
            .map(|h| 50 * h.units as i64)
            .sum();
        s.setup.stacks[p.index()] - Chips(staked)
    }

    /// Each player hand's result in chips, before the bank's stack limit.
    fn results(&self, s: &State) -> Vec<(Seat, i64)> {
        let (bank_total, _) = total(&s.bank_cards);
        let bank_natural = is_natural(&s.bank_cards, false);
        s.hands
            .iter()
            .map(|h| {
                let w = 50 * h.units as i64;
                let (t, _) = total(&h.cards);
                let natural = is_natural(&h.cards, h.from_split);
                let r = if natural {
                    if bank_natural { 0 } else { w * 3 / 2 }
                } else if t > 21 || bank_natural {
                    -w // bust, or the bank's blackjack beats any non-natural (D-BJ-5)
                } else if bank_total > 21 || t > bank_total {
                    w
                } else if t == bank_total {
                    0
                } else {
                    -w
                };
                (h.seat, r)
            })
            .collect()
    }
}

impl Game for Blackjack {
    type State = State;
    type Action = Action;
    type Observation = Observation;
    type Config = Config;
    type Carry = ();

    fn new(config: Config) -> Blackjack {
        Blackjack {
            config,
            meta: GameMeta {
                id: "blackjack",
                version: 1,
                display_name: "Blackjack (chooser is the bank)",
                seats: 2..=6,
                stake_model: StakeModel::Bankroll,
                length: GameLength::SingleHand,
                rules_ref: "Poker Bible §Blackjack; games/src/blackjack.rs",
            },
        }
    }

    fn meta(&self) -> &GameMeta {
        &self.meta
    }

    fn new_hand_with(&self, setup: &HandSetup, _carry: &()) -> State {
        let bank = setup.chooser;
        let players: Vec<Seat> = order_from_button(bank, setup.num_seats)
            .into_iter()
            .filter(|&p| p != bank && setup.dealt_in(p))
            .collect();
        State {
            setup: setup.clone(),
            bank,
            max_units: 0,
            wagers: vec![0; setup.num_seats as usize],
            players,
            phase: Phase::SetMax,
            hands: Vec::new(),
            cur: 0,
            bank_cards: SmallVec::new(),
            hole_shown: false,
            stock: CardSet::FULL_DECK,
            queue: SmallVec::new(),
            deal_log: Vec::new(),
            history: Vec::new(),
            split_used: 0,
        }
    }

    fn to_act(&self, s: &State) -> ToAct {
        if !s.queue.is_empty() {
            return ToAct::Chance;
        }
        match s.phase {
            Phase::SetMax => ToAct::Seat(s.bank),
            Phase::Wagers => ToAct::Seat(s.wager_seat().expect("wager phase")),
            Phase::Play => ToAct::Seat(s.hands[s.cur].seat),
            Phase::Bank | Phase::Done => ToAct::Terminal,
        }
    }

    fn legal_actions(&self, s: &State, out: &mut Vec<Action>) {
        out.clear();
        match self.to_act(s) {
            ToAct::Chance => out.extend(s.stock.iter().map(Action::Deal)),
            ToAct::Terminal => {}
            ToAct::Seat(p) => match s.phase {
                Phase::SetMax => out.extend((0..MAX_CHOICES.len() as u8).map(Action::SetMax)),
                Phase::Wagers => {
                    let cap = (s.setup.stacks[p.index()].0 / 50).min(s.max_units as i64) as u8;
                    out.extend((1..=cap.max(1)).map(Action::Wager));
                }
                Phase::Play => {
                    out.extend([Action::Stand]);
                    if !s.stock.is_empty() {
                        out.push(Action::Hit);
                    }
                    if self.can_double(s, s.cur) {
                        out.push(Action::Double);
                    }
                    if self.can_split(s, s.cur) {
                        out.push(Action::Split);
                    }
                }
                _ => {}
            },
        }
    }

    fn validate(&self, s: &State, a: Action) -> Result<(), IllegalAction> {
        let mut legal = Vec::new();
        self.legal_actions(s, &mut legal);
        if legal.contains(&a) {
            Ok(())
        } else {
            Err(IllegalAction(match s.phase {
                Phase::SetMax => "bank: choose the max bet ($1, $2, $5 or $10)".into(),
                Phase::Wagers => format!("bet 50¢ to {}", Chips(50 * s.max_units as i64)),
                Phase::Play => {
                    "hit, stand, double (first two cards) or split (a pair, once)".into()
                }
                _ => "not now".into(),
            }))
        }
    }

    fn apply(&self, s: &mut State, a: Action) {
        let actor = match a {
            Action::Deal(c) => {
                s.stock.remove(c);
                let slot = s.queue.remove(0);
                match slot {
                    Slot::Hand(i) => s.hands[i as usize].cards.push(c),
                    Slot::BankUp | Slot::BankHole | Slot::BankHit => s.bank_cards.push(c),
                }
                s.deal_log.push(slot);
                None
            }
            Action::SetMax(i) => {
                s.max_units = MAX_CHOICES[i as usize];
                s.phase = Phase::Wagers;
                Some(s.bank)
            }
            Action::Wager(u) => {
                let p = s.wager_seat().unwrap();
                s.wagers[p.index()] = u;
                Some(p)
            }
            Action::Hit => {
                let i = s.cur;
                s.queue.push(Slot::Hand(i as u8));
                Some(s.hands[i].seat)
            }
            Action::Stand => {
                let i = s.cur;
                s.hands[i].done = true;
                Some(s.hands[i].seat)
            }
            Action::Double => {
                let i = s.cur;
                s.hands[i].units *= 2;
                s.hands[i].done = true;
                s.queue.push(Slot::Hand(i as u8));
                Some(s.hands[i].seat)
            }
            Action::Split => {
                let i = s.cur;
                let seat = s.hands[i].seat;
                let second = s.hands[i].cards.pop().unwrap();
                let aces = second.rank() == Rank::Ace;
                s.hands[i].from_split = true;
                s.hands[i].split_aces = aces;
                let new = PlayerHand {
                    seat,
                    cards: std::iter::once(second).collect(),
                    units: s.hands[i].units,
                    from_split: true,
                    split_aces: aces,
                    done: false,
                };
                s.hands.insert(i + 1, new);
                s.split_used |= 1 << seat.0;
                // Re-number later hands' pending slots (none are pending during play).
                s.queue.push(Slot::Hand(i as u8));
                Some(seat)
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
        let n = s.setup.num_seats as usize;
        let mut out = vec![Chips::ZERO; n];
        let results = self.results(s);
        // Losing hands first (they fund the bank), then winners in seat order until the bank's
        // stack is gone (D-BJ-7).
        let mut bank_left = s.setup.stacks[s.bank.index()].0;
        for &(p, r) in results.iter().filter(|&&(_, r)| r < 0) {
            out[p.index()] += Chips(r);
            bank_left -= r;
        }
        for &(p, r) in results.iter().filter(|&&(_, r)| r > 0) {
            let paid = r.min(bank_left.max(0));
            out[p.index()] += Chips(paid);
            bank_left -= paid;
        }
        let players: Chips = out.iter().copied().sum();
        out[s.bank.index()] = -players;
        out
    }

    fn observe(&self, s: &State, seat: Seat) -> Observation {
        let mut slots = s.deal_log.iter();
        let history = s
            .history
            .iter()
            .map(|&(who, a)| match (who, a) {
                (None, Action::Deal(c)) => {
                    let slot = *slots.next().unwrap();
                    if slot == Slot::BankHole {
                        if s.hole_shown || seat == s.bank {
                            Event::Card(slot, Some(c))
                        } else {
                            Event::Card(slot, None)
                        }
                    } else {
                        Event::Card(slot, Some(c))
                    }
                }
                (Some(p), a) => Event::Act(p, a),
                (None, _) => unreachable!(),
            })
            .collect();
        Observation {
            seat,
            setup: s.setup.clone(),
            history,
        }
    }

    fn observer(&self, obs: &Observation) -> Seat {
        obs.seat
    }

    fn determinize(
        &self,
        obs: &Observation,
        _sampler: &mut dyn HiddenSampler,
        rng: &mut DetRng,
    ) -> State {
        let mut known = CardSet::EMPTY;
        for e in &obs.history {
            if let Event::Card(_, Some(c)) = *e {
                known.insert(c);
            }
        }
        let left: Vec<Card> = CardSet::FULL_DECK.difference(known).iter().collect();
        let mut deals = Vec::new();
        let mut actions = Vec::new();
        for e in &obs.history {
            match *e {
                Event::Card(_, Some(c)) => deals.push(Action::Deal(c)),
                Event::Card(_, None) => {
                    deals.push(Action::Deal(left[rng.random_range(0..left.len())]))
                }
                Event::Act(_, a) => actions.push(a),
            }
        }
        replay_interleaved(self, &obs.setup, deals, actions)
    }

    fn classify(&self, s: &State, a: Action) -> ActionClass {
        match a {
            Action::Deal(_) => ActionClass::Chance,
            Action::Stand => ActionClass::CheckOrCall { owed: Chips::ZERO },
            Action::Hit => ActionClass::CardChoice,
            Action::SetMax(_) => ActionClass::Declare,
            Action::Wager(u) => ActionClass::BetOrRaise {
                to: Chips(50 * u as i64),
            },
            Action::Double | Action::Split => ActionClass::BetOrRaise {
                to: Chips(100 * s.hands.get(s.cur).map_or(0, |h| h.units as i64)),
            },
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
        let seats = obs
            .setup
            .seats()
            .map(|p| {
                let mut v = SeatView::new(p, obs.setup.stacks[p.index()], Chips::ZERO);
                v.status = if obs.setup.dealt_in(p) {
                    SeatStatus::Active
                } else {
                    SeatStatus::Out
                };
                if p == s.bank {
                    v.badges.push("bank".into());
                    v.cards = s
                        .bank_cards
                        .iter()
                        .enumerate()
                        .map(|(i, &c)| {
                            if i == 1 && !s.hole_shown {
                                CardSlot::Down((p == obs.seat).then_some(c))
                            } else {
                                CardSlot::Up(c)
                            }
                        })
                        .collect();
                    if s.max_units > 0 {
                        v.badges
                            .push(format!("max {}", Chips(50 * s.max_units as i64)));
                    }
                } else {
                    for h in s.hands.iter().filter(|h| h.seat == p) {
                        v.cards.extend(h.cards.iter().map(|&c| CardSlot::Up(c)));
                        v.badges.push(format!(
                            "{} on {}{}",
                            Chips(50 * h.units as i64),
                            total(&h.cards).0,
                            if h.done { "" } else { " (playing)" }
                        ));
                    }
                }
                v
            })
            .collect();
        let staked: i64 = s.hands.iter().map(|h| 50 * h.units as i64).sum();
        TableView {
            game: self.meta.display_name.into(),
            phase: format!("{:?}", s.phase),
            viewer: obs.seat,
            button: s.bank,
            to_act: match self.to_act(&s) {
                ToAct::Seat(p) => Some(p),
                _ => None,
            },
            seats,
            boards: Vec::new(),
            pots: vec![PotView {
                name: "Wagers".into(),
                amount: Chips(staked),
            }],
            wilds: Vec::new(),
            notes: vec!["Bank hits 16 or less, stands on all 17s; blackjack pays 3:2".into()],
        }
    }

    fn describe_action(&self, a: Action) -> String {
        match a {
            Action::Deal(c) => format!("deal {c}"),
            Action::SetMax(i) => format!(
                "set the max bet to {}",
                Chips(50 * MAX_CHOICES[i as usize] as i64)
            ),
            Action::Wager(u) => format!("bet {}", Chips(50 * u as i64)),
            Action::Hit => "hit".into(),
            Action::Stand => "stand".into(),
            Action::Double => "double down".into(),
            Action::Split => "split".into(),
        }
    }

    fn describe_observation(&self, obs: &Observation) -> String {
        let v = self.view(obs);
        let mut out = format!("{} | {}", v.game, v.phase);
        for sv in &v.seats {
            let cards: Vec<String> = sv
                .cards
                .iter()
                .map(|c| match c {
                    CardSlot::Up(c) => c.to_string(),
                    CardSlot::Down(Some(c)) => format!("[{c}]"),
                    _ => "##".into(),
                })
                .collect();
            out += &format!(
                "\n  {}: {} {}",
                sv.seat,
                cards.join(" "),
                sv.badges.join(", ")
            );
        }
        out
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Event {
    /// A card to this slot; `None` = the bank's hole card, not yet seen by this seat.
    Card(Slot, Option<Card>),
    Act(Seat, Action),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Observation {
    pub seat: Seat,
    pub setup: HandSetup,
    pub history: SmallVec<[Event; 48]>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::testing::{Conformance, check_conformance};

    fn card(s: &str) -> Card {
        s.parse().unwrap()
    }

    #[test]
    fn conformance() {
        check_conformance(
            &Blackjack::new(Config::default()),
            &Conformance {
                hands: 150,
                seat_counts: vec![2, 4, 6],
                ..Conformance::default()
            },
        );
    }

    #[test]
    fn totals() {
        let t = |s: &str| total(&s.split_whitespace().map(card).collect::<Vec<_>>());
        assert_eq!(t("Ac Kd"), (21, true));
        assert_eq!(t("Ac Ad 9h"), (21, true));
        assert_eq!(t("Kc Qd 5h"), (25, false));
        assert_eq!(t("Ac 6d"), (17, true));
    }

    #[test]
    fn natural_pays_3_to_2_and_bank_stands_on_soft_17() {
        let g = Blackjack::new(Config::default());
        let setup = HandSetup::simple(2, Chips(10_000)); // chooser s0 = bank, s1 plays
        let mut s = g.new_hand(&setup);
        g.apply(&mut s, Action::SetMax(1)); // $2
        g.apply(&mut s, Action::Wager(4)); // $2
        // s1: Ac, bank up 6d, s1: Kh, bank hole Ah (soft 17: stands).
        for c in ["Ac", "6d", "Kh", "Ah"] {
            g.apply(&mut s, Action::Deal(card(c)));
        }
        assert_eq!(
            g.to_act(&s),
            ToAct::Terminal,
            "21 stands; bank on soft 17 stands"
        );
        assert_eq!(g.payoffs(&s), vec![Chips(-300), Chips(300)]);
    }

    #[test]
    fn split_and_double() {
        let g = Blackjack::new(Config::default());
        let setup = HandSetup::simple(2, Chips(10_000));
        let mut s = g.new_hand(&setup);
        g.apply(&mut s, Action::SetMax(2)); // $5
        g.apply(&mut s, Action::Wager(2)); // $1
        for c in ["8c", "Td", "8h", "7s"] {
            g.apply(&mut s, Action::Deal(card(c)));
        }
        let mut legal = Vec::new();
        g.legal_actions(&s, &mut legal);
        assert!(legal.contains(&Action::Split));
        g.apply(&mut s, Action::Split);
        g.apply(&mut s, Action::Deal(card("3c"))); // first hand: 8 3
        g.apply(&mut s, Action::Double);
        g.apply(&mut s, Action::Deal(card("Tc"))); // 21 for $2
        g.apply(&mut s, Action::Deal(card("9d"))); // second hand: 8 9
        g.legal_actions(&s, &mut legal);
        assert!(!legal.contains(&Action::Split), "split once");
        g.apply(&mut s, Action::Stand);
        // Bank: T 7 = 17 stands. Hand 1 wins $2, hand 2 (17) pushes.
        assert_eq!(g.to_act(&s), ToAct::Terminal);
        assert_eq!(g.payoffs(&s), vec![Chips(-200), Chips(200)]);
    }
}
