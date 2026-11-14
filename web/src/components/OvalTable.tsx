import type { ReactNode } from "react";
import { appUrl } from "../api";
import { capFirst, money } from "../cards";
import type { SeatInfo, SeatView, TableInfo, TableView } from "../protocol";
import { Board } from "./Board";
import { CardView, rankIndex } from "./CardView";

const STATUS: Record<string, string> = { Folded: "Folded", AllIn: "All in", Out: "Out" };

function wildFor(view: TableView, seat: number): number[] {
  return view.wilds.filter((w) => w.seat === null || w.seat === seat).map((w) => rankIndex(w.rank));
}

export function Avatar({ seat, size = 56 }: { seat: Pick<SeatInfo, "name" | "avatar" | "account">; size?: number }) {
  const initial = seat.name.slice(0, 1).toUpperCase();
  // Hue from the name, so everyone keeps a colour.
  const hue = [...seat.name].reduce((h, c) => (h * 31 + c.charCodeAt(0)) % 360, 7);
  return (
    <span className="avatar" style={{ width: size, height: size, background: `hsl(${hue} 55% 42%)` }} aria-hidden>
      {initial}
      {seat.avatar && seat.account !== null && (
        <img src={appUrl(`/api/avatar/${seat.account}`)} alt="" loading="lazy" onError={(e) => (e.currentTarget.style.display = "none")} />
      )}
    </span>
  );
}

/** Where table seat `i` of `n` sits on the oval, with `bottom` at the bottom (viewer's seat). */
function spot(i: number, n: number, bottom: number) {
  const k = (i - bottom + n) % n;
  const a = Math.PI / 2 + (2 * Math.PI * k) / n; // clockwise on screen: bottom, left, top, right
  return { cos: Math.cos(a), sin: Math.sin(a) };
}

const at = (cos: number, sin: number, rx: number, ry: number) => ({
  left: `${50 + rx * cos}%`,
  top: `${50 + ry * sin}%`,
});

