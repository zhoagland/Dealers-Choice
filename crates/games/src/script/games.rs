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
use super::{ActOrder, DeckKind, DeclareKind, Resolution, Script, Step, Variant, Visibility};
use engine::GameLength;
use engine::view::BoardLayout;

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
    // G2 Board shapes
    "elevator",
    "wheel_of_death",
    "spit",
    "industrial_elevator",
    "iron_cross",
    // G6 Stud (special cards and choices)
    "baseball",
    "low_in_the_hole",
    "push",
    "bullet",
    "bid",
    // G8 Multi-hand & stuns
    "guts_leg",
    "drop_321",
    "three_five_seven",
    "scare_a_moosh",
    "holdem_tempest",
    // G9 Numbers
    "seven_27",
    "five_21",
    "numbers",
    // G10 Trick-taking
    "booray",
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
        "elevator" => elevator(),
        "wheel_of_death" => wheel_of_death(),
        "spit" => spit(),
        "industrial_elevator" => industrial_elevator(),
        "iron_cross" => iron_cross(),
        "baseball" => baseball(),
        "low_in_the_hole" => low_in_the_hole(),
        "push" => push(),
        "bullet" => bullet(),
        "bid" => bid(),
        "guts_leg" => guts_leg(),
        "drop_321" => drop_321(),
        "three_five_seven" => three_five_seven(),
        "scare_a_moosh" => scare_a_moosh(),
        "holdem_tempest" => tempest(),
        "seven_27" => targets(
            "seven_27",
            "7/27",
            "Poker Bible §7/27",
            14,
            54,
            ["7", "27", "Both"],
        ),
        "five_21" => targets(
            "five_21",
            "5.5/21.5",
            "Poker Bible §5.5/21.5",
            11,
            43,
            ["5.5", "21.5", "Both"],
        ),
        "numbers" => numbers(),
        "booray" => booray(),
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
        layout: BoardLayout::Row,
        positions: 0,
        lines: Vec::new(),
        four_extra: false,
        up_joker_kills: false,
        low_hole_wild: false,
        hand_cards: 0,
        length: GameLength::SingleHand,
        resolution: Resolution::Normal,
        ante_first_hand_only: false,
        out_is_out: false,
        orbit_stun: false,
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

// ----- G2 Board shapes ------------------------------------------------------------------------

/// Board positions as static one-element slices, for flip-one-at-a-time variations.
const POS: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];

/// Split every multi-card `Flip` into single-card flips, each followed by a betting round.
fn flip_singly(s: &mut Script) {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.steps.len() {
        match s.steps[i] {
            Flip { board, positions }
                if positions.len() > 1 && s.steps.get(i + 1) == Some(&Bet) =>
            {
                for &p in positions {
                    out.extend([
                        Flip {
                            board,
                            positions: &POS[p as usize..p as usize + 1],
                        },
                        Bet,
                    ]);
                }
                i += 2;
            }
            st => {
                out.push(st);
                i += 1;
            }
        }
    }
    s.steps = out;
}

/// Elevator: PLO hands with an H of 7 community cards (positions: left column top→bottom 0-2,
/// centre 3, right column top→bottom 4-6). The red set {0, 2, 5} flips, then the black set
/// {4, 1, 6}, then the blue centre. Hands are 2 from hand + the 3 cards of one line: left column,
/// right column, or the middle row {1, 3, 5}; high and low may use different lines. Hi/lo declare.
/// **D-ELV-1** the Bible lets either set flip first; the H is mirror-symmetric, so it changes
/// nothing: red first. Variation `two_at_a_time` (the Bible's).
fn elevator() -> Script {
    Script {
        steps: vec![
            Down(4),
            Bet,
            Flip {
                board: 0,
                positions: &[0, 2, 5],
            },
            Bet,
            Flip {
                board: 0,
                positions: &[4, 1, 6],
            },
            Bet,
            Flip {
                board: 0,
                positions: &[3],
            },
            Bet,
            Declare,
        ],
        shares: vec![HIGH_OMAHA, LOW_OMAHA],
        declare: Some(DeclareKind::HiLo),
        layout: BoardLayout::H,
        positions: 7,
        lines: vec![vec![0, 1, 2], vec![4, 5, 6], vec![1, 3, 5]],
        variants: vec![Variant {
            name: "two_at_a_time",
            apply: |s| {
                s.steps = vec![Down(4), Bet];
                for positions in [&[0u8, 2][..], &[5, 4], &[1, 6], &[3]] {
                    s.steps.extend([
                        Flip {
                            board: 0,
                            positions,
                        },
                        Bet,
                    ]);
                }
                s.steps.push(Declare);
            },
        }],
        ..base(
            "elevator",
            "Elevator (hi/lo declare)",
            "Poker Bible §Elevator",
        )
    }
}

