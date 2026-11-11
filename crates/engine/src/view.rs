//! A game-agnostic picture of a table from one seat's point of view (API v2, PRD FR-WB-11,
//! `docs/tech/web.md` §4).
//!
//! Every driver turns its own `Observation` into a [`TableView`] in [`crate::Game::view`]. A UI
//! then draws any game with one renderer: seats with card slots, boards in a few named shapes,
//! pots, wild cards, and short notes. Because a view is built from an observation, it can only
//! contain what that seat may know.

use serde::{Deserialize, Serialize};

use crate::card::{Card, Rank};
use crate::game::{Chips, Seat};

/// Everything a UI needs to draw the table for one viewer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableView {
    /// The game's display name.
    pub game: String,
    /// What's happening, e.g. `"Flop: betting"`, `"Draw 2 of 3"`, `"Declare"`.
    pub phase: String,
    /// The seat this view belongs to.
    pub viewer: Seat,
    pub button: Seat,
    /// Who acts now, if a seat (not chance, not over).
    pub to_act: Option<Seat>,
    pub seats: Vec<SeatView>,
    pub boards: Vec<BoardView>,
    pub pots: Vec<PotView>,
    pub wilds: Vec<WildView>,
    /// Short public facts that don't fit elsewhere: `"Seat 2 has 1 leg"`, `"Strikes: 0 1 2"`.
    pub notes: Vec<String>,
}

/// One seat as the viewer sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeatView {
    pub seat: Seat,
    /// Chips behind (not yet bet).
    pub stack: Chips,
    /// Chips put in during the current betting round.
    pub bet: Chips,
    pub status: SeatStatus,
    pub cards: Vec<CardSlot>,
    /// Small labels: `"declared"`, `"chooser"`, `"2 legs"`, `"wild: 7"`.
    pub badges: Vec<String>,
}

impl SeatView {
    /// An active seat with no cards or badges yet.
    pub fn new(seat: Seat, stack: Chips, bet: Chips) -> SeatView {
        SeatView {
            seat,
            stack,
            bet,
            status: SeatStatus::Active,
            cards: Vec::new(),
            badges: Vec::new(),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SeatStatus {
    Active,
    Folded,
    AllIn,
    /// Not in this hand (dealt out, or out of the game).
    Out,
}

/// One card position.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CardSlot {
    /// Face up: everyone sees it.
    Up(Card),
    /// Face down. `Some` when the viewer may see it (their own hole card); `None` for a hidden
    /// card (someone else's hole card, an unflipped board card, or the viewer's own card in an
    /// Indian game).
    Down(Option<Card>),
    /// A position that will hold a card later.
    Empty,
}

impl CardSlot {
    /// A card only its owner can see: shown to the owner, hidden from everyone else.
    pub fn private(card: Card, viewer_is_owner: bool) -> CardSlot {
        CardSlot::Down(viewer_is_owner.then_some(card))
    }
}

/// Community cards. The layout tells the UI where each slot goes; `lines` lists which slot groups
/// can be used together (for shaped boards like Elevator or Wheel of Death).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardView {
    /// `"Board"`, `"Top"`, `"Bottom"`.
    pub name: String,
    pub layout: BoardLayout,
    pub slots: Vec<CardSlot>,
    /// Groups of slot indices a hand may use together. Empty = any.
    pub lines: Vec<Vec<u8>>,
}

impl BoardView {
    /// A plain row of community cards (Hold'em, Omaha).
    pub fn row(name: &str, slots: Vec<CardSlot>) -> BoardView {
        BoardView {
            name: name.into(),
            layout: BoardLayout::Row,
            slots,
            lines: Vec::new(),
        }
    }
}

/// Where a board's slots go on screen. Slot order per layout is documented by each variant.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoardLayout {
    /// Left to right.
    Row,
    /// Elevator's H: left column top→bottom, centre, right column top→bottom.
    H,
    /// A ring of `n` slots, clockwise from the top.
    Wheel,
    /// Iron Cross: row left→right, then column top→bottom without the shared centre.
    Cross,
    /// Industrial Elevator: column top→bottom, then the row's other cards left→right.
    Elevator,
    /// A loose pile in the middle (Spit, Numbers).
    Middle,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PotView {
    /// `"Main"`, `"Side 1"`, `"Carried"`.
    pub name: String,
    pub amount: Chips,
}

/// A wild rank in play.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WildView {
    pub rank: Rank,
    /// `None` = wild for the whole table; `Some(seat)` = only for that seat (Low in the Hole).
    pub seat: Option<Seat>,
}
