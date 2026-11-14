# Playtest round 1 (2026-11-14)

Feedback from the first multi-person playtest, sorted by area. Status: `[ ]` open, `[x]` done
(branch `playtest/round-1`), `[~]` done but worth checking at the next playtest.

## Table look and feel
- [x] P1-1 Spread the UI out to use the whole screen; less serious, a bit more arcade-like.
      Full-height table page, oval felt with a wood rail, chunky buttons, glowing turn ring.
- [x] P1-2 Desktop first: mobile support is lower priority, so desktop gets the space.
      Layout is designed at 1440×900; below 860 px the side panel drops under the table.
- [x] P1-3 An oval table with players' Discord profile pictures around it. Pictures come through
      `/api/avatar/{account}` so they load inside the Activity without a URL mapping.
- [x] P1-4 Card animations while dealing: cards fly out of a deck in the middle.
- [x] P1-5 Showdown screen: the hand stays on the table for 6 s after a showdown (2 s otherwise)
      with a "Showdown!" banner of winners and losers; everyone watching gets it.
- [x] P1-6 Rules for the current game on the right (collapsible), above chat.
- [x] P1-7 The text above the community cards was the engine's step name (`HitRounds { cap: 10 }`).
      Now plain words: "Hit or stand?", "Make your pairs", "Trick 2 of 5"…
- [x] P1-8 Capitalization: badges, statuses, tabs, timer, replay links.
- [x] P1-9 Four-color deck option, now in a ⚙ menu that also shows inside Discord.
- [x] P1-10 Mama's Favorite: gaps between pairs once they're made; one click takes a whole pair
      when throwing one away.

## Sound (⚙ menu, remembered per browser)
- [~] P1-11 Background music: a soft synthesized lounge loop, off by default.
- [x] P1-12 Turn chime, on by default. Also card ticks while dealing and a win jingle ("Card sounds").

## Actions
- [x] P1-13 Raises: − / + buttons and a slider in 50¢ steps between the smallest and largest raise.
- [x] P1-14 Hit / Stand, Face up / Face down, Keep it / Push it ($1), Take K♠… instead of "Choose 0/1".
- [x] P1-15 Discard, pass, pair and reveal-order decisions are made by clicking your cards
      (numbers show the reveal order; pairs light up in matching colours); tricks: click a card to play.
- [x] P1-16 Drag your cards sideways to reorder them, any time (display only).

## Flow
- [x] P1-17 Declares, In/Out, pairing, discards/draws, 3-5-7 play/drop and sealed bids are asked
      of everyone at once ("Everyone decides at once"); answers apply in seat order.
- [x] P1-18 Pass the Trash passing and ordering are asked at once too.
- [x] P1-19 Late joiners and players sitting out see the hand (board, bets, face-up cards; no hole cards).
- [x] P1-20 A 15 s break between games ("Next game in 12s"); the host can press Start now.

## Game rules
- [~] P1-21 Scare-a-moosh: naming already started left of the dealer, and the dealer button already
      moved each hand. New: after the declarer wins, the next game's deal goes to the seat left of
      the declarer. Check at the next playtest that this matches what people expect.
- [x] P1-22 Indian games: once you fold you see your own card.
- [x] P1-23 7/27: each hit round, and the betting after it, starts one seat further left.
- [x] P1-24 Whooh Ray: the player left of the dealer leads the first trick.
- [x] P1-25 Whooh Ray: the cards of the current trick (or the last one, until the next starts) sit
      in front of whoever played them.
- [x] P1-26 Whooh Ray: each player shows a trick count.
