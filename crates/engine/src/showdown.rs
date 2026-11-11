//! Optional show (owner decision G-3, `docs/tech/game-catalog.md` §3): after the required shows,
//! each losing player may Show or Muck. Bots always muck.
//!
//! This sits outside the game state machine: showing can't change chips or the hand, so it isn't
//! a game action and bots never face it. It works for every game through [`Game::view`]: a seat
//! may show when it finished the hand without folding, lost chips, and its face-down cards are
//! still hidden from the other seats. The choice is stored with the hand record so hand history
//! can show the cards.

use crate::card::Card;
use crate::game::{Chips, Game, Seat, ToAct};
use crate::view::{CardSlot, SeatStatus};

/// Seats that may choose to show their hidden cards at the end of the hand `end` (terminal).
pub fn may_show<G: Game>(game: &G, end: &G::State) -> Vec<Seat> {
    assert_eq!(game.to_act(end), ToAct::Terminal, "may_show: hand not over");
    let payoffs = game.payoffs(end);
    let n = payoffs.len() as u8;
    (0..n)
        .map(Seat)
        .filter(|&p| payoffs[p.index()] < Chips::ZERO)
        .filter(|&p| {
            // Look at p from another seat's view.
            let Some(q) = (0..n).map(Seat).find(|&q| q != p) else {
                return false;
            };
            let view = game.view(&game.observe(end, q));
            view.seats.iter().find(|s| s.seat == p).is_some_and(|s| {
                matches!(s.status, SeatStatus::Active | SeatStatus::AllIn)
                    && s.cards.contains(&CardSlot::Down(None))
            })
        })
        .filter(|&p| !own_hidden_cards(game, end, p).is_empty())
        .collect()
}

/// The cards `seat` would show: its face-down cards as it sees them.
pub fn own_hidden_cards<G: Game>(game: &G, end: &G::State, seat: Seat) -> Vec<Card> {
    let view = game.view(&game.observe(end, seat));
    view.seats
        .iter()
        .find(|s| s.seat == seat)
        .map(|s| {
            s.cards
                .iter()
                .filter_map(|c| match c {
                    CardSlot::Down(Some(card)) => Some(*card),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}
