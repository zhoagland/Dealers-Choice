//! Shared helpers for the analysis hooks (`Game::start_class`, `Game::decision_context`):
//! board-texture classes, draw detection and the Chen preflop score. Used by the Hold'em-style
//! drivers; other families add their own helpers here.

use engine::{Card, CardSet, Rank, Suit};

/// Rank index 0 (Two) ..= 12 (Ace).
pub fn rank_index(r: Rank) -> usize {
    Rank::ALL.iter().position(|&x| x == r).unwrap()
}

/// 13-bit mask of the ranks present (bit 0 = Two).
pub fn rank_mask(cards: impl IntoIterator<Item = Card>) -> u16 {
    cards
        .into_iter()
        .fold(0u16, |m, c| m | 1 << rank_index(c.rank()))
}

/// Some window of five consecutive ranks (Ace also low) holds at least `k` of `mask`'s ranks.
pub fn has_run(mask: u16, k: u32) -> bool {
    // Ace-low: put the Ace's bit below the Two.
    let ext = ((mask as u32) << 1) | ((mask as u32 >> 12) & 1);
    (0..=9).any(|lo| ((ext >> lo) & 0b11111).count_ones() >= k)
}

fn suit_counts(cards: impl IntoIterator<Item = Card>) -> [u8; 4] {
    let mut n = [0u8; 4];
    for c in cards {
        n[Suit::ALL.iter().position(|&s| s == c.suit()).unwrap()] += 1;
    }
    n
}

// ----- Hold'em-style starting hands (169 classes) ---------------------------------------------

/// Class id `a*13 + b`: pairs `a == b`; suited `a > b` (a = high); offsuit `a < b` (b = high).
/// On the standard grid (Aces top-left, suited above the diagonal) it sits at `(12-a, 12-b)`.
pub fn holdem_class(c1: Card, c2: Card) -> u16 {
    let (r1, r2) = (rank_index(c1.rank()), rank_index(c2.rank()));
    let (hi, lo) = (r1.max(r2), r1.min(r2));
    let id = if hi == lo || c1.suit() == c2.suit() {
        hi * 13 + lo
    } else {
        lo * 13 + hi
    };
    id as u16
}

pub fn holdem_class_names() -> (Vec<String>, Vec<(u8, u8)>) {
    let ch = |i: usize| Rank::ALL[i].to_char();
    let mut names = Vec::with_capacity(169);
    let mut grid = Vec::with_capacity(169);
    for id in 0..169usize {
        let (a, b) = (id / 13, id % 13);
        names.push(match a.cmp(&b) {
            std::cmp::Ordering::Equal => format!("{}{}", ch(a), ch(a)),
            std::cmp::Ordering::Greater => format!("{}{}s", ch(a), ch(b)),
            std::cmp::Ordering::Less => format!("{}{}o", ch(b), ch(a)),
        });
        grid.push(((12 - a) as u8, (12 - b) as u8));
    }
    (names, grid)
}

/// Chen preflop score (Bill Chen's formula), rounded up. Higher = stronger; AA = 20.
pub fn chen_score(c1: Card, c2: Card) -> i32 {
    let pts = |r: usize| -> f64 {
        match r {
            12 => 10.0,
            11 => 8.0,
            10 => 7.0,
            9 => 6.0,
            _ => (r as f64 + 2.0) / 2.0,
        }
    };
    let (r1, r2) = (rank_index(c1.rank()), rank_index(c2.rank()));
    let (hi, lo) = (r1.max(r2), r1.min(r2));
    let mut s = pts(hi);
    if hi == lo {
        s = (s * 2.0).max(5.0);
    } else {
        if c1.suit() == c2.suit() {
            s += 2.0;
        }
        let gap = hi - lo - 1;
        s -= match gap {
            0 => 0.0,
            1 => 1.0,
            2 => 2.0,
            3 => 4.0,
            _ => 5.0,
        };
        if gap <= 1 && hi < 10 {
            s += 1.0;
        }
    }
    s.ceil() as i32
}

