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
use super::{ActOrder, DeckKind, DeclareKind, Script, Step, Variant, Visibility};

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
    // G4 Draw & discard
    "draw5",
    "triple_draw",
    "draw27_single",
    "draw27_triple",
    "badugi",
    "three_blind_mice",
    "crazy_pineapple",
    "mamas_favorite",
    "holdem_swap",
    "fusion",
    // G5 Pass & in/out
    "pass_the_trash",
    "man_or_mouse",
    "game_43",
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
        "draw5" => draw(
            "draw5",
            "5 Card Draw",
            "Poker Bible §5 Card Draw",
            Ranking::High,
            1,
        ),
        "triple_draw" => draw(
            "triple_draw",
            "Triple Draw (A-6 low)",
            "Poker Bible §Triple Draw",
            Ranking::LowA6,
            3,
        ),
        "draw27_single" => draw(
            "draw27_single",
            "2-7 Single Draw",
            "Poker Bible §2-7 Single Draw",
            Ranking::Low27,
            1,
        ),
        "draw27_triple" => draw(
            "draw27_triple",
            "2-7 Triple Draw",
            "Poker Bible §2-7 Triple Draw",
            Ranking::Low27,
            3,
        ),
        "badugi" => badugi(),
        "three_blind_mice" => three_blind_mice(),
        "crazy_pineapple" => crazy_pineapple(),
        "mamas_favorite" => mamas_favorite(),
        "holdem_swap" => swap_holdem(),
        "fusion" => fusion(),
        "pass_the_trash" => pass_the_trash(),
        "man_or_mouse" => man_or_mouse(),
        "game_43" => game_43(),
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
        swap_instead_of_fold: false,
        variants: Vec::new(),
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
        variants: vec![
            Variant {
                name: "deal_boards_separately",
                apply: |s| {
                    let mut steps = vec![Down(4), Bet];
                    for n in [3, 1, 1] {
                        for board in [0, 1] {
                            steps.extend([Board { board, n }, Bet]);
                        }
                    }
                    steps.push(Declare);
                    s.steps = steps;
                },
            },
            Variant {
                name: "dump_two",
                apply: |s| {
                    let at = s.steps.iter().position(|x| *x == Declare).unwrap();
                    s.steps.insert(at, Discard { n: 2, pairs: false });
                },
            },
        ],
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

// ----- G4 Draw & discard ----------------------------------------------------------------------

/// Draw games: 5 cards, `draws` rounds of (draw, bet). Max 8 seats.
/// - **D-5CD-1** up to 5 cards may be drawn (the Bible gives no limit).
/// - **D-DRAW-1** if the stock runs out mid-draw, the discards so far are shuffled into a new
///   stock (public). Draws go in turn from the dealer's left.
/// - Triple Draw uses the house low (A-6); 2-7 games use deuce-to-seven (ace always high,
///   straights and flushes count against).
fn draw(
    id: &'static str,
    name: &'static str,
    rules_ref: &'static str,
    r: Ranking,
    draws: u8,
) -> Script {
    let mut steps = vec![Down(5), Bet];
    for _ in 0..draws {
        steps.extend([
            Draw {
                max: 5,
                ace_bonus: false,
            },
            Bet,
        ]);
    }
    Script {
        steps,
        max_seats: 8,
        shares: vec![ShareRule {
            name: if r == Ranking::High { "High" } else { "Low" },
            ranking: r,
            build: Build::Own,
            board: 0,
        }],
        boards: 0,
        ..base(id, name, rules_ref)
    }
}

/// Badugi: 4 cards, three draws. **D-BAD-1** the objective line says "best high poker hand",
/// contradicting the overview; standard Badugi ranking (as the overview describes) is used.
fn badugi() -> Script {
    let mut steps = vec![Down(4), Bet];
    for _ in 0..3 {
        steps.extend([
            Draw {
                max: 4,
                ace_bonus: false,
            },
            Bet,
        ]);
    }
    Script {
        steps,
        shares: vec![ShareRule {
            name: "Badugi",
            ranking: Ranking::Badugi,
            build: Build::Own,
            board: 0,
        }],
        boards: 0,
        ..base("badugi", "Badugi", "Poker Bible §Badugi")
    }
}

