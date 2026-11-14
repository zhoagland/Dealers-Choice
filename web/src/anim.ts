import { useLayoutEffect, useRef } from "react";
import { cardTick } from "./sound";
import { prefs } from "./prefs";

/**
 * Deal animation (playtest P1-4): every card element marked `data-deal-key` that wasn't on screen
 * before flies in from the deck (`.deck` inside `root`), a few at a time. Keys reset with `hand`,
 * so each new hand is dealt again.
 */
export function useDealAnimation(root: React.RefObject<HTMLElement | null>, hand: string | number | null) {
  const seen = useRef<Set<string>>(new Set());
  const lastHand = useRef<string | number | null>(null);
  useLayoutEffect(() => {
    const el = root.current;
    if (!el) return;
    if (lastHand.current !== hand) {
      seen.current = new Set();
      lastHand.current = hand;
    }
    const deck = el.querySelector<HTMLElement>(".deck");
    const cards = [...el.querySelectorAll<HTMLElement>("[data-deal-key]")];
    const fresh = cards.filter((c) => !seen.current.has(c.dataset.dealKey!));
    // The first snapshot of a table we join mid-hand isn't animated.
    const firstLook = seen.current.size === 0 && fresh.length > 12;
    for (const c of cards) seen.current.add(c.dataset.dealKey!);
    if (!deck || firstLook || window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) return;
    const from = deck.getBoundingClientRect();
    fresh.forEach((c, i) => {
      const to = c.getBoundingClientRect();
      if (to.width === 0) return;
      const dx = from.left + from.width / 2 - (to.left + to.width / 2);
      const dy = from.top + from.height / 2 - (to.top + to.height / 2);
      const delay = Math.min(i, 14) * 90;
      c.animate(
        [
          { transform: `translate(${dx}px, ${dy}px) rotate(-25deg) scale(0.6)`, opacity: 0.3 },
          { opacity: 1, offset: 0.3 },
          { transform: "translate(0, 0) rotate(0) scale(1)", opacity: 1 },
        ],
        { duration: 420, delay, easing: "cubic-bezier(.2,.8,.3,1.1)", fill: "backwards" },
      );
      if (prefs().effects) window.setTimeout(cardTick, delay + 300);
    });
  });
}