/// Preflop tiers from the Chen score, as strength ids 0..=4.
pub const PREFLOP_TIERS: [&str; 5] = [
    "preflop: weak (Chen < 5)",
    "preflop: marginal (5–6)",
    "preflop: good (7–8)",
    "preflop: strong (9–11)",
    "preflop: premium (12+)",
];

pub fn preflop_tier(c1: Card, c2: Card) -> u8 {
    match chen_score(c1, c2) {
        i32::MIN..=4 => 0,
        5..=6 => 1,
        7..=8 => 2,
        9..=11 => 3,
        _ => 4,
    }
}

// ----- Post-flop strength with two hole cards -----------------------------------------------------

/// Post-flop strength ids, after the 5 preflop tiers.
pub const POSTFLOP_STRENGTHS: [&str; 9] = [
    "nothing",
    "draw (flush or straight)",
    "weak pair",
    "top pair / overpair",
    "two pair",
    "trips / set",
    "straight",
    "flush",
    "full house or better",
];

/// Strength id (offset by `PREFLOP_TIERS.len()`) of `hole` on `board` (3–5 cards).
pub fn postflop_strength(hole: &[Card], board: &[Card], category: engine::Category) -> u8 {
    use engine::Category::*;
    let base = PREFLOP_TIERS.len() as u8;
    let id = match category {
        FullHouse | Quads | StraightFlush | FiveOfAKind | RoyalFlush => 8,
        Flush => 7,
        Straight => 6,
        Trips => 5,
        TwoPair => 4,
        HighCard | Pair => {
            let board_hi = board
                .iter()
                .map(|c| rank_index(c.rank()))
                .max()
                .unwrap_or(0);
            let board_ranks = rank_mask(board.iter().copied());
            let hole_r: Vec<usize> = hole.iter().map(|c| rank_index(c.rank())).collect();
            let pocket_pair = hole_r.len() == 2 && hole_r[0] == hole_r[1];
            let pairs_board = |r: usize| board_ranks & (1 << r) != 0;
            let overpair = pocket_pair && hole_r[0] > board_hi;
            let top_pair = hole_r.contains(&board_hi) && pairs_board(board_hi);
            let made = if overpair || top_pair {
                Some(3)
            } else if pocket_pair || hole_r.iter().any(|&r| pairs_board(r)) {
                Some(2)
            } else {
                None
            };
            match made {
                Some(m) => m,
                None if board.len() < 5 && has_draw(hole, board) => 1,
                None => 0,
            }
        }
    };
    base + id
}

/// A flush draw or straight draw that uses at least one hole card.
pub fn has_draw(hole: &[Card], board: &[Card]) -> bool {
    let all: Vec<Card> = hole.iter().chain(board).copied().collect();
    let suits = suit_counts(all.iter().copied());
    let flush_draw = Suit::ALL
        .iter()
        .enumerate()
        .any(|(i, &s)| suits[i] == 4 && hole.iter().any(|c| c.suit() == s));
    let straight_draw =
        has_run(rank_mask(all.iter().copied()), 4) && !has_run(rank_mask(board.iter().copied()), 4);
    flush_draw || straight_draw
}

// ----- Board texture (community-card games) ---------------------------------------------------

/// Board texture id: 0 = no board yet; otherwise `1 + ((pair*3 + suit)*2 + conn)*4 + high`.
pub fn board_texture(board: &[Card]) -> u16 {
    if board.is_empty() {
        return 0;
    }
    let mut counts = [0u8; 13];
    for c in board {
        counts[rank_index(c.rank())] += 1;
    }
    let pairs = counts.iter().filter(|&&n| n >= 2).count();
    let pair = if counts.iter().any(|&n| n >= 3) || pairs >= 2 {
        2
    } else if pairs == 1 {
        1
    } else {
        0
    };
    let max_suit = *suit_counts(board.iter().copied()).iter().max().unwrap();
    let suit = match max_suit {
        0 | 1 => 0,
        2 => 1,
        _ => 2,
    };
    let conn = has_run(rank_mask(board.iter().copied()), 3) as u16;
    let hi = board.iter().map(|c| rank_index(c.rank())).max().unwrap();
    let high = match hi {
        12 => 0,
        10 | 11 => 1,
        8 | 9 => 2,
        _ => 3,
    };
    1 + ((pair * 3 + suit) * 2 + conn) * 4 + high
}

