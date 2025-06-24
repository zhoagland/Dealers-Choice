use crate::modules::card::Suit;

use super::card::Card;

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum HandRanking {
    RoyalFlush = 10,
    FourOfAKind = 9,
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
    // if the rankings are the same, see which card is higher and continue until a higher card is achieved or it is a tie
    let hand1_strength = calculate_hand_strength(hand1);
    let hand2_strength = calculate_hand_strength(hand2);

    if hand1_strength > hand2_strength {
        return hand1;
    } else if hand2_strength > hand1_strength {
        return hand2;
    } else {
        // If the strengths are equal, compare the highest card in each hand
        let hand1_high_card = hand1.iter().max_by_key(|card| card.rank).unwrap();
        let hand2_high_card = hand2.iter().max_by_key(|card| card.rank).unwrap();

        // Compare the high cards
        if hand1_high_card.rank > hand2_high_card.rank {
            return hand1;
        } else if hand2_high_card.rank > hand1_high_card.rank {
            return hand2;
        } else {
            // If the high cards are also equal, return the first hand
            return hand1; // or handle tie case as neededs
        }
    }
}

fn calculate_hand_strength(hand:&Vec<Card>) -> HandRanking {

    // Iterate through each Hand ranking and see if the hand contains it.
    if is_royal_flush(hand) {
        return HandRanking::RoyalFlush;
    } else if is_four_of_a_kind(hand) {
        return HandRanking::FourOfAKind;
    } else if is_straight_flush(hand) {
        return HandRanking::StraightFlush;
    } else if is_full_house(hand) {
        return HandRanking::FullHouse;
    } else if is_flush(hand) {
        return HandRanking::Flush;
    } else if is_straight(hand) {
        return HandRanking::Straight;
    } else if is_three_of_a_kind(hand) {
        return HandRanking::ThreeOfAKind;
    } else if is_two_pair(hand) {
        return HandRanking::TwoPair;
    } else if is_one_pair(hand) {
        return HandRanking::OnePair;
    } else {
        return HandRanking::HighHand; // Default case
    }    

}

fn is_royal_flush(hand: &Vec<Card>) -> bool {

    todo!()
}

fn is_four_of_a_kind(hand: &Vec<Card>) -> bool {

    todo!()
}

fn is_straight_flush(hand: &Vec<Card>) -> bool {

    todo!()
}

fn is_full_house(hand: &Vec<Card>) -> bool {

    todo!()
}

fn is_flush(hand: &Vec<Card>) -> bool {
    todo!()
}

fn is_straight(hand: &Vec<Card>) -> bool {
    todo!()

}

fn is_three_of_a_kind(hand: &Vec<Card>) -> bool {
    todo!()
}

fn is_two_pair(hand: &Vec<Card>) -> bool {
    todo!()
}

fn is_one_pair(hand: &Vec<Card>) -> bool {

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