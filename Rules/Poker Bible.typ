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
#outline(indent: 2em, depth: 2)
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

#pagebreak()

= Definitions
== Beginner
=== Barrel (n.):
A bet on a postflop street.\
=== Brick (n.):
A turn or river card that is expected to have no impact on the outcome of the hand.\
=== Call (v.):
To match an opponent's bet.\
=== Check (v.):
To defer your right to bet for the moment. \
=== Checked around (adj.):
When all players check on a postflop street. \
=== Check-raise (v.):
To check postflop and then raise a bet from another player. \
=== Coin flip (n.):
When two hands are all in together preflop with similar equity. For example, a pocket pair against two overcards. \
=== Cold-call (v.):
To call a single preflop raise. \
=== Cold-calling range (n.):
The range of hands with which you call a single preflop raise. \
=== Combination (n.):
A specific set of hole cards. A K is not a combination. A#diamond_icon\6#club_icon is.\
=== Continuation bet (n.):
A bet made by the player who made the last bet or raise. This term is often abbreviated "c-bet." \
=== Deuces (n.):
A pair of twos. \ 
=== Double up (v.):
When you go all in and win enough chips to double your stack.\
=== Draw (n. and v.):
The hand you're trying to make. For example, if you are dealt a two and a three, and the flop includes a four and a five-you're drawing a straight.\
=== Dry board (n.):
When the table's shared cards do not present many opportunities for drawing hands. For example, a "rainbow" flop with three different suits, which does not open up the possibility of a flush draw.
=== Fire a barrel (v.):
To bet. \
=== Fire multiple barrels (v.):
To bet on multiple streets. \
=== Fire three barrels (v.):
Making a bet on the flop, turn, and river. \
=== Flop (n.):
The first three shared cards that are dealt. \
=== Game theory optimal (gto) poker (n.):
A defensive playing strategy that aims to make you unexploitable to your opponents. \
=== Get run over (v.):
To be aggressively pushed out of many pots. \
=== Gutshot draw (n.):
When you have four out of the five cards needed for a particular draw, but are missing one. \
=== Heads-up pot (n.):
A pot with two players. \
hero call(n.):
When a player has a relatively weak hand but suspects that their opponent is bluffing, so they decide to call that opponent's bet on the final round of betting. \
=== Hole cards (n.):
The two cards you're dealt. These are also described with the term "pocket." For example, if your hole cards are , you have "pocket aces." \
=== Initiative (n.):
The player who made the last bet or raise is said to have the initiative. \
=== In position (adj.):
When you act after your opponent. \
=== Lead into (v.):
In a heads-up pot, to make the first bet on a postflop street. Implies that your bet is directed at an opponent. \
=== Lead out (v.):
To make the first bet on a postflop street. \
=== Limp in (v.):
To enter the pot before the flop by simply matching the big blind's bet, rather than raising. \
=== Low-card wet board (n.):
Boards with many possible draws, but ones that don't involve high cards, such as 4#heart_icon\3#heart_icon\2#heart_icon, 6#spade_icon\7#spade_icon\9#club_icon, and 5#diamond_icon\6#club_icon\2#spade_icon.
=== Maniac (n.):
A player who plays too many hands in an overly aggressive fashion, mostly without a plan. \
=== Min-raise (n.):
A raise that is double the previous bet. Preflop this is twice the big blind. \
=== Multi-way pot (n.):
A pot with three or more players. \
=== The nuts (n.):
The absolute best hand possible. \
=== Nutted hand (n.):
A hand that almost always rates to be the best. You can also describe a player holding such a hand as "nutted." \
=== Open limp (n.):
When the first player enters the pot by choice. This doesn't include the blinds, as those are forced bets. Used interchangeably with "open raise." \
=== Out of position (adj.):
When you act before your opponent. \
=== Outs (n.):
Cards you need to complete your draw. \
=== Overbet (n.):
A bet exceeding the size of the pot. \
=== Overcards (n.):
Cards that rank higher than any card on the board. \
=== Overpair (n.):
A pair that ranks higher than any possible pair on the board. \
=== Pay off (v.):
To call a big bet. \
=== Peel (v.):
To call a bet or raise so you can see one more card on the board. \
=== The river (n.):
The fifth shared card. \
=== Set (n.):
Three of a kind when the player is holding a pocket pair. \
=== Shove (v.):
To bet all of your remaining chips. \
=== Show down (v.):
To physically reveal your cards to your opponents. \
=== Showdown (n.):
The moment after the final round of betting when the last players left in a hand reveal their cards to determine the winner(s). \
=== Slow play (v.):
To refrain from betting or raising in the hope that your opponent will do the betting for you. Also, to give your opponent a chance to make a better hand in order to extract more value. \
=== Small ball (adj.):
A style of poker that minimizes risk by consciously keeping the pot small unless the player has a strong hand. \
=== Speculative hand (n.):
Hands such as suited connectors and small pocket pairs. They don't make a good hand very often, but when they do it's likely to be the best hand. \
=== Straddle (n.):
A voluntary bet made after the blinds have been posted but before the hole cards are dealt. \
=== Street (n.):
A segment of play. Any card that is dealt or any betting round. \
=== Three-bet (v.):
To reraise another player's open raise preflop, or to reraise over the first raise on a postflop street. \
=== Tilt (n. and v.):
A poor mindset that leads a player to make costly mistakes. \
=== Trap (v.):
To refrain from betting or raising in the hope that your opponent will do the betting for you. Also, to give your opponent a chance to make a better hand in order to extract more value. Can be used interchangeably with "slow play." \
=== Treys (n.):
A pair of threes. \
=== Trips (n.):
Three of a kind when the board is paired and the player holds one card of the same value. \
=== The turn (n.):
The fourth shared card. \
=== Turn your hand faceup (v.):
When you let other players know almost for certain what you have. You don't literally flip your cards over, but you may as well have done so. \
=== Wet board (n.):
A coordinated board with multiple draws possible. This term can apply to the board at any stage, regardless of how many cards are down. \
=== Wet flop (n.):
A flop with many possible draws available. \

