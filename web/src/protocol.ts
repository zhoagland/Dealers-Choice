// Wire types shared with the Rust server (crates/protocol). `npm run typecheck` checks them
// against JSON the server really produces (src/generated/samples.ts, written by
// `cargo test -p protocol`), so the two can't drift.

/** A card: suit * 16 + rank (rank 0 = Two … 12 = Ace; suits c, d, h, s). 13 and 29 are jokers. */
export type Card = number;
export type Seat = number;
/** Money in cents. */
export type Cents = number;

export type Rank =
  | "Two" | "Three" | "Four" | "Five" | "Six" | "Seven" | "Eight"
  | "Nine" | "Ten" | "Jack" | "Queen" | "King" | "Ace";

export type CardSlot = { Up: Card } | { Down: Card | null } | "Empty";
export type SeatStatus = "Active" | "Folded" | "AllIn" | "Out";
export type BoardLayout = "Row" | "H" | "Wheel" | "Cross" | "Elevator" | "Middle";

export interface SeatView {
  seat: Seat;
  stack: Cents;
  bet: Cents;
  status: SeatStatus;
  cards: readonly CardSlot[];
  badges: readonly string[];
}

export interface BoardView {
  name: string;
  layout: BoardLayout;
  slots: readonly CardSlot[];
  lines: readonly (readonly number[])[];
}

export interface PotView {
  name: string;
  amount: Cents;
}

export interface WildView {
  rank: Rank;
  seat: Seat | null;
}

/** What one viewer may see of a hand: the same shape for every game (FR-WB-11). */
export interface TableView {
  game: string;
  phase: string;
  viewer: Seat;
  button: Seat;
  to_act: Seat | null;
  seats: readonly SeatView[];
  boards: readonly BoardView[];
  pots: readonly PotView[];
  wilds: readonly WildView[];
  notes: readonly string[];
}

// ----- client → server ----------------------------------------------------------------------

export type Choice =
  | { kind: "Option"; id: string }
  | { kind: "Amount"; id: string; cents: Cents };

export type ClientMsg =
  | { type: "Sit"; seat: number }
  | { type: "Stand" }
  | { type: "Away"; away: boolean }
  | { type: "Start" }
  | { type: "Act"; prompt_id: number; choice: Choice }
  | { type: "Chat"; text: string }
  | { type: "DeleteChat"; id: number }
  | { type: "Ping" };

// ----- server → client ----------------------------------------------------------------------

export interface Preset {
  label: string;
  cents: Cents;
}

export type PromptOption =
  | { kind: "Button"; id: string; label: string; class: string }
  | {
      kind: "Amount";
      id: string;
      label: string;
      min: Cents;
      max: Cents;
      /** 0 = only the presets; otherwise any multiple of `step` in range. */
      step: Cents;
      presets: readonly Preset[];
    }
  | { kind: "Game"; id: string; name: string; family: string; seats: string };

export interface Prompt {
  id: number;
  /** Unix ms when the turn timer plus the time bank runs out. */
  deadline_ms: number;
  bank_ms: number;
  title: string;
  options: readonly PromptOption[];
}

export interface SeatInfo {
  seat: number;
  account: number | null;
  name: string;
  avatar: string | null;
  stack: Cents;
  bankroll: Cents;
  away: boolean;
  connected: boolean;
}

export interface TableInfo {
  id: string;
  name: string;
  host: number;
  state: string;
  max_seats: number;
  timer_secs: number;
  bank_secs: number;
  seats: readonly SeatInfo[];
  game: string | null;
  game_name: string | null;
  chooser: number | null;
  pool: readonly string[];
  hand_no: number;
  log: readonly string[];
  /** The table seat of each hand seat in the current hand (TableView seats are hand seats). */
  hand_seats: readonly number[];
}

export interface ChatLine {
  id: number;
  name: string;
  text: string;
  at: number;
}

export interface SeatResult {
  name: string;
  cents: Cents;
}

export interface HandSummary {
  hand_id: number;
  game: string;
  results: readonly SeatResult[];
  text: string;
}

export type ServerMsg =
  | {
      type: "Snapshot";
      seq: number;
      you: number | null;
      view: TableView | null;
      prompt: Prompt | null;
      table: TableInfo;
    }
  | { type: "HandSummary"; summary: HandSummary }
  | { type: "Chat"; line: ChatLine }
  | { type: "ChatDeleted"; id: number }
  | { type: "Error"; code: string; message: string }
  | { type: "Pong" };

// ----- REST ---------------------------------------------------------------------------------

export interface Me {
  id: number;
  name: string;
  avatar: string | null;
  is_admin: boolean;
  bankroll: Cents;
}

export interface CreateTable {
  name: string;
  max_seats: number;
  pool: string[];
  timer_secs: number;
  listed: boolean;
}

export interface TableListing {
  id: string;
  name: string;
  players: number;
  max_seats: number;
  state: string;
  listed: boolean;
}

export interface GameInfo {
  id: string;
  name: string;
  family: string;
  seats_min: number;
  seats_max: number;
  rules: string;
}

export interface HandListing {
  id: number;
  game: string;
  at: number;
  results: readonly SeatResult[];
}

export interface ReplayStep {
  label: string;
  view: TableView;
}

export interface HandReplay {
  id: number;
  game: string;
  steps: readonly ReplayStep[];
  text: string;
  /** Player name per hand seat. */
  names: readonly string[];
}

export interface AdminAccount {
  id: number;
  name: string;
  is_admin: boolean;
  bankroll: Cents;
}

export interface BankrollChange {
  account: number;
  op: "deposit" | "reset";
  cents: Cents;
}

export interface ActivityToken {
  access_token: string;
  session: string;
  table: string;
}

export interface ServerConfig {
  discord_client_id: string | null;
  dev_mode: boolean;
}
