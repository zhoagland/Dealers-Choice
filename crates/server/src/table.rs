//! One table = one thread that owns the only full game state (web.md §2). Connections talk to it
//! over channels; it sends each connection only that viewer's `TableView` (FR-WB-1).
//!
//! Flow (FR-WB-3…6): players sit; the host starts; the chooser picks a game from the table's pool
//! (timer; on timeout a random pick); the game is played with the engine's Dealer's Choice table
//! (single hand, orbit, or until done, carrying state between hands); the choice passes left.
//! Money goes through the ledger exactly like the CLI (bankroll games move the bankroll per hand;
//! table-stake games at buy-in and cash-out).

use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::sync::mpsc::UnboundedSender;

use engine::rng::{Stream, derive_rng};
use engine::session::{SeatInfo as EngineSeat, SeatKind, SessionEvent, Table};
use engine::{Chips, Game, GameCarry, Seat, StakeModel, ToAct};
use games::registry::{GameVisitor, with_game};
use protocol::{
    CardSlot, ChatLine, ClientMsg, HandSummary, Prompt, PromptOption, SeatInfo, SeatResult,
    ServerMsg, TableInfo, TableListing, TableView,
};
use sim::encoding::{HandRecord, encode, encode_carry};
use storage::{Account, Db, LedgerKind};

use crate::catalog;
use crate::prompt;

/// Messages into a table thread.
pub enum Cmd {
    Join {
        conn: u64,
        account: Account,
        tx: UnboundedSender<ServerMsg>,
    },
    Leave {
        conn: u64,
    },
    Msg {
        conn: u64,
        msg: ClientMsg,
    },
    /// Close the table (host or admin): now if no game is running, else after it.
    Close,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Settings {
    pub max_seats: u8,
    pub pool: Vec<String>,
    pub timer_secs: u32,
    pub bank_secs: u32,
    pub listed: bool,
}

pub struct TableHandle {
    pub tx: Sender<Cmd>,
    pub listing: Arc<Mutex<TableListing>>,
    pub host: i64,
}

struct Conn {
    account: Account,
    tx: UnboundedSender<ServerMsg>,
}

#[derive(Clone)]
struct Seated {
    account: Account,
    away: bool,
    timeouts: u8,
    /// Stood up during a game: leaves the seat when it ends.
    leaving: bool,
}

/// The current decision, for snapshots and answers.
struct Pending {
    seat: u8,
    prompt: Prompt,
}

pub struct Actor {
    id: String,
    name: String,
    host: i64,
    settings: Settings,
    db: Arc<Mutex<Db>>,
    session_id: i64,
    rx: Receiver<Cmd>,
    conns: HashMap<u64, Conn>,
    seats: Vec<Option<Seated>>,
    state: &'static str,
    seq: u64,
    log: VecDeque<String>,
    chat_times: HashMap<i64, VecDeque<Instant>>,
    game: Option<(String, String)>,
    chooser: usize,
    hand_no: u64,
    listing: Arc<Mutex<TableListing>>,
    /// Views of the hand in progress, per table seat.
    views: HashMap<u8, TableView>,
    /// Table seat per hand seat, for the hand in progress.
    hand_seats: Vec<u8>,
    /// Decisions being waited for: one, or several when seats decide at the same time.
    pending: Vec<Pending>,
    /// Time bank left this game, per table seat (ms).
    bank: HashMap<u8, u64>,
    next_prompt: u64,
    start_requested: bool,
    closing: bool,
    /// The game just played says the next deal goes left of this table seat (Scare-a-moosh).
    deal_after: Option<u8>,
    /// Between games: when the next game starts (Unix ms), so late players can sit down.
    next_game_ms: Option<u64>,
}

/// Seconds between games, for players to join (playtest P1-20).
const BREAK_SECS: u64 = 15;
/// How long the end of a hand stays on screen: after a showdown, and otherwise (playtest P1-5).
const SHOWDOWN_MS: u64 = 6_000;
const HAND_END_MS: u64 = 2_000;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// A seed from the operating system's CSPRNG (web.md §2): outcomes can't be predicted.
fn os_seed() -> u64 {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).expect("OS random numbers");
    u64::from_le_bytes(b)
}

