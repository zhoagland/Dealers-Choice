import type { BoardView } from "../protocol";
import { CardView } from "./CardView";

type Pos = { r: number; c: number };

/**
 * Grid positions for a board's slots, from its layout and lines (any game, no per-game code):
 * the first line runs across; each later line runs down through the slot it shares with an
 * earlier one. H boards have two columns joined by a bar; Wheel boards are a ring.
 */
function place(b: BoardView): Map<number, Pos> | null {
  const n = b.slots.length;
  const at = new Map<number, Pos>();
  if (b.layout === "H" && n === 7) {
    const h: Pos[] = [
      { r: 0, c: 0 }, { r: 1, c: 0 }, { r: 2, c: 0 }, { r: 1, c: 1 },
      { r: 0, c: 2 }, { r: 1, c: 2 }, { r: 2, c: 2 },
    ];
    h.forEach((p, i) => at.set(i, p));
    return at;
  }
  if (b.layout === "Row" || b.layout === "Middle" || b.layout === "Wheel" || b.lines.length === 0) {
    return null;
  }
  b.lines.forEach((line, k) => {
    if (k === 0) {
      line.forEach((s, i) => at.set(s, { r: 0, c: i }));
      return;
    }
    const pivot = line.findIndex((s) => at.has(s));
    if (pivot < 0) return;
    const base = at.get(line[pivot])!;
    line.forEach((s, i) => {
      if (!at.has(s)) at.set(s, { r: base.r + (i - pivot), c: base.c });
    });
  });
  for (let s = 0; s < n; s++) if (!at.has(s)) return null;
  const minR = Math.min(...[...at.values()].map((p) => p.r));
  for (const [s, p] of at) at.set(s, { r: p.r - minR, c: p.c });
  return at;
}

export function Board({ board, wild }: { board: BoardView; wild: readonly number[] }) {
  if (board.layout === "Wheel") {
    const n = board.slots.length;
    return (
      <figure className="board wheel" aria-label={board.name}>
        <figcaption>{board.name}</figcaption>
        <div className="ring">
          {board.slots.map((slot, i) => {
            const a = (2 * Math.PI * i) / n - Math.PI / 2;
            return (
              <span
                key={i}
                className="ring-slot"
                style={{ left: `${50 + 40 * Math.cos(a)}%`, top: `${50 + 40 * Math.sin(a)}%` }}
              >
                <CardView slot={slot} size="sm" wild={wild} />
              </span>
            );
          })}
        </div>
      </figure>
    );
  }
  const grid = place(board);
  if (!grid) {
    return (
      <figure className="board row" aria-label={board.name}>
        <figcaption>{board.name}</figcaption>
        <div className="cards">
          {board.slots.map((slot, i) => (
            <CardView key={i} slot={slot} wild={wild} />
          ))}
        </div>
      </figure>
    );
  }
  return (
    <figure className="board shape" aria-label={board.name}>
      <figcaption>{board.name}</figcaption>
      <div className="shape-grid">
        {board.slots.map((slot, i) => {
          const p = grid.get(i)!;
          return (
            <span key={i} style={{ gridRow: p.r + 1, gridColumn: p.c + 1 }}>
              <CardView slot={slot} size="sm" wild={wild} />
            </span>
          );
        })}
      </div>
    </figure>
  );
}
