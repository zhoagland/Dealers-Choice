use crate::modules::card::Suit;

use super::card::Card;

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum HandRanking {
    RoyalFlush = 10,
    FiveOfAKind = 9,
    StraightFlush = 8,
    FullHouse = 7,
    Flush = 6,
    Straight = 5,
    ThreeOfAKind = 4,
    TwoPair = 3,
    OnePair = 2,
    HighHand = 1,
}


fn compare_hands<'a>(hand1:&'a Vec<Card>, hand2:&'a Vec<Card>) -> &'a Vec<Card> {

    // Compare Rankings
    // if the rankings are the same, see which card is higher and continue untill a higher card is achieved or it is a tie
    todo!()

}

fn calculate_hand_strength(hand:&Vec<Card>) -> HandRanking {

    // Iterate through each Hand ranking and see if the hand contains it.
    
    todo!()

}


#[cfg(test)]
mod tests {

    use super::*;
    use super::super::card::{Card,Rank,Suit};

    #[test]
    fn test_compare_hands() {
        let hand1 = vec![Card { suit: Suit::Clubs, rank: Rank::Ace }];
        let hand2 = vec![Card { suit: Suit::Hearts, rank: Rank::Three }];
        assert!( compare_hands(&hand1, &hand2) == &hand1 );
    }

    #[test]
    #[should_panic]
    fn test_compare_hands_2() {
        let hand1 = vec![Card { suit: Suit::Clubs, rank: Rank::Two }];
        let hand2 = vec![Card { suit: Suit::Hearts, rank: Rank::Three }];
        assert!( compare_hands(&hand1, &hand2) == &hand1 );
    }

}