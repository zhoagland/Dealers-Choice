//! The scripted Bible games (catalog §4–§5). Each function is one game: its steps, hand rules,
//! and the rule decisions (D-*) made where the Bible is silent. Every decision is provisional and
//! listed for the owner; overruling one = change + rule test + `version` bump.
//!
//! Cross-game decisions:
//! - **D-ALL-1** (owner): FTQ money structure for every game but Texas Hold'em (`Config`).
//! - **D-HE-1** Hold'em variants: the Bible's "the 2 hole cards plus 3 community cards" is the same
//!   sentence its Texas Hold'em section uses, and Texas Hold'em plays any five of seven; so the
//!   variants also play best five of hole + board, any mix ([`Build::Any`]).
//! - **D-ALL-3** hi/lo games declare High / Low / Both (house Both rule, OQ-3 split).

use engine::{CardSet, Chips};

use super::hand::{Build, Ranking, ShareRule};
use super::{ActOrder, DeckKind, DeclareKind, Script, Step, Visibility};

use Step::*;

/// Every scripted game id, in catalog order.
pub const SCRIPTED_IDS: &[&str] = &[
    // G1 Omaha
    "omaha_pl",
    "omaha5",
    "courchevel",
    "red_river",
    "plo_double_board",
    // G3 Hold'em twists
    "holdem_6plus",
    "holdem_showtime",
    "reverse_poker",
    "holdem_split",
    // G6 Stud
    "stud7",
    "razz",
    "chicago_high",
    "chicago_low",
    // G7 Indian
    "indian_poker",
    "holdem_indian",
];

/// The script for `id`.
pub fn script(id: &str) -> Option<Script> {
    Some(match id {
        "omaha_pl" => omaha_pl(),
        "omaha5" => omaha5(
            "omaha5",
            "5 Card Omaha (hi/lo declare)",
            "Poker Bible §5 Card Omaha",
            false,
        ),
        "courchevel" => omaha5(
            "courchevel",
            "Courchevel (hi/lo declare)",
            "Poker Bible §Courchevel",
            true,
        ),
        "red_river" => red_river(),
        "plo_double_board" => double_board(),
        "holdem_6plus" => six_plus(),
        "holdem_showtime" => showtime(),
        "reverse_poker" => reverse_poker(),
        "holdem_split" => split_holdem(),
        "stud7" => stud(
            "stud7",
            "7 Card Stud",
            "Poker Bible §7 Card Stud",
            vec![HIGH_ANY],
            ActOrder::BestBoard,
        ),
        "razz" => stud(
            "razz",
            "Razz",
            "Poker Bible §Razz",
            vec![LOW_ANY],
            ActOrder::LowBoard,
        ),
        "chicago_high" => stud(
            "chicago_high",
            "Chicago High",
            "Poker Bible §Chicago High",
            vec![HIGH_ANY, spade(Ranking::SpadeHigh)],
            ActOrder::BestBoard,
        ),
        "chicago_low" => stud(
            "chicago_low",
            "Chicago Low",
            "Poker Bible §Chicago Low",
            vec![HIGH_ANY, spade(Ranking::SpadeLow)],
            ActOrder::BestBoard,
        ),
        "indian_poker" => indian_poker(),
        "holdem_indian" => indian_holdem(),
        _ => return None,
    })
}

const HIGH_ANY: ShareRule = ShareRule {
    name: "High",
    ranking: Ranking::High,
    build: Build::Any,
    board: 0,
};
const LOW_ANY: ShareRule = ShareRule {
    name: "Low",
    ranking: Ranking::LowA6,
    build: Build::Any,
    board: 0,
};
const OMAHA: Build = Build::Exactly { hole: 2, board: 3 };
const HIGH_OMAHA: ShareRule = ShareRule {
    name: "High",
    ranking: Ranking::High,
    build: OMAHA,
    board: 0,
};
const LOW_OMAHA: ShareRule = ShareRule {
    name: "Low",
    ranking: Ranking::LowA6,
    build: OMAHA,
    board: 0,
};

fn spade(r: Ranking) -> ShareRule {
    ShareRule {
        name: "Spade",
        ranking: r,
        build: Build::Own,
        board: 0,
    }
}

fn base(id: &'static str, name: &'static str, rules_ref: &'static str) -> Script {
    Script {
        id,
        name,
        version: 1,
        rules_ref,
        max_seats: 9,
        deck: DeckKind::Standard,
        ante_each: Chips::ZERO,
        steps: Vec::new(),
        order: ActOrder::LeftOfButton,
        shares: vec![HIGH_ANY],
        declare: None,
        wild: CardSet::EMPTY,
        visibility: Visibility::Normal,
        boards: 1,
        has_high_only: false,
    }
}

