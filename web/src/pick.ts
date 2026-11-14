import { useEffect, useMemo, useState } from "react";
import type { CardPick, Choice, PickMode, Prompt } from "./protocol";

/** A prompt option that uses some of the viewer's cards. */
export interface CardOption {
  id: string;
  label: string;
  pick: CardPick;
}

/** Card picking for the current prompt: the player clicks their cards instead of buttons. */
export interface Picker {
  mode: PickMode;
  /** Picked positions (indices into the viewer's seat cards), in click order. */
  sel: readonly number[];
  click: (pos: number) => void;
  clear: () => void;
  /** The option the current pick makes, if any. */
  match: CardOption | null;
  /** What to do next, when the pick isn't complete. */
  hint: string;
  /** Text for the confirm button. */
  confirm: string;
  /** Positions that can be clicked at all. */
  usable: ReadonlySet<number>;
  /** Pairs mode: which pair a position belongs to (0, 1, 2…). */
  pairOf: (pos: number) => number | null;
  submit: () => void;
}

export function cardOptions(prompt: Prompt | null): CardOption[] {
  if (!prompt) return [];
  return prompt.options.flatMap((o) =>
    o.kind === "Button" && o.cards ? [{ id: o.id, label: o.label, pick: o.cards }] : [],
  );
}

const sameSet = (a: readonly number[], b: readonly number[]) =>
  a.length === b.length && [...a].sort().join() === [...b].sort().join();

const pairKey = (p: readonly number[]) => {
  const pairs: string[] = [];
  for (let i = 0; i + 1 < p.length; i += 2) pairs.push([p[i], p[i + 1]].sort().join("-"));
  return pairs.sort().join("|");
};

const plural = (n: number, w: string) => `${n} ${w}${n === 1 ? "" : "s"}`;

/**
 * `groups`: how the viewer's cards are grouped (sizes, e.g. Mama's pairs). When every option
 * takes whole groups, clicking a card picks its whole group.
 */
export function usePicker(prompt: Prompt | null, onChoose: (c: Choice) => void, groups: readonly number[] = []): Picker | null {
  const opts = useMemo(() => cardOptions(prompt), [prompt]);
  const [sel, setSel] = useState<number[]>([]);
  useEffect(() => setSel([]), [prompt?.id]);
  if (opts.length === 0 || !prompt) return null;
  const mode = opts[0].pick.mode;
  const usable = new Set(opts.flatMap((o) => o.pick.positions));
  const submit = (o: CardOption | null) => {
    if (o) onChoose({ kind: "Option", id: o.id });
  };

  let match: CardOption | null = null;
  if (mode === "Select") match = opts.find((o) => sameSet(o.pick.positions, sel)) ?? null;
  if (mode === "Pairs") match = opts.find((o) => pairKey(o.pick.positions) === pairKey(sel)) ?? null;
  if (mode === "Sequence") match = opts.find((o) => o.pick.positions.join() === sel.join()) ?? null;

  // Group of each position, when options only ever take whole groups.
  const groupOf = new Map<number, number[]>();
  if (groups.length > 1 && mode === "Select") {
    let k = 0;
    for (const g of groups) {
      const members = Array.from({ length: g }, (_, i) => k + i);
      members.forEach((m) => groupOf.set(m, members));
      k += g;
    }
    const whole = opts.every((o) => o.pick.positions.every((p) => groupOf.get(p)?.every((m) => o.pick.positions.includes(m))));
    if (!whole) groupOf.clear();
  }

  const click = (pos: number) => {
    if (!usable.has(pos)) return;
    if (mode === "Play") {
      submit(opts.find((o) => o.pick.positions[0] === pos) ?? null);
      return;
    }
    setSel((s) => {
      const at = s.indexOf(pos);
      const group = groupOf.get(pos);
      if (group) return at >= 0 ? s.filter((x) => !group.includes(x)) : [...s.filter((x) => !group.includes(x)), ...group];
      if (mode === "Select") return at >= 0 ? s.filter((x) => x !== pos) : [...s, pos];
      if (mode === "Pairs") {
        if (at < 0) return [...s, pos];
        // Remove the whole pair this card is in.
        const start = at - (at % 2);
        return s.filter((_, i) => i !== start && i !== start + 1);
      }
      // Sequence: clicking a picked card takes it and everything after it back.
      return at >= 0 ? s.slice(0, at) : [...s, pos];
    });
  };

  const sizes = [...new Set(opts.map((o) => o.pick.positions.length))].sort((a, b) => a - b);
  const total = usable.size;
  let hint = "";
  let confirm = match?.label ?? "";
  if (mode === "Select") {
    const lo = sizes[0];
    const hi = sizes[sizes.length - 1];
    hint =
      lo === hi
        ? `Pick ${plural(lo, "card")}`
        : lo === 0
          ? `Pick up to ${plural(hi, "card")}, or none`
          : `Pick ${lo} to ${plural(hi, "card")}`;
    if (match && sel.length > 0) confirm = match.label.replace(/cards? [\d,]+/, plural(sel.length, "card"));
    if (groupOf.size > 0 && lo === hi && groups.every((g) => g === 2)) hint = lo === 2 ? "Click a pair" : `Pick ${lo / 2} pairs`;
    if (!match && sel.length > 0) hint = "That choice isn't allowed. " + hint;
  } else if (mode === "Pairs") {
    hint = sel.length % 2 === 1 ? "Pick its partner" : `Click two cards to pair them (${sel.length / 2} of ${total / 2})`;
    confirm = "Lock in pairs";
  } else if (mode === "Sequence") {
    hint = `Click your cards in the order to show them (${sel.length} of ${total})`;
    confirm = "Lock in order";
  } else {
    hint = "Click a card to play it";
  }
  return {
    mode,
    sel,
    click,
    clear: () => setSel([]),
    match,
    hint,
    confirm,
    usable,
    pairOf: (pos) => {
      if (mode !== "Pairs") return null;
      const at = sel.indexOf(pos);
      return at < 0 ? null : Math.floor(at / 2);
    },
    submit: () => submit(match),
  };
}
