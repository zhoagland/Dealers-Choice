import { useEffect, useMemo, useRef, useState } from "react";
import { api } from "../api";
import { useDealAnimation } from "../anim";
import { money } from "../cards";
import { ActionBar } from "../components/ActionBar";
import { rankIndex } from "../components/CardView";
import { Chat } from "../components/Chat";
import { Hand, reconcileOrder } from "../components/Hand";
import { OvalTable } from "../components/OvalTable";
import { PrefsMenu } from "../components/PrefsMenu";
import { HandList, ReplayView } from "../components/Replay";
import { usePicker } from "../pick";
import { prefs } from "../prefs";
import type {
  ChatLine, Choice, GameInfo, HandListing, HandSummary, Me, Prompt, SeatResult, ServerMsg,
  TableInfo, TableView,
} from "../protocol";
import { TableSocket, type SocketState } from "../socket";
import { turnChime, winJingle } from "../sound";

type Tab = "chat" | "log" | "hands" | "settle";

const STATE_LABEL: Record<string, string> = {
  waiting: "Waiting for players",
  choosing: "Choosing a game",
  playing: "Playing",
  break: "Break",
  closed: "Closed",
};

function useNow(on: boolean) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (!on) return;
    const t = window.setInterval(() => setNow(Date.now()), 500);
    return () => window.clearInterval(t);
  }, [on]);
  return now;
}

/** The end of a hand, big, in the middle of the table (playtest P1-5). */
function ResultBanner({ summary, onReplay, onClose }: { summary: HandSummary; onReplay: () => void; onClose: () => void }) {
  const winners = summary.results.filter((r) => r.cents > 0);
  return (
    <div className={`result-banner${summary.showdown ? " showdown" : ""}`} role="status">
      <div className="result-title">{summary.showdown ? "Showdown!" : "Hand over"}</div>
      <div className="result-winners">
        {winners.length === 0
          ? "No winner this hand"
          : winners.map((w) => (
              <div key={w.name} className="winner">
                {w.name} wins <strong>{money(w.cents)}</strong>
              </div>
            ))}
      </div>
      <div className="result-rest">
        {summary.results
          .filter((r) => r.cents <= 0)
          .map((r) => (
            <span key={r.name} className={r.cents < 0 ? "loss" : "muted"}>
              {r.name} {r.cents < 0 ? money(r.cents) : "even"}
            </span>
          ))}
      </div>
      <div className="result-actions">
        <button className="link" onClick={onReplay}>
          Replay
        </button>
        <button className="link" onClick={onClose} aria-label="Dismiss">
          Close
        </button>
      </div>
    </div>
  );
}

