import { useEffect, useMemo, useRef, useState } from "react";
import { api } from "../api";
import { money } from "../cards";
import { ActionBar } from "../components/ActionBar";
import { Chat } from "../components/Chat";
import { HandList, ReplayView } from "../components/Replay";
import { TableRenderer } from "../components/TableRenderer";
import type {
  ChatLine, Choice, GameInfo, HandListing, HandSummary, Me, Prompt, SeatResult, ServerMsg,
  TableInfo, TableView,
} from "../protocol";
import { TableSocket, type SocketState } from "../socket";

type Tab = "chat" | "log" | "hands" | "settle";

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
  const sock = useRef<TableSocket | null>(null);
  const sent = useRef<number | null>(null);

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
          if (m.prompt && sent.current !== m.prompt.id) sent.current = null;
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

  const labels = useMemo(() => {
    return (handSeat: number) => {
      const t = info?.hand_seats[handSeat];
      const s = info?.seats.find((x) => x.seat === t);
      return s ? { name: s.name, away: s.away, connected: s.connected } : { name: `Seat ${handSeat + 1}` };
    };
  }, [info]);

  if (!info) {
    return <p className="muted center">{conn === "closed" ? "Can't reach the table; retrying…" : "Joining table…"}</p>;
  }

  const send = (m: Parameters<TableSocket["send"]>[0]) => sock.current?.send(m);
  const choose = (choice: Choice) => {
    if (!prompt || sent.current === prompt.id) return;
    sent.current = prompt.id;
    send({ type: "Act", prompt_id: prompt.id, choice });
  };
  const host = info.host === me.id || me.is_admin;
  const seated = you !== null;
  const mySeat = info.seats.find((s) => s.seat === you);
  const invite = `${window.location.origin}${window.location.pathname}#/t/${info.id}`;

  return (
    <div className="table-page">
      <header className="table-head">
        <div>
          <h2>{info.name}</h2>
          <span className="muted">
            {info.state}
            {info.game_name ? ` · ${info.game_name}` : ""}
            {info.hand_no ? ` · hand ${info.hand_no}` : ""}
            {conn !== "open" ? " · reconnecting…" : ""}
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
          {host && info.state === "waiting" && (
            <button className="btn primary" onClick={() => send({ type: "Start" })}>
              Start
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
        </div>
      </header>

      {notice && (
        <div className="notice" role="alert">
          {notice}
        </div>
      )}

      <div className="table-main">
        <div className="play">
          {view ? (
            <TableRenderer view={view} labels={labels} />
          ) : (
            <div className="seats-grid">
              {Array.from({ length: info.max_seats }, (_, i) => {
                const s = info.seats.find((x) => x.seat === i);
                return (
                  <div key={i} className={`seat-slot${s ? " taken" : ""}${info.chooser === i ? " chooser" : ""}`}>
                    {s ? (
                      <>
                        <span className="avatar" aria-hidden>
                          {s.name.slice(0, 1).toUpperCase()}
                        </span>
                        <strong>{s.name}</strong>
                        <span className="muted">{money(s.bankroll)}</span>
                        {s.away && <span className="badge away">away</span>}
                        {!s.connected && <span className="badge away">offline</span>}
                        {info.chooser === i && <span className="badge">chooser</span>}
                      </>
                    ) : seated ? (
                      <span className="muted">empty</span>
                    ) : (
                      <button className="btn primary" onClick={() => send({ type: "Sit", seat: i })}>
                        Sit here
                      </button>
                    )}
                  </div>
                );
              })}
              {info.state === "waiting" && (
                <p className="muted wide">
                  {info.seats.length < 2
                    ? "Waiting for at least two players to sit."
                    : host
                      ? "Press Start when everyone is seated."
                      : "Waiting for the host to start."}
                </p>
              )}
              {info.state === "choosing" && !prompt && (
                <p className="muted wide">
                  {info.seats.find((s) => s.seat === info.chooser)?.name ?? "The chooser"} is choosing the game…
                </p>
              )}
            </div>
          )}
          {summary && (
            <div className="summary" role="status">
              <strong>Hand over</strong>
              {summary.results.map((r, i) => (
                <span key={i} className={r.cents > 0 ? "win" : r.cents < 0 ? "loss" : ""}>
                  {r.name} {r.cents > 0 ? "+" : ""}
                  {money(r.cents)}
                </span>
              ))}
              <button className="link" onClick={() => { setReplay(summary.hand_id); setTab("hands"); }}>
                replay
              </button>
              <button className="link" onClick={() => setSummary(null)} aria-label="dismiss">
                ×
              </button>
            </div>
          )}
          {prompt && sent.current !== prompt.id && <ActionBar prompt={prompt} games={games} onChoose={choose} />}
        </div>

        <aside className="side">
          <nav className="tabs" role="tablist">
            {(["chat", "log", "hands", "settle"] as Tab[]).map((t) => (
              <button key={t} role="tab" aria-selected={tab === t} className={tab === t ? "active" : ""} onClick={() => setTab(t)}>
                {t}
                {t === "chat" && unread > 0 && tab !== "chat" ? ` (${unread})` : ""}
              </button>
            ))}
          </nav>
          {tab === "chat" && (
            <Chat
              lines={chat}
              admin={me.is_admin}
              onSend={(text) => send({ type: "Chat", text })}
              onDelete={(id) => send({ type: "DeleteChat", id })}
            />
          )}
          {tab === "log" && (
            <ul className="log">
              {info.log.map((l, i) => (
                <li key={i}>{l}</li>
              ))}
            </ul>
          )}
          {tab === "hands" &&
            (replay !== null ? (
              <ReplayView id={replay} onClose={() => setReplay(null)} />
            ) : (
              <HandList hands={hands} onOpen={setReplay} />
            ))}
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
        </aside>
      </div>
    </div>
  );
}
