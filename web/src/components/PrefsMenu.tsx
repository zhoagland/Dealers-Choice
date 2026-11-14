import { useEffect, useRef, useState } from "react";
import { usePrefs } from "../prefs";

/** Settings button: four-colour deck, music, turn sound, card sounds (also inside Discord). */
export function PrefsMenu() {
  const [p, set] = usePrefs();
  const [open, setOpen] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (e: PointerEvent) => {
      if (box.current && !box.current.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("pointerdown", close);
    return () => window.removeEventListener("pointerdown", close);
  }, [open]);
  const row = (key: keyof typeof p, label: string, hint: string) => (
    <label className="check" title={hint}>
      <input type="checkbox" checked={p[key]} onChange={(e) => set({ [key]: e.target.checked })} /> {label}
    </label>
  );
  return (
    <div className="prefs" ref={box}>
      <button className="btn icon" aria-expanded={open} aria-label="Settings" title="Settings" onClick={() => setOpen(!open)}>
        ⚙
      </button>
      {open && (
        <div className="prefs-menu" role="menu">
          {row("fourColor", "Four-color deck", "Clubs green, diamonds blue")}
          {row("music", "Background music", "A mellow loop while you play")}
          {row("turnSound", "Turn sound", "A chime when it's your turn")}
          {row("effects", "Card sounds", "Deals and wins")}
        </div>
      )}
    </div>
  );
}