/// Start a table thread.
#[allow(clippy::too_many_arguments)]
pub fn spawn(
    id: String,
    name: String,
    host: i64,
    settings: Settings,
    db: Arc<Mutex<Db>>,
    session_id: i64,
) -> TableHandle {
    let (tx, rx) = std::sync::mpsc::channel();
    let listing = Arc::new(Mutex::new(TableListing {
        id: id.clone(),
        name: name.clone(),
        players: 0,
        max_seats: settings.max_seats,
        state: "waiting".into(),
        listed: settings.listed,
    }));
    let mut actor = Actor {
        id,
        name,
        host,
        seats: vec![None; settings.max_seats as usize],
        settings,
        db,
        session_id,
        rx,
        conns: HashMap::new(),
        state: "waiting",
        seq: 0,
        log: VecDeque::new(),
        chat_times: HashMap::new(),
        game: None,
        chooser: 0,
        hand_no: 0,
        listing: listing.clone(),
        views: HashMap::new(),
        hand_seats: Vec::new(),
        pending: Vec::new(),
        bank: HashMap::new(),
        next_prompt: 1,
        start_requested: false,
        closing: false,
        deal_after: None,
        next_game_ms: None,
    };
    std::thread::spawn(move || actor.run());
    TableHandle { tx, listing, host }
}

impl Actor {
    fn run(&mut self) {
        loop {
            // Waiting for the host to start (or for players).
            while !self.start_requested || self.ready_seats().len() < 2 {
                self.start_requested &= self.ready_seats().len() >= 2;
                if self.closing {
                    self.state = "closed";
                    self.broadcast();
                    return;
                }
                match self.rx.recv() {
                    Ok(cmd) => {
                        if let Some((conn, _)) = self.handle(cmd) {
                            self.send_error(conn, "prompt", "there's nothing to answer right now");
                        }
                    }
                    Err(_) => return,
                }
            }
            self.play_game();
        }
    }

    fn ready_seats(&self) -> Vec<u8> {
        (0..self.seats.len() as u8)
            .filter(|&i| self.seats[i as usize].as_ref().is_some_and(|s| !s.away))
            .collect()
    }

    fn note(&mut self, line: String) {
        self.log.push_back(line);
        while self.log.len() > 30 {
            self.log.pop_front();
        }
    }

    fn bankroll(&self, account: &Account) -> i64 {
        self.db
            .lock()
            .unwrap()
            .bankroll(account.player_id)
            .map_or(0, |c| c.0)
    }

