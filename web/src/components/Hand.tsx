import { useRef, useState } from "react";
import type { Picker } from "../pick";
import type { CardSlot } from "../protocol";
import { CardView } from "./CardView";

/** Display order for a hand of `n` cards: keeps a previous order and appends new cards. */
export function reconcileOrder(prev: readonly number[], n: number): number[] {
  const kept = prev.filter((i) => i < n);
  if (kept.length !== prev.length) return Array.from({ length: n }, (_, i) => i); // cards left: start over
  for (let i = 0; i < n; i++) if (!kept.includes(i)) kept.push(i);
  return kept;
}

/**
 * The viewer's own cards, big. Click a card to pick it (when the decision uses cards); drag a card
 * sideways to reorder the hand any time (playtest P1-15/16). Order is display-only.
 */
export function Hand({
  slots,
  wild,
  picker,
  order,
  onOrder,
  groups,
}: {
  slots: readonly CardSlot[];
  wild: readonly number[];
  picker: Picker | null;
  order: readonly number[];
  onOrder: (o: number[]) => void;
  groups: readonly number[];
}) {
  const row = useRef<HTMLDivElement>(null);
  const [drag, setDrag] = useState<{ pos: number; dx: number; startX: number; moved: boolean } | null>(null);
  // Grouped hands (Mama's pairs) keep their groups: no reordering.
  const grouped = groups.length > 1;
  const shown = grouped ? slots.map((_, i) => i) : order.filter((i) => i < slots.length);

  // Where a dragged card would land, from the pointer's x.
  const target = (x: number): number => {
    const els = row.current ? [...row.current.querySelectorAll<HTMLElement>("[data-pos]")] : [];
    let at = 0;
    for (const el of els) {
      if (Number(el.dataset.pos) === drag?.pos) continue;
      const r = el.getBoundingClientRect();
      if (x > r.left + r.width / 2) at++;
    }
    return at;
  };

  const groupStart = new Set<number>();
  if (grouped) {
    let k = 0;
    for (const g of groups) {
      if (k > 0) groupStart.add(k);
      k += g;
    }
  }

  return (
    <div className="hand" ref={row} role="list" aria-label="Your cards">
      {shown.map((pos, idx) => {
        const slot = slots[pos];
        const picked = picker?.sel.includes(pos) ?? false;
        const usable = picker ? picker.usable.has(pos) : false;
        const pair = picker?.pairOf(pos) ?? null;
        const seq = picker?.mode === "Sequence" && picked ? picker.sel.indexOf(pos) + 1 : null;
        const dragging = drag?.pos === pos && drag.moved;
        const classes = [
          "hand-card",
          picked ? "picked" : "",
          picker && !usable ? "dim" : "",
          picker && usable ? "pickable" : "",
          pair !== null ? `pair-${pair}` : "",
          dragging ? "dragging" : "",
          groupStart.has(idx) ? "group-gap" : "",
        ].join(" ");
        return (
          <div
            key={pos}
            role="listitem"
            data-pos={pos}
            data-deal-key={`hero-${pos}`}
            className={classes}
            style={dragging ? { transform: `translateX(${drag.dx}px) translateY(-18px) rotate(${drag.dx / 30}deg)` } : undefined}
            onPointerDown={(e) => {
              if (e.button !== 0) return;
              (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
              setDrag({ pos, dx: 0, startX: e.clientX, moved: false });
            }}
            onPointerMove={(e) => {
              if (!drag || drag.pos !== pos || grouped) return;
              const dx = e.clientX - drag.startX;
              if (drag.moved || Math.abs(dx) > 6) setDrag({ ...drag, dx, moved: true });
            }}
            onPointerUp={(e) => {
              if (!drag || drag.pos !== pos) return;
              if (drag.moved) {
                const at = target(e.clientX);
                const rest = shown.filter((p) => p !== pos);
                rest.splice(at, 0, pos);
                onOrder(rest);
              } else if (picker) {
                picker.click(pos);
              }
              setDrag(null);
            }}
            onPointerCancel={() => setDrag(null)}
            onKeyDown={(e) => {
              if ((e.key === "Enter" || e.key === " ") && picker) {
                e.preventDefault();
                picker.click(pos);
              }
            }}
            tabIndex={picker && usable ? 0 : -1}
            aria-pressed={picker ? picked : undefined}
          >
            <CardView slot={slot} size="xl" wild={wild} />
            {seq !== null && <span className="seq">{seq}</span>}
          </div>
        );
      })}
    </div>
  );
}
