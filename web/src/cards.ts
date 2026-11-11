import type { Card, Cents } from "./protocol";

const RANKS = "23456789TJQKA";
const RANK_NAMES = [
  "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine", "Ten",
  "Jack", "Queen", "King", "Ace",
];
const SUITS = ["c", "d", "h", "s"] as const;
const SUIT_NAMES = ["clubs", "diamonds", "hearts", "spades"];
const SUIT_SYMBOLS = ["♣", "♦", "♥", "♠"];

export function isJoker(c: Card): boolean {
  return c === 13 || c === 29;
}

export interface CardFace {
  rank: string;
  suit: (typeof SUITS)[number] | "joker";
  symbol: string;
  /** Text alternative, e.g. "Queen of hearts". */
  alt: string;
}

export function face(c: Card): CardFace {
  if (isJoker(c)) {
    const red = c === 13;
    return { rank: "★", suit: "joker", symbol: red ? "R" : "B", alt: red ? "Red joker" : "Black joker" };
  }
  const r = c & 15;
  const s = c >> 4;
  return {
    rank: RANKS[r] === "T" ? "10" : RANKS[r],
    suit: SUITS[s],
    symbol: SUIT_SYMBOLS[s],
    alt: `${RANK_NAMES[r]} of ${SUIT_NAMES[s]}`,
  };
}

/** "$1.50", "-$2", "$10". */
export function money(cents: Cents): string {
  const sign = cents < 0 ? "-" : "";
  const a = Math.abs(cents);
  const dollars = Math.floor(a / 100);
  const rest = a % 100;
  return `${sign}$${dollars.toLocaleString()}${rest ? "." + String(rest).padStart(2, "0") : ""}`;
}
