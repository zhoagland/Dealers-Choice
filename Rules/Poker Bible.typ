//Macros
#let spade_icon = text(black, [#emoji.suit.spade])
#let heart_icon = text(red, [#emoji.suit.heart])
#let diamond_icon = text(red, [#emoji.suit.diamond])
#let club_icon = text(black, [#emoji.suit.club])

//Page Setup
#set page(paper: "us-letter", header:[
    #set text(12pt)
    ], numbering: "1",)

#set text(font: "Libertinus Serif", size: 12pt,)

//Header Page

#v(40%)
#align(center, 
text(size: 25pt, 
spade_icon + heart_icon +  box[*Dealers Choice*] + diamond_icon + club_icon + box[*Games, Rules, and Definitions*]))
#line(length: 100%)

#pagebreak()

//Table of contents
#show outline.entry.where(level: 1): it => {
  v(12pt, weak: true)
  strong(it)
}
#outline(indent: 2em)
#pagebreak()

// Heading Rules
#show heading.where(level: 1): heading_obj => [
  #set align(center)
  #heading_obj.body
]


//Document
= The History of the Games
#align(center,image("the_ogs.jpg"))
A long time ago, when the men were just boys, they spent many nights moving money between each other.
Lucky for us they passed down some of the games. It is our job to document them and secure them in stone. The following is a collection of games we were taught, we have found, and we created.
The many nights of fun we have had are owed to them. Please enjoy the games just as much as we have; and remember, *bet it like you got it*.

// Definitions
#pagebreak()
= Definitions
== Beginner Terms / Must Knows
/ Action (n.): Term used to describe a players turn. "Joe, action on you." Meaning its Joe's turn to go.
/ Barrel (n.): A bet on a postflop street.
/ Bad Beat (n.): When a really good hand loses to an even better hand. Typically when 4 of a kind lose to a bigger 4 of a kind.
/ Brick (n.): A turn or river card that is expected to have no impact on the outcome of the hand.
/ Call (v.): To match an opponent's bet.
/ Cards Speak for themselves: Despite what you believe to have, something may have been overlooked so by showing your hand face up the table will verify the winning hands.
/ Check (v.): To defer your right to bet for the moment. 
/ Checked around (adj.): When all players check on a postflop street.
/ Check-raise (v.): To check postflop and then raise a bet from another player.
/ Coin flip (n.): When two hands are all in together preflop with similar equity. For example, a pocket pair against two overcards.
/ Cold-call (v.): To call a single preflop raise.
/ Cold-calling range (n.): The range of hands with which you call a single preflop raise.
/ Combination (n.): A specific set of hole cards. A K is not a combination. A#diamond_icon\6#club_icon is.
/ Continuation bet (n.): A bet made by the player who made the last bet or raise. This term is often abbreviated "c-bet." 
/ Declaration (v.): When you disclose which pot you are playing for. high, low, or both.
/ Deuces (n.): A pair of twos.
/ Double up (v.): When you go all in and win enough chips to double your stack.
/ Draw (n. and v.): The hand you're trying to make. For example, if you are dealt a two and a three, and the flop includes a four and a five-you're drawing a straight.
/ Dry board (n.): When the table's shared cards do not present many opportunities for drawing hands. For example, a "rainbow" flop with three different suits, which does not open up the possibility of a flush draw.
/ Fire a barrel (v.): To bet. 
/ Fire multiple barrels (v.): To bet on multiple streets. 
/ Fire three barrels (v.): Making a bet on the flop, turn, and river. 
/ Five Card Draw (n.): A style of poker where a player receives 5 cards at the beginning of the game. Has the option to dump cards and draw new ones. Each game has a different ruling as to how many the player can dump. The player must use all 5 cards from their hand. Best 5 cards win.
/ Flop (n.): The first three shared cards that are dealt. 
/ Game theory optimal (gto) poker (n.): A defensive playing strategy that aims to make you unexploitable to your opponents. 
/ Get run over (v.): To be aggressively pushed out of many pots. 
/ Gutshot draw (n.): When you have four out of the five cards needed for a particular draw, but are missing one. 
/ Heads-up pot (n.): A pot with two players. 
/ Hero call(n.): When a player has a relatively weak hand but suspects that their opponent is bluffing, so they decide to call that opponent's bet on the final round of betting. 
/ Hole cards (n.): The two cards you're dealt. These are also described with the term "pocket." For example, if your hole cards are , you have "pocket aces." 
/ High Poker (n.): Best 5 cards.See table below for what the best possible high poker hands are.
/ High, Low, Both (H,L,B) : If the game calls for multiple declarations, a player uses chips hidden in their hand to declare which pot/way they are playing their hand for (ie. High/Low poker or High/Low Number, or 7/27). If applicable a player can go both ways, the player can treat each way as a separate hand. Meaning if it is an omaha style game any combination of the 4 cards in their hand can be used. The player must win both hands outright, any chopped pots is deemed a loss and the other way is forfeited. If no one went the other way the whole pot goes to the challenger of the "both ways".
/ If you're out you're out: Meaning if the player folds, they can't play future hands for that game.
/ Initiative (n.): The player who made the last bet or raise is said to have the initiative. 
/ In position (adj.): When you act after your opponent. 
/ Lead into (v.): In a heads-up pot, to make the first bet on a postflop street. Implies that your bet is directed at an opponent. 
/ Lead out (v.): To make the first bet on a postflop street. 
/ Limp in (v.): To enter the pot before the flop by simply matching the big blind's bet, rather than raising. 
/ Low-card wet board (n.): Boards with many possible draws, but ones that don't involve high cards, such as 4#heart_icon\3#heart_icon\2#heart_icon, 6#spade_icon\7#spade_icon\9#club_icon, and 5#diamond_icon\6#club_icon\2#spade_icon.
/ Low Poker (n.): Worst 5 cards. See table below for what the best possible low poker hands are.
/ Maniac (n.): A player who plays too many hands in an overly aggressive fashion, mostly without a plan. 
/ Min-raise (n.): A raise that is double the previous bet. Preflop this is twice the big blind. 
/ Multi-way pot (n.): A pot with three or more players. 
/ The nuts (n.): The absolute best hand possible. 
/ Nutted hand (n.): A hand that almost always rates to be the best. You can also describe a player holding such a hand as "nutted." 
/ Omaha (n.): A style of poker where a player receives 4 cards at the beginning of the game and must use 2 and only 2 from their hand. 5 community cards will be shown in the style of flop (3), turn (1), river (1). Typically a high poker game.
/ Open limp (n.): When the first player enters the pot by choice. This doesn't include the blinds, as those are forced bets. Used interchangeably with "open raise." 
/ Out of position (adj.): When you act before your opponent. 
/ Outs (n.): Cards you need to complete your draw. 
/ Overbet (n.): A bet exceeding the size of the pot. 
/ Overcards (n.): Cards that rank higher than any card on the board. 
/ Overpair (n.): A pair that ranks higher than any possible pair on the board. 
/ Pay off (v.): To call a big bet. 
/ Peel (v.): To call a bet or raise so you can see one more card on the board. 
/ Potential (n.): The perceived strength of your hole cards.
/ The river (n.): The fifth shared card. 
/ Set (n.): Three of a kind when the player is holding a pocket pair. 
/ Seven Card Stud (n.): A style of poker where a player receives 2 cards face down at the beginning of the game. By the end the player will have a total of 7 cards. The remaining 5 are dealt face up. The player must use 5 cards from their hand. Best 5 cards win.
/ Shove (v.): To bet all of your remaining chips. 
/ Show down (v.): To physically reveal your cards to your opponents. 
/ Showdown (n.): The moment after the final round of betting when the last players left in a hand reveal their cards to determine the winner(s). 
/ Slow play (v.): To refrain from betting or raising in the hope that your opponent will do the betting for you. Also, to give your opponent a chance to make a better hand in order to extract more value. 
/ Small ball (adj.): A style of poker that minimizes risk by consciously keeping the pot small unless the player has a strong hand. 
/ Speculative hand (n.): Hands such as suited connectors and small pocket pairs. They don't make a good hand very often, but when they do it's likely to be the best hand. 
/ Straddle (n.): A voluntary bet made after the blinds have been posted but before the hole cards are dealt. 
/ Street (n.): A segment of play. Any card that is dealt or any betting round. 
/ Stun (n.): A consequence requiring the player to pay an amount of money. The amount matches the pot up to \$10. This value may change during play.
/ Three-bet (v.): To reraise another player's open raise preflop, or to reraise over the first raise on a postflop street. 
/ Tilt (n. and v.): A poor mindset that leads a player to make costly mistakes. 
/ Trap (v.): To refrain from betting or raising in the hope that your opponent will do the betting for you. Also, to give your opponent a chance to make a better hand in order to extract more value. Can be used interchangeably with "slow play." 
/ Treys (n.): A pair of threes. 
/ Trips (n.): Three of a kind when the board is paired and the player holds one card of the same value. 
/ The turn (n.): The fourth shared card. 
/ Turn your hand faceup (v.): When you let other players know almost for certain what you have. You don't literally flip your cards over, but you may as well have done so. 
/ Two and only Two (2o2) (adj.): The player can use any combination of 2 cards from their hand and must use 2 cards from their hand.
/ Wet board (n.): A coordinated board with multiple draws possible. This term can apply to the board at any stage, regardless of how many cards are down. 
/ Wet flop (n.): A flop with many possible draws available. 

