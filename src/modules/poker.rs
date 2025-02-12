use crate::modules::card::Card as Card;


#[derive(Debug)]
enum PokerGame {
    BlackJack(GameRules),
}

#[derive(Debug)]
struct GameRules {
    max_players: u8,
    card_count: u8,
    stun : bool,

}

impl GameRules {
    pub fn new() -> GameRules {
        GameRules { 
            max_players: 0, 
            card_count: 1,
            stun: true
        }
    }
}

#[derive(Debug)]
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

#[derive(Debug)]
struct Player {
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


#[derive(Debug)]
struct Game {
    players: Vec<Player>,
    game: PokerGame,
}

impl Game {
    pub fn new() -> Game {      
        Game {
            players: Vec::new(),
            game: PokerGame::BlackJack(GameRules::new()),
        }
    }
}


