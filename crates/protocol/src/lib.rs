//! Messages between the web server and the browser (`docs/tech/web.md` §3), as JSON.
//!
//! The browser never receives a game `State`: only [`TableView`]s built from the viewer's own
//! observation (FR-WB-1). Prompts are game-agnostic lists of options built from the game's legal
//! actions (D-WEB-1), so the client has no per-game code (FR-WB-11).
//!
//! The TypeScript side (`web/src/protocol.ts`) is written by hand; `tests::export_samples`
//! writes one sample of every message to `web/src/generated/samples.ts`, which the web build
//! typechecks against those types, so the two can't drift silently.

use serde::{Deserialize, Serialize};

pub use engine::view::{
    BoardLayout, BoardView, CardPick, CardSlot, PickMode, PotView, SeatStatus, SeatView, TableView,
    WildView,
};

/// Browser → server, over the table WebSocket.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMsg {
    /// Sit in an empty seat.
    Sit {
        seat: u8,
    },
    /// Leave the seat (between games).
    Stand,
    /// Sit out from the next game (or come back).
    Away {
        away: bool,
    },
    /// Host: start dealing.
    Start,
    /// Answer the current prompt.
    Act {
        prompt_id: u64,
        choice: Choice,
    },
    Chat {
        text: String,
    },
    /// Admin: hide a chat line.
    DeleteChat {
        id: i64,
    },
    Ping,
}

/// An answer to a [`Prompt`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Choice {
    /// A button.
    Option { id: String },
    /// An amount (bet/raise to), in cents.
    Amount { id: String, cents: i64 },
}

/// Server → browser.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[allow(clippy::large_enum_variant)] // one snapshot at a time; boxing gains nothing
pub enum ServerMsg {
    /// Everything the viewer may see now. Sent after every change; safe to apply alone.
    Snapshot {
        seq: u64,
        /// The viewer's seat at the table, if seated.
        you: Option<u8>,
        /// The current hand from the viewer's seat (`None` between hands or when not playing).
        view: Option<TableView>,
        prompt: Option<Prompt>,
        table: TableInfo,
    },
    HandSummary {
        summary: HandSummary,
    },
    Chat {
        line: ChatLine,
    },
    ChatDeleted {
        id: i64,
    },
    Error {
        code: String,
        message: String,
    },
    Pong,
}