    fn info(&self) -> TableInfo {
        let connected: Vec<i64> = self.conns.values().map(|c| c.account.id).collect();
        TableInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            host: self.host,
            state: self.state.into(),
            max_seats: self.settings.max_seats,
            timer_secs: self.settings.timer_secs,
            bank_secs: self.settings.bank_secs,
            seats: self
                .seats
                .iter()
                .enumerate()
                .filter_map(|(i, s)| {
                    let s = s.as_ref()?;
                    let bankroll = self.bankroll(&s.account);
                    let stack = self
                        .views
                        .get(&(i as u8))
                        .and_then(|v| v.seats.iter().find(|x| x.seat == v.viewer))
                        .map_or(bankroll, |x| x.stack.0);
                    Some(SeatInfo {
                        seat: i as u8,
                        account: Some(s.account.id),
                        name: s.account.name.clone(),
                        avatar: s.account.avatar.clone(),
                        stack,
                        bankroll,
                        away: s.away,
                        connected: connected.contains(&s.account.id),
                    })
                })
                .collect(),
            game: self.game.as_ref().map(|g| g.0.clone()),
            game_name: self.game.as_ref().map(|g| g.1.clone()),
            chooser: (self.state != "waiting").then_some(self.chooser as u8),
            pool: self.settings.pool.clone(),
            hand_no: self.hand_no,
            log: self.log.iter().cloned().collect(),
            hand_seats: self.hand_seats.clone(),
            next_game_ms: self.next_game_ms,
        }
    }

    /// The hand as someone not in it sees it (a late joiner, a player sitting out): everything
    /// public, no face-down card faces (playtest P1-19).
    fn spectator_view(&self) -> Option<TableView> {
        let mut v = self.views.values().next()?.clone();
        for s in v.seats.iter_mut() {
            for c in s.cards.iter_mut() {
                if let CardSlot::Down(Some(_)) = c {
                    *c = CardSlot::Down(None);
                }
            }
            s.badges.retain(|b| !b.starts_with("you "));
            s.groups.clear();
        }
        Some(v)
    }

    fn seat_of(&self, account: i64) -> Option<u8> {
        self.seats
            .iter()
            .position(|s| s.as_ref().is_some_and(|s| s.account.id == account))
            .map(|i| i as u8)
    }

    /// Send every connection its own snapshot.
    fn broadcast(&mut self) {
        self.seq += 1;
        let info = self.info();
        {
            let mut l = self.listing.lock().unwrap();
            l.players = info.seats.len() as u8;
            l.state = self.state.into();
        }
        let spectator = self.spectator_view();
        for c in self.conns.values() {
            let you = self.seat_of(c.account.id);
            let view = you
                .and_then(|s| self.views.get(&s).cloned())
                .or_else(|| spectator.clone());
            let prompt = you.and_then(|s| {
                self.pending
                    .iter()
                    .find(|p| p.seat == s)
                    .map(|p| p.prompt.clone())
            });
            let _ = c.tx.send(ServerMsg::Snapshot {
                seq: self.seq,
                you,
                view,
                prompt,
                table: info.clone(),
            });
        }
    }

    fn send_error(&self, conn: u64, code: &str, message: &str) {
        if let Some(c) = self.conns.get(&conn) {
            let _ = c.tx.send(ServerMsg::Error {
                code: code.into(),
                message: message.into(),
            });
        }
    }

    /// Everything but answers to the current prompt. Returns an `Act` for the caller to handle.
    fn handle(&mut self, cmd: Cmd) -> Option<(u64, ClientMsg)> {
        match cmd {
            Cmd::Join { conn, account, tx } => {
                let history = self
                    .db
                    .lock()
                    .unwrap()
                    .chat(&self.id, 50)
                    .unwrap_or_default();
                for row in history {
                    let _ = tx.send(ServerMsg::Chat {
                        line: ChatLine {
                            id: row.id,
                            name: row.name,
                            text: row.text,
                            at: row.at,
                        },
                    });
                }
                self.conns.insert(conn, Conn { account, tx });
                self.broadcast();
            }
            Cmd::Leave { conn } => {
                self.conns.remove(&conn);
                self.broadcast();
            }
            Cmd::Close => {
                self.closing = true;
                self.start_requested = false;
                self.note("the host closed the table".into());
                self.broadcast();
            }
            Cmd::Msg { conn, msg } => {
                let account = self.conns.get(&conn).map(|c| c.account.clone())?;
                match msg {
                    ClientMsg::Ping => {
                        if let Some(c) = self.conns.get(&conn) {
                            let _ = c.tx.send(ServerMsg::Pong);
                        }
                    }
                    ClientMsg::Sit { seat } => {
                        if self.seat_of(account.id).is_some() {
                            self.send_error(conn, "seated", "you already have a seat");
                        } else if (seat as usize) >= self.seats.len()
                            || self.seats[seat as usize].is_some()
                        {
                            self.send_error(conn, "taken", "that seat isn't free");
                        } else if self.bankroll(&account) <= 0 {
                            self.send_error(conn, "broke", "your bankroll is empty; ask an admin");
                        } else {
                            self.note(format!("{} sat down", account.name));
                            self.seats[seat as usize] = Some(Seated {
                                account,
                                away: false,
                                timeouts: 0,
                                leaving: false,
                            });
                            self.broadcast();
                        }
                    }
                    ClientMsg::Stand => {
                        if let Some(s) = self.seat_of(account.id) {
                            if self.state == "playing" {
                                // Leave after this game.
                                if let Some(x) = self.seats[s as usize].as_mut() {
                                    x.away = true;
                                    x.leaving = true;
                                }
                                self.send_error(
                                    conn,
                                    "later",
                                    "you'll leave the seat after this game",
                                );
                            } else {
                                self.seats[s as usize] = None;
                                self.note(format!("{} stood up", account.name));
                            }
                            self.broadcast();
                        }
                    }
                    ClientMsg::Away { away } => {
                        if let Some(s) = self.seat_of(account.id)
                            && let Some(x) = self.seats[s as usize].as_mut()
                        {
                            x.away = away;
                            x.timeouts = 0;
                            self.broadcast();
                        }
                    }
                    ClientMsg::Start => {
                        if account.id != self.host && !account.is_admin {
                            self.send_error(conn, "host", "only the host can start");
                        } else if self.ready_seats().len() < 2 {
                            self.send_error(conn, "players", "at least two players must be seated");
                        } else if self.next_game_ms.is_some() {
                            // Skip the rest of the break between games.
                            self.next_game_ms = None;
                            self.note(format!("{} started the next game", account.name));
                            self.broadcast();
                        } else {
                            self.start_requested = true;
                            self.note(format!("{} started the table", account.name));
                        }
                    }
                    ClientMsg::Chat { text } => self.chat(conn, &account, text),
                    ClientMsg::DeleteChat { id } => {
                        if account.is_admin {
                            let _ = self.db.lock().unwrap().delete_chat(id);
                            for c in self.conns.values() {
                                let _ = c.tx.send(ServerMsg::ChatDeleted { id });
                            }
                        }
                    }
                    act @ ClientMsg::Act { .. } => return Some((conn, act)),
                }
            }
        }
        None
    }

    /// FR-WB-7: 500 characters, 5 messages per 10 s, stored, escaped by the client.
    fn chat(&mut self, conn: u64, account: &Account, text: String) {
        let text: String = text.trim().chars().take(500).collect();
        if text.is_empty() {
            return;
        }
        let times = self.chat_times.entry(account.id).or_default();
        let now = Instant::now();
        while times
            .front()
            .is_some_and(|t| now.duration_since(*t) > Duration::from_secs(10))
        {
            times.pop_front();
        }
        if times.len() >= 5 {
            self.send_error(conn, "slow", "slow down: 5 messages per 10 seconds");
            return;
        }
        times.push_back(now);
        let id = self
            .db
            .lock()
            .unwrap()
            .add_chat(&self.id, account.id, &text)
            .unwrap_or(0);
        let line = ChatLine {
            id,
            name: account.name.clone(),
            text,
            at: (now_ms() / 1000) as i64,
        };
        for c in self.conns.values() {
            let _ = c.tx.send(ServerMsg::Chat { line: line.clone() });
        }
    }

    /// Ask `seat` with `options`; returns the answer (or `None` on timeout). Other commands are
    /// handled while waiting.
    fn ask(
        &mut self,
        seat: u8,
        title: &str,
        options: Vec<PromptOption>,
    ) -> Option<protocol::Choice> {
        self.ask_many(vec![(seat, title.to_string(), options)])
            .pop()
            .flatten()
    }

    /// Ask several seats at once (each with its own timer and time bank); returns each answer
    /// in the same order (`None` on timeout). Other commands are handled while waiting.
    fn ask_many(
        &mut self,
        asks: Vec<(u8, String, Vec<PromptOption>)>,
    ) -> Vec<Option<protocol::Choice>> {
        let turn = self.settings.timer_secs as u64 * 1000;
        let started = now_ms();
        let mut answers: Vec<Option<protocol::Choice>> = vec![None; asks.len()];
        let mut open: Vec<(usize, u64)> = Vec::new(); // (index in asks, prompt id)
        self.pending.clear();
        for (k, (seat, title, options)) in asks.iter().enumerate() {
            let bank = *self
                .bank
                .entry(*seat)
                .or_insert(self.settings.bank_secs as u64 * 1000);
            let id = self.next_prompt;
            self.next_prompt += 1;
            open.push((k, id));
            self.pending.push(Pending {
                seat: *seat,
                prompt: Prompt {
                    id,
                    deadline_ms: started + turn + bank,
                    bank_ms: bank,
                    title: title.clone(),
                    options: options.clone(),
                },
            });
        }
        self.broadcast();
        let finish = |a: &mut Actor, k: usize, answered: bool| {
            let seat = asks[k].0;
            let used = now_ms().saturating_sub(started);
            if used > turn {
                let b = a.bank.get_mut(&seat).unwrap();
                *b = b.saturating_sub(used - turn);
            }
            a.pending.retain(|p| p.seat != seat);
            if let Some(s) = a.seats[seat as usize].as_mut() {
                if answered {
                    s.timeouts = 0;
                } else {
                    s.timeouts += 1;
                    if s.timeouts >= 3 {
                        // Three timeouts in a row: sit out after this game (web.md §6).
                        s.away = true;
                    }
                }
            }
        };
        while !open.is_empty() {
            let now = now_ms();
            // Time out every seat whose deadline has passed.
            let (late, still): (Vec<_>, Vec<_>) = open.iter().partition(|&&(_, id)| {
                self.pending
                    .iter()
                    .find(|p| p.prompt.id == id)
                    .is_none_or(|p| p.prompt.deadline_ms <= now)
            });
            for &(k, _) in &late {
                finish(self, k, false);
            }
            if !late.is_empty() {
                self.broadcast();
            }
            open = still;
            let Some(next) = open
                .iter()
                .filter_map(|&(_, id)| self.pending.iter().find(|p| p.prompt.id == id))
                .map(|p| p.prompt.deadline_ms)
                .min()
            else {
                break;
            };
            match self
                .rx
                .recv_timeout(Duration::from_millis(next.saturating_sub(now_ms()).max(1)))
            {
                Ok(cmd) => {
                    if let Some((conn, ClientMsg::Act { prompt_id, choice })) = self.handle(cmd) {
                        let by = self
                            .conns
                            .get(&conn)
                            .map(|c| c.account.id)
                            .and_then(|a| self.seat_of(a));
                        let hit = open
                            .iter()
                            .position(|&(k, id)| id == prompt_id && Some(asks[k].0) == by);
                        let Some(i) = hit else {
                            self.send_error(conn, "prompt", "not your turn (or an old prompt)");
                            continue;
                        };
                        let (k, _) = open.remove(i);
                        answers[k] = Some(choice);
                        finish(self, k, true);
                        self.broadcast();
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    for &(k, _) in &open {
                        finish(self, k, false);
                    }
                    break;
                }
            }
        }
        self.pending.clear();
        answers
    }

    /// Keep the table responsive (chat, sitting down) for `ms` without asking anyone anything.
    /// Returns early if the host presses Start and `skippable`.
    fn idle(&mut self, ms: u64, skippable: bool) {
        let until = now_ms() + ms;
        loop {
            let left = until.saturating_sub(now_ms());
            if left == 0 || self.closing {
                return;
            }
            match self.rx.recv_timeout(Duration::from_millis(left)) {
                Ok(cmd) => {
                    if let Some((conn, _)) = self.handle(cmd) {
                        self.send_error(conn, "prompt", "there's nothing to answer right now");
                    }
                    // The host pressed Start during the break.
                    if skippable && self.next_game_ms.is_none() {
                        return;
                    }
                }
                Err(RecvTimeoutError::Timeout) => return,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    /// One dealer's-choice game.
    fn play_game(&mut self) {
        let ready = self.ready_seats();
        if ready.len() < 2 {
            return;
        }
        if !ready.contains(&(self.chooser as u8)) {
            self.chooser = ready
                .iter()
                .map(|&s| s as usize)
                .find(|&s| s >= self.chooser)
                .unwrap_or(ready[0] as usize);
        }
        self.state = "choosing";
        self.bank.clear();
        let n = ready.len() as u8;
        let pool: Vec<String> = catalog::playable(&self.settings.pool)
            .into_iter()
            .filter(|g| g.seats_min <= n && n <= g.seats_max)
            .map(|g| g.id)
            .collect();
        if pool.is_empty() {
            self.note(format!("no game in the pool fits {n} players"));
            self.start_requested = false;
            self.state = "waiting";
            self.broadcast();
            return;
        }
        let options: Vec<PromptOption> = catalog::playable(&pool)
            .into_iter()
            .map(|g| PromptOption::Game {
                id: g.id.clone(),
                name: g.name.clone(),
                family: g.family.clone(),
                seats: format!("{}-{}", g.seats_min, g.seats_max),
            })
            .collect();
        let chooser = self.chooser as u8;
        let picked = match self.ask(chooser, "Choose the game", options) {
            Some(protocol::Choice::Option { id }) if pool.contains(&id) => id,
            _ => {
                // Timeout: a random game from the pool (web.md §6).
                let i = (os_seed() % pool.len() as u64) as usize;
                pool[i].clone()
            }
        };
        let name = catalog::playable(std::slice::from_ref(&picked))
            .first()
            .map_or(picked.clone(), |g| g.name.clone());
        let chooser_name = self.seats[self.chooser]
            .as_ref()
            .map_or("?".into(), |s| s.account.name.clone());
        self.note(format!("{chooser_name} chose {name}"));
        self.game = Some((picked.clone(), name));
        self.state = "playing";
        let _ = with_game(
            &picked,
            None,
            PlayGame {
                actor: self,
                seats: ready.clone(),
            },
        );
        self.views.clear();
        self.hand_seats.clear();
        self.game = None;
        self.state = "choosing";
        // The choice passes left (FR-SS-4), or left of the seat the game names (Scare-a-moosh's
        // winning declarer).
        let from = self.deal_after.take().map_or(self.chooser, |t| t as usize);
        let next = (1..=self.seats.len())
            .map(|k| (from + k) % self.seats.len())
            .find(|&i| self.seats[i].as_ref().is_some_and(|s| !s.away));
        if let Some(i) = next {
            self.chooser = i;
        }
        // Players who asked to stand leave now.
        for s in self.seats.iter_mut() {
            if s.as_ref().is_some_and(|x| x.leaving) {
                *s = None;
            }
        }
        if self.ready_seats().len() < 2 || self.closing {
            self.state = "waiting";
            self.start_requested = false;
            self.broadcast();
            return;
        }
        // A short break so players can sit down or step away (playtest P1-20). The host can
        // press Start to skip it.
        self.state = "break";
        self.next_game_ms = Some(now_ms() + BREAK_SECS * 1000);
        self.broadcast();
        self.idle(BREAK_SECS * 1000, true);
        self.next_game_ms = None;
        self.state = "choosing";
        self.broadcast();
    }
}

/// The seats to ask now, each with the state it decides in. Normally just the seat to act; in a
/// simultaneous step, every seat of the step: later seats' states are found by playing the
/// earlier seats' default answers (and any deals) on a copy, which is safe because the game
/// promises their choices don't change what the others may choose.
fn plan_group<G: Game>(game: &G, s: &G::State) -> Vec<(Seat, G::State)> {
    let ToAct::Seat(first) = game.to_act(s) else {
        return Vec::new();
    };
    let mut out = vec![(first, s.clone())];
    let Some(key) = game.simultaneous(s) else {
        return out;
    };
    let mut spec = s.clone();
    let mut rng = engine::rng::rng_from_seed(0);
    let mut legal = Vec::new();
    while let ToAct::Seat(p) = game.to_act(&spec) {
        game.legal_actions(&spec, &mut legal);
        if legal.is_empty() {
            break;
        }
        let d = prompt::default_action(game, &spec, &legal);
        game.apply(&mut spec, d);
        while game.to_act(&spec) == ToAct::Chance {
            let x = game.sample_chance(&spec, &mut rng);
            game.apply(&mut spec, x);
        }
        match game.to_act(&spec) {
            ToAct::Seat(q)
                if game.simultaneous(&spec) == Some(key)
                    && q != p
                    && out.iter().all(|(o, _)| *o != q) =>
            {
                out.push((q, spec.clone()));
            }
            _ => break,
        }
    }
    out
}

struct PlayGame<'a> {
    actor: &'a mut Actor,
    /// Table seats playing this game, in order.
    seats: Vec<u8>,
}

impl GameVisitor<()> for PlayGame<'_> {
    fn visit<G: Game>(self, game: G) {
        let a = self.actor;
        let seats = self.seats;
        // The engine's table: one engine seat per playing table seat, with the real bankroll.
        let engine_seats: Vec<EngineSeat> = seats
            .iter()
            .map(|&s| {
                let acc = a.seats[s as usize].as_ref().unwrap().account.clone();
                EngineSeat {
                    name: acc.name.clone(),
                    kind: SeatKind::Human,
                    bankroll: Some(Chips(a.bankroll(&acc))),
                    stack: Chips::ZERO,
                    sitting_out: false,
                }
            })
            .collect();
        let accounts: Vec<Account> = seats
            .iter()
            .map(|&s| a.seats[s as usize].as_ref().unwrap().account.clone())
            .collect();
        let mut table = Table::new(engine_seats, Chips(10_000));
        table.chooser = seats
            .iter()
            .position(|&s| s as usize == a.chooser)
            .unwrap_or(0);
        let stake = game.meta().stake_model;
        table.begin_game(game.meta());
        record_ledger(a, &mut table, stake, &accounts);
        let mut carry = GameCarry::new(&game);
        save_refund(a, &table, stake, &accounts, &game, &carry);
        let mut legal = Vec::new();
        while let Some((setup, map)) = table.next_hand_excluding(carry.seats_out(&game)) {
            a.hand_no += 1;
            let seed = os_seed();
            let mut rng = derive_rng(seed, a.hand_no, Stream::Deal);
            let carry_in = carry.carry().clone();
            let mut state = carry.new_hand(&game, &setup);
            let mut log = Vec::new();
            // Hand seat → table seat.
            let table_seat = |k: usize| seats[map[k]];
            let refresh = |a: &mut Actor, state: &G::State| {
                a.views.clear();
                a.hand_seats = (0..map.len()).map(table_seat).collect();
                for k in 0..map.len() {
                    if setup.dealt_in(Seat(k as u8)) {
                        let v = game.view(&game.observe(state, Seat(k as u8)));
                        a.views.insert(table_seat(k), v);
                    }
                }
            };
            loop {
                match game.to_act(&state) {
                    ToAct::Terminal => break,
                    ToAct::Chance => {
                        let x = game.sample_chance(&state, &mut rng);
                        game.apply(&mut state, x);
                        log.push(x);
                    }
                    ToAct::Seat(_) => {
                        refresh(a, &state);
                        // Everyone in a simultaneous step (declare, discard, pass…) is asked at
                        // once (playtest P1-17/18); otherwise just the seat to act.
                        #[allow(clippy::type_complexity)]
                        let group: Vec<(
                            Seat,
                            Vec<G::Action>,
                            prompt::Built<G::Action>,
                        )> = plan_group(&game, &state)
                            .into_iter()
                            .map(|(q, st)| {
                                let mut l = Vec::new();
                                game.legal_actions(&st, &mut l);
                                let b = prompt::build(&game, &st, &l);
                                (q, l, b)
                            })
                            .collect();
                        let title = if group.len() > 1 {
                            "Everyone decides at once"
                        } else {
                            "Your turn"
                        };
                        let asks = group
                            .iter()
                            .map(|(q, _, b)| {
                                (table_seat(q.index()), title.to_string(), b.options.clone())
                            })
                            .collect();
                        let answers = a.ask_many(asks);
                        for ((q, spec_legal, built), choice) in group.into_iter().zip(answers) {
                            while game.to_act(&state) == ToAct::Chance {
                                let x = game.sample_chance(&state, &mut rng);
                                game.apply(&mut state, x);
                                log.push(x);
                            }
                            if game.to_act(&state) != ToAct::Seat(q) {
                                break; // not as planned: ask again from here
                            }
                            game.legal_actions(&state, &mut legal);
                            let action = match choice.map(|c| {
                                built.answer(&c, |cents| {
                                    game.parse_action(
                                        &format!(
                                            "r {}",
                                            Chips(cents).to_string().trim_start_matches('$')
                                        ),
                                        &spec_legal,
                                    )
                                })
                            }) {
                                Some(Ok(x)) if game.validate(&state, x).is_ok() => x,
                                Some(Ok(_)) | Some(Err(_)) | None => {
                                    prompt::default_action(&game, &state, &legal)
                                }
                            };
                            let who = table_seat(q.index());
                            let name = a.seats[who as usize]
                                .as_ref()
                                .map_or("?".into(), |s| s.account.name.clone());
                            let line =
                                format!("{name}: {}", prompt::public_label(&game, &state, action));
                            a.note(line);
                            game.apply(&mut state, action);
                            log.push(action);
                        }
                    }
                }
            }
            refresh(a, &state);
            let payoffs = game.payoffs(&state);
            table.settle_hand(&map, &payoffs);
            table.mark_folded(&map, engine::showdown::folded_seats(&game, &state));
            if carry.finish_hand(&game, &state, &payoffs).game_over {
                table.finish_game_early();
                a.deal_after = game.next_deal_after(&state).map(|k| table_seat(k.index()));
            }
            let folded = engine::showdown::folded_seats(&game, &state);
            let showdown = (0..map.len())
                .filter(|&k| setup.dealt_in(Seat(k as u8)) && folded & (1 << k) == 0)
                .count()
                > 1;
            // Store the hand once for the table, replayable per viewer.
            let mut rec = HandRecord::new(
                game.meta().id,
                game.meta().version,
                a.hand_no,
                0,
                map.iter().map(|&t| t as u8).collect(),
                setup.clone(),
                encode_carry(&carry_in),
                log,
            );
            rec.shown_by_choice = Vec::new();
            let seats_json =
                serde_json::to_string(&map.iter().map(|&t| accounts[t].id).collect::<Vec<_>>())
                    .unwrap();
            let results_json =
                serde_json::to_string(&payoffs.iter().map(|c| c.0).collect::<Vec<_>>()).unwrap();
            let hand_id =
                a.db.lock()
                    .unwrap()
                    .store_web_hand(
                        &a.id,
                        game.meta().id,
                        game.meta().version,
                        &seats_json,
                        &results_json,
                        &encode(&rec),
                    )
                    .unwrap_or(0);
            record_ledger(a, &mut table, stake, &accounts);
            save_refund(a, &table, stake, &accounts, &game, &carry);
            // Busted players sit out the rest of this game (no rebuys at web tables in v1).
            for t in table.busted() {
                table.sit_out(t);
            }
            let results: Vec<SeatResult> = map
                .iter()
                .zip(&payoffs)
                .map(|(&t, c)| SeatResult {
                    name: accounts[t].name.clone(),
                    cents: c.0,
                })
                .collect();
            // Everyone watching gets the result; players also get the hand as they saw it.
            for c in a.conns.values() {
                let k = map.iter().position(|&t| accounts[t].id == c.account.id);
                let text = k.map_or(String::new(), |k| {
                    game.describe_observation(&game.observe(&state, Seat(k as u8)))
                });
                let _ = c.tx.send(ServerMsg::HandSummary {
                    summary: HandSummary {
                        hand_id,
                        game: game.meta().display_name.into(),
                        results: results.clone(),
                        text,
                        showdown,
                    },
                });
            }
            a.broadcast();
            // Leave the end of the hand on screen for a moment (the showdown, playtest P1-5).
            a.idle(if showdown { SHOWDOWN_MS } else { HAND_END_MS }, false);
        }
        // An unfinished multi-hand game's carried pot is split (D-G0-1).
        if let Some(game_seats) = table.game_seats().map(<[usize]>::to_vec) {
            let n = game_seats.len() as u8;
            let eligible = table.game_eligible(carry.seats_out(&game));
            if let Some(pay) = carry.close(&game, n, eligible, (1u16 << n) - 1) {
                table.settle_carry(&game_seats, &pay);
            }
        }
        table.end_game();
        record_ledger(a, &mut table, stake, &accounts);
        let _ = a.db.lock().unwrap().set_web_table_refund(&a.id, "[]");
    }
}

/// D-WEB-6: what each player gets back if the server stops before the next hand ends: table-stake
/// stacks (as they were at the start of the interrupted hand) and their share of a carried pot.
fn save_refund<G: Game>(
    a: &Actor,
    table: &Table,
    stake: StakeModel,
    accounts: &[Account],
    game: &G,
    carry: &GameCarry<G>,
) {
    let staked = matches!(stake, StakeModel::TableStake(_));
    let kind = if staked { "cash_out" } else { "hand" };
    let mut owed = vec![0i64; accounts.len()];
    if staked {
        for (i, s) in table.seats.iter().enumerate() {
            owed[i] += s.stack.0;
        }
    }
    if let Some(gs) = table.game_seats() {
        let n = gs.len() as u8;
        let eligible = table.game_eligible(carry.seats_out(game));
        if let Some(pay) = carry.close(game, n, eligible, (1u16 << n) - 1) {
            for (k, &t) in gs.iter().enumerate() {
                owed[t] += pay[k].0;
            }
        }
    }
    let rows: Vec<(i64, &str, i64)> = owed
        .iter()
        .enumerate()
        .filter(|&(_, &c)| c != 0)
        .map(|(i, &c)| (accounts[i].player_id, kind, c))
        .collect();
    let json = serde_json::to_string(&rows).unwrap();
    let _ = a.db.lock().unwrap().set_web_table_refund(&a.id, &json);
}

/// Pay back what a table owed when the server stopped (D-WEB-6), before it reopens.
pub fn apply_refund(db: &mut Db, table_id: &str, refund_json: &str, session_id: i64) {
    let rows: Vec<(i64, String, i64)> = serde_json::from_str(refund_json).unwrap_or_default();
    for (player, kind, cents) in rows {
        let kind = if kind == "cash_out" {
            LedgerKind::CashOut
        } else {
            LedgerKind::Hand
        };
        let _ = db.ledger(player, Some(session_id), &[(kind, Chips(cents))]);
    }
    let _ = db.set_web_table_refund(table_id, "[]");
}

/// Mirror the engine table's money events into the ledger (same rules as the CLI).
fn record_ledger(a: &mut Actor, table: &mut Table, stake: StakeModel, accounts: &[Account]) {
    let mut db = a.db.lock().unwrap();
    for e in table.drain_events() {
        let (seat, row) = match (e, stake) {
            (SessionEvent::BuyIn { seat, amount }, StakeModel::TableStake(_)) => {
                (seat, (LedgerKind::BuyIn, -amount))
            }
            (SessionEvent::Rebuy { seat, amount }, StakeModel::TableStake(_)) => {
                (seat, (LedgerKind::Rebuy, -amount))
            }
            (SessionEvent::CashOut { seat, amount }, StakeModel::TableStake(_)) => {
                (seat, (LedgerKind::CashOut, amount))
            }
            (SessionEvent::HandResult { seat, delta }, StakeModel::Bankroll) => {
                (seat, (LedgerKind::Hand, delta))
            }
            _ => continue,
        };
        let _ = db.ledger(accounts[seat].player_id, Some(a.session_id), &[row]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::HandSetup;
    use games::registry::{GameVisitor, with_game};

    /// Plays a hand with default answers; returns the size of every group `plan_group` asks.
    struct Groups;
    impl GameVisitor<Vec<(String, usize)>> for Groups {
        fn visit<G: Game>(self, game: G) -> Vec<(String, usize)> {
            let mut s = game.new_hand(&HandSetup::simple(3, Chips(10_000)));
            let mut rng = engine::rng::rng_from_seed(7);
            let mut legal = Vec::new();
            let mut out = Vec::new();
            loop {
                match game.to_act(&s) {
                    ToAct::Terminal => return out,
                    ToAct::Chance => {
                        let x = game.sample_chance(&s, &mut rng);
                        game.apply(&mut s, x);
                    }
                    ToAct::Seat(_) => {
                        let group = plan_group(&game, &s);
                        let phase = game.view(&game.observe(&s, Seat(0))).phase;
                        out.push((phase, group.len()));
                        // Each planned seat's options are offered; apply defaults in order.
                        for (q, st) in group {
                            while game.to_act(&s) == ToAct::Chance {
                                let x = game.sample_chance(&s, &mut rng);
                                game.apply(&mut s, x);
                            }
                            assert_eq!(game.to_act(&s), ToAct::Seat(q));
                            game.legal_actions(&st, &mut legal);
                            let spec = legal.clone();
                            game.legal_actions(&s, &mut legal);
                            assert_eq!(spec, legal, "options must not depend on others' answers");
                            let a = prompt::default_action(&game, &s, &legal);
                            game.apply(&mut s, a);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn simultaneous_steps_ask_everyone_at_once() {
        for (id, phase) in [
            ("mamas_favorite", "Make your pairs"),
            ("pass_the_trash", "Pass 3 cards"),
            ("draw5", "Draw up to 5"),
        ] {
            let groups = with_game(id, None, Groups).unwrap();
            let g = groups.iter().find(|(p, _)| p == phase);
            assert_eq!(g.map(|x| x.1), Some(3), "{id}: {groups:?}");
            // Betting is never grouped.
            assert!(
                groups
                    .iter()
                    .filter(|(p, _)| p == "Betting")
                    .all(|x| x.1 == 1)
            );
        }
    }
}