/// Deal, then flop / turn / river with a betting round after each (Hold'em shape).
fn flop_turn_river(board: u8) -> Vec<Step> {
    vec![
        Bet,
        Board { board, n: 3 },
        Bet,
        Board { board, n: 1 },
        Bet,
        Board { board, n: 1 },
        Bet,
    ]
}

// ----- G1 Omaha -------------------------------------------------------------------------------

/// Pot Limit Omaha: 4 hole cards, exactly 2 from hand + 3 from the board, high only. The Bible's
/// name is kept; the money follows D-ALL-1 (set `structure = "PotLimit"` in the game's config to
/// play it pot limit).
fn omaha_pl() -> Script {
    let mut steps = vec![Down(4)];
    steps.extend(flop_turn_river(0));
    Script {
        steps,
        shares: vec![HIGH_OMAHA],
        ..base(
            "omaha_pl",
            "Pot Limit Omaha",
            "Poker Bible §Pot Limit Omaha",
        )
    }
}

/// 5 Card Omaha / Courchevel: 5 hole cards, 2o2 high and A-6 low, hi/lo declaration.
/// - **D-5CO-1 / D-COU-1** the Bible's "3 *continuous* community cards" is copied from Wheel of
///   Death; a standard board has no geometry, so any 3 board cards.
/// - **D-5CO-2 / D-COU-2** hi/lo with declaration (D-ALL-3); `high_only` is the Bible's variation.
/// - Courchevel: the first flop card is dealt face up before the first betting round.
fn omaha5(
    id: &'static str,
    name: &'static str,
    rules_ref: &'static str,
    courchevel: bool,
) -> Script {
    let steps = if courchevel {
        vec![
            Down(5),
            Board { board: 0, n: 1 },
            Bet,
            Board { board: 0, n: 2 },
            Bet,
            Board { board: 0, n: 1 },
            Bet,
            Board { board: 0, n: 1 },
            Bet,
            Declare,
        ]
    } else {
        let mut s = vec![Down(5)];
        s.extend(flop_turn_river(0));
        s.push(Declare);
        s
    };
    Script {
        steps,
        shares: vec![HIGH_OMAHA, LOW_OMAHA],
        declare: Some(DeclareKind::HiLo),
        has_high_only: true,
        ..base(id, name, rules_ref)
    }
}

/// Red River: PLO hands, but after the river a "river+1" is always dealt (**D-RR-1**), and while
/// the newest card is red another card and betting round follow. **D-RR-2** hands are 2 from hand
/// plus the best 3 of *all* board cards. **D-RR-3** if the deck runs out, the board ends. Hi/lo
/// declaration (the Bible's objective and game play list it).
fn red_river() -> Script {
    let mut steps = vec![Down(4)];
    steps.extend(flop_turn_river(0));
    steps.extend([RedRiver { board: 0 }, Declare]);
    Script {
        steps,
        shares: vec![HIGH_OMAHA, LOW_OMAHA],
        declare: Some(DeclareKind::HiLo),
        ..base(
            "red_river",
            "Red River (hi/lo declare)",
            "Poker Bible §Red River",
        )
    }
}

/// PLO Double Board Bomb Pot. **D-DB-1 (owner)** every seat antes $1 (plus the dealer ante) and
/// the preflop round is kept. **D-DB-2 (owner)** declare Top, Bottom, or Both; each board's half
/// goes to the best Omaha *high* hand on that board among the seats that declared it.
fn double_board() -> Script {
    let steps = vec![
        Down(4),
        Bet,
        Board { board: 0, n: 3 },
        Board { board: 1, n: 3 },
        Bet,
        Board { board: 0, n: 1 },
        Board { board: 1, n: 1 },
        Bet,
        Board { board: 0, n: 1 },
        Board { board: 1, n: 1 },
        Bet,
        Declare,
    ];
    Script {
        steps,
        ante_each: Chips(100),
        shares: vec![
            ShareRule {
                name: "Top",
                ..HIGH_OMAHA
            },
            ShareRule {
                name: "Bottom",
                board: 1,
                ..HIGH_OMAHA
            },
        ],
        declare: Some(DeclareKind::Boards),
        boards: 2,
        ..base(
            "plo_double_board",
            "PLO Double Board Bomb Pot",
            "Poker Bible §PLO Double Board Bomb Pot",
        )
    }
}

// ----- G3 Hold'em twists ----------------------------------------------------------------------

fn holdem_steps() -> Vec<Step> {
    let mut s = vec![Down(2)];
    s.extend(flop_turn_river(0));
    s
}

