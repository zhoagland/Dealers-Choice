import { useEffect, useState } from "react";
import { setMusic, unlock } from "./sound";

/** Per-viewer settings, remembered in this browser. */
export interface Prefs {
  fourColor: boolean;
  music: boolean;
  turnSound: boolean;
  effects: boolean;
}

const DEFAULTS: Prefs = { fourColor: false, music: false, turnSound: true, effects: true };

function load(): Prefs {
  try {
    const raw = localStorage.getItem("prefs");
    const old = localStorage.getItem("fourColor") === "1";
    return { ...DEFAULTS, fourColor: old, ...(raw ? (JSON.parse(raw) as Partial<Prefs>) : {}) };
  } catch {
    return DEFAULTS;
  }
}

let current: Prefs = load();
const listeners = new Set<(p: Prefs) => void>();

export function prefs(): Prefs {
  return current;
}

export function setPrefs(change: Partial<Prefs>): void {
  current = { ...current, ...change };
  try {
    localStorage.setItem("prefs", JSON.stringify(current));
  } catch {
    /* private mode */
  }
  apply();
  listeners.forEach((l) => l(current));
}

function apply() {
  document.body.classList.toggle("four-color", current.fourColor);
}

let started = false;
/** Apply the saved settings; music starts with the first click (browsers require one). */
export function startPrefs(): void {
  apply();
  if (started) return;
  started = true;
  const first = () => {
    unlock();
    setMusic(current.music);
    window.removeEventListener("pointerdown", first);
    window.removeEventListener("keydown", first);
  };
  window.addEventListener("pointerdown", first);
  window.addEventListener("keydown", first);
}

export function usePrefs(): [Prefs, (c: Partial<Prefs>) => void] {
  const [p, setP] = useState(current);
  useEffect(() => {
    listeners.add(setP);
    return () => {
      listeners.delete(setP);
    };
  }, []);
  return [
    p,
    (c) => {
      unlock();
      if (c.music !== undefined) setMusic(c.music);
      setPrefs(c);
    },
  ];
}