== Advanced Terms
/ Balanced range (n.): An unexploitable range that includes both strong hands and weak hands with potential. Even if your opponents know what your range is, they won't be able to exploit it if it's perfectly balanced.
/ Barreling frequency (n.): The frequency at which you bet the turn and follow up on the river.
/ Blocking bet (n.): A small bet designed to stop your opponent from making a large bet that is difficult to call.
/ Board texture (n.): The characteristics of the flop-whether wet, with many draws and nutted hands possible, or dry, without draws and no hand better than a set possible.
/ Capped range (n.): A range that doesn't include many of the strongest hands possible. 
/ Checking range (n.): The range of hands with which a player checks. 
/ Connect with the board (v.): How well your hand hit a particular board. This is measurable in terms of equity. 
/ Continuing range (n.): The range of hands with which you call or raise a bet from another player. 
/ Edge (n.): When you have more equity than your opponent.
/ Fold equity (n.): The equity you gain when your opponent folds. The more frequently your opponent will fold, the more fold equity you have in proportion to the pot size. 
/ Hand range (n.): All the hands a player has in a specific situation. This changes as the hand progresses. 
/ High-equity draw (n.): A draw that has many outs. 
/ Lag (n.): Loose aggressive player. They play a wide range of hands in an effective manner. 
/ Leak (n.): A habit that loses you money. 
/ Loose player (n.): A player who plays a wide range preflop. 
/ Loose range (n): A range of hands containing many combinations. Used interchangeably with "wide range." 
/ Metagame (n.): The integration of your playing history with your opponents into your decision-making process. 
/ Nitty (adj.): A conservative, tight style of play. 
/ Polarized (adj.): When a range consists of only strong hands and very weak hands. The more polarized a range, the stronger the value hands and the weaker the bluffs. 
/ Range advantage (n.): An advantage in which your range of hands compares favorably to another range. 
/ Sticky player (n.): An opponent who usually doesn't fold marginal hands in the hope of completing their draw. 
/ Tag (n.): Tight aggressive player. They play around 20 percent of hands and open raise around 17 percent. This type of player is likely a professional. 
/ Tight range (n.): A range of hands containing few combinations. 
/ Value bet (n.): A bet where you expect to have more equity than your opponent. 
/ Value hand (n.): A hand that you can bet with and expect to be called by worse hands frequently enough to show a profit. 
/ Whale (n.): A big fish who plays too many hands in a passive manner without any skill. They are always likely to call big bets with weak hands. 
/ Wide range (n.): A range of hands containing many combinations. Used interchangeably with "loose range." 
/ Winning player (n.): Any profitable player. 

