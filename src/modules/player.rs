use super::card::Card as Card;

#[derive(Debug, PartialEq, Eq)]
struct Strategy {
    bluff_percentage : u8,

}

impl Strategy {
    pub fn new() -> Strategy {
        Strategy {
            bluff_percentage: 1
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Player {
    hand : Vec<Card>,
    chip_count: u128,
    stratagy: Strategy,

}      

impl Player {
    pub fn new() -> Player {
        Player { 
            hand: Vec::new(),
            chip_count: 100,
            stratagy: Strategy::new(),
        }
    }
}

