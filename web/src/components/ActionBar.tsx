import { useEffect, useMemo, useState } from "react";
import { money } from "../cards";
import type { Choice, GameInfo, Prompt, PromptOption } from "../protocol";

function Countdown({ deadline, bank }: { deadline: number; bank: number }) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const t = window.setInterval(() => setNow(Date.now()), 250);
    return () => window.clearInterval(t);
  }, []);
  const left = Math.max(0, deadline - now);
  const turn = Math.max(0, left - bank);
  const inBank = turn === 0;
  return (
    <span className={`timer${inBank ? " bank" : ""}`} role="timer" aria-live="off">
      {inBank ? `time bank ${Math.ceil(left / 1000)}s` : `${Math.ceil(turn / 1000)}s`}
    </span>
  );
}

type AmountOpt = Extract<PromptOption, { kind: "Amount" }>;

function AmountPicker({ opt, onChoose }: { opt: AmountOpt; onChoose: (c: Choice) => void }) {
  const [cents, setCents] = useState(opt.presets[0]?.cents ?? opt.min);
  useEffect(() => {
    setCents(opt.presets[0]?.cents ?? opt.min);
  }, [opt]);
  const free = opt.step > 0;
  return (
    <div className="amount">
      <div className="presets">
        {opt.presets.map((p) => (
          <button
            key={p.label + p.cents}
            className={`btn bet${p.cents === cents ? " selected" : ""}`}
            onClick={() => (free ? setCents(p.cents) : onChoose({ kind: "Amount", id: opt.id, cents: p.cents }))}
          >
            {p.label}
          </button>
        ))}
      </div>
      {free && (
        <div className="slider">
          <input
            type="range"
            min={opt.min}
            max={opt.max}
            step={opt.step}
            value={cents}
            aria-label={opt.label}
            onChange={(e) => setCents(Number(e.target.value))}
          />
          <button className="btn bet primary" onClick={() => onChoose({ kind: "Amount", id: opt.id, cents })}>
            {opt.label} {money(cents)}
          </button>
        </div>
      )}
    </div>
  );
}

type GameOpt = Extract<PromptOption, { kind: "Game" }>;

function GamePicker({
  games,
  info,
  onChoose,
}: {
  games: GameOpt[];
  info: Map<string, GameInfo>;
  onChoose: (c: Choice) => void;
}) {
  const [q, setQ] = useState("");
  const [open, setOpen] = useState<string | null>(null);
  const families = useMemo(() => {
    const m = new Map<string, GameOpt[]>();
    const needle = q.trim().toLowerCase();
    for (const g of games) {
      if (needle && !g.name.toLowerCase().includes(needle) && !g.family.toLowerCase().includes(needle)) continue;
      m.set(g.family, [...(m.get(g.family) ?? []), g]);
    }
    return [...m.entries()];
  }, [games, q]);
  return (
    <div className="picker">
      <input
        type="search"
        placeholder="Search games"
        value={q}
        onChange={(e) => setQ(e.target.value)}
        aria-label="Search games"
      />
      {families.map(([family, list]) => (
        <section key={family}>
          <h4>{family}</h4>
          <ul>
            {list.map((g) => (
              <li key={g.id}>
                <div className="game-row">
                  <button className="btn game" onClick={() => onChoose({ kind: "Option", id: g.id })}>
                    {g.name}
                  </button>
                  <span className="muted">{g.seats} players</span>
                  {info.get(g.id)?.rules && (
                    <button
                      className="link"
                      aria-expanded={open === g.id}
                      onClick={() => setOpen(open === g.id ? null : g.id)}
                    >
                      rules
                    </button>
                  )}
                </div>
                {open === g.id && <p className="rules">{info.get(g.id)?.rules}</p>}
              </li>
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}

/** The viewer's decision: built from whatever options the server sends (D-WEB-1). */
export function ActionBar({
  prompt,
  games,
  onChoose,
}: {
  prompt: Prompt;
  games: Map<string, GameInfo>;
  onChoose: (c: Choice) => void;
}) {
  const buttons = prompt.options.filter((o): o is Extract<PromptOption, { kind: "Button" }> => o.kind === "Button");
  const amounts = prompt.options.filter((o): o is AmountOpt => o.kind === "Amount");
  const gameOpts = prompt.options.filter((o): o is GameOpt => o.kind === "Game");
  return (
    <div className={`action-bar${gameOpts.length > 0 ? " picking" : ""}`} role="region" aria-label="Your decision">
      <div className="prompt-title">
        <strong>{prompt.title}</strong> <Countdown deadline={prompt.deadline_ms} bank={prompt.bank_ms} />
      </div>
      {gameOpts.length > 0 && <GamePicker games={gameOpts} info={games} onChoose={onChoose} />}
      {buttons.length > 0 && (
        <div className="buttons">
          {buttons.map((b) => (
            <button key={b.id} className={`btn ${b.class}`} onClick={() => onChoose({ kind: "Option", id: b.id })}>
              {b.label}
            </button>
          ))}
        </div>
      )}
      {amounts.map((a) => (
        <AmountPicker key={a.id} opt={a} onChoose={onChoose} />
      ))}
    </div>
  );
}