const MICE_DRAW: Step = Draw {
    max: 3,
    ace_bonus: true,
};

/// 3 Blind Mice: 5 cards, draw up to 3 (4 keeping an Ace, which is shown), then 3 community
/// cards flipped together; best 5 of all 8. Variation `flip_one_at_a_time` (the Bible's).
/// **D-3BM-1** the 4-card draw is open to everyone (so choices reveal nothing); the kept card is
/// shown, and if it isn't an Ace the hand is fouled (folds).
fn three_blind_mice() -> Script {
    Script {
        steps: vec![Down(5), Bet, MICE_DRAW, Bet, Board { board: 0, n: 3 }, Bet],
        variants: vec![Variant {
            name: "flip_one_at_a_time",
            apply: |s| {
                s.steps = vec![Down(5), Bet, MICE_DRAW, Bet];
                for _ in 0..3 {
                    s.steps.extend([Board { board: 0, n: 1 }, Bet]);
                }
            },
        }],
        ..base(
            "three_blind_mice",
            "3 Blind Mice",
            "Poker Bible §3 Blind Mice",
        )
    }
}

/// Crazy Pineapple. **D-CP-1** the overview says 5 cards but Setup says 3: deal 3 and discard 1
/// after the flop (standard Crazy Pineapple), before that round's betting.
fn crazy_pineapple() -> Script {
    Script {
        steps: vec![
            Down(3),
            Bet,
            Board { board: 0, n: 3 },
            Discard { n: 1, pairs: false },
            Bet,
            Board { board: 0, n: 1 },
            Bet,
            Board { board: 0, n: 1 },
            Bet,
        ],
        ..base(
            "crazy_pineapple",
            "Crazy Pineapple",
            "Poker Bible §Crazy Pineapple",
        )
    }
}

/// Mama's Favorite: 6 cards split into 3 secret pairs before the first betting round; one pair is
/// thrown away after the flop and another after the turn (before that round's betting).
/// **D-MF-1** the split is a hidden choice (15 ways); thrown pairs are not shown. Max 7 seats.
fn mamas_favorite() -> Script {
    Script {
        steps: vec![
            Down(6),
            PairUp,
            Bet,
            Board { board: 0, n: 3 },
            Discard { n: 2, pairs: true },
            Bet,
            Board { board: 0, n: 1 },
            Discard { n: 2, pairs: true },
            Bet,
            Board { board: 0, n: 1 },
            Bet,
        ],
        max_seats: 7,
        ..base(
            "mamas_favorite",
            "Mama's Favorite",
            "Poker Bible §Mama's Favorite",
        )
    }
}

/// Swap Hold'em. **D-SW-1 (owner)** any time a player could fold, they may instead swap 1 or 2
/// hole cards for new ones from the deck and stay in by calling; at most once per street.
fn swap_holdem() -> Script {
    Script {
        steps: holdem_steps(),
        swap_instead_of_fold: true,
        ..base("holdem_swap", "Swap Hold'em", "Poker Bible §Swap Hold'em")
    }
}

/// Fusion. **D-FU-1** the overview is a copy of Tempest's; the game play list is used: Hold'em
/// plus one more hole card after the flop and after the turn (4 by the river), best five of hole
/// and board. Variation `hi_lo` (the Bible's): A-6 low half with a declaration.
fn fusion() -> Script {
    Script {
        steps: vec![
            Down(2),
            Bet,
            Board { board: 0, n: 3 },
            Down(1),
            Bet,
            Board { board: 0, n: 1 },
            Down(1),
            Bet,
            Board { board: 0, n: 1 },
            Bet,
        ],
        variants: vec![Variant {
            name: "hi_lo",
            apply: |s| {
                s.shares.push(LOW_ANY);
                s.declare = Some(DeclareKind::HiLo);
                s.steps.push(Declare);
            },
        }],
        ..base("fusion", "Fusion", "Poker Bible §Fusion")
    }
}