/// Wheel of Death: PLO hands; 9 community cards in a ring; a hand uses 2 from hand + 3 consecutive
/// ring cards. Flips {0, 3, 6}, {1, 4, 7}, {2, 5, 8}. Hi/lo declare. Variations `eight_cards`
/// and `flip_one_at_a_time` (the Bible's).
fn wheel_of_death() -> Script {
    let ring = |n: u8| (0..n).map(|i| vec![i, (i + 1) % n, (i + 2) % n]).collect();
    Script {
        steps: vec![
            Down(4),
            Bet,
            Flip {
                board: 0,
                positions: &[0, 3, 6],
            },
            Bet,
            Flip {
                board: 0,
                positions: &[1, 4, 7],
            },
            Bet,
            Flip {
                board: 0,
                positions: &[2, 5, 8],
            },
            Bet,
            Declare,
        ],
        shares: vec![HIGH_OMAHA, LOW_OMAHA],
        declare: Some(DeclareKind::HiLo),
        layout: BoardLayout::Wheel,
        positions: 9,
        lines: ring(9),
        variants: vec![
            Variant {
                name: "eight_cards",
                apply: |s| {
                    s.positions = 8;
                    s.lines = (0..8u8)
                        .map(|i| vec![i, (i + 1) % 8, (i + 2) % 8])
                        .collect();
                    for st in s.steps.iter_mut() {
                        if let Flip { positions, .. } = st
                            && positions.contains(&8)
                        {
                            *positions = &[2, 5];
                        }
                    }
                },
            },
            Variant {
                name: "flip_one_at_a_time",
                apply: flip_singly,
            },
        ],
        ..base(
            "wheel_of_death",
            "Wheel of Death (hi/lo declare)",
            "Poker Bible §Wheel of Death",
        )
    }
}

/// Spit: 5 hole cards; everyone throws one face down into the middle, which is topped up to 9
/// cards and shuffled; the middle cards are turned one at a time with a betting round after each.
/// Hi/lo declare. Variation `three_at_a_time` (the Bible's).
/// - **D-SPIT-1** throws are chosen in turn and stay hidden; a player knows only that their own
///   card is somewhere in the middle.
/// - **D-SPIT-2** hands are 2 from the remaining 4 hole cards + any 3 middle cards.
/// - **D-SPIT-3** max 9 seats (the middle always has 9).
fn spit() -> Script {
    let mut steps = vec![Down(5), Spit, FillPool { total: 9 }, Bet];
    for _ in 0..9 {
        steps.extend([FlipPool { board: 0, n: 1 }, Bet]);
    }
    steps.push(Declare);
    Script {
        steps,
        shares: vec![HIGH_OMAHA, LOW_OMAHA],
        declare: Some(DeclareKind::HiLo),
        layout: BoardLayout::Middle,
        variants: vec![Variant {
            name: "three_at_a_time",
            apply: |s| {
                s.steps = vec![Down(5), Spit, FillPool { total: 9 }, Bet];
                for _ in 0..3 {
                    s.steps.extend([FlipPool { board: 0, n: 3 }, Bet]);
                }
                s.steps.push(Declare);
            },
        }],
        ..base("spit", "Spit (hi/lo declare)", "Poker Bible §Spit")
    }
}

const IE_SPLITS: &[(u8, u8)] = &[(2, 3), (3, 2)];

