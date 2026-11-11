import { useEffect, useMemo, useState } from "react";
import { api } from "../api";
import type { GameInfo, Me, TableListing } from "../protocol";

function CreateForm({ games, onCreated }: { games: GameInfo[]; onCreated: (t: TableListing) => void }) {
  const [name, setName] = useState("Friday night");
  const [seats, setSeats] = useState(6);
  const [timer, setTimer] = useState(30);
  const [listed, setListed] = useState(false);
  const [all, setAll] = useState(true);
  const [pool, setPool] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const families = useMemo(() => {
    const m = new Map<string, GameInfo[]>();
    for (const g of games) m.set(g.family, [...(m.get(g.family) ?? []), g]);
    return [...m.entries()];
  }, [games]);
  const toggle = (id: string) => {
    const p = new Set(pool);
    if (p.has(id)) p.delete(id);
    else p.add(id);
    setPool(p);
  };
  return (
    <form
      className="card-panel"
      onSubmit={(e) => {
        e.preventDefault();
        setError(null);
        api
          .createTable({ name, max_seats: seats, timer_secs: timer, listed, pool: all ? [] : [...pool] })
          .then(onCreated, (e: Error) => setError(e.message));
      }}
    >
      <h3>New table</h3>
      <label>
        Name <input value={name} maxLength={40} onChange={(e) => setName(e.target.value)} required />
      </label>
      <label>
        Seats{" "}
        <select value={seats} onChange={(e) => setSeats(Number(e.target.value))}>
          {[2, 3, 4, 5, 6, 7, 8, 9].map((n) => (
            <option key={n}>{n}</option>
          ))}
        </select>
      </label>
      <label>
        Turn timer{" "}
        <select value={timer} onChange={(e) => setTimer(Number(e.target.value))}>
          {[15, 20, 30, 45, 60, 90].map((n) => (
            <option key={n} value={n}>
              {n} s
            </option>
          ))}
        </select>
      </label>
      <label className="check">
        <input type="checkbox" checked={listed} onChange={(e) => setListed(e.target.checked)} /> Show in the lobby
        (otherwise invite link only)
      </label>
      <label className="check">
        <input type="checkbox" checked={all} onChange={(e) => setAll(e.target.checked)} /> Every game (
        {games.length})
      </label>
      {!all && (
        <div className="pool">
          {families.map(([family, list]) => (
            <fieldset key={family}>
              <legend>{family}</legend>
              {list.map((g) => (
                <label key={g.id} className="check">
                  <input type="checkbox" checked={pool.has(g.id)} onChange={() => toggle(g.id)} /> {g.name}
                </label>
              ))}
            </fieldset>
          ))}
        </div>
      )}
      {error && <p className="error">{error}</p>}
      <button className="btn primary" disabled={!all && pool.size === 0}>
        Create table
      </button>
    </form>
  );
}

export function Lobby({ me }: { me: Me }) {
  const [tables, setTables] = useState<TableListing[] | null>(null);
  const [games, setGames] = useState<GameInfo[]>([]);
  const [creating, setCreating] = useState(false);
  useEffect(() => {
    const load = () => api.tables().then(setTables, () => {});
    load();
    api.games().then(setGames, () => {});
    const t = window.setInterval(load, 5000);
    return () => window.clearInterval(t);
  }, []);
  return (
    <div className="lobby">
      <div className="lobby-head">
        <h2>Tables</h2>
        <button className="btn primary" onClick={() => setCreating(!creating)}>
          {creating ? "Cancel" : "New table"}
        </button>
      </div>
      {creating && <CreateForm games={games} onCreated={(t) => (window.location.hash = `#/t/${t.id}`)} />}
      {tables === null ? (
        <p className="muted">Loading…</p>
      ) : tables.length === 0 ? (
        <p className="muted">No open tables. Start one, or open an invite link from a friend.</p>
      ) : (
        <ul className="tables">
          {tables.map((t) => (
            <li key={t.id}>
              <a href={`#/t/${t.id}`} className="table-link">
                <strong>{t.name}</strong>
                <span className="muted">
                  {t.players}/{t.max_seats} · {t.state}
                  {!t.listed ? " · private" : ""}
                </span>
              </a>
            </li>
          ))}
        </ul>
      )}
      <p className="muted">
        Signed in as {me.name}. <a href="#/hands">My hands</a>
        {me.is_admin && (
          <>
            {" · "}
            <a href="#/admin">Admin</a>
          </>
        )}
      </p>
    </div>
  );
}