// ----- G5 Pass & in/out -----------------------------------------------------------------------

const HIGH_OWN: ShareRule = ShareRule {
    name: "High",
    ranking: Ranking::High,
    build: Build::Own,
    board: 0,
};
const LOW_OWN: ShareRule = ShareRule {
    name: "Low",
    ranking: Ranking::LowA6,
    build: Build::Own,
    board: 0,
};

/// Pass the Trash: 7 cards; pass 3, bet, pass 2, bet, pass 1 in the chooser's direction; throw away
/// 2; order the 5 face down and roll them one at a time with a betting round after each of the
/// first 4 (best hand showing opens); declare; show down. Max 7 seats.
/// - **D-PT-1** the chooser picks the direction (left/right) as a public action at the start.
/// - **D-PT-2** "Declare (High/Low)" is the standard declaration including Both.
/// - **D-PT-3** passes are chosen in turn but made all at once; the reveal order is a hidden
///   choice made before the first card is turned.
fn pass_the_trash() -> Script {
    Script {
        steps: vec![
            ChooseDirection,
            Down(7),
            Bet,
            Pass(3),
            Bet,
            Pass(2),
            Bet,
            Pass(1),
            Discard { n: 2, pairs: false },
            Order,
            Reveal,
            Bet,
            Reveal,
            Bet,
            Reveal,
            Bet,
            Reveal,
            Bet,
            Declare,
        ],
        max_seats: 7,
        order: ActOrder::BestBoard,
        shares: vec![HIGH_OWN, LOW_OWN],
        declare: Some(DeclareKind::HiLo),
        boards: 0,
        ..base(
            "pass_the_trash",
            "Pass the Trash",
            "Poker Bible §Pass the Trash",
        )
    }
}

/// Man or Mouse: 7 cards and one face-down community card; bet; pass 2 in the chooser's direction;
/// In/Out; bet; flip the community card; bet; declare. Hands: best 5 of the 7 + the community card.
/// - **D-MM-1** In/Out is simultaneous (revealed together); Out is a fold. If everyone is Out the
///   hand is void and every chip goes back.
/// - Direction as D-PT-1. Variation `five_card_start` (the Bible's): 5 cards, In/Out, bet, 2 more
///   cards, bet, community card, bet, declare (no passing).
fn man_or_mouse() -> Script {
    Script {
        steps: vec![
            ChooseDirection,
            Down(7),
            Bet,
            Pass(2),
            InOut { price: Chips::ZERO },
            Bet,
            Board { board: 0, n: 1 },
            Bet,
            Declare,
        ],
        max_seats: 7,
        shares: vec![HIGH_ANY, LOW_ANY],
        declare: Some(DeclareKind::HiLo),
        variants: vec![Variant {
            name: "five_card_start",
            apply: |s| {
                s.steps = vec![
                    Down(5),
                    InOut { price: Chips::ZERO },
                    Bet,
                    Down(2),
                    Bet,
                    Board { board: 0, n: 1 },
                    Bet,
                    Declare,
                ];
            },
        }],
        ..base("man_or_mouse", "Man or Mouse", "Poker Bible §Man or Mouse")
    }
}

/// 43: 4 cards; In/Out with a $1 chip (In puts it in the pot); 3 more cards; bet; declare.
/// Best 5 of 7, hi/lo. **D-43-1** In/Out is simultaneous and In posts $1. If everyone is Out the
/// hand is void. Max 7 seats.
fn game_43() -> Script {
    Script {
        steps: vec![Down(4), InOut { price: Chips(100) }, Down(3), Bet, Declare],
        max_seats: 7,
        shares: vec![HIGH_OWN, LOW_OWN],
        declare: Some(DeclareKind::HiLo),
        boards: 0,
        ..base("game_43", "43", "Poker Bible §43")
    }
}