export function TablePage({ id, me, embedded }: { id: string; me: Me; embedded: boolean }) {
  const [info, setInfo] = useState<TableInfo | null>(null);
  const [view, setView] = useState<TableView | null>(null);
  const [prompt, setPrompt] = useState<Prompt | null>(null);
  const [you, setYou] = useState<number | null>(null);
  const [chat, setChat] = useState<ChatLine[]>([]);
  const [summary, setSummary] = useState<HandSummary | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [conn, setConn] = useState<SocketState>("connecting");
  const [tab, setTab] = useState<Tab>("chat");
  const [games, setGames] = useState<Map<string, GameInfo>>(new Map());
  const [hands, setHands] = useState<HandListing[]>([]);
  const [settle, setSettle] = useState<SeatResult[]>([]);
  const [replay, setReplay] = useState<number | null>(null);
  const [unread, setUnread] = useState(0);
  const [rulesOpen, setRulesOpen] = useState(true);
  const [order, setOrder] = useState<number[]>([]);
  const [sentId, setSentId] = useState<number | null>(null);
  const sock = useRef<TableSocket | null>(null);
  const root = useRef<HTMLDivElement>(null);

  useEffect(() => {
    api.games().then((g) => setGames(new Map(g.map((x) => [x.id, x]))), () => {});
  }, []);

  useEffect(() => {
    const onMsg = (m: ServerMsg) => {
      switch (m.type) {
        case "Snapshot":
          setInfo(m.table);
          setView(m.view);
          setYou(m.you);
          setPrompt(m.prompt);
          break;
        case "Chat":
          setChat((c) => [...c.filter((x) => x.id !== m.line.id), m.line].slice(-200));
          setUnread((n) => n + 1);
          break;
        case "ChatDeleted":
          setChat((c) => c.filter((x) => x.id !== m.id));
          break;
        case "HandSummary":
          setSummary(m.summary);
          break;
        case "Error":
          setNotice(m.message);
          window.setTimeout(() => setNotice(null), 4000);
          break;
        case "Pong":
          break;
      }
    };
    const s = new TableSocket(id, onMsg, setConn);
    sock.current = s;
    return () => s.close();
  }, [id]);

  useEffect(() => {
    if (tab === "hands") api.tableHands(id).then(setHands, () => {});
    if (tab === "settle") api.settle(id).then(setSettle, () => {});
    if (tab === "chat") setUnread(0);
  }, [tab, id, summary]);

  // A new prompt: chime (playtest P1-12).
  useEffect(() => {
    if (prompt && prefs().turnSound) turnChime();
  }, [prompt?.id]); // eslint-disable-line react-hooks/exhaustive-deps

  // The result banner: a win jingle; it clears when the next hand's cards arrive.
  useEffect(() => {
    if (!summary) return;
    const mine = summary.results.find((r) => r.name === me.name);
    if (mine && mine.cents > 0 && prefs().effects) winJingle();
    const t = window.setTimeout(() => setSummary(null), summary.showdown ? 7000 : 3000);
    return () => window.clearTimeout(t);
  }, [summary, me.name]);

  const handNo = info?.hand_no ?? 0;
  const inHand = you !== null && !!info && info.hand_seats.includes(you);
  const heroView = inHand && view ? (view.seats.find((s) => s.seat === view.viewer) ?? null) : null;
  const heroCount = heroView?.cards.length ?? 0;
  const handOrder = reconcileOrder(order, heroCount);
  useEffect(() => setOrder([]), [handNo]);

  const send = (m: Parameters<TableSocket["send"]>[0]) => sock.current?.send(m);
  const live = prompt && sentId !== prompt.id ? prompt : null;
  const choose = (choice: Choice) => {
    if (!live) return;
    setSentId(live.id);
    send({ type: "Act", prompt_id: live.id, choice });
  };
  const picker = usePicker(live, choose, heroView?.groups);
  useDealAnimation(root, view ? handNo : null);
  const now = useNow(!!info?.next_game_ms);

  const wild = useMemo(
    () => (view && heroView ? view.wilds.filter((w) => w.seat === null || w.seat === heroView.seat).map((w) => rankIndex(w.rank)) : []),
    [view, heroView],
  );

  if (!info) {
    return <p className="muted center">{conn === "closed" ? "Can't reach the table; retrying…" : "Joining table…"}</p>;
  }

  const host = info.host === me.id || me.is_admin;
  const seated = you !== null;
  const mySeat = info.seats.find((s) => s.seat === you);
  const invite = `${window.location.origin}${window.location.pathname}#/t/${info.id}`;
  const rules = info.game ? games.get(info.game) : undefined;
  const chooserName = info.seats.find((s) => s.seat === info.chooser)?.name ?? "The chooser";

  // What the middle of the table says when no hand is on.
  let center: React.ReactNode = null;
  if (summary) {
    center = (
      <ResultBanner
        summary={summary}
        onReplay={() => {
          setReplay(summary.hand_id);
          setTab("hands");
        }}
        onClose={() => setSummary(null)}
      />
    );
  } else if (!view) {
    const secs = info.next_game_ms ? Math.max(0, Math.ceil((info.next_game_ms - now) / 1000)) : 0;
    center = (
      <div className="center-msg">
        {info.state === "waiting" && (
          <>
            <div className="big-msg">{info.seats.length < 2 ? "Waiting for players" : host ? "Ready when you are" : "Waiting for the host"}</div>
            <div className="muted">{info.seats.length < 2 ? "At least two players need a seat." : host ? "Press Start when everyone is seated." : ""}</div>
            {host && info.seats.length >= 2 && (
              <button className="btn primary big" onClick={() => send({ type: "Start" })}>
                Start
              </button>
            )}
          </>
        )}
        {info.state === "break" && (
          <>
            <div className="big-msg">Next game in {secs}s</div>
            <div className="muted">Grab a seat now to be dealt in. {chooserName} chooses next.</div>
            {host && (
              <button className="btn primary" onClick={() => send({ type: "Start" })}>
                Start now
              </button>
            )}
          </>
        )}
        {info.state === "choosing" && !live && <div className="big-msg">{chooserName} is choosing the game…</div>}
        {info.state === "closed" && <div className="big-msg">This table is closed</div>}
      </div>
    );
  }

  return (
    <div className="table-page" ref={root}>
      <header className="table-head">
        <div className="title">
          <h2>{info.name}</h2>
          <span className="muted">
            {STATE_LABEL[info.state] ?? info.state}
            {info.game_name ? ` · ${info.game_name}` : ""}
            {info.hand_no ? ` · Hand ${info.hand_no}` : ""}
            {conn !== "open" ? " · Reconnecting…" : ""}
          </span>
        </div>
        <div className="head-actions">
          {!embedded && (
            <button className="btn" onClick={() => void navigator.clipboard?.writeText(invite)} title={invite}>
              Copy invite link
            </button>
          )}
          {seated && (
            <button className="btn" onClick={() => send({ type: "Away", away: !mySeat?.away })}>
              {mySeat?.away ? "I'm back" : "Sit out"}
            </button>
          )}
          {seated && (
            <button className="btn" onClick={() => send({ type: "Stand" })}>
              Stand up
            </button>
          )}
          {host && !embedded && (
            <button
              className="btn danger"
              onClick={() => {
                if (window.confirm("Close this table for everyone?")) void api.closeTable(info.id).then(() => (window.location.hash = "#/"));
              }}
            >
              Close table
            </button>
          )}
          {embedded && <PrefsMenu />}
        </div>
      </header>

      {notice && (
        <div className="notice" role="alert">
          {notice}
        </div>
      )}

      <div className="table-main">
        <div className="play">
          <div className="arena">
            <OvalTable
              info={info}
              view={view}
              you={you}
              canSit={!seated}
              onSit={(seat) => send({ type: "Sit", seat })}
              center={center}
            />
          </div>
          <div className={`hero-zone${live ? " my-turn" : ""}`}>
            {heroView && view && (
              <Hand slots={heroView.cards} wild={wild} picker={picker} order={handOrder} onOrder={setOrder} groups={heroView.groups} />
            )}
            {!inHand && view && (
              <p className="muted watch">{seated ? "You're sitting this one out; you'll be dealt in next game." : "Watching. Pick an empty seat to play the next game."}</p>
            )}
            {live && <ActionBar prompt={live} games={games} onChoose={choose} picker={picker} />}
          </div>
        </div>

        <aside className="side">
          {rules && (
            <section className="rules-panel">
              <button className="rules-head" aria-expanded={rulesOpen} onClick={() => setRulesOpen(!rulesOpen)}>
                <span>Rules: {rules.name}</span>
                <span aria-hidden>{rulesOpen ? "▾" : "▸"}</span>
              </button>
              {rulesOpen && <p className="rules-text">{rules.rules}</p>}
            </section>
          )}
          <nav className="tabs" role="tablist">
            {(["chat", "log", "hands", "settle"] as Tab[]).map((t) => (
              <button key={t} role="tab" aria-selected={tab === t} className={tab === t ? "active" : ""} onClick={() => setTab(t)}>
                {t[0].toUpperCase() + t.slice(1)}
                {t === "chat" && unread > 0 && tab !== "chat" ? ` (${unread})` : ""}
              </button>
            ))}
          </nav>
          <div className="tab-body">
            {tab === "chat" && (
              <Chat lines={chat} admin={me.is_admin} onSend={(text) => send({ type: "Chat", text })} onDelete={(id) => send({ type: "DeleteChat", id })} />
            )}
            {tab === "log" && (
              <ul className="log">
                {info.log.map((l, i) => (
                  <li key={i}>{l}</li>
                ))}
              </ul>
            )}
            {tab === "hands" && (replay !== null ? <ReplayView id={replay} onClose={() => setReplay(null)} /> : <HandList hands={hands} onOpen={setReplay} />)}
            {tab === "settle" && (
              <div>
                <p className="muted">Net results at this table over the last 24 hours (play money).</p>
                <ul className="settle">
                  {settle.map((r) => (
                    <li key={r.name} className={r.cents > 0 ? "win" : r.cents < 0 ? "loss" : ""}>
                      {r.name}: {r.cents > 0 ? "+" : ""}
                      {money(r.cents)}
                    </li>
                  ))}
                </ul>
              </div>
            )}
          </div>
        </aside>
      </div>
    </div>
  );
}
