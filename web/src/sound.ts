// Table sounds, made with Web Audio (no sound files to load or proxy through Discord).
// Browsers only allow audio after a click or key press, so `unlock()` runs on the first one.

let ctx: AudioContext | null = null;
let master: GainNode | null = null;
let musicTimer: number | null = null;
let musicGain: GainNode | null = null;

function audio(): AudioContext | null {
  if (ctx) return ctx;
  const AC = window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
  if (!AC) return null;
  ctx = new AC();
  master = ctx.createGain();
  master.gain.value = 0.6;
  master.connect(ctx.destination);
  return ctx;
}

/** Call from a user gesture so later sounds may play. */
export function unlock(): void {
  const a = audio();
  if (a && a.state === "suspended") void a.resume();
}

function tone(freq: number, at: number, dur: number, vol: number, type: OscillatorType = "sine", out?: AudioNode) {
  const a = audio();
  if (!a || !master) return;
  const o = a.createOscillator();
  const g = a.createGain();
  o.type = type;
  o.frequency.value = freq;
  g.gain.setValueAtTime(0, at);
  g.gain.linearRampToValueAtTime(vol, at + 0.015);
  g.gain.exponentialRampToValueAtTime(0.0001, at + dur);
  o.connect(g).connect(out ?? master);
  o.start(at);
  o.stop(at + dur + 0.05);
}

/** "Your turn": a bright two-note chime. */
export function turnChime(): void {
  const a = audio();
  if (!a || a.state !== "running") return;
  const t = a.currentTime;
  tone(880, t, 0.35, 0.25, "triangle");
  tone(1318.5, t + 0.12, 0.5, 0.22, "triangle");
}

/** A card landing: a short soft tick. */
export function cardTick(): void {
  const a = audio();
  if (!a || a.state !== "running" || !master) return;
  const t = a.currentTime;
  const len = Math.floor(a.sampleRate * 0.04);
  const buf = a.createBuffer(1, len, a.sampleRate);
  const d = buf.getChannelData(0);
  for (let i = 0; i < len; i++) d[i] = (Math.random() * 2 - 1) * (1 - i / len) ** 3;
  const src = a.createBufferSource();
  src.buffer = buf;
  const f = a.createBiquadFilter();
  f.type = "bandpass";
  f.frequency.value = 2200;
  const g = a.createGain();
  g.gain.value = 0.35;
  src.connect(f).connect(g).connect(master);
  src.start(t);
}

/** Winning chips: a quick rising arpeggio. */
export function winJingle(): void {
  const a = audio();
  if (!a || a.state !== "running") return;
  const t = a.currentTime;
  [523.25, 659.25, 783.99, 1046.5].forEach((f, i) => tone(f, t + i * 0.08, 0.3, 0.18, "square"));
}

// ----- background music: a mellow looping lounge progression ---------------------------------

const CHORDS = [
  [220.0, 261.63, 329.63, 392.0], // Am7
  [146.83, 220.0, 261.63, 349.23], // Dm7
  [196.0, 246.94, 293.66, 349.23], // G7
  [130.81, 196.0, 246.94, 329.63], // Cmaj7
];
const BAR = 2.4; // seconds per chord

function scheduleBar(k: number, at: number) {
  if (!musicGain) return;
  const chord = CHORDS[k % CHORDS.length];
  // Soft pad.
  for (const f of chord) tone(f, at, BAR * 1.1, 0.05, "sine", musicGain);
  // Bass on beats 1 and 3.
  tone(chord[0] / 2, at, 0.6, 0.12, "triangle", musicGain);
  tone(chord[0] / 2, at + BAR / 2, 0.6, 0.09, "triangle", musicGain);
  // A little arpeggio.
  [0, 1, 2, 3, 2, 1].forEach((n, i) => tone(chord[n] * 2, at + (i * BAR) / 6, 0.25, 0.025, "triangle", musicGain!));
}

export function setMusic(on: boolean): void {
  const a = audio();
  if (!a || !master) return;
  if (!on) {
    if (musicTimer !== null) window.clearInterval(musicTimer);
    musicTimer = null;
    if (musicGain) {
      musicGain.gain.setTargetAtTime(0, a.currentTime, 0.3);
      const g = musicGain;
      window.setTimeout(() => g.disconnect(), 1500);
    }
    musicGain = null;
    return;
  }
  if (musicGain) return;
  musicGain = a.createGain();
  musicGain.gain.value = 0;
  musicGain.gain.setTargetAtTime(0.5, a.currentTime, 0.8);
  musicGain.connect(master);
  let bar = 0;
  let next = a.currentTime + 0.1;
  const tick = () => {
    // Keep about two bars scheduled ahead.
    while (next < a.currentTime + BAR * 2) {
      scheduleBar(bar++, next);
      next += BAR;
    }
  };
  tick();
  musicTimer = window.setInterval(tick, 500);
}