/// Industrial Elevator: 5 hole cards; a hand is 2 hole + 3 board or 3 hole + 2 board, with the
/// board cards from one line. Board: a column of 4 (positions 0-3, top→bottom) crossed at its
/// second card by a row (left 4, then 5, 6 to the right). High only.
/// - **D-IE-1** a usable board set is cards from one straight line (the column or the row).
/// - **D-IE-2** flips: the two on the right (flop), the one on the left (turn), then the column
///   top to bottom, one at a time, each with a betting round.
/// - Variation `five_in_elevator` (the Bible's): a column of 5.
fn industrial_elevator() -> Script {
    let mut steps = vec![
        Down(5),
        Bet,
        Flip {
            board: 0,
            positions: &[5, 6],
        },
        Bet,
        Flip {
            board: 0,
            positions: &[4],
        },
        Bet,
    ];
    for p in 0..4usize {
        steps.extend([
            Flip {
                board: 0,
                positions: &POS[p..p + 1],
            },
            Bet,
        ]);
    }
    Script {
        steps,
        shares: vec![ShareRule {
            build: Build::Combos(IE_SPLITS),
            ..HIGH_ANY
        }],
        layout: BoardLayout::Elevator,
        positions: 7,
        lines: vec![vec![0, 1, 2, 3], vec![4, 1, 5, 6]],
        variants: vec![Variant {
            name: "five_in_elevator",
            apply: |s| {
                s.positions = 8;
                s.lines = vec![vec![0, 1, 2, 3, 4], vec![5, 1, 6, 7]];
                s.steps = vec![
                    Down(5),
                    Bet,
                    Flip {
                        board: 0,
                        positions: &[6, 7],
                    },
                    Bet,
                    Flip {
                        board: 0,
                        positions: &[5],
                    },
                    Bet,
                ];
                for p in 0..5usize {
                    s.steps.extend([
                        Flip {
                            board: 0,
                            positions: &POS[p..p + 1],
                        },
                        Bet,
                    ]);
                }
            },
        }],
        ..base(
            "industrial_elevator",
            "Industrial Elevator",
            "Poker Bible §Industrial Elevator",
        )
    }
}

const IC_SPLITS: &[(u8, u8)] = &[(4, 1), (3, 2), (2, 3)];

/// Iron Cross: 4 hole cards and a $1 ante each; one betting round; then the cross (a row of 3
/// and a column of 3 sharing the centre: row 0-2, column top 3 and bottom 4) is turned and hands
/// are shown. High only.
/// - **D-IC-1** best 5 from the 4 hole cards + the row *or* the column.
/// - **D-IC-2** a single betting round (as written).
/// - **D-IC-3** variation `stun_orbit` (the Bible's): one orbit of hands; each hand's winner takes
///   the pot and the losers at showdown are stunned into the next hand's pot; on the orbit's last
///   hand the losers pay half a stun to the winner instead.
fn iron_cross() -> Script {
    Script {
        steps: vec![
            Down(4),
            Bet,
            Flip {
                board: 0,
                positions: &[0, 1, 2, 3, 4],
            },
        ],
        ante_each: Chips(100),
        shares: vec![ShareRule {
            build: Build::Combos(IC_SPLITS),
            ..HIGH_ANY
        }],
        layout: BoardLayout::Cross,
        positions: 5,
        lines: vec![vec![0, 1, 2], vec![3, 1, 4]],
        variants: vec![Variant {
            name: "stun_orbit",
            apply: |s| {
                s.length = GameLength::UntilDone { max_hands: 9 };
                s.resolution = Resolution::StunOrbit;
                s.orbit_stun = true;
            },
        }],
        ..base("iron_cross", "Iron Cross", "Poker Bible §Iron Cross")
    }
}

// ----- G6 Stud variants (special cards and choices) -------------------------------------------

/// The 7 Card Stud deal: 2 down, bet, 4 up with bets, 1 down, bet.
fn stud_steps() -> Vec<Step> {
    vec![
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
    ]
}

fn wild_ranks(ranks: &[engine::Rank]) -> CardSet {
    let mut w = CardSet::EMPTY;
    for &r in ranks {
        for su in engine::Suit::ALL {
            w.insert(engine::Card::new(r, su));
        }
    }
    w
}

fn buy_eighth(s: &mut Script) {
    let at = s
        .steps
        .iter()
        .position(|x| *x == Declare)
        .unwrap_or(s.steps.len());
    s.steps.splice(
        at..at,
        [
            BuyCard {
                up: Chips(500),
                down: Chips(700),
            },
            Bet,
        ],
    );
}

