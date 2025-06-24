#[derive(Debug, PartialEq, Eq)]
pub enum Suit {
    Hearts,
    Spades,
    Diamonds,
    Clubs
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
/// Represents the rank of a card in a standard deck.
/// The ranks are ordered from Ace (1) to King (13).
/// The Ace is considered the lowest rank in this implementation.
pub enum Rank {
    Ace = 1,
    Two = 2,
    Three = 3,
    Four = 4,
    Five = 5,
    Six = 6,
    Seven = 7,
    Eight = 8,
    Nine = 9,
    Ten = 10,
    Jack = 11,
    Queen = 12,
    King = 13, 
}

impl Rank {
    pub fn strength_of_rank(rank: Rank) -> u8 {
        match rank {
            Rank::Ace => 1,
            Rank::Two => 2,
            Rank::Three => 3,
            Rank::Four => 4,
            Rank::Five => 5,
            Rank::Six => 6,
            Rank::Seven => 7,
            Rank::Eight => 8,
            Rank::Nine => 9,
            Rank::Ten => 10,
            Rank::Jack => 11,
            Rank::Queen => 12,
            Rank::King => 13,       
        }
    }
}


#[derive(Debug, PartialEq, Eq)]
pub struct Card {
    pub suit : Suit,
    pub rank : Rank,
}

impl Card {
    pub fn new() -> Card {
        Card {
            suit: Suit::Spades,
            rank: Rank::Ace
        }
    }
}