== Advanced
=== Balanced range (n.):
An unexploitable range that includes both strong hands and weak hands with potential. Even if your opponents know what your range is, they won't be able to exploit it if it's perfectly balanced.\
=== Barreling frequency (n.):
The frequency at which you bet the turn and follow up on the river.\
=== Blocking bet (n.):
A small bet designed to stop your opponent from making a large bet that is difficult to call.\
=== Board texture (n.):
The characteristics of the flop-whether wet, with many draws and nutted hands possible, or dry, without draws and no hand better than a set possible.\
=== Capped range (n.):
A range that doesn't include many of the strongest hands possible. \
=== Checking range (n.):
The range of hands with which a player checks. \
=== Connect with the board (v.):
How well your hand hit a particular board. This is measurable in terms of equity. \
=== Continuing range (n.):
The range of hands with which you call or raise a bet from another player. \
=== Edge (n.):
When you have more equity than your opponent.\
=== Fold equity (n.):
The equity you gain when your opponent folds. The more frequently your opponent will fold, the more fold equity you have in proportion to the pot size. \
=== Hand range (n.):
All the hands a player has in a specific situation. This changes as the hand progresses. \
=== High-equity draw (n.):
A draw that has many outs. \
=== Lag (n.):
Loose aggressive player. They play a wide range of hands in an effective manner. \
=== Leak (n.):
A habit that loses you money. \
=== Loose player (n.):
A player who plays a wide range preflop. \
=== Loose range (n):
A range of hands containing many combinations. Used interchangeably with "wide range." \
=== Metagame (n.):
The integration of your playing history with your opponents into your decision-making process. \
=== Nitty (adj.):
A conservative, tight style of play. \
=== Polarized (adj.):
When a range consists of only strong hands and very weak hands. The more polarized a range, the stronger the value hands and the weaker the bluffs. \
=== Range advantage (n.):
An advantage in which your range of hands compares favorably to another range. \
=== Sticky player (n.):
An opponent who usually doesn't fold marginal hands in the hope of completing their draw. \
=== Tag (n.):
Tight aggressive player. They play around 20 percent of hands and open raise around 17 percent. This type of player is likely a professional. \
=== Tight range (n.):
A range of hands containing few combinations. \
=== Value bet (n.):
A bet where you expect to have more equity than your opponent. \
=== Value hand (n.):
A hand that you can bet with and expect to be called by worse hands frequently enough to show a profit. \
=== Whale (n.):
A big fish who plays too many hands in a passive manner without any skill. They are always likely to call big bets with weak hands. \
=== Wide range (n.):
A range of hands containing many combinations. Used interchangeably with "loose range." \
=== Winning player (n.):
Any profitable player. \


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
The dealer pays an ante of #sym.dollar\2.00 at the beginning of the game of their choice. This is a fee for choosing the game and it juices the pot.

== Chip Values
Each chip represents a real portion of your buy in amount. No scaling is involved.
We use the following designations with the listed starting amount to reach our #sym.dollar\100.00 buy in.
\
#text(blue, [Blue - #sym.dollar\0.50]) - 20 Chips
\
#text(red, [Red - #sym.dollar\1.00]) - 20 Chips
\
White - #sym.dollar\5.00 - 14 Chips

#pagebreak()
= Omaha Variants

#pagebreak()
= 5 Card Draw Variants

#pagebreak()
= Texas Holdem' Variants

#pagebreak()
= Blackjack / Numbers Variants

#pagebreak()
= 7 Card Stud Variants


#pagebreak()
= Misc.
