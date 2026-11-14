import { capFirst, money } from "../cards";
import type { SeatView, TableView } from "../protocol";
import { Board } from "./Board";
import { CardView, rankIndex } from "./CardView";

export interface SeatLabel {
  name: string;
  away?: boolean;
  connected?: boolean;
}

const STATUS: Record<string, string> = { Folded: "Folded", AllIn: "All in", Out: "Out" };

function wildFor(view: TableView, seat: number): number[] {
  return view.wilds
    .filter((w) => w.seat === null || w.seat === seat)
    .map((w) => rankIndex(w.rank));
}

function SeatBox({
  s,
  label,
  view,
  hero,
}: {
  s: SeatView;
  label: SeatLabel;
  view: TableView;
  hero: boolean;
}) {
  const acting = view.to_act === s.seat;
  const classes = [
    "seat",
    hero ? "hero" : "",
    acting ? "acting" : "",
    s.status !== "Active" ? "inactive" : "",
    label.connected === false ? "offline" : "",
  ].join(" ");
  return (
    <section className={classes} aria-label={`${label.name}${acting ? ", to act" : ""}`}>
      <header>
        <span className="avatar" aria-hidden>
          {label.name.slice(0, 1).toUpperCase()}
        </span>
        <span className="name">{label.name}</span>
        {view.button === s.seat && (
          <span className="badge dealer" title="dealer button">
            D
          </span>
        )}
        {s.badges.map((b) => (
          <span key={b} className="badge">
            {capFirst(b)}
          </span>
        ))}
        {label.away && <span className="badge away">Away</span>}
        {label.connected === false && <span className="badge away">Offline</span>}
      </header>
      <div className="cards">
        {s.cards.map((c, i) => (
          <CardView key={i} slot={c} size={hero ? "lg" : "sm"} wild={wildFor(view, s.seat)} />
        ))}
      </div>
      <footer>
        <span className="stack">{money(s.stack)}</span>
        {s.bet > 0 && <span className="bet">Bet {money(s.bet)}</span>}
        {STATUS[s.status] && <span className="status">{STATUS[s.status]}</span>}
      </footer>
    </section>
  );
}

/**
 * Draws any game from its `TableView` (FR-WB-11): opponents across the top in seat order from
 * the viewer's left, the boards, pots and wild cards in the middle, the viewer at the bottom.
 */
export function TableRenderer({ view, labels }: { view: TableView; labels: (seat: number) => SeatLabel }) {
  const n = view.seats.length;
  const hero = view.seats.find((s) => s.seat === view.viewer);
  const others = [...view.seats]
    .filter((s) => s.seat !== view.viewer)
    .sort((a, b) => ((a.seat - view.viewer + n) % n) - ((b.seat - view.viewer + n) % n));
  const tableWild = view.wilds.filter((w) => w.seat === null).map((w) => rankIndex(w.rank));
  return (
    <div className="felt">
      <div className="opponents">
        {others.map((s) => (
          <SeatBox key={s.seat} s={s} label={labels(s.seat)} view={view} hero={false} />
        ))}
      </div>
      <div className="middle">
        <div className="phase">
          <strong>{view.game}</strong> · {view.phase}
        </div>
        {view.boards.map((b, i) => (
          <Board key={i} board={b} wild={tableWild} />
        ))}
        <div className="pots">
          {view.pots.map((p, i) => (
            <span key={i} className="pot">
              {p.name}: {money(p.amount)}
            </span>
          ))}
        </div>
        {view.wilds.length > 0 && (
          <div className="wilds">
            Wild:{" "}
            {view.wilds
              .map((w) => (w.seat === null ? w.rank : `${w.rank} (${labels(w.seat).name})`))
              .join(", ")}
          </div>
        )}
        {view.notes.length > 0 && (
          <ul className="notes">
            {view.notes.map((t, i) => (
              <li key={i}>{t}</li>
            ))}
          </ul>
        )}
      </div>
      {hero && <SeatBox s={hero} label={labels(hero.seat)} view={view} hero />}
    </div>
  );
}