/// "Your turn": the options and the deadline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Prompt {
    pub id: u64,
    /// Unix milliseconds when the turn timer (including the time bank) runs out.
    pub deadline_ms: u64,
    /// Milliseconds left in the time bank when this prompt was sent.
    pub bank_ms: u64,
    pub title: String,
    pub options: Vec<PromptOption>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum PromptOption {
    /// One action: fold, check, call $2, declare High, throw away cards 1,3 …
    Button {
        id: String,
        label: String,
        /// "fold", "check", "call", "bet", "declare", "cards", "other".
        class: String,
        /// The viewer's cards this option uses, so the player can pick it by clicking cards.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cards: Option<CardPick>,
    },
    /// Bet or raise to any amount in `min..=max` (cents), in `step`s.
    Amount {
        id: String,
        label: String,
        min: i64,
        max: i64,
        step: i64,
        presets: Vec<Preset>,
    },
    /// The chooser's game picker.
    Game {
        id: String,
        name: String,
        family: String,
        seats: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub label: String,
    pub cents: i64,
}

/// The table between hands: who sits where, the game, the chooser.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TableInfo {
    pub id: String,
    pub name: String,
    pub host: i64,
    /// "waiting", "choosing", "playing".
    pub state: String,
    pub max_seats: u8,
    pub timer_secs: u32,
    pub bank_secs: u32,
    pub seats: Vec<SeatInfo>,
    pub game: Option<String>,
    pub game_name: Option<String>,
    pub chooser: Option<u8>,
    pub pool: Vec<String>,
    pub hand_no: u64,
    /// Recent table events, newest last.
    pub log: Vec<String>,
    /// The table seat of each hand seat in the current hand (`TableView` seats are hand seats).
    pub hand_seats: Vec<u8>,
    /// During the break between games: when the next game starts (Unix ms).
    #[serde(default)]
    pub next_game_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SeatInfo {
    pub seat: u8,
    pub account: Option<i64>,
    pub name: String,
    pub avatar: Option<String>,
    /// Chips on the table (cents).
    pub stack: i64,
    pub bankroll: i64,
    pub away: bool,
    pub connected: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatLine {
    pub id: i64,
    pub name: String,
    pub text: String,
    pub at: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HandSummary {
    pub hand_id: i64,
    pub game: String,
    pub results: Vec<SeatResult>,
    /// The hand as the viewer saw it at the end (text, as the CLI prints); empty for watchers.
    pub text: String,
    /// More than one hand was still in at the end, so cards were shown.
    #[serde(default)]
    pub showdown: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SeatResult {
    pub name: String,
    pub cents: i64,
}

// ----- REST bodies ----------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Me {
    pub id: i64,
    pub name: String,
    pub avatar: Option<String>,
    pub is_admin: bool,
    pub bankroll: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CreateTable {
    pub name: String,
    pub max_seats: u8,
    /// Game ids the chooser may pick from (empty = every game).
    pub pool: Vec<String>,
    pub timer_secs: u32,
    /// Shown in the lobby to every allowed user (otherwise invite link only).
    pub listed: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TableListing {
    pub id: String,
    pub name: String,
    pub players: u8,
    pub max_seats: u8,
    pub state: String,
    pub listed: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GameInfo {
    pub id: String,
    pub name: String,
    pub family: String,
    pub seats_min: u8,
    pub seats_max: u8,
    pub rules: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HandListing {
    pub id: i64,
    pub game: String,
    pub at: i64,
    pub results: Vec<SeatResult>,
}

/// A past hand step by step, from the viewer's own seat only (FR-WB-8).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HandReplay {
    pub id: i64,
    pub game: String,
    pub steps: Vec<ReplayStep>,
    pub text: String,
    /// Player name per hand seat.
    pub names: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReplayStep {
    pub label: String,
    pub view: TableView,
}

/// Admin page: an account and its bankroll.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AdminAccount {
    pub id: i64,
    pub name: String,
    pub is_admin: bool,
    pub bankroll: i64,
}

/// Admin: deposit `cents` into an account, or reset its bankroll to `cents` (D-WEB-3).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BankrollChange {
    pub account: i64,
    /// "deposit" or "reset".
    pub op: String,
    pub cents: i64,
}

/// `GET /api/config`: what the client needs before sign-in.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ServerConfig {
    pub discord_client_id: Option<String>,
    pub dev_mode: bool,
}

/// Discord Activity: the browser sends the SDK's authorization code; the server answers with the
/// Discord access token (for the SDK's `authenticate`) and a session token for this app.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActivityToken {
    pub access_token: String,
    pub session: String,
    pub table: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::{Card, Chips, Rank, Seat};

    fn view() -> TableView {
        let mut s = SeatView::new(Seat(0), Chips(9_800), Chips(200));
        s.cards = vec![
            CardSlot::Down(Some("Ah".parse().unwrap())),
            CardSlot::Down(None),
            CardSlot::Up("Kd".parse::<Card>().unwrap()),
            CardSlot::Empty,
        ];
        s.badges = vec!["chooser".into()];
        s.played = vec!["Qs".parse().unwrap()];
        s.groups = vec![2, 2];
        s.tricks = Some(1);
        TableView {
            game: "Follow the Queen".into(),
            phase: "Declare".into(),
            viewer: Seat(0),
            button: Seat(1),
            to_act: Some(Seat(0)),
            seats: vec![s],
            boards: vec![BoardView {
                name: "Board".into(),
                layout: BoardLayout::H,
                slots: vec![CardSlot::Up("2c".parse().unwrap()), CardSlot::Empty],
                lines: vec![vec![0, 1]],
            }],
            pots: vec![PotView {
                name: "Pot".into(),
                amount: Chips(400),
            }],
            wilds: vec![WildView {
                rank: Rank::Queen,
                seat: None,
            }],
            notes: vec!["note".into()],
        }
    }

    fn table() -> TableInfo {
        TableInfo {
            id: "t1".into(),
            name: "Friday".into(),
            host: 1,
            state: "playing".into(),
            max_seats: 6,
            timer_secs: 30,
            bank_secs: 60,
            seats: vec![SeatInfo {
                seat: 0,
                account: Some(1),
                name: "Ann".into(),
                avatar: None,
                stack: 9_800,
                bankroll: 10_000,
                away: false,
                connected: true,
            }],
            game: Some("ftq".into()),
            game_name: Some("Follow the Queen".into()),
            chooser: Some(0),
            pool: vec!["ftq".into()],
            hand_no: 3,
            log: vec!["Ann chose Follow the Queen".into()],
            hand_seats: vec![0],
            next_game_ms: Some(1),
        }
    }

    fn samples() -> (Vec<ClientMsg>, Vec<ServerMsg>) {
        let prompt = Prompt {
            id: 7,
            deadline_ms: 1,
            bank_ms: 60_000,
            title: "Your turn".into(),
            options: vec![
                PromptOption::Button {
                    id: "0".into(),
                    label: "Fold".into(),
                    class: "fold".into(),
                    cards: None,
                },
                PromptOption::Button {
                    id: "1".into(),
                    label: "Throw away cards 1,2".into(),
                    class: "cards".into(),
                    cards: Some(CardPick {
                        mode: PickMode::Select,
                        positions: vec![0, 1],
                    }),
                },
                PromptOption::Amount {
                    id: "raise".into(),
                    label: "Raise to".into(),
                    min: 50,
                    max: 200,
                    step: 50,
                    presets: vec![Preset {
                        label: "Max".into(),
                        cents: 200,
                    }],
                },
                PromptOption::Game {
                    id: "ftq".into(),
                    name: "Follow the Queen".into(),
                    family: "Stud".into(),
                    seats: "2-7".into(),
                },
            ],
        };
        let client = vec![
            ClientMsg::Sit { seat: 1 },
            ClientMsg::Stand,
            ClientMsg::Away { away: true },
            ClientMsg::Start,
            ClientMsg::Act {
                prompt_id: 7,
                choice: Choice::Option { id: "0".into() },
            },
            ClientMsg::Act {
                prompt_id: 7,
                choice: Choice::Amount {
                    id: "raise".into(),
                    cents: 150,
                },
            },
            ClientMsg::Chat { text: "hi".into() },
            ClientMsg::DeleteChat { id: 3 },
            ClientMsg::Ping,
        ];
        let server = vec![
            ServerMsg::Snapshot {
                seq: 1,
                you: Some(0),
                view: Some(view()),
                prompt: Some(prompt),
                table: table(),
            },
            ServerMsg::HandSummary {
                summary: HandSummary {
                    hand_id: 9,
                    game: "ftq".into(),
                    results: vec![SeatResult {
                        name: "Ann".into(),
                        cents: 200,
                    }],
                    text: "…".into(),
                    showdown: true,
                },
            },
            ServerMsg::Chat {
                line: ChatLine {
                    id: 1,
                    name: "Ann".into(),
                    text: "gg".into(),
                    at: 0,
                },
            },
            ServerMsg::ChatDeleted { id: 1 },
            ServerMsg::Error {
                code: "bad".into(),
                message: "no".into(),
            },
            ServerMsg::Pong,
        ];
        (client, server)
    }

    #[test]
    fn messages_round_trip_as_json() {
        let (client, server) = samples();
        for m in client {
            let j = serde_json::to_string(&m).unwrap();
            assert_eq!(serde_json::from_str::<ClientMsg>(&j).unwrap(), m);
        }
        for m in server {
            let j = serde_json::to_string(&m).unwrap();
            assert_eq!(serde_json::from_str::<ServerMsg>(&j).unwrap(), m);
        }
    }

    /// Writes `web/src/generated/samples.ts`; the web build typechecks it against
    /// `web/src/protocol.ts`. CI fails if this changes the file without it being committed.
    #[test]
    fn export_samples() {
        let (client, server) = samples();
        let rest = serde_json::json!({
            "me": Me { id: 1, name: "Ann".into(), avatar: None, is_admin: false, bankroll: 10_000 },
            "create": CreateTable { name: "Friday".into(), max_seats: 6, pool: vec![], timer_secs: 30, listed: true },
            "listing": TableListing { id: "t1".into(), name: "Friday".into(), players: 2, max_seats: 6, state: "waiting".into(), listed: true },
            "game": GameInfo { id: "ftq".into(), name: "FTQ".into(), family: "Stud".into(), seats_min: 2, seats_max: 7, rules: "Poker Bible".into() },
            "hand": HandListing { id: 1, game: "ftq".into(), at: 0, results: vec![] },
            "replay": HandReplay { id: 1, game: "ftq".into(), steps: vec![ReplayStep { label: "start".into(), view: view() }], text: String::new(), names: vec!["Ann".into()] },
            "activity": ActivityToken { access_token: "a".into(), session: "s".into(), table: "t".into() },
        });
        let ts = format!(
            "// Generated by `cargo test -p protocol` (crates/protocol/src/lib.rs). Do not edit.\n\
             import type * as P from \"../protocol\";\n\n\
             export const client = {} as const satisfies readonly P.ClientMsg[];\n\n\
             export const server = {} as const satisfies readonly P.ServerMsg[];\n\n\
             const rest = {} as const;\n\
             export const me: P.Me = rest.me;\n\
             export const create: P.CreateTable = {{ ...rest.create, pool: [] }};\n\
             export const listing: P.TableListing = rest.listing;\n\
             export const game: P.GameInfo = rest.game;\n\
             export const hand: P.HandListing = {{ ...rest.hand, results: [] }};\n\
             export const replay: P.HandReplay = rest.replay;\n\
             export const activity: P.ActivityToken = rest.activity;\n",
            serde_json::to_string_pretty(&client).unwrap(),
            serde_json::to_string_pretty(&server).unwrap(),
            serde_json::to_string_pretty(&rest).unwrap(),
        );
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../web/src/generated/samples.ts");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let old = std::fs::read_to_string(&path).unwrap_or_default();
        if old.replace("\r\n", "\n") != ts {
            std::fs::write(&path, &ts).unwrap();
        }
    }
}