// Hand Rankings
#pagebreak()
= Hand Rankings
== High Poker
The table below shows the ranking of all possible high poker hands as well as the best version of each.
#table(
  align: center, 
  columns: (1fr, auto, auto, 1fr, 1.5fr, 1.5fr),
  rows: 6,
  stroke: .5pt,
  table.header( [Ranking], [Example], [Name], [Ranking], [Example], [Name]),
  [1],   [A#spade_icon\K#spade_icon\Q#spade_icon\J#spade_icon\T#spade_icon],  [Royal Flush],    [6], [A#spade_icon\K#spade_icon\Q#heart_icon\J#club_icon\T#diamond_icon], [Straight],
  [1.5], [A#spade_icon\A#club_icon\A#diamond_icon\A#heart_icon + Wild],       [5 of a kind],    [7], [A#spade_icon\A#club_icon\A#diamond_icon],                           [3 of a Kind],
  [2],   [\K#spade_icon\Q#spade_icon\J#spade_icon\T#spade_icon\9#spade_icon], [Straight Flush], [8], [A#spade_icon\A#club_icon\K#spade_icon\K#club_icon], [2 Pair],
  [3],   [A#spade_icon\A#club_icon\A#diamond_icon\A#heart_icon],              [4 of a kind],    [9], [A#spade_icon\A#club_icon], [1 Pair],
  [4],   [A#spade_icon\A#club_icon\A#diamond_icon\K#spade_icon\K#club_icon],  [Full House],     [10], [A#spade_icon], [High Card],
  [5],   [A#spade_icon\K#spade_icon\Q#spade_icon\J#spade_icon\9#spade_icon], [Flush], table.cell(colspan: 3, ["#spade_icon" is used as an example. No suit is better than another. ])
)

== Low Poker 
The table below shows the ranking of all possible high poker hands as well as the best version of each. The best way to figure out the strength of your hand is to sort in descending order. Then compare each number off. The player who has the lowest number in each position wins. 

#table(
  align: center,
  columns: (auto, auto),
  rows: 6,
  stroke:.5pt,
  table.header([Ranking], [Example]),
  [1],   [A#spade_icon\2#spade_icon\3#spade_icon\4#spade_icon\6#diamond_icon #sym.arrow.r 64321], 
  [2],   [A#spade_icon\2#spade_icon\3#spade_icon\5#spade_icon\6#diamond_icon #sym.arrow.r 65321],
  [3],   [A#spade_icon\2#spade_icon\4#spade_icon\5#spade_icon\6#diamond_icon #sym.arrow.r 65421],             
  [4],   [A#spade_icon\3#spade_icon\4#spade_icon\5#spade_icon\6#diamond_icon #sym.arrow.r 65431], 
  [5],   [A#spade_icon\2#spade_icon\3#spade_icon\4#spade_icon\7#diamond_icon #sym.arrow.r 74321], 
)

Note: 65432 was skipped because that hand contains a straight.

// Money
#pagebreak()
= Money
== Betting
=== Blinds or Antes
Before any cards are dealt, players might need to toss some chips into the pot. This is called the ante or blinds, depending on the game. It ensures there's something worth fighting over right off the bat.

=== Betting Rounds
There are multiple betting rounds depending on the game. The betting round is when players go around the tale and take action.

=== Actions you can take
Check: If no one's bet yet, you can stay in without wagering more. \
Bet: If you're feeling bold, you can throw some chips into the pot. \
Call: Match the current bet if someone's already made one. \
Raise: Up the ante by increasing the bet. \
Fold: If you reckon your hand ain't worth it, you can toss your cards and sit this one out.

=== Showdown
If there's still more than one player left after all the betting's done, it's time for the showdown. Players reveal their hands, and the best one takes the pot. If everyone else folds before the showdown, the last player standing wins without having to show their cards.

== Dealer Ante
The dealer pays an ante of \$2.00 at the beginning of the game of their choice. This is a fee for choosing the game and it juices the pot.

== Chip Values
Each chip represents a real portion of your buy in amount. No scaling is involved.
We use the following designations with the listed starting amount to reach our \$100.00 buy in.

#text(blue, [Blue - \$0.50]) - 20 Chips

#text(red, [Red - \$1.00]) - 20 Chips

White - \$5.00 - 14 Chips


// Omaha
#pagebreak()
= Omaha Variants
#pagebreak()
== Pot Limit Omaha
#v(.15in)
*Overview:* Omaha is similar to Texas Hold'em in some ways, but the players receive four hole cards, and the hand must consist of exactly two cards from the player's hole cards and exactly three cards from the board. 
#v(.15in)
*Objective:* Best high poker hand.
#v(.15in)
*Hand Construction:* 2o2 cards from the players hand plus 3 community cards.
#v(.15in)
*Setup:* Players are dealt 4 cards.
#v(.15in)
*Game play:*
+ Round of betting on potential.
+ Dealer burns a card and deals the flop.
+ Round of action.
+ Dealer burns a card and flips the turn card.
+ Round of action.
+ Dealer burns a card and flips the river card.
+ Round of action.
+ Showdown.



#pagebreak()
== Red River
#v(.15in)
*Overview:* This game is similar to traditional PLO but with the variation that if the river card is red (#diamond_icon or #heart_icon) you keep playing.
#v(.15in)
*Objective:* Best high or best low poker hand.
#v(.15in)
*Hand Construction:* 2o2 cards from the players hand plus 3 community cards.
#v(.15in)
*Setup:* Players are dealt 4 cards.
#v(.15in)
*Game play:*
+ Round of betting on potential.
+ Dealer deals the flop (no burn card).
+ Round of action.
+ Dealer deals the turn (no burn card).
+ Round of action.
+ Dealer deals the river (no burn card).
+ Round of action.
+ Dealer deals the river+1 (no burn card).
+ Round of action.
+ If river+1 was red (#diamond_icon or #heart_icon) and card is flipped and another round of action is added. This continues until river+N is a black (#club_icon or #spade_icon).
+ Declaration.
+ Showdown.



#pagebreak()
== PLO Double Board Bomb Pot
#v(.15in)
*Overview:* This game is similar to traditional PLO but with the variation that two boards are played at the same time. 
#v(.15in)
*Objective:* Best high or best low poker hand.
#v(.15in)
*Hand Construction:* 2o2 cards from the players hand plus 3 community cards.
#v(.15in)
*Setup:* Players are dealt 4 cards.
#v(.15in)
*Game play:*
+ Round of betting on potential.
+ Dealer deals the flop (no burn card) for the top and bottom board.
+ Round of action.
+ Dealer deals the turn (no burn card) for the top and bottom board.
+ Round of action.
+ Dealer deals the river (no burn card) for the top and bottom board.
+ Round of action.
+ Declaration (Top or Bottom Board).
+ Showdown.
#v(.15in)
*Variations:*
+ Dealer can turn flop, turn, and river for each board separately to increase the rounds of betting
+ Players dump two cards before showdown and declaration



#pagebreak()
== Elevator
#v(.1in)
*Overview:* This game is similar to traditional PLO but with the variation that more community cards are available in the shape of an H. 
#v(.1in)
*Objective:* Best high or best low poker hand.
#v(.1in)
*Hand Construction:* 2o2 cards from the players hand plus 3 community cards.
#v(.1in)
*Setup:* Players are dealt 4 cards.
The community cards are placed in the shape of an "H" face down. See below.
#align(center, 
  table(
    align: center,
    column-gutter: 0pt,
    row-gutter: 5pt,
    columns: (auto, auto, auto ),
    stroke: 0pt,
    rows: 3,
    [#text(red,size:25pt,[#emoji.playingcard.joker])],[],[#text(black,size:25pt,[#emoji.playingcard.joker])],
    [#text(black,size:25pt,[#emoji.playingcard.joker])],[#text(blue,size: 25pt,[#emoji.playingcard.joker])],[#text(red,size:25pt,[#emoji.playingcard.joker])],
    [#text(red,size:25pt,[#emoji.playingcard.joker])],[],[#text(black,size:25pt,[#emoji.playingcard.joker])],
  )
)
#v(.1in)
*Game play:*
+ Round of betting on potential.
+ 3 cards are flipped. Either the red or black set in the diagram.
+ Round of action.
+ The other set of 3 cards are flipped
+ Round of action.
+ The blue card is flipped
+ Round of action.
+ Declaration.
+ Showdown.
Hands are constructed using 2 cards from the players hand and 3 cards, in a row, from the community cards. See examples below. Blue indicates those are the cards the player is using. If going both ways any example can be used for either hand doesn't have to be the same 3 cards.
#align(center,
  table(
    align: center,
    column-gutter: 0pt,
    columns: (1fr,1fr,1fr,  1fr,1fr,1fr,   1fr,1fr,1fr),
    rows: 4,
    stroke: none,
    table.cell(colspan: 3, [Example 1]), table.cell(colspan: 3, [Example 2]), table.cell(colspan: 3, [Example 3]),
    [#text(black,size:25pt,[#emoji.playingcard.joker])],[],[#text(black,size:25pt,[#emoji.playingcard.joker])], [#text(blue,size:25pt,[#emoji.playingcard.joker])],[#text(blue,size:25pt,[#emoji.playingcard.joker])],[#text(blue,size:25pt,[#emoji.playingcard.joker])],[#text(black,size:25pt,[#emoji.playingcard.joker])],[],[#text(black,size:25pt,[#emoji.playingcard.joker])],
    [#text(blue,size:25pt,[#emoji.playingcard.joker])],[#text(blue,size: 25pt,[#emoji.playingcard.joker])],[#text(blue,size:25pt,[#emoji.playingcard.joker])], [#text(black,size:25pt,[#emoji.playingcard.joker])],[],[#text(black,size:25pt,[#emoji.playingcard.joker])], [#text(black,size:25pt,[#emoji.playingcard.joker])],[],[#text(black,size:25pt,[#emoji.playingcard.joker])],
    [#text(black,size:25pt,[#emoji.playingcard.joker])],[],[#text(black,size:25pt,[#emoji.playingcard.joker])], [#text(black,size:25pt,[#emoji.playingcard.joker])],[],[#text(black,size:25pt,[#emoji.playingcard.joker])],[#text(blue,size:25pt,[#emoji.playingcard.joker])],[#text(blue,size:25pt,[#emoji.playingcard.joker])],[#text(blue,size:25pt,[#emoji.playingcard.joker])]
  )
)
#v(.1in)
*Variations:*
+ Cards can be turned 2 at a time to increase the amount of betting rounds.



#pagebreak()
== Spit
#v(.15in)
*Overview:* This game is similar to traditional PLO but with the variation that the community cards consist of discards from the players and some random cards. 
#v(.15in)
*Objective:* Best high or best low poker hand.
#v(.15in)
*Hand Construction:* 2o2 cards from the players hand plus 3 community cards.#v(.15in)
*Setup:* Players are dealt 5 cards.
#v(.15in)
*Game play:*
+ Players discard 1 card into the cauldron of mystery to be used as a community card.
+ Total community cards sum to 9. Therefore 9 - \# of players = amount of additional cards added
+ Round of betting on potential
+ The 1st card from the middle is flipped
+ Round of betting
+ The 2nd card from the middle is flipped
+ Round of betting
+ The 3rd card from the middle is flipped
+ Round of betting
+ The 4th card from the middle is flipped
+ Round of betting
+ The 5th card from the middle is flipped
+ Round of betting
+ The 6th card from the middle is flipped
+ Round of betting
+ The 7th card from the middle is flipped
+ Round of betting
+ The 8th card from the middle is flipped
+ Round of betting
+ The 9th card from the middle is flipped
+ Round of betting
+ Declaration
+ Showdown
#v(.15in)
*Variations:*
+ Cards can be flipped 3 at a time to reduce the amount of betting rounds.



#pagebreak()
== Wheel of Death
#v(.15in)
*Overview:* This game is similar to traditional PLO but with the variation that the community cards are formed in a wheel and a play must use three continuos cards. 
#v(.15in)
*Objective:* Best high or best low poker hand.
#v(.15in)
*Hand Construction:* 2o2 cards from the players hand plus 3 continuos community cards.
#v(.15in)
*Setup:* Players are dealt 4 cards. 9 community cards are in the center face down in the shape of a ring
#v(.15in)
*Game play:*
+ Round of betting on potential
+ 3 cards are flipped spaced 2 apart from each other
+ Round of betting
+ 3 more cards are flipped adjacent to the previous three
+ Round of betting
+ Final 3 cards are flipped
+ Round of betting
+ Declaration
+ Showdown
#v(.15in)
*Variations:*
+ 8 Cards in the wheel.
+ Cards can be flipped 1 at a time to increase the amount of betting rounds.



#pagebreak()
== 5 Card Omaha
#v(.15in)
*Overview:* This game is similar to traditional PLO but with the variation that a fifth hole card is dealt to each player preflop. 
#v(.15in)
*Objective:* Best high or best low poker hand.
#v(.15in)
*Hand Construction:* 2o2 cards from the players hand plus 3 continuos community cards.
#v(.15in)
*Setup:* Players are dealt 5 cards.
#v(.15in)
*Game play:*
+ Round of betting on potential.
+ Dealer burns a card and deals the flop.
+ Round of action.
+ Dealer burns a card and flips the turn card.
+ Round of action.
+ Dealer burns a card and flips the river card.
+ Round of action.
+ Showdown.
#v(.15in)
*Variations:*
+ Can be played high hand only.



#pagebreak()
== Courchevel
#v(.15in)
*Overview:* This game is similar to traditional PLO but with the variation that a fifth hole card is dealt to each player preflop and the first flop card is dealt face up at the beginning og the hand. 
#v(.15in)
*Objective:* Best high or best low poker hand.
#v(.15in)
*Hand Construction:* 2o2 cards from the players hand plus 3 continuos community cards.
#v(.15in)
*Setup:* Players are dealt 5 cards. The first flop card is dealt face up.
#v(.15in)
*Game play:*
+ Round of betting on potential.
+ Dealer burns a card and deals the remaining card of the flop.
+ Round of action.
+ Dealer burns a card and flips the turn card.
+ Round of action.
+ Dealer burns a card and flips the river card.
+ Round of action.
+ Showdown.
#v(.15in)
*Variations:*
+ Can be played high hand only.



// 5 Card Draw
#pagebreak()
= 5 Card Draw Variants
#pagebreak()
== 5 Card Draw
#v(.15in)
*Overview:* In Five Card Draw, the idea is to make the best five-card hand after one 'draw' - or substitution of unwanted cards. Players are dealt five hole cards, and once the first round of betting is complete, players select the hole cards they wish to discard. Once players in the hand have completed their draws there is another round of betting, and the winner is either the player left after everyone else has folded, or the player with the best five-card hand.
#v(.15in)
*Objective:* Best high poker hand.
#v(.15in)
*Hand Construction:* The 5 cards in the players hand.
#v(.15in)
*Setup:* Players are dealt 5 cards.
#v(.15in)
*Game play:*
+ Round of betting of potential.
+ Players have the option to discard and draw new cards. (Clockwise starting with the player left of the dealer)
+ Round of betting.
+ Showdown.



#pagebreak()
== Guts Leg
#v(.15in)
*Overview:* This game is similar to traditional 5 card draw however a player needs to win 2 hands to win the pot. If every player gets a win, "leg", the counts reset and everyone is back to 0 wins.
#v(.15in)
*Objective:* Best high poker hand.
#v(.15in)
*Hand Construction:* The 5 cards in the players hand.
#v(.15in)
*Setup:* Players are dealt 5 cards.
#v(.15in)
*Game play:*
+ Round of betting on potential.
+ Starting left of the dealer, Players draw up to 3 cards or four with an Ace.
+ Round of betting
+ Showdown.
+ The player who wins the showdown has a leg.
+ Repeat until one player has 2 legs and they take the whole pot. Dealer position moves one to the left after each hand
#v(.15pt)
*Variants:*
+ If you're out you're out.



#pagebreak()
== 3 Blind Mice
#v(.15in)
*Overview:* This game is similar to traditional 5 card draw however there are 3 community cards which can be used.
#v(.15in)
*Objective:* Best high poker hand.
#v(.15in)
*Hand Construction:* Any 5 cards from the players hand and the community cards.
#v(.15in)
*Setup:* Players are dealt 5 cards.
#v(.15in)
*Game play:*
+ Round of betting on potential
+ Starting left of the dealer, Players draw up to 3 cards or four with an Ace.
+ Round of betting
+ 3 community cards in the middle are flipped
+ Round of betting
+ Showdown
#v(.15pt)
*Variants:*
+ Community cards can be flipped one at a time to add additional betting rounds.



#pagebreak()
== Triple Draw
#v(.15in)
*Overview:* This game is similar to traditional 5 card draw however there are 3 rounds to draw and you want the best low poker hand.
#v(.15in)
*Objective:* Best low poker hand.
#v(.15in)
*Hand Construction:* The 5 cards in the players hand.
#v(.15in)
*Setup:* Players are dealt 5 cards.
#v(.15in)
*Game play:*
+ Round of betting on potential
+ Round of drawing
+ Round of betting
+ Round of drawing
+ Round of betting
+ Round of drawing
+ Round of betting
+ Showdown 



#pagebreak()
== Scare-a-moosh
#v(.15in)
*Overview:* This game is similar to traditional 5 card draw however a player can choose the game for a set price for everyone else to play.
#v(.15in)
*Objective:* Best low poker hand, best high poker, high numbers, or low numbers depending on the dealers choice.
#v(.15in)
*Hand Construction:* The 5 cards in the players hand.
#v(.15in)
*Setup:* Players are dealt 5 cards.
#v(.15in)
*Game play:*
+ Starting left of the dealer the player has the option to pick the game. High poker, low poker, high numbers, or low numbers. As well as how much it cost to play up to \$2. They may also pass this right to the person on the left. If everyone passes, hands are re-dealt.
+ Declare if the player is in or out, If you're out you're out. 
+ A round of drawing cards
+ A round of betting
+ Showdown
+ If the player does not win the game they declare, they get hit with a stun.
+ Game continues and the dealer position rotates 1 to the left. Pot grows until the player who declares the game wins the hand
#v(.15in)
*Variations:*
+ No, if you're out you're out.



#pagebreak()
== 2-7 Single Draw
#v(.15in)
*Overview:* This game is similar to traditional 5 card draw however you want the best low poker hand and an Ace is *always* high.
#v(.15in)
*Objective:* Best low poker hand.
#v(.15in)
*Hand Construction:* The 5 cards in the players hand.
#v(.15in)
*Setup:* Players are dealt 5 cards.
#v(.15in)
*Game play:*
+ Round of betting of potential.
+ Players have the option to discard and draw new cards. (Clockwise starting with the player left of the dealer)
+ Round of betting.
+ Showdown.


#pagebreak()
== 2-7 Triple Draw
#v(.15in)
*Overview:* This game is similar to traditional 5 card draw however you want the best low poker hand and an Ace is *always* high.
#v(.15in)
*Objective:* Best low poker hand.
#v(.15in)
*Hand Construction:* The 5 cards in the players hand.
#v(.15in)
*Setup:* Players are dealt 5 cards.
#v(.15in)
*Game play:*
+ Round of betting on potential
+ Round of drawing
+ Round of betting
+ Round of drawing
+ Round of betting
+ Round of drawing
+ Round of betting
+ Showdown 



// Texas Hold'em
#pagebreak()
= Texas Hold'em Variants
#pagebreak()
== Texas Hold'em
#v(.15in)
*Overview:* Each player is dealt two cards ('hole cards') which belong only to that player. Five community cards are dealt face-up on the 'board' and all players may use these cards in conjunction with their own hole cards to make the best five-card poker hand possible
#v(.15in)
*Objective:* Best high poker hand.
#v(.15in)
*Hand Construction:* The 2 hole cards in the players hand plus 3 community cards.
#v(.15in)
*Setup:* Players Place \$20 in front of their main stack of money. This is the only money the player can bet with. Players are dealt 2 cards.
#v(.15in)
*Game play:*
+ Dealer starts with the dealer who chose the game.
#set enum(numbering: "a.", indent: .5in)
+  2 cards are dealt to each player face down.
+  The player left of the dealer is small blind (\$0.50)
+  The player left of the dealer +1 is big blind (\$1.00)
+  Round of betting starting left of the big blind.
+  Dealer burns a card and shows the flop.
+  Round of betting starts with the player closest to the left dealer still in the hand.
+  Dealer burns a card and shows the turn.
+  Round of betting starts with the player closest to the left dealer still in the hand.
+  Dealer burns a card and shows the river.
+  Round of betting starts with the player closest to the left dealer still in the hand.
+  Showdown. The player who last bet (the aggressor) shows. The players left have the option to show; if their hand is worse they can just fold (muck their hand).
+  Winner of the showdown takes the pot.
#set enum(numbering: "1.", indent: 0in)
3. This repeats for one revolution of the table. When the dealer chip returns to the player who decided the game. Players may return their chips to their main stack and the player left of the dealer decides the next game.



#pagebreak()
== Reverse Poker
#v(.15in)
*Overview:* This game is similar to traditional texas hold'em but the card counts are reversed.
#v(.15in)
*Objective:* Best high poker hand.
#v(.15in)
*Hand Construction:*  Any 5 cards from either the players hand or the center.
#v(.15in)
*Setup:* Players are dealt 5 cards and  2 cards go face down in the middle.
#v(.15in)
*Game play:*
+ Round of betting on potential
+ The 1st card is flipped
+ Round of betting
+ The 2nd card is flipped
+ Round of betting
+ Showdown



#pagebreak()
== Indian Texas Hold'em
#v(.15in)
*Overview:* This game is similar to traditional texas hold'em but the hole cards are shown to everyone but yourself by putting them up to your head.
#v(.15in)
*Objective:* Best high poker hand.
#v(.15in)
*Hand Construction:* The 2 hole cards in the players hand plus 3 community cards.
#v(.15in)
*Setup:* Players are dealt 2 cards and do not look at them.
#v(.15in)
*Game play:*
+ The players all place the cards to their forehead showing everyone at the table their cards.
+ Round of betting on potential
+ Dealer burns a card and shows the flop.
+ Round of betting starts with the player closest to the left dealer still in the hand.
+ Dealer burns a card and shows the turn.
+ Round of betting starts with the player closest to the left dealer still in the hand.
+ Dealer burns a card and shows the river.
+ Round of betting starts with the player closest to the left dealer still in the hand.
+ Showdown.



#pagebreak()
== Mama's Favorite
#v(.15in)
*Overview:* This game is similar to traditional texas hold'em but players are dealt 6 cards and form 3 hands.
#v(.15in)
*Objective:* Best high poker hand.
#v(.15in)
*Hand Construction:* The 2 hole cards in the players hand plus 3 community cards.
#v(.15in)
*Setup:* Players are dealt 6 cards.
#v(.15in)
*Game play:*
+ Players begin by splitting their 6 cards into 3 pairs of 2.
+ Round of betting on potential
+ Burn and turn the flop
+ Players discard one pair
+ Round of betting
+ Burn and turn the turn
+ Players discard one pair
+ Round of betting.
+ Burn and turn the river.
+ Round of betting.
+ Showdown.



#pagebreak()
== Industrial Elevator
#v(.15in)
*Overview:* This game is similar to traditional texas hold'em but players are dealt 5 cards and use 2 or 3 community cards.
#v(.15in)
*Objective:* Best high poker hand.
#v(.15in)
*Hand Construction:* The 2 or 3 hole cards in the players hand plus 3 or 2 community cards, respectively.
#v(.15in)
*Setup:* Players are dealt 5 cards.
Setup the board with all the cards face down. See diagram below.
#align(center, 
  table(
    align: center,
    column-gutter: 0pt,
    row-gutter: 5pt,
    columns: (auto, auto, auto, auto),
    stroke: 0pt,
    rows: 4,
    [],[#text(black,size:25pt,[#emoji.playingcard.joker])],[],[],
    [#text(black,size:25pt,[#emoji.playingcard.joker])],[#text(black,size:25pt,[#emoji.playingcard.joker])],[#text(black,size:25pt,[#emoji.playingcard.joker])],[#text(black,size:25pt,[#emoji.playingcard.joker])],
    [],[#text(black,size: 25pt,[#emoji.playingcard.joker])],[],[],
    [],[#text(black,size: 25pt,[#emoji.playingcard.joker])],[],[]
  )
)
#v(.15in)
*Game play:*
+ Round of betting on potential
+ Turn the flop, these are the two cards on the right side.
+ Round of betting
+ Turn the turn, this is the card on the left side of the elevator.
+ Round of betting.
+ Turn the river, this is one of the cards in the elevator.
+ Round of betting.
+ Continue to turn river cards and betting until are a face-up.
+ Showdown.
#v(.25in)
*Variations:*
+ 5 Cards in the elevator to increase the rounds of betting.



#pagebreak()
== Crazy Pineapple
#v(.15in)
*Overview:* This game is similar to traditional texas hold'em but players are dealt 5 cards and use 2 or 3 community cards.
#v(.15in)
*Objective:* Best high poker hand.
#v(.15in)
*Hand Construction:* The 2 hole cards in the players hand plus 3 community cards.
#v(.15in)
*Setup:* Players are dealt 3 cards.
#v(.15in)
*Game play:*
+ Round of betting on potential
+ Burn and turn the flop
+ Players discard one card
+ Round of betting
+ Burn and turn the turn
+ Round of betting.
+ Burn and turn the river.
+ Round of betting.
+ Showdown.



#pagebreak()
== 6+ Hold'em (Short Deck)
#v(.15in)
*Overview:* This game is similar to traditional texas hold'em but all cards lower than a 6 are removed from the deck. Flush beats a full-house and A,6,7,8,9 is classified as a straight.
#v(.15in)
*Objective:* Best high poker hand.
#v(.15in)
*Hand Construction:* The 2 hole cards in the players hand plus 3 community cards.
#v(.15in)
*Setup:* Players are dealt 2 cards.
#v(.15in)
*Game play:*
+ Round of betting on potential
+ Burn and turn the flop
+ Round of betting
+ Burn and turn the turn
+ Round of betting.
+ Burn and turn the river.
+ Round of betting.
+ Showdown.



#pagebreak()
== Showtime Hold'em
#v(.15in)
*Overview:* This game is similar to traditional texas hold'em but all cards folded by other players remain face-up.
#v(.15in)
*Objective:* Best high poker hand.
#v(.15in)
*Hand Construction:* The 2 hole cards in the players hand plus 3 community cards.
#v(.15in)
*Setup:* Players are dealt 2 cards.
#v(.15in)
*Game play:*
+ Round of betting on potential
+ Burn and turn the flop
+ Round of betting
+ Burn and turn the turn
+ Round of betting.
+ Burn and turn the river.
+ Round of betting.
+ Showdown.



#pagebreak()
== Swap Hold'em
#v(.15in)
*Overview:* This game is similar to traditional texas hold'em but with the twist that before any player folds they are given the option to swap out one or both of their hold cards.
#v(.15in)
*Objective:* Best high poker hand.
#v(.15in)
*Hand Construction:* The 2 hole cards in the players hand plus 3 community cards.
#v(.15in)
*Setup:* Players are dealt 2 cards.
#v(.15in)
*Game play:*
+ Round of betting on potential
+ Burn and turn the flop
+ Round of betting
+ Burn and turn the turn
+ Round of betting.
+ Burn and turn the river.
+ Round of betting.
+ Showdown.



#pagebreak()
== Split Hold'em
#v(.15in)
*Overview:* This game is similar to traditional texas hold'em but two boards are dealt simultaneously. The winner of each board gets half the pot.
#v(.15in)
*Objective:* Best high poker hand.
#v(.15in)
*Hand Construction:* The 2 hole cards in the players hand plus 3 community cards.
#v(.15in)
*Setup:* Players are dealt 2 cards.
#v(.15in)
*Game play:*
+ Round of betting on potential
+ Burn and turn the flop
+ Round of betting
+ Burn and turn the turn
+ Round of betting.
+ Burn and turn the river.
+ Round of betting.
+ Declaration.
+ Showdown.



#pagebreak()
== Tempest Hold'em
#v(.15in)
*Overview:* This unique format ups the ante on traditional Texas Hold'em games. Not only do the antes increase every hand until a showdown is reached, but there's an extra pre-flop blind - known as a Giant Blind - and you only have two options: all-in or fold. Hands play out per regular Texas Hold'em rules, but every pot is 'capped'. That means no player can wager or lose more than 10 Giant Blinds in a single hand.
#v(.15in)
*Objective:* Best high poker hand.
#v(.15in)
*Hand Construction:* The 2 hole cards in the players hand plus 3 community cards.
#v(.15in)
*Setup:* Players are dealt 2 cards.
#v(.15in)
*Game play:*
+ Round of betting on potential
+ Burn and turn the flop
+ Round of betting
+ Burn and turn the turn
+ Round of betting.
+ Burn and turn the river.
+ Round of betting.
+ Showdown.



// Black jack numbers
#pagebreak()
= Blackjack / Numbers Variants
#pagebreak()
== Blackjack
#v(.15in)
*Overview:* Players play against the dealer to get closest to 21 without going over.
#v(.15in)
*Objective:* Closest to 21 without going over.
#v(.15in)
*Hand Construction:* The cards in the players hand.
#v(.15in)
*Setup:* Deal 2 cards face up to each player.
#v(.15in)
*Game play:*
+ The player who decides to play acts as the house. The dealer decides max bet size.
+ Players receive 2 cards face up and try to get as close to 21 without going over.
+ They need a better hand than the dealer to get paid out 1:1 otherwise they lose their initial bet.
+ An Ace and a face card is a blackjack and pays out 3:2.
+ If a player has 2 of the same card they can split the 2 for another wager amount and play them as separate hands.
+ If the player matches the dealer it is a push and the player does not win or lose their money
#v(.15in)
*Variations:*
+ Double down after splitting Aces is up to the dealer.


#pagebreak()
== 7/27
#v(.15in)
*Overview:* This game is similar to blackjack expect there are 2 different target values.
#v(.15in)
*Objective:* Closest to 7 or 27 without going over.
#v(.15in)
*Hand Construction:* The cards in the players hand.
#v(.15in)
*Setup:* Deal 2 cards, one face up and one face down, to each player.
#v(.15in)
*Game play:*
+ Card values: 2-10 the amount shown on the card. Face cards 0.5 pts. Aces 1 or 11.
+ Round of betting occurs.
+ Players are then offered to hit (receive another card) or stand (leave their hand alone).
+ If any player hits another round of betting occurs.
+ The hit/stand offer and betting continues until everyone stands.
+ Declaration (Closest to 7, closest to 27, or closest to both)
+ Showdown
#v(.15in)
*Variations:*
+ Can bluff if the player went over 7 or 27 and if no one else declares it the player can win that portion of the pot.
+ The player can go 7 or 27 and it becomes closest



#pagebreak()
== 5.5/21.5
#v(.15in)
*Overview:* This game is similar to blackjack expect there are 2 different target values.
#v(.15in)
*Objective:* Closest to 7 or 27 without going over.
#v(.15in)
*Hand Construction:* The cards in the players hand.
#v(.15in)
*Setup:* Deal 2 cards, one face up and one face down, to each player.
#v(.15in)
*Game play:*
+ Card values: 2-10 the amount shown on the card. Face cards 0.5 pts. Aces 1 or 11.
+ Round of betting occurs.
+ Players are then offered to hit (receive another card) or stand (leave their hand alone).
+ If any player hits another round of betting occurs.
+ The hit/stand offer and betting continues until everyone stands.
+ Declaration (Closest to 5.5, closest to 21.5, or closest to both)
+ Showdown
#v(.15in)
*Variations:*
+ The player can't go over 5.5 or 21.5
+ Can bluff if the player went over 5.5 or 21.5 and if no one else declares it the player can win that portion of the pot. (Only if 1st variation is enabled)



#pagebreak()
== Numbers
#v(.15in)
*Overview:* This game uses the number values from blackjack to determine the value of a players hand.
#v(.15in)
*Objective:* Sum of the players cards as low or as high as possible. If the player discard all their cards the player scoops the pot
#v(.15in)
*Hand Construction:* The remaining cards in the players hand
#v(.15in)
*Setup:* Deal each player 6 cards.
#v(.15in)
*Game play:*
+ Card values: 2-10 the amount shown on the card. Face cards 0.5 pts. Aces 1 or 11.
+ 4 cards go into the middle face down
+ Round of betting on potential
+ 1 card in the middle is flipped. If the player has the card +1 or -1 from it the player discards that from their hand. (a flipped Ace means 2 and Kings get dropped)
+ Round of betting.
+ The 2nd card in the middle gets flipped. If it is the same as any other card a new one is drawn.
+ Round of betting.
+ The 3rd card in the middle gets flipped. If it is the same as any other card a new one is drawn.
+ Round of betting.
+ The 4th card in the middle gets flipped. If it is the same as any other card a new one is drawn.
+ Round of betting.
+ Declaration, assuming no one has ginned (dropped all their cards and scooped the pot).
+ Showdown
#v(.15in)
*Variations:*
+ Players start with 5-7 cards to make showdown more or less likely.


// 7 Card stud
#pagebreak()
= 7 Card Stud Variants
#pagebreak()
== 7 Card Stud
#v(.15in)
*Overview:* This game is similar to texas hold'em however on top of your hole cards you receive 5 cards face up to build your hand.
#v(.15in)
*Objective:*  Best high poker hand.
#v(.15in)
*Hand Construction:* 5 of the 7 cards in the players hand.
#v(.15in)
*Setup:* Deal each player 2 cards face down.
#v(.15in)
*Game play:*
+ Round of betting on potential.
+ Players receive 3rd card face up
+ Round of betting
+ Players receive 4th card face up
+ Round of betting
+ Players receive 5th card face up
+ Round of betting
+ Players receive 6th card face up
+ Round of betting
+ Players receive 7th card face down
+ Round of betting
+ Showdown



#pagebreak()
== Razz
#v(.15in)
*Overview:* This game is similar to 7 card stud but you want the best low poker hand. 
#v(.15in)
*Objective:* Best low poker hand.
#v(.15in)
*Hand Construction:* 5 of the 7 cards in the players hand.
#v(.15in)
*Setup:* Players are dealt 2 cards face down.
#v(.15in)
*Game play:*
+ Round of betting on potential.
+ Players receive 3rd card face up
+ Round of betting
+ Players receive 4th card face up
+ Round of betting
+ Players receive 5th card face up
+ Round of betting
+ Players receive 6th card face up
+ Round of betting
+ Players receive 7th card face down
+ Round of betting
+ Showdown



#pagebreak()
== Bid
#v(.15in)
*Overview:* This game is similar to 7 card stud however the 5 face up cards are placed in the center and the selecting order is bid for.
#v(.15in)
*Objective:* Best high orr low poker hands.
#v(.15in)
*Hand Construction:* 5 of the 7 cards in the players hand.
#v(.15in)
*Setup:* Players are dealt 2 cards face down.
#v(.15in)
*Game play:*
+ Round of betting on potential.
#set enum(numbering: "a.", indent: .5in)
+ The amount of players remaining = the amount of cards that go face up in the middle
+ Players put the denomination that they want to bid for their place in the picking order in their hand
+ Everyone reveals how much they want to pay and the money goes into the pot.
+ The person who bid the most picks the first card, the person who bid the 2nd most, till the person who bid the least. Ties go to whoever is closest to left of the dealer
+ Round of betting
#set enum(numbering: "1.", indent: 0in)
2. That continues till every remaining player has 7 cards total. 5 up and 2 down.
+ Declaration
+ Showdown

#v(.15in)
*Variations:*
+ Players can buy an 8th card. \$5 face up or \$7 for face down


#pagebreak()
== Low in the Hole
#v(.15in)
*Overview:* This game is similar to 7 card stud however the lowest card in the hole is a wild for your hand. Ace is high in the hole. Face up versions of your wild card counts as a wild as well; allowing for a 5 of a kind.
#v(.15in)
*Objective:* Best high or low poker hand.
#v(.15in)
*Hand Construction:* 5 of the 7 cards in the players hand.
#v(.15in)
*Setup:* Players are dealt 2 cards face down. The lower value of the 2 cards is wild for that players hand (Ace is high)
#v(.15in)
*Game play:*
+ Round of betting on potential.
+ Players receive 3rd card face up
+ Round of betting
+ Players receive 4th card face up
+ Round of betting
+ Players receive 5th card face up
+ Round of betting
+ Players receive 6th card face up
+ Round of betting
+ Players receive 7th card face up or down to try and get a lower card to change their wild
+ Round of betting
+ Declaration
+ Showdown



#pagebreak()
== Baseball
#v(.15in)
*Overview:* This game is similar to 7 card stud however 3s and 6s are wild and a face-up 4 is an extra card. The extra card can be up or down.
#v(.15in)
*Objective:* Best high or low poker hand
#v(.15in)
*Hand Construction:* 5 of the 7 cards in the players hand.
#v(.15in)
*Setup:* Players are dealt 2 cards face down.
#v(.15in)
*Game play:*
+ Round of betting on potential 
+ Everyone receives 3rd card
+ Round of betting
+ Players receive 4th card face up
+ Round of betting
+ Players receive 5th card face up
+ Round of betting
+ Players receive 6th card face up
+ Round of betting
+ Players receive 7th card face down
+ Round of betting
+ Declaration (High/Low/Both Poker)
+ Showdown



#pagebreak()
== Follow the Queen
#v(.15in)
*Overview:* This game is similar to 7 card stud however when a queen is shown face up the following card value becomes a wild for the whole table.
#v(.15in)
*Objective:* Best high or low poker hand
#v(.15in)
*Hand Construction:* 5 of the 7 cards in the players hand.
#v(.15in)
*Setup:* Players are dealt 2 cards face down.
#v(.15in)
*Game play:*
+ Round of betting on potential 
+ Everyone receives a 3rd card. If a queen is flipped, the next card to follow from the deck becomes the wild card. Ie. a queen is shown and a 2 comes next, anyone who has a 2 + has a wild card
+ Round of betting
+ Players receive 4th card face up. If a queen is flipped, the next card to follow from the deck becomes the wild card
+ Round of betting
+ Players receive 5th card face up. If a queen is flipped, the next card to follow from the deck becomes the wild card
+ Round of betting
+ Players receive 6th card face up. If a queen is flipped, the next card to follow from the deck becomes the wild card
+ Round of betting
+ Players receive 7th card face down.
+ Round of betting
+ Declaration
+ Showdown



#pagebreak()
== Bullet
#v(.15in)
*Overview:* This game is similar to 7 card stud however the jokers are added back into the deck and a player only gets 5 cards total. A face up joker kills the player and a face down joker becomes a wild.
#v(.15in)
*Objective:* Best high or low poker hand
#v(.15in)
*Hand Construction:* The 5 cards in the players hand.
#v(.15in)
*Setup:* Players are dealt 2 cards face down and the jokers are added to the deck.
#v(.15in)
*Game play:*
+ Round of betting on potential 
+ Players have the option to take a card. If a player receives a Joker they are dead and lose the hand
+ A round of betting occurs starting with the best high poker hand.
+ Option to take a card and betting continues till everyone has at least 5 cards total.
+ Declaration
+ Showdown
 


#pagebreak()
== Chicago High
#v(.15in)
*Overview:* This game is similar to 7 card stud however the player with the highest #spade_icon automatically receives half the pot.
#v(.15in)
*Objective:* Best high poker hand.
#v(.15in)
*Hand Construction:* 5 of the 7 cards in the players hand.
#v(.15in)
*Setup:* Players are dealt 2 cards face down.
#v(.15in)
*Game play:*
+ Round of betting on potential.
+ Players receive 3rd card face up
+ Round of betting
+ Players receive 4th card face up
+ Round of betting
+ Players receive 5th card face up
+ Round of betting
+ Players receive 6th card face up
+ Round of betting
+ Players receive 7th card face down
+ Round of betting
+ Showdown (Player with highest Spade card gets half the pot and best poker hand gets the other half)



#pagebreak()
== Chicago Low
#v(.15in)
*Overview:*  This game is similar to 7 card stud however the player with the lowest #spade_icon automatically receives half the pot.
#v(.15in)
*Objective:* Best high poker hand
#v(.15in)
*Hand Construction:* 5 of the 7 cards in the players hand.
#v(.15in)
*Setup:* Players are dealt 2 cards face down.
#v(.15in)
*Game play:*
+ Round of betting on potential.
+ Players receive 3rd card face up
+ Round of betting
+ Players receive 4th card face up
+ Round of betting
+ Players receive 5th card face up
+ Round of betting
+ Players receive 6th card face up
+ Round of betting
+ Players receive 7th card face down
+ Round of betting
+ Showdown (Player with lowest spade card gets half the pot and best poker hand gets the other half)



#pagebreak()
== Push
#v(.15in)
*Overview:* This game is similar to 7 card stud however when a player is given a up card they can pay \$1 to push it to the next player.
#v(.15in)
*Objective:* Best high or low poker hand
#v(.15in)
*Hand Construction:* 5 of the 7 cards in the players hand.
#v(.15in)
*Setup:* Players are dealt 2 cards face down.
#v(.15in)
*Game play:*
+ Round of betting on potential.
+ Players receive 3rd card face up. If they don't want it they can pay \$1 to push it to the next person. If you receive a pushed card you can keep it or pay \$1 to receive the next card in the deck.
+ Round of betting
+ Players receive 4th card face up. If they don't want it they can pay \$1 to push it to the next person. If you receive a pushed card you can keep it or pay \$1 to receive the next card in the deck.
+ Round of betting
+ Players receive 5th card face up. If they don't want it they can pay \$1 to push it to the next person. If you receive a pushed card you can keep it or pay \$1 to receive the next card in the deck.
+ Round of betting
+ Players receive 6th card face up. If they don't want it they can pay \$1 to push it to the next person. If you receive a pushed card you can keep it or pay \$1 to receive the next card in the deck.
+ Round of betting
+ Players receive 7th card face down
+ Round of betting
+ Showdown
#v(.15in)
*Variations:*
+ Players can buy an 8th card. \$5 face up or \$7 for face down




// Misc.
#pagebreak()
= Misc.
#pagebreak()
== Indian Poker
#v(.15in)
*Overview:* Players hold a card to their head the highest card wins.
#v(.15in)
*Objective:* Highest Card.
#v(.15in)
*Hand Construction:* The single card in the players hand.
#v(.15in)
*Setup:* Players are dealt one card and place it to their forehead without looking at it. Showing the rest of the table.
#v(.15in)
*Game play:*
+ Round of betting
+ Showdown



#pagebreak()
== Whooh Ray for Booray
#v(.15in)
*Overview:* This game is a variation of a follow the suit game, such as spades, high low jack, or pitch.
#v(.15in)
*Objective:* Win as many tricks as possible.
#v(.15in)
*Hand Construction:* The 5 cards in the players hand.
#v(.15in)
*Setup:* Players receive 5 cards.
#v(.15in)
*Game play:*
+ The player in the dealer position has one of their cards flipped. The suit of that card becomes the trump suit.
+ Players declare whether they are in or out
+ A round of drawing
+ The player in the dealer position leads by placing a none trump card down.
+ The following players must play the same suit if it is in their hand.
+ The person who plays the highest value card (Ace high) takes that trick and plays the first card the next round
+ This continues until all 5 cards are played from everyone's hand.
+ If the player doesn't have the initial suit the player can play a trump card. Only once a trump card is played can someone lead with trump.
+ After all 5 cards are played if a player wins 3 tricks they scoop the pot
+ Then if a player didn't win a trick they "fry" and get hit with a stun
+ If the player didn't fry the player pays \$1 to the pot
+ The dealer position rotates to the left and everyone is dealt a new hand
+ The game ends when someone scoops the pot and no one fries
#v(.15in)
*Variations:*
+ If you're out you're out.



#pagebreak()
== Man or Mouse
#v(.15in)
*Overview:* This game is a fusion of pass the trash, texas hold'em, and 7 card stud.
#v(.15in)
*Objective:* Best high or low poker hand.
#v(.15in)
*Hand Construction:* 5 of the 7 cards in the players hand and the 1 community card. 
#v(.15in)
*Setup:* Players receive 7 cards and there is 1 face down community card.
#v(.15in)
*Game play:*
+ Bet on potential
+ Pass two cards to the dealer specified direction
+ Declare (In or Out)
+ Round of betting
+ Community card is flipped.
+ Round of betting
+ Declaration
+ Showdown
#v(.15in)
*Variations:*
+ Dealt 5 cards, In or out, betting, deal 2 more, betting, one community card, betting, declare



#pagebreak()
== 43
#v(.15in)
*Overview:* This game is a fusion of a draw game and 7 card stud.
#v(.15in)
*Objective:* Best high or low poker hand
#v(.15in)
*Hand Construction:* 5 of the 7 cards in the players hand.
#v(.15in)
*Setup:* Players receive 4 cards.
#v(.15in)
*Game play:*
+ Declaration (In/out). Using a \$1 chip, if you are in you place the money into the pot.
+ Players are dealt 3 more cards
+ Round of betting
+ Declaration
+ Showdown.



#pagebreak()
== Pass the Trash
#v(.15in)
*Overview:* This game is a fusion of a draw game and 7 card stud.
#v(.15in)
*Objective:* Best high or low poker hand
#v(.15in)
*Hand Construction:* 5 of the 7 cards in the players hand.
#v(.15in)
*Setup:* Players receive 7 cards. The dealer specifies the direction to pass the cards.
#v(.15in)
*Game play:*
+ The dealer specifies a direction to pass the cards. 
+ Players are dealt 7 Cards face down.
+ Round of betting, on potential.
+ Pass 3 cards to the direction stated by the dealer..
+ Round of betting.
+ Pass 2 cards in the same direction.
+ Round of betting.
+ Pass 1 card in the same direction.
+ Discard the 2 cards from the players hand the player won't be using
+ Place all 5 face down in front of the player in the order the player wants to reveal them.
+ Turn 1st Card.
+ Round of betting starting with the highest poker hand.
+ Turn 2nd Card.
+ Round of betting starting with the highest poker hand.
+ Turn 3rd Card.
+ Round of betting starting with the highest poker hand.
+ Turn 4th Card.
+ Round of betting starting with the highest poker hand.
+ Declare (High/Low)
+ Showdown.



#pagebreak()
== Badugi
#v(.15in)
*Overview:* Badugi is a form of draw poker, where the object of the game is to make the best low hand of different suits, from the four cards you are dealt. After the first betting round, remaining players can discard unwanted cards and draw. Another round of betting then ensues, after which there are two more draw and betting rounds. It's important to understand that in Badugi, pairs and suited cards don't count at all; a 'badugi' is a hand of four different ranks and all four suits. The best hand possible is 4, 3, 2, A, of different suits. Please review the Badugi hand ranking system below before playing.
#v(.15in)
*Objective:* Best high poker hand.
#v(.15in)
*Hand Construction:* The 4 cards from the players hand.
#v(.15in)
*Setup:* Players are dealt 4 cards.
#v(.15in)
*Game play:*
+ Round of betting on potential
+ Round of drawing
+ Round of betting
+ Round of drawing
+ Round of betting
+ Round of drawing
+ Round of betting
+ Showdown



#pagebreak()
== Fusion
#v(.15in)
*Overview:* This unique format ups the ante on traditional Texas Hold'em games. Not only do the antes increase every hand until a showdown is reached, but there's an extra pre-flop blind - known as a Giant Blind - and you only have two options: all-in or fold. Hands play out per regular Texas Hold'em rules, but every pot is 'capped'. That means no player can wager or lose more than 10 Giant Blinds in a single hand.
#v(.15in)
*Objective:* Best high poker hand.
#v(.15in)
*Hand Construction:* The 2 hole cards in the players hand plus 3 community cards.
#v(.15in)
*Setup:* Players are dealt 2 cards.
#v(.15in)
*Game play:*
+ Round of betting on potential
+ Burn and turn the flop. Give every remaining player and additional card.
+ Round of betting
+ Burn and turn the turn. Give every remaining player and additional card.
+ Round of betting.
+ Burn and turn the river.
+ Round of betting.
+ Showdown.
#v(.15in)
*Variations:*
+ High/Low poker hands.



#pagebreak()
== 3-2-1 Drop
#v(.15in)
*Overview:* 3-2-1 drop is a simple poker game that only uses three-card hands. Like Iron Cross and 3-5-7 (which it greatly resembles), players are not directly betting against each other, but instead risking the amount in the pot for a chance at winning it. Straights and flushes don't count. Three of a kind is the best possible hand. The two red kings (K#diamond_icon and K#heart_icon) are wild.
#v(.15in)
*Objective:* Best high poker hand.
#v(.15in)
*Hand Construction:* The 3 cards in the players hand.
#v(.15in)
*Setup:* All players pay an ante. Deal three cards to each player, as well as an extra three-card hand to the pot.
#v(.15in)
*Game play:*
+ Players hold their hand about an inch from the table. (low enough that none of their cards are exposed but high enough that it's clear they are not on the table).
+ The dealer then calls "3...2...1...drop."" On "drop", players wishing to fold drop their hand to the table; any player that keeps hold of their hand is playing it.
+ Showdown. In the event that only one person plays their hand, the extra hand dealt to the pot is exposed, with the lone player hoping to beat it. If the player wins, they take the pot, and all other players ante. If the player loses, they pay the amount of the pot, and the money remains there and play continues.
#v(.15in)
*Variations:*
+ The losers get hit with a stun and you go around the table one time. for the final hand the losers pay half of the stun to the winner of the final pot.



#pagebreak()
== Iron Cross
#v(.15in)
*Overview:* Iron Cross is a unique poker game involving five community cards. However, unlike Texas Hold'em and Omaha, you don't get to use all of the cards...and you don't even know what some of them are!
#v(.15in)
*Objective:* Best high poker hand.
#v(.15in)
*Hand Construction:* The 4 cards in the players hand and 3 community cards.
#v(.15in)
*Setup:* Players are dealt 4 cards and put a \$1 ante into the pot. A small cross is formed with 3 cards in a row and 3 cards in the column.
#v(.15in)
*Game play:*
+ Betting on potential.
+ Then the face down cards in the cross are revealed.
+ Showdown.
#v(.15in)
*Variations:*
+ The losers get hit with a stun and you go around the table one time. for the final hand the losers pay half of the stun to the winner of the final pot.



#pagebreak()
== 3-5-7
#v(.15in)
*Overview:* 3-5-7 is a poker game with an unusual betting structure. Rather than confronting one another directly by raising and calling each other's bets, players simply decide whether or not to fold. The risk comes when you don't fold and you lose-which is really much like regular poker, come to think of it.
#v(.15in)
*Objective:* Best high poker hand.
#v(.15in)
*Hand Construction:* The cards in the players hand which changes throughout the game.
#v(.15in)
*Setup:* Deal three cards to each player. Players are dealt 4 cards and put a \$1 ante into the pot.
#v(.15in)
*Game play:*
+ At any point in time, one card is a wild card; this card is the same number as however many cards everyone has. Initially, everyone has three cards, so all threes are wild Players examine their hands to determine whether they want to play or fold.
+ Declaration.
+ Showdown, without letting the players who folded see. The winner is determined using regular poker hands, minus all the straights and flushes (so a three-of-a-kind is the strongest possible hand). Loser who played stun.
+ Now, two cards are dealt to all players, giving them five cards, and fives are now wild. Game play is as before, but with the usual poker hands (straights and flushes count again). Then, two more cards are dealt, and hands are formed with five of the seven cards available to them. Hands are shuffled and play begins again with new three-card hands.
+ If, at any time, only one player elects to play a hand, all other players having folded, the player who stayed in gets a strike marker. When one player collects three strike markers, the game ends, and the pot goes to the player to get the three strike markers.
#v(.15in)
*Variations:*
+ Require all other players to make a contribution to the pot whenever a player receives a strike. This will increase the size of the reward for the player that finishes off the game. It also makes staying in the game increasingly intimidating as it goes on, making the game go quicker, since the higher stakes will induce people to play tighter, creating more strikes


