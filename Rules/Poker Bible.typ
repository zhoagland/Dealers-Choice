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
The many nights of fun we have had are owed to them. Please enjoy the games just as much as we have and remember, *bet it like you got it*.

#pagebreak()

= Definitions
== Beginner
== Advanced

#pagebreak()
= Hand Rankings
== High Poker
#table(
  align: center, 
  columns: (1fr, 1fr, 1fr, 1fr, 1fr, 1fr),
  rows: 6,
  stroke: .5pt,
  table.header( [Ranking], [Example], [Name], [Ranking], [Example], [Name]),
  [1],   [], [Royal Flush], [6], [], [],
  [1.5], [], [5 of a kind], [7], [], [],
  [2],   [], [Straight Flush], [8], [], [],
  [3],   [], [4 of a kind], [9], [], [],
  [4],   [], [Full House], [9], [], [],
  [5],   [], [], table.cell(colspan: 3, [])
)

== Low Poker 

#pagebreak()
= Betting

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
