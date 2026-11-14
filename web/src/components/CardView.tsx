import { face } from "../cards";
import type { Card, CardSlot } from "../protocol";

const RANK_INDEX: Record<string, number> = {
  Two: 0, Three: 1, Four: 2, Five: 3, Six: 4, Seven: 5, Eight: 6, Nine: 7, Ten: 8,
  Jack: 9, Queen: 10, King: 11, Ace: 12,
};
export function rankIndex(name: string): number {
  return RANK_INDEX[name] ?? -1;
}

interface Props {
  slot: CardSlot;
  size?: "sm" | "md" | "lg" | "xl";
  /** Rank indices that are wild for this card's owner. */
  wild?: readonly number[];
}

function Face({ card, size, wild, privateCard }: { card: Card; size: string; wild: boolean; privateCard: boolean }) {
  const f = face(card);
  const label = f.alt + (privateCard ? " (only you can see it)" : "") + (wild ? ", wild" : "");
  return (
    <span
      className={`card ${size} suit-${f.suit}${privateCard ? " private" : ""}${wild ? " wild" : ""}`}
      role="img"
      aria-label={label}
      title={label}
    >
      <span className="rank">{f.rank}</span>
      <span className="pip">{f.symbol}</span>
    </span>
  );
}

/** One card slot: face up, face down (with the face only for its owner), or empty. */
export function CardView({ slot, size = "md", wild = [] }: Props) {
  if (slot === "Empty") return <span className={`card ${size} empty`} aria-label="no card yet" />;
  if ("Up" in slot) {
    return <Face card={slot.Up} size={size} wild={wild.includes(slot.Up & 15)} privateCard={false} />;
  }
  if (slot.Down === null) {
    return <span className={`card ${size} back`} role="img" aria-label="face-down card" />;
  }
  return <Face card={slot.Down} size={size} wild={wild.includes(slot.Down & 15)} privateCard />;
}
