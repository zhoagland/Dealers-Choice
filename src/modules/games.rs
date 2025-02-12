use super::player::Player as Player;

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