/// Baseball: 7 Card Stud with 3s and 6s wild; a face-up 4 earns an extra card. Hi/lo declare.
/// - **D-BB-1** the player chooses whether the extra card comes up or down; it is dealt after the
///   street's other cards. No "buy the 3" rule (it isn't in the Bible).
/// - **D-BB-2** max 6 seats so the deck can't run out (42 cards + at most 4 extras).
fn baseball() -> Script {
    let mut steps = stud_steps();
    steps.push(Declare);
    Script {
        steps,
        max_seats: 6,
        order: ActOrder::BestBoard,
        shares: vec![HIGH_OWN, LOW_OWN],
        declare: Some(DeclareKind::HiLo),
        wild: wild_ranks(&[engine::Rank::Three, engine::Rank::Six]),
        four_extra: true,
        boards: 0,
        ..base(
            "baseball",
            "Baseball (hi/lo declare)",
            "Poker Bible §Baseball",
        )
    }
}

/// Low in the Hole: each seat's lowest face-down card (ace high) is wild for that seat, and so
/// are its face-up cards of that rank. The 7th card comes up or down by the player's choice.
/// Hi/lo declare. **D-LH-1** the wild rank is recomputed from all face-down cards at showdown.
fn low_in_the_hole() -> Script {
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
            UpOrDown,
            Bet,
            Declare,
        ],
        max_seats: 7,
        order: ActOrder::BestBoard,
        shares: vec![HIGH_OWN, LOW_OWN],
        declare: Some(DeclareKind::HiLo),
        low_hole_wild: true,
        boards: 0,
        ..base(
            "low_in_the_hole",
            "Low in the Hole (hi/lo declare)",
            "Poker Bible §Low in the Hole",
        )
    }
}

/// Push: 7 Card Stud where, on 3rd to 6th street, a player dealt an up card may pay $1 to push it
/// to the next player; the receiver keeps it or pays $1 for the next card instead. Hi/lo declare
/// (D-ALL-3: the objective is high or low but the game play has no declaration). Max 6 seats.
/// - **D-PU-1** the pusher then receives a fresh card, which can't be pushed; a card is pushed at
///   most once.
/// - **D-PU-2** the last seat served on a street can't push (nobody is left waiting for a card).
/// - Replacing is offered only while the stock covers everyone's remaining cards.
/// - Variation `buy_eighth` (the Bible's): an 8th card for $5 up or $7 down.
fn push() -> Script {
    let mut steps = vec![Down(2), Bet];
    for _ in 0..4 {
        steps.extend([PushStreet, Bet]);
    }
    steps.extend([Down(1), Bet, Declare]);
    Script {
        steps,
        max_seats: 6,
        order: ActOrder::BestBoard,
        shares: vec![HIGH_OWN, LOW_OWN],
        declare: Some(DeclareKind::HiLo),
        hand_cards: 7,
        boards: 0,
        variants: vec![Variant {
            name: "buy_eighth",
            apply: buy_eighth,
        }],
        ..base("push", "Push (hi/lo declare)", "Poker Bible §Push")
    }
}

/// Bullet: jokers in the deck; 2 cards down, then rounds of "take a card?" with betting after each
/// (best board first) until everyone has 5 cards. A face-up joker kills the hand; a face-down joker
/// is wild. Hi/lo declare.
/// - **D-BU-1** extra cards come face up except the 5th, which comes face down.
/// - **D-BU-2** a player who declines stays in and is offered again next round.
/// - **D-BU-3** at most 6 rounds; after that short hands are filled (up, last down).
fn bullet() -> Script {
    Script {
        steps: vec![Down(2), Bet, TakeRounds { target: 5, cap: 6 }, Declare],
        max_seats: 8,
        deck: DeckKind::WithJokers,
        order: ActOrder::BestBoard,
        shares: vec![HIGH_OWN, LOW_OWN],
        declare: Some(DeclareKind::HiLo),
        wild: CardSet::JOKERS,
        up_joker_kills: true,
        boards: 0,
        ..base("bullet", "Bullet (hi/lo declare)", "Poker Bible §Bullet")
    }
}

