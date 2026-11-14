import { useEffect, useState } from "react";
import { api } from "../api";
import { money } from "../cards";
import type { HandListing, HandReplay } from "../protocol";
import { TableRenderer } from "./TableRenderer";

/** A list of hands; picking one replays it step by step from the viewer's seat (FR-WB-8). */
export function HandList({ hands, onOpen }: { hands: readonly HandListing[]; onOpen: (id: number) => void }) {
  if (hands.length === 0) return <p className="muted">No hands yet.</p>;
  return (
    <ul className="hand-list">
      {hands.map((h) => (
        <li key={h.id}>
          <button className="link" onClick={() => onOpen(h.id)}>
            #{h.id} {h.game}
          </button>{" "}
          <span className="muted">{new Date(h.at * 1000).toLocaleString()}</span>
          <div className="results">
            {h.results.map((r, i) => (
              <span key={i} className={r.cents > 0 ? "win" : r.cents < 0 ? "loss" : ""}>
                {r.name} {r.cents > 0 ? "+" : ""}
                {money(r.cents)}
              </span>
            ))}
          </div>
        </li>
      ))}
    </ul>
  );
}

export function ReplayView({ id, onClose }: { id: number; onClose: () => void }) {
  const [replay, setReplay] = useState<HandReplay | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [step, setStep] = useState(0);
  const [showText, setShowText] = useState(false);
  useEffect(() => {
    setReplay(null);
    setStep(0);
    api.hand(id).then(setReplay, (e: Error) => setError(e.message));
  }, [id]);
  if (error) return <p className="error">{error}</p>;
  if (!replay) return <p className="muted">Loading hand…</p>;
  const s = replay.steps[Math.min(step, replay.steps.length - 1)];
  const last = replay.steps.length - 1;
  return (
    <div className="replay">
      <div className="replay-head">
        <strong>
          Hand #{replay.id} · {replay.game}
        </strong>
        <button className="link" onClick={onClose}>
          close
        </button>
      </div>
      <div className="stepper">
        <button className="btn" disabled={step === 0} onClick={() => setStep(0)} aria-label="first step">
          ⏮
        </button>
        <button className="btn" disabled={step === 0} onClick={() => setStep(step - 1)} aria-label="previous step">
          ◀
        </button>
        <span>
          {step + 1}/{replay.steps.length}: {s.label}
        </span>
        <button className="btn" disabled={step >= last} onClick={() => setStep(step + 1)} aria-label="next step">
          ▶
        </button>
        <button className="btn" disabled={step >= last} onClick={() => setStep(last)} aria-label="last step">
          ⏭
        </button>
      </div>
      <TableRenderer view={s.view} labels={(seat) => ({ name: replay.names[seat] ?? `Seat ${seat + 1}` })} />
      <button className="link" onClick={() => setShowText(!showText)}>
        {showText ? "Hide" : "Show"} text
      </button>
      {showText && (
        <>
          <pre className="hand-text">{replay.text}</pre>
          <button className="btn" onClick={() => void navigator.clipboard?.writeText(replay.text)}>
            Copy
          </button>
        </>
      )}
    </div>
  );
}
