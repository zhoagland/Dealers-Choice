#[derive(Debug, PartialEq)]
enum Suit {
    Hearts,
    Spades,
    Diamonds,
    Clubs
}

#[derive(Debug, PartialEq, Eq)]
enum Rank {
    Ace,
    Two,
    Three,
    Four,
    Five,
    Six,
    Seven,
    Eight,
    Nine,
    Ten,
    Jack,
    Queen,
    King
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


#[derive(Debug)]
pub struct Card {
    suit : Suit,
    rank : Rank,
}

impl Card {
    pub fn new() -> Card {
        Card {
            suit: Suit::Spades,
            rank: Rank::Ace
        }
    }
}