/// Bid: 2 down, bet; then five rounds of: one card per player still in turned face up in the
/// middle, sealed bids for picking order (paid into the pot), picks in bid order, bet. Hi/lo
/// declare. Max 7 seats.
/// - **D-BID-1** bids are $0 to $5 in 50¢ units, placed in turn but hidden until all are in;
///   ties pick in order from the dealer's left.
/// - Variation `buy_eighth` (the Bible's).
fn bid() -> Script {
    let mut steps = vec![Down(2), Bet];
    for _ in 0..5 {
        steps.extend([BidRound { max_units: 10 }, Bet]);
    }
    steps.push(Declare);
    Script {
        steps,
        max_seats: 7,
        order: ActOrder::BestBoard,
        shares: vec![HIGH_OWN, LOW_OWN],
        declare: Some(DeclareKind::HiLo),
        boards: 0,
        variants: vec![Variant {
            name: "buy_eighth",
            apply: buy_eighth,
        }],
        ..base("bid", "Bid (hi/lo declare)", "Poker Bible §Bid")
    }
}

// ----- G8 Multi-hand & stuns ------------------------------------------------------------------
//
// These games last several hands (`UntilDone`): the pot (and legs, strikes, who is out) carry
// from hand to hand; the button moves each hand. Stuns follow OQ-9 (the pot, capped at $10, +$N
// per further stunned player in the hand; N = `Config::stun_step`). At the hand cap the carried
// pot is split evenly (D-G0-1). Side pots are not formed in carried pots: whoever wins the pot
// takes all of it.

/// Guts Leg: 5 Card Draw (draw up to 3, or 4 keeping a shown Ace) repeated; the hand winner earns
/// a leg; the first to 2 legs takes the whole pot. If every player in the game has a leg, the
/// counts reset.
/// - **D-GL-1** the pot carries over and each hand's betting adds to it.
/// - **D-GL-2** a hand won by everyone else folding still earns a leg.
/// - **D-GL-3** tied winners each earn a leg; tied finishers split the pot.
/// - Ace bonus as D-3BM-1. Variation `out_is_out` (the Bible's).
fn guts_leg() -> Script {
    Script {
        steps: vec![
            Down(5),
            Bet,
            Draw {
                max: 3,
                ace_bonus: true,
            },
            Bet,
        ],
        max_seats: 7,
        shares: vec![HIGH_OWN],
        boards: 0,
        length: GameLength::UntilDone { max_hands: 30 },
        resolution: Resolution::Legs { need: 2 },
        variants: vec![Variant {
            name: "out_is_out",
            apply: |s| s.out_is_out = true,
        }],
        ..base("guts_leg", "Guts Leg", "Poker Bible §Guts Leg")
    }
}

/// 3-2-1 Drop: 3-card hands (no straights or flushes), red kings wild; everyone holds or drops at
/// once. A lone player plays the pot's dummy hand: beating it takes the pot (and ends the game);
/// otherwise they are stunned. With 2+ players in, the best hand takes the pot and every other
/// player who stayed in is stunned into the next hand's pot.
/// - **D-DROP-1/2** as above; stun per OQ-9.
/// - **D-DROP-3** the game ends when a hand leaves the pot empty.
/// - **D-DROP-4** $1 ante from everyone on the first hand only; the dummy hand is dealt only when
///   it's needed (nobody sees it otherwise, so this changes nothing).
/// - **D-DROP-5** variation `stun_orbit` (the Bible's): the game lasts one orbit; on its last hand
///   losers pay half a stun to the winner; a pot nobody wins on that hand is split evenly.
fn drop_321() -> Script {
    let mut red_kings = CardSet::EMPTY;
    red_kings.insert("Kd".parse().unwrap());
    red_kings.insert("Kh".parse().unwrap());
    Script {
        steps: vec![Down(3), InOut { price: Chips::ZERO }, DummyIfLone { n: 3 }],
        ante_each: Chips(100),
        ante_first_hand_only: true,
        max_seats: 9,
        shares: vec![ShareRule {
            name: "High",
            ranking: Ranking::ThreeCard,
            build: Build::Own,
            board: 0,
        }],
        wild: red_kings,
        length: GameLength::UntilDone { max_hands: 30 },
        resolution: Resolution::Drop,
        variants: vec![Variant {
            name: "stun_orbit",
            apply: |s| s.orbit_stun = true,
        }],
        ..base("drop_321", "3-2-1 Drop", "Poker Bible §3-2-1 Drop")
    }
}