pub fn board_texture_names() -> Vec<String> {
    let mut v = vec!["no board (preflop)".to_string()];
    for pair in ["unpaired", "paired", "double-paired/trips"] {
        for suit in ["rainbow", "two-tone", "flush possible"] {
            for conn in ["disconnected", "connected"] {
                for high in ["A-high", "K/Q-high", "J/T-high", "9-high or lower"] {
                    v.push(format!("{pair} · {suit} · {conn} · {high}"));
                }
            }
        }
    }
    v
}

/// Convenience for drivers: the set of cards as a `CardSet`.
pub fn set(cards: &[Card]) -> CardSet {
    cards.iter().copied().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(s: &str) -> Card {
        s.parse().unwrap()
    }

    #[test]
    fn holdem_classes_cover_169_and_name_correctly() {
        let (names, grid) = holdem_class_names();
        assert_eq!(names.len(), 169);
        assert_eq!(names[holdem_class(c("As"), c("Ad")) as usize], "AA");
        assert_eq!(names[holdem_class(c("As"), c("Ks")) as usize], "AKs");
        assert_eq!(names[holdem_class(c("Kd"), c("As")) as usize], "AKo");
        assert_eq!(names[holdem_class(c("7c"), c("2d")) as usize], "72o");
        assert_eq!(grid[holdem_class(c("As"), c("Ad")) as usize], (0, 0));
        assert_eq!(
            grid[holdem_class(c("As"), c("Ks")) as usize],
            (0, 1),
            "suited above diagonal"
        );
        assert_eq!(grid[holdem_class(c("As"), c("Kd")) as usize], (1, 0));
        let mut seen = std::collections::BTreeSet::new();
        for &a in &CardSet::FULL_DECK.iter().collect::<Vec<_>>() {
            for b in CardSet::FULL_DECK.iter() {
                if a != b {
                    seen.insert(holdem_class(a, b));
                }
            }
        }
        assert_eq!(seen.len(), 169);
    }

    #[test]
    fn chen_scores_match_known_values() {
        assert_eq!(chen_score(c("As"), c("Ad")), 20);
        assert_eq!(chen_score(c("As"), c("Ks")), 12);
        assert_eq!(chen_score(c("Th"), c("Td")), 10);
        assert_eq!(chen_score(c("5h"), c("5d")), 5);
        assert_eq!(chen_score(c("7h"), c("2d")), -1);
    }

    #[test]
    fn textures_and_strength() {
        let names = board_texture_names();
        assert_eq!(names.len(), 73);
        let t = board_texture(&[c("Ah"), c("Kh"), c("Qh")]);
        assert_eq!(
            names[t as usize],
            "unpaired · flush possible · connected · A-high"
        );
        let t = board_texture(&[c("7c"), c("7d"), c("2d")]);
        assert_eq!(
            names[t as usize],
            "paired · two-tone · disconnected · 9-high or lower"
        );
        let s = |h: [&str; 2], b: &[&str]| {
            let hole = [c(h[0]), c(h[1])];
            let board: Vec<Card> = b.iter().map(|x| c(x)).collect();
            let cat = engine::eval::eval_high(set(&hole).union(set(&board))).category();
            POSTFLOP_STRENGTHS[(postflop_strength(&hole, &board, cat) - 5) as usize]
        };
        assert_eq!(s(["As", "Kd"], &["Ah", "7c", "2d"]), "top pair / overpair");
        assert_eq!(s(["Qs", "Qd"], &["Jh", "7c", "2d"]), "top pair / overpair");
        assert_eq!(s(["7s", "8d"], &["Ah", "7c", "2d"]), "weak pair");
        assert_eq!(s(["9h", "8h"], &["Ah", "7h", "2h"]), "flush");
        assert_eq!(
            s(["9h", "8h"], &["Ah", "7h", "2c"]),
            "draw (flush or straight)"
        );
        assert_eq!(
            s(["9s", "8d"], &["Th", "7c", "2d"]),
            "draw (flush or straight)"
        );
        assert_eq!(s(["3s", "4d"], &["Kh", "9c", "Jd"]), "nothing");
    }
}