function Plate({
  info,
  sv,
  view,
  hero,
  showCards,
  acting,
}: {
  info: SeatInfo;
  sv: SeatView | null;
  view: TableView | null;
  hero: boolean;
  showCards: boolean;
  acting: boolean;
}) {
  const out = sv && sv.status !== "Active" && sv.status !== "AllIn";
  const classes = ["plate", hero ? "hero" : "", acting ? "acting" : "", out ? "out" : "", info.connected ? "" : "offline"].join(" ");
  return (
    <div className={classes} aria-label={`${info.name}${acting ? ", to act" : ""}`}>
      <div className="plate-top">
        <Avatar seat={info} />
        {view && sv && view.button === sv.seat && (
          <span className="dealer-chip" title="Dealer">
            D
          </span>
        )}
        {sv?.tricks !== null && sv?.tricks !== undefined && (
          <span className="tricks" title="Tricks taken">
            {sv.tricks} {sv.tricks === 1 ? "trick" : "tricks"}
          </span>
        )}
      </div>
      <div className="nameplate">
        <span className="name">{info.name}</span>
        <span className="stack">{money(sv ? sv.stack : info.bankroll)}</span>
      </div>
      <div className="badges">
        {sv && STATUS[sv.status] && <span className="badge status">{STATUS[sv.status]}</span>}
        {sv?.badges.map((b) => (
          <span key={b} className="badge">
            {capFirst(b)}
          </span>
        ))}
        {info.away && <span className="badge away">Sitting out</span>}
        {!info.connected && <span className="badge away">Offline</span>}
      </div>
      {showCards && sv && sv.cards.length > 0 && view && (
        <div className="seat-cards">
          {sv.cards.map((c, i) => (
            <span key={i} data-deal-key={c === "Empty" ? undefined : `s${info.seat}-${i}`}>
              <CardView slot={c} size="sm" wild={wildFor(view, sv.seat)} />
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

/**
 * The live table (playtest P1-1/3): an oval felt, everyone around it with their Discord picture,
 * the deck, boards and pot in the middle, each seat's bet and trick cards in front of it. The
 * viewer sits at the bottom; their own cards are drawn bigger below the table by the page.
 */
export function OvalTable({
  info,
  view,
  you,
  canSit,
  onSit,
  center,
}: {
  info: TableInfo;
  view: TableView | null;
  you: number | null;
  canSit: boolean;
  onSit: (seat: number) => void;
  center?: ReactNode;
}) {
  // Once you're seated, only the taken seats are drawn, spread evenly round the table; while
  // you're choosing a seat, every seat is.
  const shownSeats = canSit || info.seats.length === 0 ? Array.from({ length: info.max_seats }, (_, i) => i) : info.seats.map((s) => s.seat).sort((a, b) => a - b);
  const n = shownSeats.length;
  const bottom = Math.max(0, shownSeats.indexOf(you ?? shownSeats[0]));
  const inHand = you !== null && info.hand_seats.includes(you);
  const handSeat = (t: number) => {
    const k = info.hand_seats.indexOf(t);
    return k >= 0 && view ? (view.seats.find((s) => s.seat === k) ?? null) : null;
  };
  const tableWild = view ? view.wilds.filter((w) => w.seat === null).map((w) => rankIndex(w.rank)) : [];
  const toAct = view?.to_act ?? null;
  return (
    <div className="oval-wrap">
      <div className="felt-oval" />
      <div className="felt-center">
        <div className="deck" aria-hidden>
          <span className="card back sm" />
          <span className="card back sm" />
          <span className="card back sm" />
        </div>
        {view && (
          <>
            <div className="phase-pill">
              {view.game} · {view.phase}
            </div>
            <div className="boards">
              {view.boards.map((b, i) => (
                <Board key={i} board={b} wild={tableWild} dealKey={`b${i}`} />
              ))}
            </div>
            <div className="pots">
              {view.pots.map((p, i) => (
                <span key={i} className="pot">
                  <span className="chip-stack" aria-hidden />
                  {view.pots.length > 1 ? `${p.name}: ` : ""}
                  {money(p.amount)}
                </span>
              ))}
            </div>
            {view.wilds.length > 0 && (
              <div className="wilds">
                Wild:{" "}
                {view.wilds
                  .map((w) =>
                    w.seat === null ? w.rank : `${w.rank} (${info.seats.find((s) => s.seat === info.hand_seats[w.seat!])?.name ?? "?"})`,
                  )
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
          </>
        )}
        {center}
      </div>
      {shownSeats.map((t, idx) => {
        const p = spot(idx, n, bottom);
        const seated = info.seats.find((s) => s.seat === t);
        const sv = handSeat(t);
        const hero = t === you;
        const acting = sv !== null && toAct === sv.seat;
        return (
          <div key={t}>
            <div className={`seat-spot${hero ? " me" : ""}`} style={at(p.cos, p.sin, 44, 41)}>
              {seated ? (
                <Plate
                  info={seated}
                  sv={sv}
                  view={view}
                  hero={hero}
                  showCards={!(hero && inHand)}
                  acting={acting}
                />
              ) : canSit ? (
                <button className="btn sit" onClick={() => onSit(t)}>
                  Sit here
                </button>
              ) : (
                <span className="empty-seat" aria-label="Empty seat" />
              )}
            </div>
            {sv && sv.bet > 0 && (
              <div className="bet-spot" style={at(p.cos, p.sin, 27, 24)}>
                <span className="chip-stack" aria-hidden />
                {money(sv.bet)}
              </div>
            )}
            {sv && sv.played.length > 0 && view && (
              <div className="played-spot" style={at(p.cos, p.sin, 19, 15)} aria-label={`${seated?.name ?? "Seat"} played`}>
                {sv.played.map((c, i) => (
                  <CardView key={i} slot={{ Up: c }} size="md" wild={wildFor(view, sv.seat)} />
                ))}
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}