/// 3-5-7: one hand = one 3-5-7 cycle. Everyone antes $1 (D-357-1, owner), gets 3 cards (3s wild,
/// 3-card ranking), and secretly plays or drops; then 2 more (5 cards, 5s wild), play or drop;
/// then 2 more (best 5 of 7, 7s wild), play or drop. A lone player gets a strike; with 2+ playing,
/// the losers are stunned into the pot. 3 strikes takes the pot and ends the game.
/// - **D-357-2** hands that play a round are shown to the table.
/// - **D-357-3** dropping a round doesn't leave the cycle.
fn three_five_seven() -> Script {
    use engine::Rank;
    Script {
        steps: vec![
            Down(3),
            PlayRound {
                wild: Rank::Three,
                three: true,
            },
            Down(2),
            PlayRound {
                wild: Rank::Five,
                three: false,
            },
            Down(2),
            PlayRound {
                wild: Rank::Seven,
                three: false,
            },
        ],
        ante_each: Chips(100),
        max_seats: 7,
        shares: vec![HIGH_OWN],
        boards: 0,
        length: GameLength::UntilDone { max_hands: 40 },
        resolution: Resolution::Strikes { need: 3 },
        ..base("three_five_seven", "3-5-7", "Poker Bible §3-5-7")
    }
}

/// Scare-a-moosh: 5 cards; from the dealer's left each player may name the game (high poker, low
/// poker, high numbers, low numbers) and a price up to $2, or pass it on. Then In/Out (the
/// declarer is in; In pays the price), one draw, one betting round, showdown. The declarer must
/// win outright or is stunned; the pot carries until the declarer wins. Out is out for the rest
/// of the game.
/// - **D-SAM-1** everyone who's in pays the price into the pot.
/// - **D-SAM-2** if all pass, redeal (counts toward the hand cap).
/// - **D-SAM-3** the draw allows up to 5 cards (as 5 Card Draw).
/// - **D-SAM-4** "numbers" are blackjack values (2-10, pictures ½, aces 1 or 11): high numbers
///   = highest total, low numbers = lowest.
/// - Variation `stay_in` (the Bible's "No, if you're out you're out"): Out only for the hand.
fn scare_a_moosh() -> Script {
    Script {
        steps: vec![
            Down(5),
            NameGame { max_units: 4 },
            InOut { price: Chips::ZERO },
            Draw {
                max: 5,
                ace_bonus: false,
            },
            Bet,
        ],
        max_seats: 7,
        shares: vec![HIGH_OWN],
        boards: 0,
        length: GameLength::UntilDone { max_hands: 30 },
        resolution: Resolution::Declarer,
        out_is_out: true,
        variants: vec![Variant {
            name: "stay_in",
            apply: |s| s.out_is_out = false,
        }],
        ..base(
            "scare_a_moosh",
            "Scare-a-moosh",
            "Poker Bible §Scare-a-moosh",
        )
    }
}

/// Tempest Hold'em: Hold'em where everyone antes, the seat left of the dealer posts a $2 Giant
/// Blind, and the only options are all-in or fold, capped at 10 Giant Blinds ($20) per player per
/// hand. Hands repeat (button moving) until one reaches showdown.
/// - **D-TEM-1 (owner)** ante starts at 50¢ and rises 50¢ each hand until a hand reaches
///   showdown, which ends the game.
fn tempest() -> Script {
    Script {
        steps: holdem_steps(),
        length: GameLength::UntilDone { max_hands: 30 },
        resolution: Resolution::Tempest,
        ..base(
            "holdem_tempest",
            "Tempest Hold'em",
            "Poker Bible §Tempest Hold'em",
        )
    }
}

// ----- G9 Numbers -----------------------------------------------------------------------------

