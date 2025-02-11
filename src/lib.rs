mod poker{

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
    struct Card {
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

    
}