/// 6+ Hold'em: 36-card deck, flush beats full house, A-6-7-8-9 is the lowest straight.
/// **D-6P-1** three of a kind still ranks below a straight.
fn six_plus() -> Script {
    Script {
        steps: holdem_steps(),
        deck: DeckKind::Short36,
        shares: vec![ShareRule {
            ranking: Ranking::ShortDeck,
            ..HIGH_ANY
        }],
        ..base(
            "holdem_6plus",
            "6+ Hold'em (Short Deck)",
            "Poker Bible §6+ Hold'em",
        )
    }
}

/// Showtime Hold'em: folded hole cards turn face up for everyone.
fn showtime() -> Script {
    Script {
        steps: holdem_steps(),
        visibility: Visibility::Showtime,
        ..base(
            "holdem_showtime",
            "Showtime Hold'em",
            "Poker Bible §Showtime Hold'em",
        )
    }
}

/// Reverse Poker: 5 hole cards, a 2-card board flipped one at a time; best 5 of 7.
fn reverse_poker() -> Script {
    Script {
        steps: vec![
            Down(5),
            Bet,
            Board { board: 0, n: 1 },
            Bet,
            Board { board: 0, n: 1 },
            Bet,
        ],
        ..base(
            "reverse_poker",
            "Reverse Poker",
            "Poker Bible §Reverse Poker",
        )
    }
}

/// Split Hold'em: two boards dealt together, half the pot each, with a Top / Bottom / Both
/// declaration (the Bible's game play lists one).
fn split_holdem() -> Script {
    let steps = vec![
        Down(2),
        Bet,
        Board { board: 0, n: 3 },
        Board { board: 1, n: 3 },
        Bet,
        Board { board: 0, n: 1 },
        Board { board: 1, n: 1 },
        Bet,
        Board { board: 0, n: 1 },
        Board { board: 1, n: 1 },
        Bet,
        Declare,
    ];
    Script {
        steps,
        shares: vec![
            ShareRule {
                name: "Top",
                ..HIGH_ANY
            },
            ShareRule {
                name: "Bottom",
                board: 1,
                ..HIGH_ANY
            },
        ],
        declare: Some(DeclareKind::Boards),
        boards: 2,
        ..base(
            "holdem_split",
            "Split Hold'em",
            "Poker Bible §Split Hold'em",
        )
    }
}

// ----- G6 Stud --------------------------------------------------------------------------------

/// 7 Card Stud and its plain variants: 2 down, a betting round on those, then four up cards and a
/// last down card, with a betting round after each (as the Bible lists). Max 7 seats (7 × 7 = 49).
/// - **D-STUD-1** first to act: best board showing (high games) or lowest board showing (Razz);
///   before any upcard, left of the dealer.
/// - **D-CHI-1 (owner)** the spade half counts spades among all 7 cards.
/// - **D-CHI-2 (owner)** Chicago Low: ace is high, so 2♠ is lowest. A seat must reach showdown to
///   win the spade half; with no spade at showdown the hand winner takes the whole pot (the
///   unwinnable share rolls over, `award_shares`). The spade half is won even if that seat loses
///   the hand half.
/// - Razz uses the house low (A-6, no qualifier).
fn stud(
    id: &'static str,
    name: &'static str,
    rules_ref: &'static str,
    shares: Vec<ShareRule>,
    order: ActOrder,
) -> Script {
    Script {
        steps: vec![
            Down(2),
            Bet,
            Up(1),
            Bet,
            Up(1),
            Bet,
            Up(1),
            Bet,
            Up(1),
            Bet,
            Down(1),
            Bet,
        ],
        max_seats: 7,
        order,
        shares,
        boards: 0,
        ..base(id, name, rules_ref)
    }
}

// ----- G7 Indian ------------------------------------------------------------------------------

/// Indian Poker: one card each, visible to everyone but its owner; one betting round; the highest
/// card wins (same rank splits). **D-IP-1** FTQ spread betting.
fn indian_poker() -> Script {
    Script {
        steps: vec![Down(1), Bet],
        shares: vec![ShareRule {
            name: "High card",
            ranking: Ranking::HighCard,
            build: Build::Own,
            board: 0,
        }],
        visibility: Visibility::Indian,
        boards: 0,
        ..base("indian_poker", "Indian Poker", "Poker Bible §Indian Poker")
    }
}

/// Indian Texas Hold'em: Hold'em with hole cards visible to everyone except their owner.
fn indian_holdem() -> Script {
    Script {
        steps: holdem_steps(),
        visibility: Visibility::Indian,
        ..base(
            "holdem_indian",
            "Indian Texas Hold'em",
            "Poker Bible §Indian Texas Hold'em",
        )
    }
}
