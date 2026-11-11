import { useEffect, useRef, useState } from "react";
import type { ChatLine } from "../protocol";

/** Table chat (FR-WB-7). Text is rendered by React, so it is always escaped. */
export function Chat({
  lines,
  admin,
  onSend,
  onDelete,
}: {
  lines: readonly ChatLine[];
  admin: boolean;
  onSend: (text: string) => void;
  onDelete: (id: number) => void;
}) {
  const [text, setText] = useState("");
  const end = useRef<HTMLLIElement>(null);
  // Braces matter: newer browsers return a Promise from scrollIntoView, and React would
  // treat a returned value as the effect's cleanup.
  useEffect(() => {
    end.current?.scrollIntoView({ block: "nearest" });
  }, [lines.length]);
  return (
    <div className="chat">
      <ul className="chat-lines" aria-live="polite">
        {lines.map((l) => (
          <li key={l.id}>
            <span className="muted">{new Date(l.at * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}</span>{" "}
            <strong>{l.name}</strong>: {l.text}
            {admin && (
              <button className="link" aria-label="delete message" onClick={() => onDelete(l.id)}>
                ×
              </button>
            )}
          </li>
        ))}
        <li ref={end} />
      </ul>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          if (text.trim()) onSend(text);
          setText("");
        }}
      >
        <input
          value={text}
          maxLength={500}
          placeholder="Say something"
          aria-label="Chat message"
          onChange={(e) => setText(e.target.value)}
        />
        <button className="btn">Send</button>
      </form>
    </div>
  );
}