/// 7/27 and 5.5/21.5: card values 2-10 face value, pictures ½, aces 1 or 11. One card down and
/// one up; a betting round; then rounds of hit or stand (hits face up) with a betting round after
/// each, until a round in which everyone stands; then declare (low target, high target, both).
/// Targets are in half points (7 = 14, 27 = 54, 5.5 = 11, 21.5 = 43).
/// - **D-727-1** closest *without going over* wins each target; a bust on one target can still win
///   the other.
/// - **D-727-2** "Both" follows the house Both rule.
/// - **D-727-3** at most 10 hit rounds.
/// - Variation `bluff_bust` (the Bible's): a busted hand can still win a target nobody else
///   declared.
fn targets(
    id: &'static str,
    name: &'static str,
    rules_ref: &'static str,
    low: u16,
    high: u16,
    names: [&'static str; 3],
) -> Script {
    let share = |nm: &'static str, half: u16| ShareRule {
        name: nm,
        ranking: Ranking::Target {
            half,
            bust_ok: false,
        },
        build: Build::Own,
        board: 0,
    };
    Script {
        steps: vec![Down(1), Up(1), Bet, HitRounds { cap: 10 }, Declare],
        max_seats: 7,
        shares: vec![share(names[0], low), share(names[1], high)],
        declare: Some(DeclareKind::Custom(names)),
        boards: 0,
        variants: vec![Variant {
            name: "bluff_bust",
            apply: |s| {
                for sh in s.shares.iter_mut() {
                    if let Ranking::Target { bust_ok, .. } = &mut sh.ranking {
                        *bust_ok = true;
                    }
                }
            },
        }],
        ..base(id, name, rules_ref)
    }
}

/// Numbers: 6 cards each and 4 middle cards turned one at a time, with a betting round before
/// the first and after each. Each turned card makes every player throw away (face up) its cards
/// of that rank and the ranks next to it; an empty hand ("gin") scoops the pot at once. Then
/// declare High / Low / Both numbers (card totals: aces 11 for high, 1 for low).
/// - **D-NUM-1** a turned rank removes that rank and its neighbours (ace is next to 2 and King).
/// - **D-NUM-2** a turned card of a rank already turned is replaced.
/// - **D-NUM-3** several gins on one card split the pot.
/// - Variations `five_cards` and `seven_cards` (the Bible's "5-7 cards").
fn numbers() -> Script {
    let mut steps = vec![Down(6), Bet];
    for _ in 0..4 {
        steps.extend([NumbersFlip, Bet]);
    }
    steps.push(Declare);
    Script {
        steps,
        max_seats: 7,
        shares: vec![
            ShareRule {
                name: "High numbers",
                ranking: Ranking::NumbersHigh,
                build: Build::Own,
                board: 0,
            },
            ShareRule {
                name: "Low numbers",
                ranking: Ranking::NumbersLow,
                build: Build::Own,
                board: 0,
            },
        ],
        declare: Some(DeclareKind::HiLo),
        layout: BoardLayout::Middle,
        variants: vec![
            Variant {
                name: "five_cards",
                apply: |s| s.steps[0] = Down(5),
            },
            Variant {
                name: "seven_cards",
                apply: |s| s.steps[0] = Down(7),
            },
        ],
        ..base("numbers", "Numbers (hi/lo declare)", "Poker Bible §Numbers")
    }
}

// ----- G10 Trick-taking -----------------------------------------------------------------------

/// Whooh Ray for Booray: 5 cards (the dealer's last one face up: its suit is trump); In or Out;
/// one draw; then 5 tricks led by the dealer (follow suit; highest trump, else highest of the led
/// suit, ace high, wins). 3 tricks scoop the pot; a player who played and took no trick "fries"
/// (a stun); every other player who played pays $1. The game repeats (button moving) until someone
/// scoops and nobody fries.
/// - **D-BOO-1** without a scoop the pot carries and grows by the stuns and $1 payments.
/// - **D-BOO-2** trump can't be led until one has been played (unless a player holds only trumps).
/// - **D-BOO-3** everyone antes $1 on the first hand; when the game ends, the $1 payments are not
///   made (the pot isn't played again).
/// - **D-BOO-4** the draw allows up to 4 of the face-down cards (the trump card stays).
/// - **D-BOO-5** if only one player is In, they take every trick (scoop).
/// - Variation `out_is_out` (the Bible's).
fn booray() -> Script {
    Script {
        steps: vec![
            Down(4),
            DealerUp,
            InOut { price: Chips::ZERO },
            Draw {
                max: 4,
                ace_bonus: false,
            },
            Tricks { n: 5 },
        ],
        ante_each: Chips(100),
        ante_first_hand_only: true,
        max_seats: 7,
        boards: 0,
        length: GameLength::UntilDone { max_hands: 30 },
        resolution: Resolution::Booray,
        variants: vec![Variant {
            name: "out_is_out",
            apply: |s| s.out_is_out = true,
        }],
        ..base(
            "booray",
            "Whooh Ray for Booray",
            "Poker Bible §Whooh Ray for Booray",
        )
    }
}
